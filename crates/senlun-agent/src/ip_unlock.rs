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
                    let task: Option<IpUnlockTask> =
                        serde_json::from_slice(&bytes).ok().flatten();
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
        .header(USER_AGENT, concat!("senlun-agent/", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .map_err(|_| "collection_failed")?;
    if !response.status().is_success() {
        return Err("collection_failed");
    }
    let body = read_capped(response, 128).await.ok_or("collection_failed")?;
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
    let start = body.find(&pattern)?;
    let rest = body[start + pattern.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    valid_region(&rest[..end])
}

fn query_param_region(url: &str, param: &str) -> Option<String> {
    let (_, query) = url.split_once('?')?;
    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=')
            && key == param
        {
            return valid_region(value);
        }
    }
    None
}

fn region_from_google_location(location: &str) -> Option<String> {
    let lower = location.to_ascii_lowercase();
    for (suffix, code) in [
        (".google.cn", "CN"),
        (".google.com.hk", "HK"),
        (".google.co.jp", "JP"),
        (".google.com.sg", "SG"),
        (".google.com.tw", "TW"),
        (".google.co.kr", "KR"),
        (".google.co.uk", "GB"),
        (".google.de", "DE"),
        (".google.fr", "FR"),
        (".google.ca", "CA"),
        (".google.com.au", "AU"),
        (".google.nl", "NL"),
    ] {
        if lower.contains(suffix) {
            return Some(code.to_owned());
        }
    }
    None
}

async fn probe_google_and_youtube(client: &Client) -> (GoogleUnlockStatus, UnlockServiceStatus) {
    let (yt_region, yt_cn, yt_status) = probe_youtube(client).await;
    let (g_cn, g_captcha, g_region, g_ok) = probe_google_home(client).await;

    let google = if yt_cn || g_cn {
        GoogleUnlockStatus {
            status: "china".to_owned(),
            region: Some("CN".to_owned()),
        }
    } else if g_captcha {
        GoogleUnlockStatus {
            status: "captcha".to_owned(),
            region: yt_region.clone().or(g_region),
        }
    } else if yt_region.is_some() || g_region.is_some() || g_ok {
        GoogleUnlockStatus {
            status: "normal".to_owned(),
            region: yt_region.clone().or(g_region),
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
    let Ok(mut response) = client
        .get("https://www.youtube.com/premium")
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
        let lower_loc = loc.to_ascii_lowercase();
        if lower_loc.contains("google.cn") {
            return (Some("CN".to_owned()), true, "china");
        }
        if lower_loc.starts_with("https://consent.youtube.com/")
            && let Some(region) = query_param_region(&loc, "gl")
        {
            if region == "CN" {
                return (Some("CN".to_owned()), true, "china");
            }
            return (Some(region), false, "unlocked");
        }
        if loc.starts_with("https://www.youtube.com/")
            && let Ok(next) = client
                .get(&loc)
                .header(USER_AGENT, BROWSER_UA)
                .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
                .send()
                .await
        {
            response = next;
        }
    }
    let status = response.status();
    let body = read_capped(response, MAX_HTML_BODY).await.unwrap_or_default();
    let region = extract_json_region(&body, "INNERTUBE_CONTEXT_GL")
        .or_else(|| extract_json_region(&body, "countryCode"));
    let is_cn = region.as_deref() == Some("CN") || body.contains("www.google.cn");
    if is_cn {
        return (Some("CN".to_owned()), true, "china");
    }
    if body.contains("YouTube Premium is not available in your country") {
        return (region, false, "blocked");
    }
    if region.is_some() || status.is_success() {
        return (region, false, "unlocked");
    }
    if is_blocked_status(status) {
        return (region, false, "blocked");
    }
    (region, false, "unknown")
}

async fn probe_google_home(client: &Client) -> (bool, bool, Option<String>, bool) {
    let Ok(response) = client
        .get("https://www.google.com/")
        .header(USER_AGENT, BROWSER_UA)
        .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
        .send()
        .await
    else {
        return (false, false, None, false);
    };
    let status = response.status();
    let loc = location_header(&response).unwrap_or_default();
    let lower_loc = loc.to_ascii_lowercase();
    let is_cn = lower_loc.contains("://www.google.cn")
        || lower_loc.contains("://google.cn")
        || lower_loc.contains(".google.cn/");
    let is_captcha = status == StatusCode::TOO_MANY_REQUESTS
        || lower_loc.contains("/sorry/")
        || lower_loc.contains("sorry/index");
    let region = region_from_google_location(&loc);
    let ok = status.is_success() || (status.is_redirection() && !is_cn && !is_captcha);
    (is_cn, is_captcha, region, ok)
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
    let api_body = read_capped(response, MAX_SMALL_BODY).await.unwrap_or_default();
    if api_code == StatusCode::FORBIDDEN || is_reddit_waf_block(&api_body) {
        return service_status("reddit", "blocked", None);
    }
    let api_reachable = api_code == StatusCode::UNAUTHORIZED || api_code.is_success();
    if !api_reachable {
        return service_status("reddit", "unknown", None);
    }

    // Layer 2: verify whether anonymous web browsing is also open or app/login-only.
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
        let web_ok = web_code.is_success() || web_code.is_redirection();
        if web_ok && !is_reddit_waf_block(&web_body) {
            return service_status("reddit", "unlocked", None);
        }
    }
    service_status("reddit", "app_only", None)
}

fn netflix_region_from_url(location: &str) -> Option<String> {
    let path = location
        .strip_prefix("https://www.netflix.com/")
        .or_else(|| location.strip_prefix('/'))?;
    let segment = path.split('/').next()?;
    let candidate = segment.split('-').next()?;
    valid_region(candidate)
}

async fn netflix_title_check(
    client: &Client,
    title_id: &str,
) -> Option<(StatusCode, Option<String>)> {
    let url = format!("https://www.netflix.com/title/{title_id}");
    let mut response = client
        .get(&url)
        .header(USER_AGENT, BROWSER_UA)
        .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
        .send()
        .await
        .ok()?;
    let mut region = None;
    if response.status().is_redirection()
        && let Some(loc) = location_header(&response)
    {
        region = netflix_region_from_url(&loc);
        let next_url = if loc.starts_with("https://www.netflix.com/") {
            Some(loc)
        } else if loc.starts_with('/') {
            Some(format!("https://www.netflix.com{loc}"))
        } else {
            None
        };
        if let Some(target) = next_url
            && let Ok(followed) = client
                .get(&target)
                .header(USER_AGENT, BROWSER_UA)
                .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
                .send()
                .await
        {
            response = followed;
        }
    }
    let code = response.status();
    if region.is_none()
        && let Some(origin) = response
            .headers()
            .get("x-originating-url")
            .and_then(|value| value.to_str().ok())
    {
        region = netflix_region_from_url(origin);
    }
    if code.is_success() && region.is_none() {
        region = Some("US".to_owned());
    }
    Some((code, region))
}

async fn probe_netflix(client: &Client) -> UnlockServiceStatus {
    let Some((status, region)) = netflix_title_check(client, "81280792").await else {
        return service_status("netflix", "unknown", None);
    };
    if status.is_success() {
        return service_status("netflix", "unlocked", region);
    }
    if status == StatusCode::FORBIDDEN {
        return service_status("netflix", "blocked", None);
    }
    if status == StatusCode::NOT_FOUND
        && let Some((orig_status, orig_region)) = netflix_title_check(client, "80018499").await
    {
        if orig_status.is_success() {
            return service_status("netflix", "originals_only", orig_region);
        }
        if orig_status == StatusCode::FORBIDDEN {
            return service_status("netflix", "blocked", None);
        }
    }
    service_status("netflix", "unknown", None)
}

fn disney_region_from_location(location: &str) -> Option<String> {
    let path = location
        .strip_prefix("https://www.disneyplus.com/")
        .or_else(|| location.strip_prefix('/'))?;
    let segment = path.split(['/', '?']).next()?;
    let (_, region) = segment.rsplit_once('-')?;
    valid_region(region)
}

async fn probe_disney_plus(client: &Client) -> UnlockServiceStatus {
    let Ok(response) = client
        .get("https://www.disneyplus.com/")
        .header(USER_AGENT, BROWSER_UA)
        .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
        .send()
        .await
    else {
        return service_status("disney_plus", "unknown", None);
    };
    let status = response.status();
    if is_blocked_status(status) {
        return service_status("disney_plus", "blocked", None);
    }
    if status.is_redirection()
        && let Some(loc) = location_header(&response)
    {
        if loc.to_ascii_lowercase().contains("unavailable") {
            return service_status("disney_plus", "blocked", None);
        }
        if let Some(region) = disney_region_from_location(&loc) {
            return service_status("disney_plus", "unlocked", Some(region));
        }
    }
    let body = read_capped(response, MAX_HTML_BODY / 2).await.unwrap_or_default();
    if body.contains("\"inSupportedLocation\":false") {
        return service_status("disney_plus", "blocked", None);
    }
    let region = extract_json_region(&body, "countryCode");
    if status.is_success() || region.is_some() {
        return service_status("disney_plus", "unlocked", region);
    }
    service_status("disney_plus", "unknown", None)
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
    if loc.as_deref().is_some_and(|code| CHATGPT_UNSUPPORTED_REGIONS.contains(&code)) {
        return service_status("chatgpt", "blocked", loc);
    }
    let Ok(response) = client
        .get("https://ios.chat.openai.com/public-api/mobile/server_status/v1")
        .header(USER_AGENT, BROWSER_UA)
        .header(ACCEPT, "application/json")
        .send()
        .await
    else {
        return service_status("chatgpt", "unknown", loc);
    };
    let status = response.status();
    let body = read_capped(response, MAX_SMALL_BODY).await.unwrap_or_default();
    if is_blocked_status(status) || body.contains("unsupported_country") {
        return service_status("chatgpt", "blocked", loc);
    }
    if status.is_success() {
        return service_status("chatgpt", "unlocked", loc);
    }
    service_status("chatgpt", "unknown", loc)
}

async fn probe_tiktok(client: &Client) -> UnlockServiceStatus {
    let Ok(mut response) = client
        .get("https://www.tiktok.com/")
        .header(USER_AGENT, BROWSER_UA)
        .header(ACCEPT_LANGUAGE, ACCEPT_LANG)
        .send()
        .await
    else {
        return service_status("tiktok", "unknown", None);
    };
    if response.status().is_redirection()
        && let Some(loc) = location_header(&response)
    {
        let next_url = if loc.starts_with("https://www.tiktok.com/") {
            Some(loc)
        } else if loc.starts_with('/') {
            Some(format!("https://www.tiktok.com{loc}"))
        } else {
            None
        };
        if let Some(target) = next_url
            && let Ok(next) = client
                .get(&target)
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
        return service_status("tiktok", "blocked", None);
    }
    let body = read_capped(response, MAX_HTML_BODY / 2).await.unwrap_or_default();
    let region = extract_json_region(&body, "region");
    if matches!(region.as_deref(), Some("CN" | "HK"))
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
    fn extracts_region_and_distinguishes_hk_from_cn() {
        assert_eq!(
            region_from_google_location("https://www.google.com.hk/?gws_rd=ssl"),
            Some("HK".to_owned())
        );
        assert_eq!(
            region_from_google_location("https://www.google.cn/"),
            Some("CN".to_owned())
        );
        assert_eq!(
            extract_json_region(r#"{"INNERTUBE_CONTEXT_GL": "HK"}"#, "INNERTUBE_CONTEXT_GL"),
            Some("HK".to_owned())
        );
        assert_eq!(
            query_param_region(
                "https://consent.youtube.com/m?continue=https://www.youtube.com/premium&gl=DE",
                "gl"
            ),
            Some("DE".to_owned())
        );
        assert_eq!(
            disney_region_from_location("https://www.disneyplus.com/zh-hant-hk"),
            Some("HK".to_owned())
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
        assert!(!is_reddit_waf_block(r#"{"message":"Unauthorized","error":401}"#));
    }
}
