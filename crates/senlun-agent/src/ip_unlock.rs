//! One fixed, opt-in, one-shot IP streaming/AI unlock and Google region check.
//! Existing Agent authentication stays on the Service client; target probes use
//! an isolated, credential-free HTTPS client with bounded reads and timeouts.
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::{
    Client, StatusCode, Url,
    header::{ACCEPT, ACCEPT_LANGUAGE, LOCATION, USER_AGENT},
};
use senlun_protocol::{
    GoogleUnlockStatus, IP_UNLOCK_SCHEMA_VERSION, IpUnlockClaim, IpUnlockOutcome, IpUnlockReport,
    IpUnlockTask, UnlockServiceStatus,
};
use tokio::task::JoinHandle;
use uuid::Uuid;

const POLL: Duration = Duration::from_secs(15);
const MAX_CONTROL_RESPONSE: usize = 16 * 1024;
const MAX_SMALL_BODY: usize = 32 * 1024;
const MAX_HTML_BODY: usize = 512 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(35);

const BROWSER_UA: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 ",
    "(KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
);
const REDDIT_API_UA: &str = "linux:senlun-unlock:v1 (by /u/senlun)";
const ACCEPT_LANG: &str = "en-US,en;q=0.9";

const CHATGPT_UNSUPPORTED_REGIONS: &[&str] = &[
    "AF", "BY", "CN", "CU", "HK", "IR", "KP", "MO", "RU", "SY", "VE",
];

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(u64::MAX, |d| {
            u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
        })
}

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
    let mut pending: Option<IpUnlockOutcome> = None;
    let mut deadline = 0;
    loop {
        if let Some(outcome) = &pending {
            if now_ms() >= deadline {
                pending = None;
            } else {
                match client
                    .post(result_url.clone())
                    .bearer_auth(&token)
                    .json(outcome)
                    .send()
                    .await
                {
                    Ok(response) if response.status() == StatusCode::UNAUTHORIZED => return,
                    Ok(response)
                        if response.status().is_success()
                            || response.status() == StatusCode::CONFLICT
                            || response.status() == StatusCode::BAD_REQUEST =>
                    {
                        pending = None;
                    }
                    _ => {}
                }
            }
        }
        if pending.is_none() {
            let claim = IpUnlockClaim {
                schema_version: IP_UNLOCK_SCHEMA_VERSION,
                agent_version: env!("CARGO_PKG_VERSION").to_owned(),
                available: true,
            };
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
                        let now = now_ms();
                        if task.expires_at_unix_ms > now
                            && task.expires_at_unix_ms <= now.saturating_add(180_000)
                            && Uuid::parse_str(&task.id).is_ok()
                        {
                            deadline = task.expires_at_unix_ms;
                            let result = collect(&task).await;
                            pending = Some(IpUnlockOutcome {
                                id: task.id,
                                report: result.as_ref().ok().cloned(),
                                error: result.err().map(str::to_owned),
                            });
                            continue;
                        }
                    }
                }
            }
        }
        tokio::time::sleep(POLL).await;
    }
}

fn target_client(family: u8) -> Result<Client, &'static str> {
    let bind = match family {
        4 => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        6 => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        _ => return Err("invalid_task"),
    };
    Client::builder()
        .https_only(true)
        .no_proxy()
        .local_address(bind)
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .pool_max_idle_per_host(0)
        .build()
        .map_err(|_| "unavailable")
}

async fn collect(task: &IpUnlockTask) -> Result<IpUnlockReport, &'static str> {
    let expected: IpAddr = task.address.parse().map_err(|_| "invalid_task")?;
    if !matches!(
        (task.family, expected),
        (4, IpAddr::V4(_)) | (6, IpAddr::V6(_))
    ) {
        return Err("invalid_task");
    }
    let client = target_client(task.family)?;
    tokio::time::timeout(TOTAL_TIMEOUT, async {
        verify_egress(&client, task.family, expected).await?;
        let ((google, youtube), netflix, disney_plus, chatgpt, reddit, tiktok) = tokio::join!(
            probe_google_and_youtube(&client),
            probe_netflix(&client),
            probe_disney_plus(&client),
            probe_chatgpt(&client),
            probe_reddit(&client),
            probe_tiktok(&client),
        );
        Ok(IpUnlockReport {
            family: task.family,
            address: expected.to_string(),
            checked_at_unix_ms: now_ms(),
            google,
            services: vec![youtube, netflix, disney_plus, chatgpt, reddit, tiktok],
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
    Some(String::from_utf8_lossy(&bytes).into_owned())
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

fn query_param_region(base: &str, location: &str, param: &str) -> Option<String> {
    let parsed = Url::parse(base).ok()?.join(location).ok()?;
    parsed
        .query_pairs()
        .find_map(|(key, value)| (key == param).then(|| valid_region(&value)).flatten())
}

fn is_google_cn_url(base: &str, location: &str) -> bool {
    Url::parse(base)
        .ok()
        .and_then(|u| u.join(location).ok())
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|host| host == "google.cn" || host.ends_with(".google.cn"))
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
        if is_google_cn_url(base, &loc) {
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
    let region = extract_json_region(&body, "INNERTUBE_CONTEXT_GL")
        .or_else(|| extract_json_region(&body, "countryCode"));
    let is_cn = region.as_deref() == Some("CN") || body.contains("www.google.cn");
    if is_cn {
        return (Some("CN".to_owned()), true, "china");
    }
    if body.contains("YouTube Premium is not available in your country") {
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
    let loc_cn = is_google_cn_url(base, &loc) || consent_region.as_deref() == Some("CN");
    let lower_loc = loc.to_ascii_lowercase();
    let is_captcha = status == StatusCode::TOO_MANY_REQUESTS
        || lower_loc.contains("/sorry/")
        || lower_loc.contains("sorry/index");
    let region = consent_region.or_else(|| region_from_google_location(base, &loc));
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

async fn probe_reddit(client: &Client) -> UnlockServiceStatus {
    // Layer 1: check oauth API with a compliant descriptive UA to avoid Fastly's
    // anonymous HTML WAF false positives on datacenter IPs.
    let api_response = client
        .get("https://oauth.reddit.com/api/v1/me")
        .header(USER_AGENT, REDDIT_API_UA)
        .header(ACCEPT, "application/json")
        .send()
        .await;
    let Ok(response) = api_response else {
        return service_status("reddit", "unknown", None);
    };
    let api_code = response.status();
    let api_body = read_capped(response, MAX_SMALL_BODY)
        .await
        .unwrap_or_default();
    if api_code == StatusCode::FORBIDDEN || is_reddit_waf_block(&api_body) {
        return service_status("reddit", "blocked", None);
    }
    let api_reachable = api_code == StatusCode::UNAUTHORIZED || api_code.is_success();
    if !api_reachable {
        return service_status("reddit", "unknown", None);
    }

    // Layer 2: verify whether anonymous web browsing returns 2xx without WAF block.
    if let Ok(web) = client
        .get("https://www.reddit.com/")
        .header(USER_AGENT, BROWSER_UA)
        .header(ACCEPT, "text/html")
        .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
        .send()
        .await
    {
        let web_code = web.status();
        let web_body = read_capped(web, MAX_SMALL_BODY).await.unwrap_or_default();
        if web_code.is_success() && !is_reddit_waf_block(&web_body) {
            return service_status("reddit", "unlocked", None);
        }
    }
    service_status("reddit", "app_only", None)
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

async fn netflix_title_check(
    client: &Client,
    title_id: &str,
) -> Option<(StatusCode, Option<String>)> {
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
    if code.is_success()
        && region.is_none()
        && originating
            .as_deref()
            .is_some_and(|origin| origin.contains("/title/"))
    {
        region = Some("US".to_owned());
    }
    Some((code, region))
}

async fn probe_netflix(client: &Client) -> UnlockServiceStatus {
    for licensed in ["81280792", "70143836"] {
        let Some((status, region)) = netflix_title_check(client, licensed).await else {
            return service_status("netflix", "unknown", None);
        };
        if status.is_success() {
            return service_status("netflix", "unlocked", region);
        }
        if status == StatusCode::FORBIDDEN {
            return service_status("netflix", "blocked", None);
        }
    }
    if let Some((orig_status, orig_region)) = netflix_title_check(client, "80018499").await {
        if orig_status.is_success() {
            return service_status("netflix", "originals_only", orig_region);
        }
        if matches!(orig_status, StatusCode::FORBIDDEN | StatusCode::NOT_FOUND) {
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

async fn probe_disney_plus(client: &Client) -> UnlockServiceStatus {
    let base = "https://www.disneyplus.com/";
    let Ok(mut response) = client
        .get(base)
        .header(USER_AGENT, BROWSER_UA)
        .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
        .send()
        .await
    else {
        return service_status("disney_plus", "unknown", None);
    };
    let mut redirect_region = None;
    if response.status().is_redirection()
        && let Some(loc) = location_header(&response)
    {
        if loc.to_ascii_lowercase().contains("unavailable") {
            return service_status("disney_plus", "blocked", None);
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
        return service_status("disney_plus", "blocked", redirect_region);
    }
    let body = read_capped(response, MAX_HTML_BODY / 2)
        .await
        .unwrap_or_default();
    if body.contains("\"inSupportedLocation\":false")
        || body.contains("\"inSupportedLocation\": false")
        || body.contains("Sorry, Disney+ is not available in your region")
    {
        return service_status("disney_plus", "blocked", redirect_region);
    }
    let region = redirect_region.or_else(|| extract_json_region(&body, "countryCode"));
    if status.is_success() && region.is_some() {
        return service_status("disney_plus", "unlocked", region);
    }
    service_status("disney_plus", "unknown", region)
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

async fn probe_tiktok(client: &Client) -> UnlockServiceStatus {
    let base = "https://www.tiktok.com/";
    let Ok(mut response) = client
        .get(base)
        .header(USER_AGENT, BROWSER_UA)
        .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
        .send()
        .await
    else {
        return service_status("tiktok", "unknown", None);
    };
    if response.status().is_redirection()
        && let Some(loc) = location_header(&response)
        && let Some(next_url) = same_origin_https_redirect(base, &loc)
        && let Ok(next) = client
            .get(&next_url)
            .header(USER_AGENT, BROWSER_UA)
            .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
            .send()
            .await
    {
        response = next;
    }
    let status = response.status();
    if is_blocked_status(status) {
        return service_status("tiktok", "blocked", None);
    }
    let body = read_capped(response, MAX_HTML_BODY)
        .await
        .unwrap_or_default();
    let region = extract_json_region(&body, "region");
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
        assert!(is_google_cn_url(base, "https://www.google.cn/"));
        assert!(!is_google_cn_url(
            base,
            "https://www.google.com/?continue=https://www.google.cn/"
        ));
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
    }

    #[test]
    fn detects_reddit_fastly_waf_block_pages() {
        assert!(is_reddit_waf_block(
            "<html>You've been blocked by network security.</html>"
        ));
        assert!(is_reddit_waf_block("Whoa there, pardner!"));
        assert!(!is_reddit_waf_block(
            r#"{"message":"Unauthorized","error":401}"#
        ));
    }
}
