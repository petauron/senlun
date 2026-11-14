//! Opt-in, admin-only IP enrichment. No Agent command channel or plugin runtime.
use crate::{
    ApiError, AppState,
    storage::{Storage, StorageError, format_timestamp},
};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::HeaderMap,
    routing::{get, post},
};
use rusqlite::{Connection, params};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    net::IpAddr,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const MAX_CACHE: usize = 256;
const MAX_BODY: usize = 256 * 1024;
const MAX_ENTRY: usize = 16 * 1024;
const DAY: u64 = 86_400;
const PROVIDER: &str = "https://ip.net.coffee";

pub(crate) struct IpInfo {
    client: reqwest::Client,
    inner: Mutex<Cache>,
}
#[derive(Default)]
struct Cache {
    entries: HashMap<String, Entry>,
    next_request: Option<Instant>,
    day: u64,
    calls: u32,
    minute: u64,
    minute_calls: u32,
}
#[derive(Clone)]
pub(crate) struct Entry {
    data: Value,
    updated: u64,
    ttl: u64,
}

impl IpInfo {
    pub(crate) fn new(entries: HashMap<String, Entry>) -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(20))
                .connect_timeout(Duration::from_secs(3))
                .no_proxy()
                .build()?,
            inner: Mutex::new(Cache {
                entries,
                ..Cache::default()
            }),
        })
    }

    async fn fetch(
        &self,
        state: &AppState,
        cache: &mut Cache,
        ip: IpAddr,
        latency: bool,
    ) -> Result<Value, ApiError> {
        let now = crate::current_time()? / 1000;
        let key = format!("{ip}:{latency}");
        let old = cache.entries.get(&key).cloned();
        if let Some(entry) = &old
            && !entry.stale(now)
        {
            return Ok(envelope(entry, now, "hit", None));
        }
        if cache.day != now / DAY {
            cache.day = now / DAY;
            cache.calls = 0;
        }
        if cache.minute != now / 60 {
            cache.minute = now / 60;
            cache.minute_calls = 0;
        }
        if cache.calls >= 200
            || cache.minute_calls >= 6
            || cache.next_request.is_some_and(|next| Instant::now() < next)
        {
            return stale_or_error(old.as_ref(), now, "IP information rate limit; retry later");
        }
        cache.calls += 1;
        cache.minute_calls += 1;
        // Set before I/O so cancellation cannot bypass the cooldown.
        cache.next_request = Some(Instant::now() + Duration::from_secs(10));
        let url = if latency {
            format!(
                "{PROVIDER}/api/ping/global?host={ip}&node=n02&node=n03&node=n04&node=n09&node=n11&node=n13"
            )
        } else {
            format!("{PROVIDER}/api/ip/lookup/{ip}")
        };
        let result = self.request(&url).await.and_then(|raw| {
            if latency {
                normalize_latency(&raw, ip)
            } else {
                let data = normalize_lookup(&raw, ip)?;
                if data["reputation"]["trust_score"].is_null()
                    && old.as_ref().is_some_and(|entry| {
                        entry.data["reputation"]["trust_score"].is_number()
                    })
                {
                    return Err(ApiError::unavailable("IP score unavailable"));
                }
                Ok(data)
            }
        });
        if let Ok(data) = result {
            let entry = Entry {
                data,
                updated: now,
                ttl: if latency { 3600 } else { DAY },
            };
            // A successful response is durable before it is visible in memory.
            let saved_key = key.clone();
            let saved_entry = entry.clone();
            state
                .database(move |storage| storage.save_ip_info_cache(&saved_key, &saved_entry))
                .await?;
            let result = envelope(&entry, now, "miss", None);
            cache.entries.insert(key, entry);
            if cache.entries.len() > MAX_CACHE
                && let Some(key) = cache
                    .entries
                    .iter()
                    .min_by(|(left_key, left), (right_key, right)| {
                        left.updated
                            .cmp(&right.updated)
                            .then_with(|| left_key.cmp(right_key))
                    })
                    .map(|(key, _)| key.clone())
            {
                cache.entries.remove(&key);
            }
            // A lookup and its following latency request are one logical operation.
            cache.next_request = None;
            Ok(result)
        } else {
            cache.next_request = Some(Instant::now() + Duration::from_secs(300));
            stale_or_error(old.as_ref(), now, "IP provider unavailable")
        }
    }

    async fn request(&self, url: &str) -> Result<Value, ApiError> {
        let mut response = self
            .client
            .get(url)
            .timeout(Duration::from_secs(if url.contains("/ping/") {
                20
            } else {
                6
            }))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|_| ApiError::unavailable("IP provider unavailable"))?;
        if response
            .content_length()
            .is_some_and(|size| size > MAX_BODY as u64)
        {
            return Err(ApiError::unavailable("IP response too large"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| ApiError::unavailable("IP response failed"))?
        {
            if bytes.len() + chunk.len() > MAX_BODY {
                return Err(ApiError::unavailable("IP response too large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| ApiError::unavailable("invalid IP response"))
    }
}

impl Entry {
    fn stale(&self, now: u64) -> bool {
        now < self.updated || now >= self.updated.saturating_add(self.ttl)
    }
}

pub(crate) fn migrate(db: &Connection) -> rusqlite::Result<()> {
    // Leave room for TTL and stale grace inside the RFC 3339 calendar range.
    db.execute_batch(
        "CREATE TABLE ip_info_cache (
            cache_key TEXT PRIMARY KEY CHECK(length(cache_key) BETWEEN 3 AND 51),
            data TEXT NOT NULL CHECK(json_valid(data) AND length(CAST(data AS BLOB)) <= 16384),
            updated_at INTEGER NOT NULL CHECK(updated_at > 0 AND updated_at <= 253402127999)
        ) WITHOUT ROWID;",
    )
}

impl Storage {
    pub(crate) fn ip_info_cache(&self) -> Result<HashMap<String, Entry>, StorageError> {
        let db = self.connection()?;
        let mut statement = db.prepare(
            "SELECT cache_key,data,updated_at FROM ip_info_cache
             ORDER BY updated_at DESC,cache_key DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([MAX_CACHE], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })?;
        let mut entries = HashMap::new();
        for row in rows {
            let (key, raw, updated) = row?;
            let (address, kind) = key
                .rsplit_once(':')
                .ok_or(StorageError::InvalidInput("invalid IP cache key"))?;
            let ip: IpAddr = address
                .parse()
                .map_err(|_| StorageError::InvalidInput("invalid IP cache address"))?;
            let latency = match kind {
                "false" => false,
                "true" => true,
                _ => return Err(StorageError::InvalidInput("invalid IP cache kind")),
            };
            let data: Value = serde_json::from_str(&raw)
                .map_err(|_| StorageError::InvalidInput("invalid IP cache data"))?;
            let family = if ip.is_ipv4() { 4 } else { 6 };
            if !public_ip(ip)
                || key != format!("{ip}:{latency}")
                || data["schema_version"] != 6
                || data["address"]["value"] != ip.to_string()
                || data["address"]["family"] != family
                || data["provider"]["id"] != "net-coffee"
            {
                return Err(StorageError::InvalidInput("invalid IP cache identity"));
            }
            entries.insert(
                key,
                Entry {
                    data,
                    updated,
                    ttl: if latency { 3600 } else { DAY },
                },
            );
        }
        Ok(entries)
    }

    fn save_ip_info_cache(&self, key: &str, entry: &Entry) -> Result<(), StorageError> {
        let raw = entry.data.to_string();
        if raw.len() > MAX_ENTRY {
            return Err(StorageError::InvalidInput("IP cache entry too large"));
        }
        let mut db = self.connection()?;
        let transaction = db.transaction()?;
        transaction.execute(
            "INSERT INTO ip_info_cache(cache_key,data,updated_at) VALUES(?1,?2,?3)
             ON CONFLICT(cache_key) DO UPDATE SET data=excluded.data,updated_at=excluded.updated_at",
            params![key, raw, entry.updated],
        )?;
        transaction.execute(
            "DELETE FROM ip_info_cache WHERE cache_key NOT IN
             (SELECT cache_key FROM ip_info_cache ORDER BY updated_at DESC,cache_key DESC LIMIT ?1)",
            [MAX_CACHE],
        )?;
        transaction.commit()?;
        Ok(())
    }
}

fn envelope(entry: &Entry, now: u64, cache: &str, warning: Option<&str>) -> Value {
    json!({"ok":true,"data":entry.data,"meta":{
        "cache":cache,"stale":entry.stale(now),"warning":warning,
        "updated_at":format_timestamp(entry.updated * 1000),
        "expires_at":format_timestamp((entry.updated + entry.ttl) * 1000),
        "stale_until":format_timestamp((entry.updated + entry.ttl + DAY) * 1000)}})
}
fn stale_or_error(old: Option<&Entry>, now: u64, warning: &str) -> Result<Value, ApiError> {
    if let Some(entry) = old.filter(|entry| now < entry.updated + entry.ttl + DAY) {
        return Ok(envelope(entry, now, "stale", Some(warning)));
    }
    Err(ApiError::unavailable(warning))
}

pub(crate) fn valid_address(raw: &str, ipv6: bool) -> bool {
    raw.is_empty()
        || raw
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_ipv6() == ipv6 && public_ip(ip))
}
pub(crate) fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(matches!(a, 0 | 10 | 127 | 224..=255)
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192
                    && (b == 168 || (b == 0 && matches!(c, 0 | 2)) || (b == 88 && c == 99)))
                || (a == 198 && (matches!(b, 18 | 19) || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            (0x2000..0x4000).contains(&s[0])
                && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
                && s[0] != 0x2002
                && !(s[0] == 0x3fff && s[1] <= 0xfff)
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct Input {
    uuid: String,
    ip: String,
    #[serde(default)]
    include_latency: bool,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/public/ip-info/v1/status", get(status))
        .route("/api/admin/ip-info/v1/status", get(status))
        .route("/api/public/ip-info/v1/lookup", get(lookup))
        .route("/api/public/ip-info/v1/latency", get(latency))
        .route("/api/admin/ip-info/v1/refresh", post(refresh))
}
async fn status(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let enabled = state.database(Storage::settings).await?.ip_info_enabled;
    Ok(Json(
        json!({"ok":true,"data":{"available":enabled,"version":"senlun-1","schema_version":6,
        "mainland_china_excluded":true,"capabilities":{"geo":enabled,"network":enabled,
        "native_classification":enabled,"global_latency":enabled,"reputation":enabled,"media_unlock":false,"ai_unlock":false}}}),
    ))
}
async fn validate(state: &AppState, input: &Input) -> Result<IpAddr, ApiError> {
    let ip = input
        .ip
        .parse::<IpAddr>()
        .map_err(|_| ApiError::bad_request("invalid IP"))?;
    if !public_ip(ip) || uuid::Uuid::parse_str(&input.uuid).is_err() {
        return Err(ApiError::bad_request("invalid public IP or node"));
    }
    let admin = state.database(Storage::admin_state).await?;
    if admin["settings"]["ip_info_enabled"] != true {
        return Err(ApiError::bad_request("IP information is disabled"));
    }
    let node = admin["nodes"]
        .as_array()
        .and_then(|nodes| {
            nodes
                .iter()
                .find(|node| node["id"] == input.uuid && node["disabled"] == false)
        })
        .ok_or_else(|| ApiError::bad_request("node unavailable"))?;
    if !["ipv4", "ipv6"].iter().any(|key| {
        node[key]
            .as_str()
            .and_then(|value| value.parse::<IpAddr>().ok())
            == Some(ip)
    }) {
        return Err(ApiError::bad_request("IP is not registered to this node"));
    }
    if matches!(
        node["region"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_uppercase()
            .as_str(),
        "CN" | "CHINA" | "中国" | "🇨🇳"
    ) {
        return Err(ApiError::bad_request("mainland China nodes are excluded"));
    }
    Ok(ip)
}
async fn run(
    state: &AppState,
    input: &Input,
    latency: bool,
    refresh: bool,
) -> Result<Value, ApiError> {
    // One bounded serialized provider lane coalesces normal duplicate requests via cache.
    let mut cache = state.ip_info.inner.lock().await;
    let ip = validate(state, input).await?;
    let mut base = state.ip_info.fetch(state, &mut cache, ip, false).await?;
    base["data"]["uuid"] = json!(input.uuid);
    if latency && base["data"]["excluded"] != true {
        let result = state.ip_info.fetch(state, &mut cache, ip, true).await;
        match result {
            Ok(mut profile) => {
                profile["data"]["uuid"] = json!(input.uuid);
                profile["data"]["classification"] = base["data"]["classification"].clone();
                profile["data"]["provider"]["classification_available"] =
                    json!(base["data"]["classification"]["type"] != "unknown");
                if !refresh {
                    return Ok(profile);
                }
                base["related"] = json!({"latency":profile});
            }
            Err(error) if refresh => {
                base["meta"]["latency_warning"] = json!(error.message);
            }
            Err(error) => return Err(error),
        }
    } else if latency && !refresh {
        return Err(ApiError::bad_request("mainland China IPs are excluded"));
    }
    Ok(base)
}
async fn lookup(
    State(state): State<AppState>,
    Query(input): Query<Input>,
) -> Result<Json<Value>, ApiError> {
    run(&state, &input, false, false).await.map(Json)
}
async fn latency(
    State(state): State<AppState>,
    Query(input): Query<Input>,
) -> Result<Json<Value>, ApiError> {
    run(&state, &input, true, false).await.map(Json)
}
async fn refresh(
    State(state): State<AppState>,
    Json(input): Json<Input>,
) -> Result<Json<Value>, ApiError> {
    run(&state, &input, input.include_latency, true)
        .await
        .map(Json)
}

pub(crate) async fn enrich_nodes(
    state: &AppState,
    headers: &HeaderMap,
    value: &mut Value,
) -> Result<(), ApiError> {
    if state.auth.authorize(headers, true, true).await.is_err() {
        return Ok(());
    }
    let admin = state.database(Storage::admin_state).await?;
    if admin["settings"]["ip_info_enabled"] != true {
        return Ok(());
    }
    let Some(nodes) = admin["nodes"].as_array() else {
        return Ok(());
    };
    enrich(value, nodes);
    let cache = state.ip_info.inner.lock().await;
    enrich_scores(value, &cache, crate::current_time()? / 1000);
    Ok(())
}

fn enrich(value: &mut Value, nodes: &[Value]) {
    if let Some(id) = value["uuid"].as_str() {
        if let Some(node) = nodes
            .iter()
            .find(|node| node["id"] == id && node["disabled"] == false)
        {
            value["ipv4"] = node["ipv4"].clone();
            value["ipv6"] = node["ipv6"].clone();
        }
    } else if let Some(items) = value.as_object_mut() {
        for item in items.values_mut() {
            enrich(item, nodes);
        }
    } else if let Some(items) = value.as_array_mut() {
        for item in items {
            enrich(item, nodes);
        }
    }
}
// Dashboard reads never call the provider. Cache keys include the actual address,
// so changing a registered IP cannot reuse another address's score.
fn enrich_scores(value: &mut Value, cache: &Cache, now: u64) {
    if value["uuid"].is_string() {
        let scores: Vec<Value> = [("ipv4", 4), ("ipv6", 6)]
            .into_iter()
            .filter_map(|(key, family)| {
                let ip = value[key].as_str()?.parse::<IpAddr>().ok()?;
                let entry = cache.entries.get(&format!("{ip}:false"));
                let score =
                    entry.and_then(|entry| entry.data["reputation"]["trust_score"].as_f64());
                Some(
                    json!({"family":family,"score":score,"provider":"Net.Coffee",
                    "updated_at_unix_ms":entry.map(|entry|entry.updated * 1000),
                    "stale":entry.is_some_and(|entry|entry.stale(now))}),
                )
            })
            .collect();
        value["ip_scores"] = json!(scores);
    } else if let Some(items) = value.as_object_mut() {
        for item in items.values_mut() {
            enrich_scores(item, cache, now);
        }
    } else if let Some(items) = value.as_array_mut() {
        for item in items {
            enrich_scores(item, cache, now);
        }
    }
}

fn string(raw: &Value, key: &str) -> Value {
    raw[key]
        .as_str()
        .filter(|s| s.len() <= 512)
        .map_or(Value::Null, |s| json!(s))
}
fn country(raw: &Value, key: &str) -> Value {
    raw[key]
        .as_str()
        .filter(|s| s.len() == 2 && s.bytes().all(|c| c.is_ascii_alphabetic()))
        .map_or(Value::Null, |s| json!(s.to_uppercase()))
}
fn normalize_lookup(raw: &Value, ip: IpAddr) -> Result<Value, ApiError> {
    let code = country(raw, "countryCode");
    if raw["ip"].as_str().and_then(|s| s.parse::<IpAddr>().ok()) != Some(ip)
        || raw["is_bogon"] == true
        || code.is_null()
    {
        return Err(ApiError::unavailable("invalid IP provider identity"));
    }
    let registered = country(raw, "registered_country_code");
    let verdict = raw["ai_verdict"]["label"].as_str().unwrap_or("");
    let (kind, label, source) = if verdict.contains("任播")
        || raw["public_service"]["service_type"]
            .as_str()
            .is_some_and(|v| v.eq_ignore_ascii_case("anycast"))
    {
        ("anycast", "任播 IP", "provider_verdict")
    } else if verdict.contains("广播") {
        ("broadcast", "广播 IP", "provider_verdict")
    } else if verdict.contains("原生") {
        ("native", "原生 IP", "provider_verdict")
    } else if !registered.is_null() {
        if code == registered {
            ("native", "原生 IP", "country_comparison")
        } else {
            ("broadcast", "广播 IP", "country_comparison")
        }
    } else {
        ("unknown", "未知", "unavailable")
    };
    // Net.Coffee trust_score is a provider score, not an abuse percentage.
    let trust_score = raw["trust_score"]
        .as_f64()
        .filter(|score| (0.0..=100.0).contains(score));
    let asn = raw["asn"].as_u64();
    let geo = raw["geo_sources"].as_array().and_then(|sources| {
        sources
            .iter()
            .find(|source| {
                source["src"] == raw["src"]
                    && source["lat"].is_number()
                    && source["lon"].is_number()
            })
            .or_else(|| {
                sources
                    .iter()
                    .find(|source| source["lat"].is_number() && source["lon"].is_number())
            })
    });
    let latitude = geo
        .and_then(|geo| geo["lat"].as_f64())
        .filter(|n| (-90.0..=90.0).contains(n));
    let longitude = geo
        .and_then(|geo| geo["lon"].as_f64())
        .filter(|n| (-180.0..=180.0).contains(n));
    Ok(
        json!({"schema_version":6,"excluded":code=="CN","excluded_reason":if code=="CN" {json!("mainland_china")} else {Value::Null},
        "address":{"value":ip.to_string(),"family":if ip.is_ipv4(){4}else{6}},
        "location":{"continent":null,"country":string(raw,"country"),"country_code":code,
            "registered_country":string(raw,"registered_country"),"registered_country_code":registered,
            "region":string(raw,"region"),"city":string(raw,"city"),"timezone":null,"latitude":latitude,"longitude":longitude},
        "network":{"asn":asn.map(|n| format!("AS{n}")),"asn_number":asn,"organization":string(raw,"asOrganization"),
            "operator":string(raw,"isp"),"network_type":string(raw,"asn_kind"),"route":string(raw,"cidr"),"rir":null,"domain":string(raw,"rdns"),"datacenter":null},
        "classification":{"type":kind,"label":label,"geolocated_country_code":code,"registered_country_code":registered,
            "confidence":raw["ai_verdict"]["confidence"].as_f64().filter(|n| (0.0..=100.0).contains(n)),"source":source},
        "reputation":{"available":trust_score.is_some(),"trust_score":trust_score,"purity_score":null,"risk_score":null,"pollution_score":null,"risk_level":null,"pollution_level":null,
            "signals":{"proxy":null,"tor":null,"vpn":null,"datacenter":null,"abuser":null,"crawler":null},"method":{"id":"net_coffee_trust","status":if trust_score.is_some(){"available"}else{"unavailable"},"higher_is_better":true}},
        "capabilities":{"media_unlock":false,"ai_unlock":false},
        "provider":{"id":"net-coffee","name":"Net.Coffee","homepage":PROVIDER,"base_source":"net-coffee","security_data_available":false,"trust_score_available":trust_score.is_some()}}),
    )
}
fn normalize_latency(raw: &Value, ip: IpAddr) -> Result<Value, ApiError> {
    if !raw["results"].is_object() {
        return Err(ApiError::unavailable("invalid latency response"));
    }
    let mut available = 0;
    let mut timeouts = 0;
    let nodes: Vec<Value> = [
        ("n02", "HK", "香港"),
        ("n03", "JP", "东京"),
        ("n04", "SG", "新加坡"),
        ("n09", "US", "洛杉矶"),
        ("n11", "CA", "温哥华"),
        ("n13", "DE", "法兰克福"),
    ]
    .into_iter()
    .map(|(id, code, city)| {
        let ms = raw["results"][id]
            .as_f64()
            .filter(|n| (0.0..=60000.0).contains(n));
        let status = if ms.is_some() {
            available += 1;
            "ok"
        } else if raw["timeouts"]
            .as_array()
            .is_some_and(|a| a.contains(&json!(id)))
        {
            timeouts += 1;
            "timeout"
        } else {
            "unavailable"
        };
        json!({"id":id,"name":city,"city":city,"country_code":code,"latency_ms":ms,"status":status})
    })
    .collect();
    Ok(
        json!({"schema_version":6,"address":{"value":ip.to_string(),"family":if ip.is_ipv4(){4}else{6}},
        "latency":{"nodes":nodes,"available_count":available,"timeout_count":timeouts,"provider_cached":raw["cached"]==true},
        "provider":{"id":"net-coffee","name":"Net.Coffee","homepage":PROVIDER,"latency_available":available>0,"classification_available":false}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cache_hits_do_not_consume_quota_and_expired_results_survive_limits() {
        let directory = tempfile::tempdir().unwrap();
        let state = AppState::open(&crate::ServiceConfig::with_database(
            directory.path().join("senlun.db"),
        ))
        .unwrap();
        let service = &state.ip_info;
        let now = crate::current_time().unwrap() / 1000;
        let mut cache = Cache {
            day: now / DAY,
            calls: 200,
            ..Cache::default()
        };
        let ip = "1.1.1.1".parse().unwrap();
        cache.entries.insert(
            format!("{ip}:false"),
            Entry {
                data: json!({"test":true}),
                updated: now,
                ttl: 60,
            },
        );
        assert_eq!(
            service.fetch(&state, &mut cache, ip, false).await.unwrap()["meta"]["cache"],
            "hit"
        );
        cache.entries.get_mut(&format!("{ip}:false")).unwrap().updated = now - 61;
        assert_eq!(
            service.fetch(&state, &mut cache, ip, false).await.unwrap()["meta"]["cache"],
            "stale"
        );
        assert_eq!(cache.calls, 200);
        assert!(service.fetch(&state, &mut cache, ip, true).await.is_err());
    }
    #[test]
    fn blocks_non_public_and_mapped_addresses() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "169.254.169.254",
            "100.64.0.1",
            "192.0.2.1",
            "198.18.0.1",
            "203.0.113.1",
            "::1",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
            "2001:db8::1",
            "2002::1",
            "3fff::1",
            "fe80::1",
            "fc00::1",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(public_ip(ip.parse().unwrap()));
        }
        assert!(!valid_address("8.8.8.8", true));
    }
    #[test]
    fn validates_provider_identity_and_classification() {
        let ip = "8.8.8.8".parse().unwrap();
        assert!(normalize_lookup(&json!({"ip":"1.1.1.1","countryCode":"US"}), ip).is_err());
        assert!(normalize_lookup(&json!({"ip":"8.8.8.8"}), ip).is_err());
        let result = normalize_lookup(
            &json!({"ip":"8.8.8.8","countryCode":"US","registered_country_code":"US"}),
            ip,
        )
        .unwrap();
        assert_eq!(result["classification"]["type"], "native");
        assert_eq!(result["reputation"]["available"], false);
        assert_eq!(
            normalize_lookup(&json!({"ip":"8.8.8.8","countryCode":"CN"}), ip).unwrap()["excluded"],
            true
        );
    }
    #[test]
    fn latency_does_not_invent_measurements() {
        let result = normalize_latency(
            &json!({"results":{"n02":25,"n03":-1},"timeouts":["n04"]}),
            "8.8.8.8".parse().unwrap(),
        )
        .unwrap();
        assert_eq!(result["latency"]["available_count"], 1);
        assert_eq!(result["latency"]["timeout_count"], 1);
        assert!(result["latency"]["nodes"][1]["latency_ms"].is_null());
    }
    #[test]
    fn stale_cache_is_bounded() {
        let entry = Entry {
            data: json!({}),
            updated: 100,
            ttl: 60,
        };
        assert_eq!(
            stale_or_error(Some(&entry), 161, "offline").unwrap()["meta"]["stale"],
            true
        );
        assert!(stale_or_error(Some(&entry), 160 + DAY, "offline").is_err());
    }
}
