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
    sync::RwLock,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const MAX_CACHE: u16 = 256;
const MAX_BODY: usize = 256 * 1024;
const MAX_ENTRY: usize = 16 * 1024;
const DAY: u64 = 86_400;
const PROVIDER: &str = "https://ip.net.coffee";

pub(crate) struct IpInfo {
    client: reqwest::Client,
    entries: RwLock<HashMap<String, Entry>>,
    provider: Mutex<ProviderState>,
}
#[derive(Default)]
struct ProviderState {
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
            entries: RwLock::new(entries),
            provider: Mutex::new(ProviderState::default()),
        })
    }

    fn cached_entry(&self, key: &str) -> Result<Option<Entry>, ApiError> {
        let entries = self
            .entries
            .read()
            .map_err(|_| ApiError::unavailable("IP cache unavailable"))?;
        Ok(entries.get(key).cloned())
    }

    fn store_entry(&self, key: String, entry: Entry) -> Result<(), ApiError> {
        let mut entries = self
            .entries
            .write()
            .map_err(|_| ApiError::unavailable("IP cache unavailable"))?;
        entries.insert(key, entry);
        if entries.len() > usize::from(MAX_CACHE)
            && let Some(oldest) = entries
                .iter()
                .min_by(|(left_key, left), (right_key, right)| {
                    left.updated
                        .cmp(&right.updated)
                        .then_with(|| left_key.cmp(right_key))
                })
                .map(|(key, _)| key.clone())
        {
            entries.remove(&oldest);
        }
        Ok(())
    }

    async fn fetch(
        &self,
        state: &AppState,
        provider: &mut ProviderState,
        ip: IpAddr,
        latency: bool,
    ) -> Result<Value, ApiError> {
        let request = |url: String| async move { self.request(&url).await };
        self.fetch_with(state, provider, ip, latency, request).await
    }

    async fn fetch_with<F, Fut>(
        &self,
        state: &AppState,
        provider: &mut ProviderState,
        ip: IpAddr,
        latency: bool,
        request: F,
    ) -> Result<Value, ApiError>
    where
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = Result<Value, ApiError>>,
    {
        let now = crate::current_time()? / 1000;
        let key = format!("{ip}:{latency}");
        let old = self.cached_entry(&key)?;
        if let Some(entry) = &old
            && !entry.stale(now)
        {
            return Ok(envelope(entry, now, "hit", None));
        }
        if provider.day != now / DAY {
            provider.day = now / DAY;
            provider.calls = 0;
        }
        if provider.minute != now / 60 {
            provider.minute = now / 60;
            provider.minute_calls = 0;
        }
        if provider.calls >= 200
            || provider.minute_calls >= 6
            || provider
                .next_request
                .is_some_and(|next| Instant::now() < next)
        {
            return stale_or_error(old.as_ref(), now, "IP information rate limit; retry later");
        }
        provider.calls += 1;
        provider.minute_calls += 1;
        // Set before I/O so cancellation cannot bypass the cooldown.
        provider.next_request = Some(Instant::now() + Duration::from_secs(10));
        let url = if latency {
            format!(
                "{PROVIDER}/api/ping/global?host={ip}&node=n02&node=n03&node=n04&node=n09&node=n11&node=n13"
            )
        } else {
            format!("{PROVIDER}/api/ip/lookup/{ip}")
        };
        let result = request(url).await.and_then(|raw| {
            if latency {
                normalize_latency(&raw, ip)
            } else {
                let data = normalize_lookup(&raw, ip)?;
                if data["reputation"]["trust_score"].is_null()
                    && old
                        .as_ref()
                        .is_some_and(|entry| entry.data["reputation"]["trust_score"].is_number())
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
            self.store_entry(key, entry)?;
            // A lookup and its following latency request are one logical operation.
            provider.next_request = None;
            Ok(result)
        } else {
            provider.next_request = Some(Instant::now() + Duration::from_secs(300));
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
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut entries = HashMap::new();
        for row in rows {
            let (key, raw, updated) = row?;
            let updated = u64::try_from(updated)
                .map_err(|_| StorageError::InvalidInput("invalid IP cache timestamp"))?;
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
        let updated = i64::try_from(entry.updated)
            .map_err(|_| StorageError::InvalidInput("invalid IP cache timestamp"))?;
        let mut db = self.connection()?;
        let transaction = db.transaction()?;
        transaction.execute(
            "INSERT INTO ip_info_cache(cache_key,data,updated_at) VALUES(?1,?2,?3)
             ON CONFLICT(cache_key) DO UPDATE SET data=excluded.data,updated_at=excluded.updated_at",
            params![key, raw, updated],
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
    // One bounded serialized provider lane keeps validation fresh before any outbound
    // call and coalesces duplicate misses without blocking dashboard cache reads.
    let mut provider = state.ip_info.provider.lock().await;
    let ip = validate(state, input).await?;
    let mut base = state.ip_info.fetch(state, &mut provider, ip, false).await?;
    base["data"]["uuid"] = json!(input.uuid);
    if latency && base["data"]["excluded"] != true {
        let result = state.ip_info.fetch(state, &mut provider, ip, true).await;
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
    let include_addresses = state.auth.authorize(headers, true, true).await.is_ok();
    let (include_scores, nodes) = state
        .database(move |db| {
            let enabled = db.settings()?.ip_info_enabled;
            if !enabled && !include_addresses {
                return Ok((false, HashMap::new()));
            }
            let connection = db.connection()?;
            let mut statement = connection.prepare(
                "SELECT n.id, coalesce(json_extract(o.data,'$.ipv4'),''), coalesce(json_extract(o.data,'$.ipv6'),'')
                 FROM nodes n LEFT JOIN node_options o ON o.node_id=n.id WHERE n.disabled_at_ms IS NULL",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    [row.get::<_, String>(1)?, row.get::<_, String>(2)?],
                ))
            })?;
            Ok((enabled, rows.collect::<Result<HashMap<_, _>, _>>()?))
        })
        .await?;
    if !include_scores {
        if include_addresses {
            enrich_scores(value, &nodes, None, true);
        }
        return Ok(());
    }
    let entries = state
        .ip_info
        .entries
        .read()
        .map_err(|_| ApiError::unavailable("IP cache unavailable"))?;
    enrich_scores(value, &nodes, Some(&entries), include_addresses);
    Ok(())
}

// Only attach cached scores to nodes already visible in this response. Public
// clients receive the family and score metadata without the registered address.
fn enrich_scores(
    value: &mut Value,
    nodes: &HashMap<String, [String; 2]>,
    entries: Option<&HashMap<String, Entry>>,
    include_addresses: bool,
) {
    if let Some(id) = value["uuid"].as_str() {
        let Some(addresses) = nodes.get(id) else {
            return;
        };
        if include_addresses {
            value["ipv4"] = json!(addresses[0]);
            value["ipv6"] = json!(addresses[1]);
        }
        if let Some(entries) = entries {
            let scores: Vec<Value> = addresses
                .iter()
                .zip([4, 6])
                .filter_map(|(address, family)| {
                    let ip = address.parse::<IpAddr>().ok()?;
                    let entry = entries.get(&format!("{ip}:false"));
                    let score =
                        entry.and_then(|entry| entry.data["reputation"]["trust_score"].as_f64());
                    Some(json!({
                        "family": family,
                        "score": score,
                        "provider": "Net.Coffee",
                        "updated_at_unix_ms": entry.map(|entry| entry.updated * 1000)
                    }))
                })
                .collect();
            value["ip_scores"] = json!(scores);
        }
    } else if let Some(items) = value.as_object_mut() {
        for item in items.values_mut() {
            enrich_scores(item, nodes, entries, include_addresses);
        }
    } else if let Some(items) = value.as_array_mut() {
        for item in items {
            enrich_scores(item, nodes, entries, include_addresses);
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
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use tower::ServiceExt;

    use super::*;

    fn test_state_with_node(ip_info_enabled: bool) -> (tempfile::TempDir, AppState, String) {
        let directory = tempfile::tempdir().unwrap();
        let state = AppState::open(&crate::ServiceConfig::with_database(
            directory.path().join("senlun.db"),
        ))
        .unwrap();
        let mut settings = state.storage.settings().unwrap();
        settings.private_site = false;
        settings.ip_info_enabled = ip_info_enabled;
        state.storage.save_settings(&settings).unwrap();
        let now_ms = crate::storage::unix_time_ms().unwrap();
        let secret = state.storage.create_enrollment(600, now_ms).unwrap();
        let request: senlun_protocol::EnrollmentRequest = serde_json::from_value(json!({
            "protocol_version": senlun_protocol::PROTOCOL_VERSION,
            "node_name": "Synthetic IP",
            "agent_version": "test",
            "region": "SG",
            "group": ""
        }))
        .unwrap();
        let enrolled = state
            .storage
            .enroll(
                &request,
                &crate::storage::hash_token(&secret.token),
                100,
                now_ms,
            )
            .unwrap();
        let options = json!({"ipv4": "198.51.100.10"}).to_string();
        state
            .storage
            .connection()
            .unwrap()
            .execute(
                "INSERT INTO node_options (node_id, data) VALUES (?1, ?2)",
                params![enrolled.node_id, options],
            )
            .unwrap();
        (directory, state, enrolled.node_id)
    }

    async fn admin_headers(state: &AppState) -> HeaderMap {
        let status_response = crate::router(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/auth/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let cookie = status_response
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let body = to_bytes(status_response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let status: Value = serde_json::from_slice(&body).unwrap();
        let setup_body = json!({
            "username": "admin",
            "password": "example-test-password-please-change"
        });
        let setup_response = crate::router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/setup")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ORIGIN, "http://127.0.0.1:8080")
                    .header(header::COOKIE, cookie)
                    .header("X-CSRF-Token", status["csrf_token"].as_str().unwrap())
                    .body(Body::from(setup_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(setup_response.status(), StatusCode::OK);
        let session_cookie = setup_response
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, session_cookie.parse().unwrap());
        headers
    }

    #[tokio::test]
    async fn cache_hits_do_not_consume_quota_and_expired_results_survive_limits() {
        let directory = tempfile::tempdir().unwrap();
        let state = AppState::open(&crate::ServiceConfig::with_database(
            directory.path().join("senlun.db"),
        ))
        .unwrap();
        let service = &state.ip_info;
        let now = crate::current_time().unwrap() / 1000;
        let mut limit = ProviderState {
            day: now / DAY,
            calls: 200,
            ..ProviderState::default()
        };
        let ip = "1.1.1.1".parse().unwrap();
        service
            .store_entry(
                format!("{ip}:false"),
                Entry {
                    data: json!({"test":true}),
                    updated: now,
                    ttl: 60,
                },
            )
            .unwrap();
        assert_eq!(
            service.fetch(&state, &mut limit, ip, false).await.unwrap()["meta"]["cache"],
            "hit"
        );
        service
            .store_entry(
                format!("{ip}:false"),
                Entry {
                    data: json!({"test":true}),
                    updated: now - 61,
                    ttl: 60,
                },
            )
            .unwrap();
        assert_eq!(
            service.fetch(&state, &mut limit, ip, false).await.unwrap()["meta"]["cache"],
            "stale"
        );
        assert_eq!(limit.calls, 200);
        assert!(service.fetch(&state, &mut limit, ip, true).await.is_err());
    }

    #[tokio::test]
    async fn dashboard_enrichment_does_not_block_behind_slow_provider_fetch() {
        let (_directory, state, node_id) = test_state_with_node(true);
        let now = crate::current_time().unwrap() / 1000;
        let cached_ip: IpAddr = "198.51.100.10".parse().unwrap();
        state
            .ip_info
            .store_entry(
                format!("{cached_ip}:false"),
                Entry {
                    data: json!({"reputation": {"trust_score": 88.5}}),
                    updated: now,
                    ttl: DAY,
                },
            )
            .unwrap();

        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let fetch_state = state.clone();
        let fetch_task = tokio::spawn(async move {
            let mut provider = fetch_state.ip_info.provider.lock().await;
            let target: IpAddr = "203.0.113.20".parse().unwrap();
            let request = |_url: String| async move {
                let _ = started_tx.send(());
                release_rx.await.unwrap()
            };
            fetch_state
                .ip_info
                .fetch_with(&fetch_state, &mut provider, target, false, request)
                .await
        });
        started_rx.await.unwrap();

        let mut visitor = json!({"data": [{"uuid": node_id}]});
        tokio::time::timeout(
            Duration::from_secs(2),
            enrich_nodes(&state, &HeaderMap::new(), &mut visitor),
        )
        .await
        .expect("dashboard score enrichment must not wait on provider I/O")
        .unwrap();
        assert_eq!(visitor["data"][0]["ip_scores"][0]["score"], json!(88.5));
        assert!(visitor["data"][0].get("ipv4").is_none());

        release_tx
            .send(Ok(json!({
                "ip": "203.0.113.20",
                "countryCode": "US",
                "registered_country_code": "US",
                "trust_score": 91.0
            })))
            .unwrap();
        let fetched = fetch_task.await.unwrap().unwrap();
        assert_eq!(fetched["meta"]["cache"], "miss");
        assert!(
            state
                .ip_info
                .cached_entry("203.0.113.20:false")
                .unwrap()
                .is_some()
        );
        let persisted: i64 = state
            .storage
            .connection()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM ip_info_cache WHERE cache_key = ?1",
                ["203.0.113.20:false"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted, 1);
    }

    #[tokio::test]
    async fn disabled_ip_info_enrichment_skips_score_cache_lock() {
        let (_directory, state, node_id) = test_state_with_node(false);
        let headers = admin_headers(&state).await;

        let lock_state = state.clone();
        let (locked_tx, locked_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let locker = std::thread::spawn(move || {
            let guard = lock_state.ip_info.entries.write().unwrap();
            let _ = locked_tx.send(());
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
            drop(guard);
        });
        locked_rx.await.unwrap();

        let mut visitor = json!({"uuid": node_id});
        tokio::time::timeout(
            Duration::from_secs(2),
            enrich_nodes(&state, &HeaderMap::new(), &mut visitor),
        )
        .await
        .expect("disabled visitor enrichment must skip score cache lock")
        .unwrap();
        assert!(visitor.get("ipv4").is_none());
        assert!(visitor.get("ip_scores").is_none());

        let mut admin = json!({"uuid": node_id});
        tokio::time::timeout(
            Duration::from_secs(2),
            enrich_nodes(&state, &headers, &mut admin),
        )
        .await
        .expect("disabled admin enrichment must skip score cache lock")
        .unwrap();
        assert_eq!(admin["ipv4"], "198.51.100.10");
        assert!(admin.get("ip_scores").is_none());

        release_tx.send(()).unwrap();
        locker.join().unwrap();
    }

    #[tokio::test]
    async fn provider_failure_sets_backoff_and_cache_remains_bounded() {
        let directory = tempfile::tempdir().unwrap();
        let state = AppState::open(&crate::ServiceConfig::with_database(
            directory.path().join("senlun.db"),
        ))
        .unwrap();
        let now = crate::current_time().unwrap() / 1000;
        let ip: IpAddr = "198.51.100.10".parse().unwrap();
        state
            .ip_info
            .store_entry(
                format!("{ip}:false"),
                Entry {
                    data: json!({"test": true}),
                    updated: now - DAY - 10,
                    ttl: DAY,
                },
            )
            .unwrap();

        let mut provider = ProviderState::default();
        let request = |_url: String| async { Err(ApiError::unavailable("synthetic outage")) };
        let response = state
            .ip_info
            .fetch_with(&state, &mut provider, ip, false, request)
            .await
            .unwrap();
        assert_eq!(response["meta"]["cache"], "stale");
        assert!(
            provider
                .next_request
                .is_some_and(|next| next > Instant::now() + Duration::from_secs(200))
        );

        for index in 0..=usize::from(MAX_CACHE) + 4 {
            let offset = u64::try_from(index).unwrap();
            state
                .ip_info
                .store_entry(
                    format!("2001:db8::{index:x}:false"),
                    Entry {
                        data: json!({"index": index}),
                        updated: now + offset,
                        ttl: DAY,
                    },
                )
                .unwrap();
        }
        assert_eq!(
            state.ip_info.entries.read().unwrap().len(),
            usize::from(MAX_CACHE)
        );
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
