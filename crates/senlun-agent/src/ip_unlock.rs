//! One fixed, opt-in, one-shot IP streaming/AI unlock and Google region check.
//! Existing Agent authentication stays on the Service client; target probes use
//! an isolated, credential-free HTTPS client with bounded reads and timeouts.
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::{Duration, Instant},
};

use reqwest::{
    Client, StatusCode, Url,
    header::{
        ACCEPT, ACCEPT_LANGUAGE, AUTHORIZATION, CONTENT_TYPE, LOCATION, ORIGIN, REFERER, USER_AGENT,
    },
    tls,
};
use senlun_protocol::{
    GoogleUnlockStatus, IP_UNLOCK_LEASE_MS, IP_UNLOCK_SCHEMA_VERSION, IpUnlockClaim,
    IpUnlockOutcome, IpUnlockReport, IpUnlockTask, UnlockServiceStatus,
};
use serde_json::Value;
use tokio::task::JoinHandle;

use crate::task_lease::{ClaimValidation, TaskLease, validate_claimed_task};

const POLL: Duration = Duration::from_secs(15);
const MAX_CONTROL_RESPONSE: usize = 16 * 1024;
const MAX_SMALL_BODY: usize = 32 * 1024;
const MAX_REDDIT_BODY: usize = 512 * 1024;
const MAX_HTML_BODY: usize = 2 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(35);

const BROWSER_UA: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 ",
    "(KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36"
);
const ACCEPT_LANG: &str = "en-US,en;q=0.9";
const BROWSER_ACCEPT_HTML: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";
// Public unauthenticated web-browser client key (`disney&browser&1.0.0`) used by
// Disney+'s anonymous device bootstrap endpoint; contains no user credentials.
const DISNEY_PUBLIC_CLIENT_KEY: &str =
    "ZGlzbmV5JmJyb3dzZXImMS4wLjA.Cu56AgSfBTDag5NiRA81oLHkDZfu5L3CKadnefEAY84";
const DISNEY_DEVICE_BODY: &str = concat!(
    r#"{"deviceFamily":"browser","applicationRuntime":"chrome","#,
    r#""deviceProfile":"windows","attributes":{}}"#
);
const DISNEY_REFRESH_QUERY: &str = concat!(
    "mutation refreshToken($input: RefreshTokenInput!) ",
    "{ refreshToken(refreshToken: $input) { activeSession { sessionId } } }"
);
const TIKTOK_EXPLORE_URL: &str = "https://www.tiktok.com/explore";

const CHATGPT_UNSUPPORTED_REGIONS: &[&str] = &[
    "AF", "BY", "CN", "CU", "HK", "IR", "KP", "MO", "RU", "SY", "VE",
];

pub struct UnlockRunner(JoinHandle<()>);

impl UnlockRunner {
    #[must_use]
    pub fn start(client: Client, url: Url, token: String) -> Self {
        Self(tokio::spawn(run(client, url, token)))
    }
}

impl Drop for UnlockRunner {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn run(client: Client, url: Url, token: String) {
    let Ok(claim_url) = url.join("api/v1/agents/ip-unlock/claim") else {
        return;
    };
    let Ok(result_url) = url.join("api/v1/agents/ip-unlock/result") else {
        return;
    };
    let mut pending: Option<(IpUnlockOutcome, Instant)> = None;
    loop {
        if let Some((outcome, deadline)) = &pending {
            if Instant::now() >= *deadline {
                pending = None;
            } else {
                match tokio::time::timeout_at(
                    (*deadline).into(),
                    client
                        .post(result_url.clone())
                        .bearer_auth(&token)
                        .json(outcome)
                        .send(),
                )
                .await
                {
                    Ok(Ok(response)) if response.status() == StatusCode::UNAUTHORIZED => return,
                    Ok(Ok(response))
                        if response.status().is_success()
                            || response.status() == StatusCode::CONFLICT
                            || response.status() == StatusCode::BAD_REQUEST =>
                    {
                        pending = None;
                    }
                    Err(_) => pending = None,
                    Ok(Ok(_) | Err(_)) => {}
                }
            }
        }
        if pending.is_none() {
            let claim = IpUnlockClaim {
                schema_version: IP_UNLOCK_SCHEMA_VERSION,
                agent_version: env!("CARGO_PKG_VERSION").to_owned(),
                available: true,
            };
            let claim_started = Instant::now();
            if let Ok(mut response) = client
                .post(claim_url.clone())
                .bearer_auth(&token)
                .json(&claim)
                .send()
                .await
            {
                if response.status() == StatusCode::UNAUTHORIZED {
                    return;
                }
                if response.status().is_success() {
                    let mut bytes = Vec::new();
                    while let Ok(Some(chunk)) = response.chunk().await {
                        if bytes.len().saturating_add(chunk.len()) > MAX_CONTROL_RESPONSE {
                            bytes.clear();
                            break;
                        }
                        bytes.extend_from_slice(&chunk);
                    }
                    let task: Option<IpUnlockTask> = serde_json::from_slice(&bytes).ok().flatten();
                    if let Some(task) = task {
                        let claim_received = Instant::now();
                        match validate_claimed_task(
                            &task.id,
                            task.expires_at_unix_ms,
                            IP_UNLOCK_LEASE_MS,
                            claim_started,
                            claim_received,
                        ) {
                            ClaimValidation::Ignore => {}
                            ClaimValidation::InvalidTask { deadline } => {
                                pending = Some((
                                    IpUnlockOutcome {
                                        id: task.id,
                                        report: None,
                                        error: Some("invalid_task".to_owned()),
                                    },
                                    deadline,
                                ));
                                continue;
                            }
                            ClaimValidation::Ready(lease) => {
                                let result = collect(&task, lease).await;
                                if Instant::now() < lease.deadline {
                                    pending = Some((
                                        IpUnlockOutcome {
                                            id: task.id,
                                            report: result.as_ref().ok().cloned(),
                                            error: result.err().map(str::to_owned),
                                        },
                                        lease.deadline,
                                    ));
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
        }
        tokio::time::sleep(POLL).await;
    }
}

fn target_client_builder(family: u8) -> Result<reqwest::ClientBuilder, &'static str> {
    let bind = match family {
        4 => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        6 => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        _ => return Err("invalid_task"),
    };
    Ok(Client::builder()
        .https_only(true)
        .no_proxy()
        .local_address(bind)
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .pool_max_idle_per_host(0))
}

fn target_client(family: u8) -> Result<Client, &'static str> {
    target_client_builder(family)?
        .build()
        .map_err(|_| "unavailable")
}

async fn resolve_reddit_ipv6() -> Option<SocketAddr> {
    for host in ["www.reddit.com:443", "dualstack.reddit.map.fastly.net:443"] {
        if let Ok(Ok(mut addrs)) =
            tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::lookup_host(host)).await
            && let Some(addr) = addrs.find(SocketAddr::is_ipv6)
        {
            return Some(addr);
        }
    }
    None
}

async fn reddit_client(family: u8) -> Option<Client> {
    let mut builder = target_client_builder(family)
        .ok()?
        .tls_version_min(tls::Version::TLS_1_3);
    if family == 6 {
        let addr = resolve_reddit_ipv6().await?;
        builder = builder.resolve("www.reddit.com", addr);
    }
    builder.build().ok()
}

async fn collect(task: &IpUnlockTask, lease: TaskLease) -> Result<IpUnlockReport, &'static str> {
    let expected: IpAddr = task.address.parse().map_err(|_| "invalid_task")?;
    if !matches!(
        (task.family, expected),
        (4, IpAddr::V4(_)) | (6, IpAddr::V6(_))
    ) {
        return Err("invalid_task");
    }
    let collection_deadline = lease.collection_deadline(Instant::now(), TOTAL_TIMEOUT);
    if Instant::now() >= collection_deadline {
        return Err("timeout");
    }
    tokio::time::timeout_at(collection_deadline.into(), async {
        let client = target_client(task.family)?;
        verify_egress(&client, task.family, expected).await?;
        let ((google, youtube), netflix, disney_plus, chatgpt, reddit, tiktok) = tokio::join!(
            probe_google_and_youtube(&client),
            probe_netflix(&client),
            probe_disney_plus(&client),
            probe_chatgpt(&client),
            probe_reddit(task.family),
            probe_tiktok_for_family(&client, task.family),
        );
        let mut services = vec![youtube, netflix, disney_plus, chatgpt, reddit];
        if let Some(tiktok) = tiktok {
            services.push(tiktok);
        }
        Ok(IpUnlockReport {
            family: task.family,
            address: expected.to_string(),
            checked_at_unix_ms: lease.checked_at_unix_ms(Instant::now()),
            google,
            services,
        })
    })
    .await
    .map_err(|_| "timeout")?
}

fn is_blocked_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::FORBIDDEN | StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS
    )
}

async fn verify_egress(client: &Client, family: u8, expected: IpAddr) -> Result<(), &'static str> {
    let url = if family == 4 {
        "https://api.ipify.org"
    } else {
        "https://api6.ipify.org"
    };
    let response = client
        .get(url)
        .header(
            USER_AGENT,
            concat!("senlun-agent/", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .await
        .map_err(|_| "collection_failed")?;
    if !response.status().is_success() {
        return Err("collection_failed");
    }
    let body = read_capped(response, 128)
        .await
        .ok_or("collection_failed")?;
    let actual: IpAddr = body.trim().parse().map_err(|_| "collection_failed")?;
    if actual != expected {
        return Err("address_mismatch");
    }
    Ok(())
}

async fn read_capped(mut response: reqwest::Response, limit: usize) -> Option<String> {
    let mut bytes = Vec::new();
    while bytes.len() < limit {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                let remaining = limit - bytes.len();
                let take = chunk.len().min(remaining);
                bytes.extend_from_slice(&chunk[..take]);
            }
            Ok(None) => break,
            Err(_) => {
                if bytes.is_empty() {
                    return None;
                }
                break;
            }
        }
    }
    let text = String::from_utf8(bytes)
        .unwrap_or_else(|err| String::from_utf8_lossy(err.as_bytes()).into_owned());
    Some(text)
}

fn location_header(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn same_origin_https_redirect(base: &str, location: &str) -> Option<String> {
    let base_url = Url::parse(base).ok()?;
    let target = base_url.join(location).ok()?;
    if target.scheme() == "https"
        && target.host_str() == base_url.host_str()
        && target.port().is_none()
        && target.username().is_empty()
        && target.password().is_none()
    {
        Some(target.into())
    } else {
        None
    }
}

fn allowlisted_https_redirect(base: &str, location: &str, host: &str) -> Option<String> {
    let target = Url::parse(base).ok()?.join(location).ok()?;
    if target.scheme() == "https"
        && target.host_str() == Some(host)
        && target.port().is_none()
        && target.username().is_empty()
        && target.password().is_none()
    {
        Some(target.into())
    } else {
        None
    }
}

fn valid_region(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() == 2 && trimmed.bytes().all(|b| b.is_ascii_alphabetic()) {
        Some(trimmed.to_ascii_uppercase())
    } else {
        None
    }
}

fn extract_json_region(body: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\"");
    for (start, _) in body.match_indices(&pattern) {
        let rest = body[start + pattern.len()..].trim_start();
        let Some(rest) = rest.strip_prefix(':') else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('"') else {
            continue;
        };
        let Some(end) = rest.find('"') else {
            continue;
        };
        if let Some(code) = valid_region(&rest[..end]) {
            return Some(code);
        }
    }
    None
}

fn extract_html_attr_region(body: &str, attr: &str) -> Option<String> {
    let pattern = format!("{attr}=\"");
    for (start, _) in body.match_indices(&pattern) {
        if let Some(prev) = start
            .checked_sub(1)
            .and_then(|idx| body.as_bytes().get(idx))
            && !prev.is_ascii_whitespace()
        {
            continue;
        }
        let rest = &body[start + pattern.len()..];
        let Some(end) = rest.find('"') else {
            continue;
        };
        if let Some(code) = valid_region(&rest[..end]) {
            return Some(code);
        }
    }
    None
}

fn query_param_region(base: &str, location: &str, param: &str) -> Option<String> {
    let parsed = Url::parse(base).ok()?.join(location).ok()?;
    parsed
        .query_pairs()
        .find_map(|(key, value)| (key == param).then(|| valid_region(&value)).flatten())
}

fn is_google_cn_redirect(base: &str, location: &str) -> bool {
    let Ok(parsed) = Url::parse(base).and_then(|u| u.join(location)) else {
        return false;
    };
    let Some(host) = parsed.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    if host == "google.cn" || host.ends_with(".google.cn") {
        return true;
    }
    (host == "google.com.hk" || host.ends_with(".google.com.hk"))
        && parsed
            .query_pairs()
            .any(|(key, value)| key == "pref" && value.eq_ignore_ascii_case("hkredirect"))
}

fn region_from_google_location(base: &str, location: &str) -> Option<String> {
    let host = Url::parse(base)
        .ok()?
        .join(location)
        .ok()?
        .host_str()?
        .to_ascii_lowercase();
    for (suffix, code) in [
        ("google.cn", "CN"),
        ("google.com.hk", "HK"),
        ("google.co.jp", "JP"),
        ("google.com.sg", "SG"),
        ("google.com.tw", "TW"),
        ("google.co.kr", "KR"),
        ("google.co.uk", "GB"),
        ("google.de", "DE"),
        ("google.fr", "FR"),
        ("google.ca", "CA"),
        ("google.com.au", "AU"),
        ("google.nl", "NL"),
    ] {
        if host == suffix || host.ends_with(&format!(".{suffix}")) {
            return Some(code.to_owned());
        }
    }
    None
}

struct GoogleSearchProbe {
    china: bool,
    captcha: bool,
    region: Option<String>,
    ok: bool,
}

async fn probe_google_and_youtube(client: &Client) -> (GoogleUnlockStatus, UnlockServiceStatus) {
    let ((yt_region, yt_cn, yt_status), google_probe) =
        tokio::join!(probe_youtube(client), probe_google_search(client));

    let google = if yt_cn || google_probe.china {
        GoogleUnlockStatus {
            status: "china".to_owned(),
            region: Some("CN".to_owned()),
        }
    } else if google_probe.captcha {
        GoogleUnlockStatus {
            status: "captcha".to_owned(),
            region: yt_region.clone().or(google_probe.region),
        }
    } else if yt_region.is_some() || google_probe.region.is_some() || google_probe.ok {
        GoogleUnlockStatus {
            status: "normal".to_owned(),
            region: yt_region.clone().or(google_probe.region),
        }
    } else {
        GoogleUnlockStatus {
            status: "unknown".to_owned(),
            region: None,
        }
    };

    let youtube = UnlockServiceStatus {
        id: "youtube".to_owned(),
        status: yt_status.to_owned(),
        region: if yt_cn {
            Some("CN".to_owned())
        } else {
            yt_region
        },
        access_type: None,
    };
    (google, youtube)
}

async fn probe_youtube(client: &Client) -> (Option<String>, bool, &'static str) {
    let base = "https://www.youtube.com/premium?hl=en";
    let Ok(mut response) = client
        .get(base)
        .header(USER_AGENT, BROWSER_UA)
        .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
        .send()
        .await
    else {
        return (None, false, "unknown");
    };
    if response.status().is_redirection()
        && let Some(loc) = location_header(&response)
    {
        if is_google_cn_redirect(base, &loc) {
            return (Some("CN".to_owned()), true, "china");
        }
        if allowlisted_https_redirect(base, &loc, "consent.youtube.com").is_some() {
            if let Some(region) = query_param_region(base, &loc, "gl") {
                if region == "CN" {
                    return (Some("CN".to_owned()), true, "china");
                }
                return (Some(region), false, "unknown");
            }
            return (None, false, "unknown");
        }
        if let Some(next_url) = same_origin_https_redirect(base, &loc)
            && let Ok(next) = client
                .get(&next_url)
                .header(USER_AGENT, BROWSER_UA)
                .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
                .send()
                .await
        {
            response = next;
        }
    }
    let status = response.status();
    let body = read_capped(response, MAX_HTML_BODY)
        .await
        .unwrap_or_default();
    let region = extract_json_region(&body, "contentRegion")
        .or_else(|| extract_json_region(&body, "INNERTUBE_CONTEXT_GL"))
        .or_else(|| extract_json_region(&body, "countryCode"));
    let is_cn = region.as_deref() == Some("CN") || body.contains("www.google.cn");
    if is_cn {
        return (Some("CN".to_owned()), true, "china");
    }
    if body.contains("Premium is not available in your country") {
        return (region, false, "blocked");
    }
    if region.is_some() && status.is_success() {
        return (region, false, "unlocked");
    }
    if is_blocked_status(status) {
        return (region, false, "blocked");
    }
    (region, false, "unknown")
}

async fn probe_google_search(client: &Client) -> GoogleSearchProbe {
    let base = "https://www.google.com/search?q=1&hl=en";
    let Ok(response) = client
        .get(base)
        .header(USER_AGENT, BROWSER_UA)
        .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
        .send()
        .await
    else {
        return GoogleSearchProbe {
            china: false,
            captcha: false,
            region: None,
            ok: false,
        };
    };
    let status = response.status();
    let loc = location_header(&response).unwrap_or_default();
    let consent_region = if allowlisted_https_redirect(base, &loc, "consent.google.com").is_some() {
        query_param_region(base, &loc, "gl")
    } else {
        None
    };
    let loc_cn = is_google_cn_redirect(base, &loc) || consent_region.as_deref() == Some("CN");
    let lower_loc = loc.to_ascii_lowercase();
    let is_captcha = status == StatusCode::TOO_MANY_REQUESTS
        || lower_loc.contains("/sorry/")
        || lower_loc.contains("sorry/index");
    let region = if loc_cn {
        Some("CN".to_owned())
    } else {
        consent_region.or_else(|| region_from_google_location(base, &loc))
    };
    let ok = status.is_success() || (status.is_redirection() && !loc_cn && !is_captcha);
    GoogleSearchProbe {
        china: loc_cn,
        captcha: is_captcha,
        region,
        ok,
    }
}

fn is_reddit_waf_block(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("blocked by network security")
        || lower.contains("whoa there, pardner")
        || lower.contains("you've been blocked")
}

async fn probe_reddit(family: u8) -> UnlockServiceStatus {
    let Some(client) = reddit_client(family).await else {
        return service_status("reddit", "unknown", None);
    };

    // Primary: shreddit/reddit-chat over TLS 1.3 reports both access and country="XX".
    let Ok(response) = client
        .get("https://www.reddit.com/svc/shreddit/reddit-chat")
        .header(USER_AGENT, BROWSER_UA)
        .header(ACCEPT, "*/*")
        .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
        .header(REFERER, "https://www.reddit.com/")
        .header(ORIGIN, "https://www.reddit.com")
        .send()
        .await
    else {
        return service_status("reddit", "unknown", None);
    };
    let chat_status = response.status();
    let chat_body = read_capped(response, MAX_REDDIT_BODY)
        .await
        .unwrap_or_default();
    if chat_status.is_success() && !is_reddit_waf_block(&chat_body) {
        let region = extract_html_attr_region(&chat_body, "country");
        return service_status("reddit", "unlocked", region);
    }
    let chat_blocked = is_blocked_status(chat_status) || is_reddit_waf_block(&chat_body);

    // Fallback: passive GET https://www.reddit.com/ distinguishes clean IPs (200)
    // from Fastly network-security blocked IPs (403) without solving challenges.
    if let Ok(web) = client
        .get("https://www.reddit.com/")
        .header(USER_AGENT, BROWSER_UA)
        .header(ACCEPT, BROWSER_ACCEPT_HTML)
        .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
        .send()
        .await
    {
        let status = web.status();
        let body = read_capped(web, MAX_REDDIT_BODY).await.unwrap_or_default();
        if status.is_success() && !is_reddit_waf_block(&body) {
            let region = extract_html_attr_region(&body, "country");
            return service_status("reddit", "unlocked", region);
        }
        if is_blocked_status(status) || is_reddit_waf_block(&body) {
            return service_status("reddit", "blocked", None);
        }
    }

    if chat_blocked {
        service_status("reddit", "blocked", None)
    } else {
        service_status("reddit", "unknown", None)
    }
}

fn netflix_region_from_url(location: &str) -> Option<String> {
    let parsed = Url::parse("https://www.netflix.com/")
        .ok()?
        .join(location)
        .ok()?;
    let segment = parsed.path().trim_start_matches('/').split('/').next()?;
    let candidate = segment.split('-').next()?;
    valid_region(candidate)
}

struct NetflixTitleResult {
    status: StatusCode,
    available: bool,
    soft_unavailable: bool,
    region: Option<String>,
}

fn extract_netflix_body_region(body: &str) -> Option<String> {
    if let Some(idx) = body.find("\"requestCountry\"") {
        let rest = &body[idx..];
        if let Some(end) = rest.find('}')
            && end <= 128
            && let Some(code) = extract_json_region(&rest[..end], "id")
        {
            return Some(code);
        }
    }
    let marker = "\",\"countryName\":\"";
    let bytes = body.as_bytes();
    for (idx, _) in body.match_indices(marker) {
        if idx >= 8
            && &bytes[idx - 8..idx - 2] == b"\"id\":\""
            && let Ok(candidate) = std::str::from_utf8(&bytes[idx - 2..idx])
            && let Some(code) = valid_region(candidate)
        {
            return Some(code);
        }
    }
    None
}

async fn netflix_title_check(client: &Client, title_id: &str) -> Option<NetflixTitleResult> {
    let mut current_url = format!("https://www.netflix.com/title/{title_id}");
    let mut region = None;
    let mut response = client
        .get(&current_url)
        .header(USER_AGENT, BROWSER_UA)
        .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
        .send()
        .await
        .ok()?;
    for _ in 0..2 {
        if !response.status().is_redirection() {
            break;
        }
        let Some(loc) = location_header(&response) else {
            break;
        };
        if region.is_none() {
            region = netflix_region_from_url(&loc);
        }
        let Some(next_url) = same_origin_https_redirect(&current_url, &loc) else {
            break;
        };
        current_url = next_url;
        let Ok(followed) = client
            .get(&current_url)
            .header(USER_AGENT, BROWSER_UA)
            .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
            .send()
            .await
        else {
            break;
        };
        response = followed;
    }
    let code = response.status();
    let originating = response
        .headers()
        .get("x-originating-url")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    if region.is_none()
        && let Some(origin) = &originating
    {
        region = netflix_region_from_url(origin);
    }
    let mut available = false;
    let mut soft_unavailable = false;
    if code.is_success()
        && let Some(body) = read_capped(response, MAX_HTML_BODY).await
        && !body.is_empty()
    {
        if region.is_none() {
            region = extract_netflix_body_region(&body);
        }
        if region.is_none()
            && originating
                .as_deref()
                .is_some_and(|origin| origin.contains("/title/"))
        {
            region = Some("US".to_owned());
        }
        soft_unavailable = body.contains("Oh no!");
        available = !soft_unavailable;
    }
    Some(NetflixTitleResult {
        status: code,
        available,
        soft_unavailable,
        region,
    })
}

async fn probe_netflix(client: &Client) -> UnlockServiceStatus {
    let mut fallback_region = None;
    for licensed in ["81280792", "70143836"] {
        let Some(check) = netflix_title_check(client, licensed).await else {
            return service_status("netflix", "unknown", None);
        };
        if fallback_region.is_none() {
            fallback_region.clone_from(&check.region);
        }
        if check.available {
            let region = check.region.or(fallback_region);
            return service_status("netflix", "unlocked", region);
        }
        if is_blocked_status(check.status) {
            return service_status("netflix", "blocked", None);
        }
    }
    if let Some(orig) = netflix_title_check(client, "80018499").await {
        if orig.available {
            let region = orig.region.or(fallback_region);
            return service_status("netflix", "originals_only", region);
        }
        if is_blocked_status(orig.status)
            || orig.status == StatusCode::NOT_FOUND
            || orig.soft_unavailable
        {
            return service_status("netflix", "blocked", None);
        }
    }
    service_status("netflix", "unknown", None)
}

fn disney_region_from_location(base: &str, location: &str) -> Option<String> {
    let parsed = Url::parse(base).ok()?.join(location).ok()?;
    let segment = parsed.path().trim_start_matches('/').split('/').next()?;
    let (prefix, region) = segment.rsplit_once('-')?;
    let valid_lang = match prefix.split_once('-') {
        None => prefix.len() == 2 && prefix.bytes().all(|b| b.is_ascii_alphabetic()),
        Some((lang, script)) => {
            lang.len() == 2
                && lang.bytes().all(|b| b.is_ascii_alphabetic())
                && script.len() == 4
                && script.bytes().all(|b| b.is_ascii_alphabetic())
        }
    };
    if valid_lang {
        valid_region(region)
    } else {
        None
    }
}

#[derive(Debug, PartialEq, Eq)]
enum DisneyProbeResult {
    Unlocked(String),
    Blocked(Option<String>),
    Unknown(Option<String>),
}

fn is_safe_jwt_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 4096
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

fn parse_disney_graphql(body: &str) -> DisneyProbeResult {
    let Ok(json) = serde_json::from_str::<Value>(body) else {
        return DisneyProbeResult::Unknown(None);
    };
    let country = json
        .pointer("/extensions/sdk/session/location/countryCode")
        .and_then(Value::as_str)
        .and_then(valid_region);
    let supported = json
        .pointer("/extensions/sdk/session/inSupportedLocation")
        .and_then(Value::as_bool);
    if let Some(code) = country {
        if code == "JP" || supported == Some(true) {
            return DisneyProbeResult::Unlocked(code);
        }
        if supported == Some(false) {
            return DisneyProbeResult::Blocked(Some(code));
        }
        return DisneyProbeResult::Unknown(Some(code));
    }
    if supported == Some(false) {
        return DisneyProbeResult::Blocked(None);
    }
    DisneyProbeResult::Unknown(None)
}

async fn probe_disney_bamgrid(client: &Client) -> DisneyProbeResult {
    let bearer = format!("Bearer {DISNEY_PUBLIC_CLIENT_KEY}");

    let Ok(dev_resp) = client
        .post("https://disney.api.edge.bamgrid.com/devices")
        .header(USER_AGENT, BROWSER_UA)
        .header(AUTHORIZATION, &bearer)
        .header(CONTENT_TYPE, "application/json; charset=UTF-8")
        .body(DISNEY_DEVICE_BODY)
        .send()
        .await
    else {
        return DisneyProbeResult::Unknown(None);
    };
    if is_blocked_status(dev_resp.status()) {
        return DisneyProbeResult::Blocked(None);
    }
    let Some(dev_body) = read_capped(dev_resp, MAX_SMALL_BODY).await else {
        return DisneyProbeResult::Unknown(None);
    };
    let Ok(dev_json) = serde_json::from_str::<Value>(&dev_body) else {
        return DisneyProbeResult::Unknown(None);
    };
    let Some(assertion) = dev_json.get("assertion").and_then(Value::as_str) else {
        return DisneyProbeResult::Unknown(None);
    };
    if !is_safe_jwt_token(assertion) {
        return DisneyProbeResult::Unknown(None);
    }

    let token_form = format!(
        "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Atoken-exchange\
         &latitude=0&longitude=0&platform=browser&subject_token={assertion}\
         &subject_token_type=urn%3Abamtech%3Aparams%3Aoauth%3Atoken-type%3Adevice"
    );
    let Ok(tok_resp) = client
        .post("https://disney.api.edge.bamgrid.com/token")
        .header(USER_AGENT, BROWSER_UA)
        .header(AUTHORIZATION, &bearer)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(token_form)
        .send()
        .await
    else {
        return DisneyProbeResult::Unknown(None);
    };
    let tok_status = tok_resp.status();
    let tok_body = read_capped(tok_resp, MAX_SMALL_BODY)
        .await
        .unwrap_or_default();
    if is_blocked_status(tok_status)
        || tok_body.contains("forbidden-location")
        || tok_body.contains("403 ERROR")
    {
        return DisneyProbeResult::Blocked(None);
    }
    let Ok(tok_json) = serde_json::from_str::<Value>(&tok_body) else {
        return DisneyProbeResult::Unknown(None);
    };
    let Some(refresh_token) = tok_json.get("refresh_token").and_then(Value::as_str) else {
        return DisneyProbeResult::Unknown(None);
    };

    let gql_payload = serde_json::json!({
        "query": DISNEY_REFRESH_QUERY,
        "variables": { "input": { "refreshToken": refresh_token } },
        "operationName": "refreshToken",
    });
    let Ok(gql_resp) = client
        .post("https://disney.api.edge.bamgrid.com/graph/v1/device/graphql")
        .header(USER_AGENT, BROWSER_UA)
        .header(AUTHORIZATION, DISNEY_PUBLIC_CLIENT_KEY)
        .json(&gql_payload)
        .send()
        .await
    else {
        return DisneyProbeResult::Unknown(None);
    };
    if is_blocked_status(gql_resp.status()) {
        return DisneyProbeResult::Blocked(None);
    }
    let Some(gql_body) = read_capped(gql_resp, MAX_SMALL_BODY).await else {
        return DisneyProbeResult::Unknown(None);
    };
    parse_disney_graphql(&gql_body)
}

async fn probe_disney_landing(client: &Client) -> DisneyProbeResult {
    let base = "https://www.disneyplus.com/";
    let Ok(mut response) = client
        .get(base)
        .header(USER_AGENT, BROWSER_UA)
        .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
        .send()
        .await
    else {
        return DisneyProbeResult::Unknown(None);
    };
    let mut redirect_region = None;
    if response.status().is_redirection()
        && let Some(loc) = location_header(&response)
    {
        if loc.to_ascii_lowercase().contains("unavailable") {
            return DisneyProbeResult::Blocked(None);
        }
        redirect_region = disney_region_from_location(base, &loc);
        if let Some(next_url) = same_origin_https_redirect(base, &loc)
            && let Ok(next) = client
                .get(&next_url)
                .header(USER_AGENT, BROWSER_UA)
                .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
                .send()
                .await
        {
            response = next;
        }
    }
    let status = response.status();
    if is_blocked_status(status) {
        return DisneyProbeResult::Blocked(redirect_region);
    }
    let body = read_capped(response, MAX_HTML_BODY / 4)
        .await
        .unwrap_or_default();
    if body.contains("\"inSupportedLocation\":false")
        || body.contains("\"inSupportedLocation\": false")
        || body.contains("Sorry, Disney+ is not available in your region")
    {
        return DisneyProbeResult::Blocked(redirect_region);
    }
    let region = redirect_region.or_else(|| extract_json_region(&body, "countryCode"));
    if status.is_success()
        && let Some(code) = region
    {
        return DisneyProbeResult::Unlocked(code);
    }
    DisneyProbeResult::Unknown(region)
}

fn into_disney_region(result: DisneyProbeResult) -> Option<String> {
    match result {
        DisneyProbeResult::Unlocked(code) => Some(code),
        DisneyProbeResult::Blocked(region) | DisneyProbeResult::Unknown(region) => region,
    }
}

async fn probe_disney_plus(client: &Client) -> UnlockServiceStatus {
    let (api_result, landing_result) =
        tokio::join!(probe_disney_bamgrid(client), probe_disney_landing(client));
    match (api_result, landing_result) {
        (api, DisneyProbeResult::Blocked(region)) => {
            let merged = region.or_else(|| into_disney_region(api));
            service_status("disney_plus", "blocked", merged)
        }
        (DisneyProbeResult::Blocked(region), landing) => {
            let merged = region.or_else(|| into_disney_region(landing));
            service_status("disney_plus", "blocked", merged)
        }
        (DisneyProbeResult::Unlocked(code), _)
        | (DisneyProbeResult::Unknown(_), DisneyProbeResult::Unlocked(code)) => {
            service_status("disney_plus", "unlocked", Some(code))
        }
        (DisneyProbeResult::Unknown(api_region), DisneyProbeResult::Unknown(landing_region)) => {
            let merged = api_region.or(landing_region);
            service_status("disney_plus", "unknown", merged)
        }
    }
}

async fn probe_chatgpt(client: &Client) -> UnlockServiceStatus {
    let loc = match client
        .get("https://chatgpt.com/cdn-cgi/trace")
        .header(USER_AGENT, BROWSER_UA)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => read_capped(resp, 4096).await.and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("loc=").and_then(valid_region))
        }),
        _ => None,
    };
    if loc
        .as_deref()
        .is_some_and(|code| CHATGPT_UNSUPPORTED_REGIONS.contains(&code))
    {
        return service_status("chatgpt", "blocked", loc);
    }
    let base = "https://ios.chat.openai.com/public-api/mobile/server_status/v1";
    let Ok(mut response) = client
        .get(base)
        .header(USER_AGENT, BROWSER_UA)
        .header(ACCEPT, "application/json")
        .send()
        .await
    else {
        return service_status("chatgpt", "unknown", loc);
    };
    if response.status().is_redirection()
        && let Some(loc_hdr) = location_header(&response)
        && let Some(next_url) = allowlisted_https_redirect(base, &loc_hdr, "chatgpt.com")
        && let Ok(next) = client
            .get(&next_url)
            .header(USER_AGENT, BROWSER_UA)
            .header(ACCEPT, "application/json")
            .send()
            .await
    {
        response = next;
    }
    let status = response.status();
    let cf_challenge = response.headers().contains_key("cf-mitigated");
    let body = read_capped(response, MAX_SMALL_BODY)
        .await
        .unwrap_or_default();
    if body.contains("unsupported_country") {
        return service_status("chatgpt", "blocked", loc);
    }
    if cf_challenge {
        return service_status("chatgpt", "unknown", loc);
    }
    if is_blocked_status(status) {
        return service_status("chatgpt", "blocked", loc);
    }
    if status.is_success() {
        return service_status("chatgpt", "unlocked", loc);
    }
    service_status("chatgpt", "unknown", loc)
}

fn is_tiktok_blocked_redirect(base: &str, location: &str) -> bool {
    let Ok(parsed) = Url::parse(base).and_then(|u| u.join(location)) else {
        return false;
    };
    let Some(host) = parsed.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    if host != "www.tiktok.com" && host != "tiktok.com" {
        return false;
    }
    let path = parsed.path().to_ascii_lowercase();
    path == "/about" || path.starts_with("/about/") || path.contains("unavailable")
}

enum TikTokFetch {
    Blocked,
    Body(String),
    Unknown,
}

async fn fetch_tiktok_page(client: &Client, start_url: &str) -> TikTokFetch {
    let mut current_url = start_url.to_owned();
    let Ok(mut response) = client
        .get(&current_url)
        .header(USER_AGENT, BROWSER_UA)
        .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
        .send()
        .await
    else {
        return TikTokFetch::Unknown;
    };
    for _ in 0..2 {
        if !response.status().is_redirection() {
            break;
        }
        let Some(loc) = location_header(&response) else {
            break;
        };
        if is_tiktok_blocked_redirect(&current_url, &loc) {
            return TikTokFetch::Blocked;
        }
        let Some(next_url) = same_origin_https_redirect(&current_url, &loc) else {
            break;
        };
        current_url = next_url;
        let Ok(next) = client
            .get(&current_url)
            .header(USER_AGENT, BROWSER_UA)
            .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
            .send()
            .await
        else {
            return TikTokFetch::Unknown;
        };
        response = next;
    }
    if response.status().is_redirection() {
        if let Some(loc) = location_header(&response)
            && is_tiktok_blocked_redirect(&current_url, &loc)
        {
            return TikTokFetch::Blocked;
        }
        return TikTokFetch::Unknown;
    }
    if is_blocked_status(response.status()) {
        return TikTokFetch::Blocked;
    }
    let Some(body) = read_capped(response, MAX_HTML_BODY).await else {
        return TikTokFetch::Unknown;
    };
    TikTokFetch::Body(body)
}

fn should_probe_tiktok(family: u8) -> bool {
    family == 4
}

async fn probe_tiktok_for_family(client: &Client, family: u8) -> Option<UnlockServiceStatus> {
    if !should_probe_tiktok(family) {
        return None;
    }
    Some(probe_tiktok(client).await)
}

async fn probe_tiktok(client: &Client) -> UnlockServiceStatus {
    let mut body = match fetch_tiktok_page(client, "https://www.tiktok.com/").await {
        TikTokFetch::Blocked => return service_status("tiktok", "blocked", None),
        TikTokFetch::Body(text) => text,
        TikTokFetch::Unknown => return service_status("tiktok", "unknown", None),
    };
    let mut region = extract_json_region(&body, "region");
    if region.is_none() && body.contains("Please wait...") {
        drop(body);
        match fetch_tiktok_page(client, TIKTOK_EXPLORE_URL).await {
            TikTokFetch::Blocked => return service_status("tiktok", "blocked", None),
            TikTokFetch::Body(text) => {
                region = extract_json_region(&text, "region");
                body = text;
            }
            TikTokFetch::Unknown => return service_status("tiktok", "unknown", None),
        }
    }
    if matches!(region.as_deref(), Some("CN" | "HK" | "IN"))
        || body.contains("TikTok isn't available")
        || body.contains("not available in your")
    {
        return service_status("tiktok", "blocked", region);
    }
    if region.is_some() {
        return service_status("tiktok", "unlocked", region);
    }
    service_status("tiktok", "unknown", None)
}

fn service_status(id: &str, status: &str, region: Option<String>) -> UnlockServiceStatus {
    UnlockServiceStatus {
        id: id.to_owned(),
        status: status.to_owned(),
        region,
        access_type: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_regions_and_redirects_safely() {
        let base = "https://www.google.com/search?q=1";
        assert_eq!(
            region_from_google_location(base, "https://www.google.com.hk/?gws_rd=ssl"),
            Some("HK".to_owned())
        );
        assert!(is_google_cn_redirect(base, "https://www.google.cn/"));
        let hk_cn_redirect = concat!(
            "https://www.google.com.hk/url?sa=p&hl=zh-CN&pref=hkredirect",
            "&pval=yes&q=https://www.google.com.hk/search%3Fq%3D1"
        );
        assert!(is_google_cn_redirect(base, hk_cn_redirect));
        let hk_normal = "https://www.google.com.hk/?gws_rd=ssl";
        assert!(!is_google_cn_redirect(base, hk_normal));
        let spoof = "https://www.google.com/?continue=https://www.google.cn/";
        assert!(!is_google_cn_redirect(base, spoof));
        let sample = r#"{"region":null,"region":"","region":"JP"}"#;
        assert_eq!(extract_json_region(sample, "region"), Some("JP".to_owned()));
        let consent = "https://consent.youtube.com/m?continue=x&gl=DE#frag";
        assert_eq!(
            query_param_region(base, consent, "gl"),
            Some("DE".to_owned())
        );
        assert_eq!(
            disney_region_from_location("https://www.disneyplus.com/", "/zh-hant-hk"),
            Some("HK".to_owned())
        );
        assert_eq!(
            disney_region_from_location("https://www.disneyplus.com/", "/sign-in"),
            None
        );
        assert_eq!(
            same_origin_https_redirect("https://www.netflix.com/title/1", "//evil.example/x"),
            None
        );
        assert_eq!(
            netflix_region_from_url("https://www.netflix.com/hk-en/title/81280792"),
            Some("HK".to_owned())
        );
        let nf_body = r#"{"id":"HK","countryName":"Hong Kong"}"#;
        assert_eq!(extract_netflix_body_region(nf_body), Some("HK".to_owned()));
        let nf_req = concat!(
            r#"{"id":"ZZ","countryName":"Other","#,
            r#""requestCountry":{"id":"US","countryName":"United States"}}"#
        );
        assert_eq!(extract_netflix_body_region(nf_req), Some("US".to_owned()));
        let tt_base = "https://www.tiktok.com/";
        let tt_about = "https://www.tiktok.com/about?lang=en";
        assert!(is_tiktok_blocked_redirect(tt_base, tt_about));
        assert!(!is_tiktok_blocked_redirect(tt_base, TIKTOK_EXPLORE_URL));
        assert!(should_probe_tiktok(4));
        assert!(!should_probe_tiktok(6));
    }

    #[test]
    fn parses_reddit_and_disney_probe_responses() {
        assert!(is_reddit_waf_block(
            "<html>You've been blocked by network security.</html>"
        ));
        assert!(is_reddit_waf_block("Whoa there, pardner!"));
        assert!(!is_reddit_waf_block(
            r#"{"message":"Unauthorized","error":401}"#
        ));
        let reddit_html = concat!(
            r#"<div data-country="ZZ" obj.country="CA" country="US""#,
            r#" lang="en"></div>"#
        );
        assert_eq!(
            extract_html_attr_region(reddit_html, "country"),
            Some("US".to_owned())
        );
        let disney_ok = concat!(
            r#"{"extensions":{"sdk":{"session":{"inSupportedLocation":true,"#,
            r#""location":{"countryCode":"HK"}}}}}"#
        );
        assert_eq!(
            parse_disney_graphql(disney_ok),
            DisneyProbeResult::Unlocked("HK".to_owned())
        );
        let disney_blocked = concat!(
            r#"{"extensions":{"sdk":{"session":{"inSupportedLocation":false,"#,
            r#""location":{"countryCode":"CN"}}}}}"#
        );
        assert_eq!(
            parse_disney_graphql(disney_blocked),
            DisneyProbeResult::Blocked(Some("CN".to_owned()))
        );
        assert!(is_safe_jwt_token("eyJ0eXAi.eyJzdWI.sig_123-abc"));
        assert!(!is_safe_jwt_token("bad&param=1"));
    }
}
