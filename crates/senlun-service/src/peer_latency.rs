//! Latest bounded, directional latency for administrator-registered node addresses.

use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
    ops::Bound::{Excluded, Unbounded},
};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::get,
};
use rusqlite::{Connection, Row, params, types::Type};
use senlun_protocol::{
    MAX_PEER_PROBE_BATCH, MAX_PEER_PROBE_TARGETS, PEER_PROBE_SCHEMA_VERSION, PeerProbeBatch,
    PeerProbeKind, PeerProbePlan, PeerProbeStatus, PeerProbeTarget,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ApiError, AppState, bearer_token,
    control::{authenticated_node, effective_offline_ms, integer, read_settings, unsigned},
    current_time,
    storage::{Storage, StorageError},
};

// A round of 99 dual-stack targets can take about 150 seconds at four
// concurrent targets, leaving the rest of this window for transport.
const MAX_PLAN_AGE_MS: u64 = 180_000;
const FRESHNESS_MS: u64 = 300_000;
const RETENTION_MS: u64 = 1_800_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PeerAddressFamily {
    #[default]
    Auto,
    Ipv4,
    Ipv6,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PeerProbeSettings {
    pub(crate) kind: PeerProbeKind,
    pub(crate) family: PeerAddressFamily,
    pub(crate) disabled: bool,
}

#[derive(Deserialize)]
struct PeerQuery {
    #[serde(default)]
    after: String,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/agents/peer-latency", get(plan).post(report))
        .route("/api/v1/nodes/{id}/peer-latency", get(details))
}

async fn plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PeerQuery>,
) -> Result<Json<PeerProbePlan>, ApiError> {
    let token = bearer_token(&headers)?.to_owned();
    let now = current_time()?;
    let offline = state.offline_after_seconds;
    Ok(Json(
        state
            .database(move |storage| storage.peer_probe_plan(&token, &query.after, now, offline))
            .await?,
    ))
}

async fn report(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(batch): Json<PeerProbeBatch>,
) -> Result<(), ApiError> {
    let token = bearer_token(&headers)?.to_owned();
    let now = current_time()?;
    let offline = state.offline_after_seconds;
    state
        .database(move |storage| storage.record_peer_probes(&token, batch, now, offline))
        .await?;
    Ok(())
}

async fn details(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<PeerLatency>>, ApiError> {
    let now = current_time()?;
    let offline = state.offline_after_seconds;
    Ok(Json(
        state
            .database(move |storage| storage.peer_latencies(&id, now, offline))
            .await?,
    ))
}

pub(crate) fn initialize(db: &Connection) -> Result<(), StorageError> {
    // Each source can cache up to MAX_PEER_PROBE_TARGETS rows. Let SQLite
    // spill beyond the page cache into a file it deletes on close.
    db.execute_batch(
        "PRAGMA temp_store=FILE;
         ATTACH DATABASE '' AS peer_cache;
         PRAGMA peer_cache.cache_size=-1024;
         CREATE TABLE peer_cache.peer_latencies (
             source_id TEXT NOT NULL,
             target_id TEXT NOT NULL,
             address TEXT NOT NULL,
             kind TEXT NOT NULL,
             status TEXT NOT NULL,
             latency_ms REAL,
             issued_at_ms INTEGER NOT NULL,
             PRIMARY KEY(source_id,target_id)
         ) WITHOUT ROWID;
         CREATE INDEX peer_cache.peer_latencies_age ON peer_latencies(issued_at_ms);
         CREATE INDEX peer_cache.peer_latencies_target ON peer_latencies(target_id);
         CREATE INDEX peer_cache.peer_latencies_ready ON peer_latencies(source_id,issued_at_ms)
             WHERE status='ready';",
    )?;
    Ok(())
}

pub(crate) fn invalidate_node(db: &Connection, id: &str) {
    // Call after committing node changes. A cache I/O failure must not roll
    // back a control decision, and readers recheck current node metadata.
    if let Err(error) = db.execute(
        "DELETE FROM peer_cache.peer_latencies WHERE source_id=?1 OR target_id=?1",
        [id],
    ) {
        tracing::warn!(%error, "could not invalidate peer latency cache");
    }
}

pub(crate) fn prune(db: &Connection, now: u64) -> Result<(), StorageError> {
    db.execute(
        "DELETE FROM peer_cache.peer_latencies WHERE issued_at_ms<?1",
        [integer(now.saturating_sub(RETENTION_MS))],
    )?;
    Ok(())
}

pub(crate) struct PeerNode {
    name: String,
    online: bool,
    target: Option<PeerProbeTarget>,
}

pub(crate) type PeerDirectory = BTreeMap<String, PeerNode>;

fn assigned_targets<'a>(peers: &'a PeerDirectory, source: &str) -> BTreeSet<&'a str> {
    // Start after the source ID and wrap around to distribute selection across
    // larger sites. Keep offline targets assigned so pages stay stable as they
    // go online and offline.
    peers
        .range::<str, _>((Excluded(source), Unbounded))
        .chain(peers.range::<str, _>((Unbounded, Excluded(source))))
        .filter(|(_, peer)| peer.target.is_some())
        .take(MAX_PEER_PROBE_TARGETS)
        .map(|(id, _)| id.as_str())
        .collect()
}

fn configured_target(
    id: &str,
    settings: PeerProbeSettings,
    ipv4: &str,
    ipv6: &str,
) -> Option<PeerProbeTarget> {
    if settings.disabled {
        return None;
    }
    let ipv4 = ipv4.parse::<IpAddr>().ok().filter(IpAddr::is_ipv4);
    let ipv6 = ipv6.parse::<IpAddr>().ok().filter(IpAddr::is_ipv6);
    let ipv4 = ipv4.filter(|ip| crate::ip_info::public_ip(*ip));
    let ipv6 = ipv6.filter(|ip| crate::ip_info::public_ip(*ip));
    let (address, fallback_address) = match settings.family {
        PeerAddressFamily::Auto => (ipv4.or(ipv6), ipv4.and(ipv6)),
        PeerAddressFamily::Ipv4 => (ipv4, None),
        PeerAddressFamily::Ipv6 => (ipv6, None),
    };
    Some(PeerProbeTarget {
        node_id: id.to_owned(),
        address: address?,
        fallback_address,
        kind: settings.kind,
    })
}

impl PeerNode {
    fn accepts_address(&self, address: IpAddr) -> bool {
        self.target.as_ref().is_some_and(|target| {
            target.address == address || target.fallback_address == Some(address)
        })
    }
}

pub(crate) fn directory(
    db: &Connection,
    now: u64,
    offline: u64,
) -> Result<PeerDirectory, StorageError> {
    let offline_ms = effective_offline_ms(db, offline.saturating_mul(1_000))?;
    let mut stmt = db.prepare_cached(
        "SELECT n.id,coalesce(json_extract(o.data,'$.name'),n.name),s.last_seen_at_ms,
            coalesce(json_extract(o.data,'$.ipv4'),''),
            coalesce(json_extract(o.data,'$.ipv6'),''),
            json_extract(o.data,'$.peer_probe')
         FROM nodes n
         LEFT JOIN node_options o ON o.node_id=n.id
         LEFT JOIN node_state s ON s.node_id=n.id
         WHERE n.disabled_at_ms IS NULL AND coalesce(json_extract(o.data,'$.hidden'),0)=0",
    )?;
    let mut rows = stmt.query([])?;
    let mut peers = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let name = row.get(1)?;
        let last_seen: Option<i64> = row.get(2)?;
        let ipv4: String = row.get(3)?;
        let ipv6: String = row.get(4)?;
        let settings = row
            .get::<_, Option<String>>(5)?
            .map_or(Ok(PeerProbeSettings::default()), |raw| {
                serde_json::from_str(&raw)
            })
            .map_err(|_| StorageError::InvalidInput("invalid stored peer probe settings"))?;
        let target = configured_target(&id, settings, &ipv4, &ipv6);
        let online = last_seen.is_some_and(|seen| now.saturating_sub(unsigned(seen)) <= offline_ms);
        peers.insert(
            id,
            PeerNode {
                name,
                online,
                target,
            },
        );
    }
    Ok(peers)
}

impl Storage {
    fn peer_probe_plan(
        &self,
        token: &str,
        after: &str,
        now: u64,
        offline: u64,
    ) -> Result<PeerProbePlan, StorageError> {
        if !after.is_empty() && Uuid::parse_str(after).is_err() {
            return Err(StorageError::InvalidInput("invalid peer cursor"));
        }
        let db = self.connection()?;
        let source = authenticated_node(&db, token)?;
        let interval_seconds = read_settings(&db)?.peer_probe_interval();
        let peers = directory(&db, now, offline)?;
        let mut targets = Vec::new();
        let mut next_after = None;
        if peers.get(&source).is_some_and(|node| node.online) {
            let mut eligible = assigned_targets(&peers, &source)
                .into_iter()
                .filter(|id| *id > after)
                .filter_map(|id| {
                    let peer = peers.get(id)?;
                    peer.target.as_ref().filter(|_| peer.online)
                });
            targets.extend(eligible.by_ref().take(MAX_PEER_PROBE_BATCH).cloned());
            if eligible.next().is_some() {
                next_after = targets.last().map(|target| target.node_id.clone());
            }
        }
        Ok(PeerProbePlan {
            schema_version: PEER_PROBE_SCHEMA_VERSION,
            issued_at_unix_ms: now,
            interval_seconds,
            targets,
            next_after,
        })
    }

    fn record_peer_probes(
        &self,
        token: &str,
        batch: PeerProbeBatch,
        now: u64,
        offline: u64,
    ) -> Result<(), StorageError> {
        if batch.schema_version != PEER_PROBE_SCHEMA_VERSION
            || batch.samples.len() > MAX_PEER_PROBE_BATCH
            || batch.issued_at_unix_ms > now
            || now - batch.issued_at_unix_ms > MAX_PLAN_AGE_MS
        {
            return Err(StorageError::InvalidInput("invalid peer probe batch"));
        }
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let source = authenticated_node(&tx, token)?;
        let peers = directory(&tx, now, offline)?;
        if !peers.get(&source).is_some_and(|node| node.online) {
            return Ok(());
        }
        let assigned = assigned_targets(&peers, &source);
        let assignments = serde_json::to_string(&assigned)
            .map_err(|_| StorageError::InvalidInput("could not encode assigned peer targets"))?;
        // A configuration change can replace targets between reports. Remove
        // their old rows before upserting results for the current assignment.
        tx.execute(
            "DELETE FROM peer_cache.peer_latencies WHERE source_id=?1
             AND target_id NOT IN (SELECT value FROM json_each(?2))",
            params![source, assignments],
        )?;
        for sample in batch.samples {
            if sample
                .latency_ms
                .is_some_and(|value| !value.is_finite() || !(0.0..=3000.0).contains(&value))
                || (sample.status == PeerProbeStatus::Ready) != sample.latency_ms.is_some()
                || sample.node_id == source
            {
                return Err(StorageError::InvalidInput("invalid peer probe result"));
            }
            // An address/settings change, deletion or offline transition can
            // invalidate one result while other targets on its page remain valid.
            if !assigned.contains(sample.node_id.as_str())
                || !peers.get(&sample.node_id).is_some_and(|node| {
                    node.online
                        && node.accepts_address(sample.address)
                        && node
                            .target
                            .as_ref()
                            .is_some_and(|target| target.kind == sample.kind)
                })
            {
                continue;
            }
            let status = match sample.status {
                PeerProbeStatus::Ready => "ready",
                PeerProbeStatus::Timeout => "timeout",
                PeerProbeStatus::Unavailable => "unavailable",
                PeerProbeStatus::Failed => "failed",
            };
            tx.execute(
                "INSERT INTO peer_cache.peer_latencies VALUES(?1,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT(source_id,target_id) DO UPDATE SET address=excluded.address,
                    kind=excluded.kind,status=excluded.status,latency_ms=excluded.latency_ms,issued_at_ms=excluded.issued_at_ms
                 WHERE excluded.issued_at_ms > peer_latencies.issued_at_ms",
                params![source, sample.node_id, sample.address.to_string(), kind_name(sample.kind), status, sample.latency_ms, integer(batch.issued_at_unix_ms)],
            )?;
        }
        prune(&tx, now)?;
        tx.commit()?;
        Ok(())
    }

    fn peer_latencies(
        &self,
        id: &str,
        now: u64,
        offline: u64,
    ) -> Result<Vec<PeerLatency>, StorageError> {
        let db = self.connection()?;
        let peers = directory(&db, now, offline)?;
        let source = peers.get(id).ok_or(StorageError::NotFound)?;
        let assigned = assigned_targets(&peers, id);
        let samples = samples(&db, id)?;
        let mut result: Vec<_> = peers
            .iter()
            .filter(|(target, peer)| {
                target.as_str() != id
                    && (peer.target.is_none() || assigned.contains(target.as_str()))
            })
            .map(|(target, peer)| latency(target, peer, source.online, samples.get(target), now))
            .collect();
        result.sort_by(compare_latency);
        Ok(result)
    }
}

fn kind_name(kind: PeerProbeKind) -> &'static str {
    match kind {
        PeerProbeKind::Tcp => "tcp",
        PeerProbeKind::Icmp => "icmp",
    }
}

struct Sample {
    address: IpAddr,
    kind: String,
    status: String,
    latency_ms: Option<f64>,
    issued_at_ms: u64,
}

fn read_sample(row: &Row<'_>) -> rusqlite::Result<Sample> {
    let address = row
        .get::<_, String>(1)?
        .parse()
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(1, Type::Text, Box::new(err)))?;
    Ok(Sample {
        address,
        kind: row.get(2)?,
        status: row.get(3)?,
        latency_ms: row.get(4)?,
        issued_at_ms: unsigned(row.get(5)?),
    })
}

fn samples(db: &Connection, source: &str) -> Result<BTreeMap<String, Sample>, StorageError> {
    let mut statement = db.prepare_cached(
        "SELECT target_id,address,kind,status,latency_ms,issued_at_ms
         FROM peer_cache.peer_latencies WHERE source_id=?1",
    )?;
    let rows = statement.query_map([source], |row| Ok((row.get(0)?, read_sample(row)?)))?;
    rows.collect::<Result<_, _>>().map_err(Into::into)
}

#[derive(Serialize)]
pub(crate) struct PeerLatency {
    node_id: String,
    name: String,
    kind: Option<PeerProbeKind>,
    family: Option<u8>,
    status: String,
    latency_ms: Option<f64>,
    expires_at_unix_ms: Option<u64>,
}

fn latency(
    id: &str,
    peer: &PeerNode,
    source_online: bool,
    sample: Option<&Sample>,
    now: u64,
) -> PeerLatency {
    let target = peer.target.as_ref();
    let matching = sample.filter(|sample| {
        peer.accepts_address(sample.address)
            && target.is_some_and(|target| sample.kind == kind_name(target.kind))
    });
    let status = if target.is_none() {
        "unconfigured"
    } else if !source_online || !peer.online {
        "offline"
    } else if let Some(sample) = matching {
        if now.saturating_sub(sample.issued_at_ms) >= FRESHNESS_MS {
            "stale"
        } else {
            &sample.status
        }
    } else {
        "pending"
    };
    PeerLatency {
        node_id: id.to_owned(),
        name: peer.name.clone(),
        kind: target.map(|target| target.kind),
        family: target.map(|target| {
            let address = matching.map_or(target.address, |sample| sample.address);
            if address.is_ipv4() { 4 } else { 6 }
        }),
        status: status.into(),
        latency_ms: matching.and_then(|sample| {
            if status == "ready" {
                sample.latency_ms
            } else {
                None
            }
        }),
        expires_at_unix_ms: matching.map(|sample| sample.issued_at_ms.saturating_add(FRESHNESS_MS)),
    }
}

fn compare_latency(a: &PeerLatency, b: &PeerLatency) -> std::cmp::Ordering {
    a.latency_ms
        .unwrap_or(f64::INFINITY)
        .total_cmp(&b.latency_ms.unwrap_or(f64::INFINITY))
        .then_with(|| a.name.cmp(&b.name))
        .then_with(|| a.node_id.cmp(&b.node_id))
}

pub(crate) fn best(
    db: &Connection,
    peers: &PeerDirectory,
    id: &str,
    now: u64,
) -> Result<Option<PeerLatency>, StorageError> {
    if !peers.get(id).is_some_and(|node| node.online) {
        return Ok(None);
    }
    let mut statement = db.prepare_cached(
        "SELECT target_id,address,kind,status,latency_ms,issued_at_ms
         FROM peer_cache.peer_latencies
         WHERE source_id=?1 AND status='ready' AND issued_at_ms>?2",
    )?;
    let mut rows = statement.query(params![id, integer(now) - integer(FRESHNESS_MS)])?;
    let mut assigned = None;
    let mut best: Option<PeerLatency> = None;
    while let Some(row) = rows.next()? {
        let assigned = assigned.get_or_insert_with(|| assigned_targets(peers, id));
        let target: String = row.get(0)?;
        if assigned.contains(target.as_str())
            && let Some(peer) = peers.get(&target)
        {
            let sample = read_sample(row)?;
            let result = latency(&target, peer, true, Some(&sample), now);
            if result.latency_ms.is_some()
                && best
                    .as_ref()
                    .is_none_or(|best| compare_latency(&result, best).is_lt())
            {
                best = Some(result);
            }
        }
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        control::NodeOptions,
        control_tests::{fixture, sample},
        storage::{hash_token, unix_time_ms},
    };
    use senlun_protocol::{
        DEFAULT_PEER_PROBE_INTERVAL_SECONDS, EnrollmentRequest, EnrollmentResponse,
        MAX_PEER_PROBE_INTERVAL_SECONDS, MIN_PEER_PROBE_INTERVAL_SECONDS, PROTOCOL_VERSION,
        PeerProbeSample,
    };

    fn enroll(storage: &Storage, now: u64) -> EnrollmentResponse {
        let secret = storage.create_enrollment(60, now).unwrap();
        storage
            .enroll(
                &EnrollmentRequest {
                    protocol_version: PROTOCOL_VERSION,
                    node_name: "peer".into(),
                    agent_version: "test".into(),
                    region: String::new(),
                    group: String::new(),
                },
                &hash_token(&secret.token),
                1_000,
                now,
            )
            .unwrap()
    }

    fn configure(
        storage: &Storage,
        node: &EnrollmentResponse,
        kind: PeerProbeKind,
        family: PeerAddressFamily,
        now: u64,
    ) {
        storage
            .save_node_options(
                &node.node_id,
                &mut NodeOptions {
                    name: "peer".into(),
                    currency: "USD".into(),
                    traffic_reset_day: 1,
                    traffic_limit_type: "sum".into(),
                    ipv4: "1.1.1.1".into(),
                    ipv6: "2606:4700:4700::1111".into(),
                    peer_probe: Some(PeerProbeSettings {
                        kind,
                        family,
                        disabled: false,
                    }),
                    ..NodeOptions::default()
                },
            )
            .unwrap();
        heartbeat(storage, node, now);
    }

    fn heartbeat(storage: &Storage, node: &EnrollmentResponse, now: u64) {
        storage
            .ingest(&hash_token(&node.agent_token), &sample(now, 0, 0), now)
            .unwrap();
    }

    #[test]
    fn peer_interval_is_configurable_and_preserved_on_omitted_settings_updates() {
        let (_dir, storage, source) = fixture();
        let now = unix_time_ms().unwrap();
        let interval = || {
            storage
                .peer_probe_plan(&source.agent_token, "", now, 60)
                .unwrap()
                .interval_seconds
        };
        assert_eq!(interval(), DEFAULT_PEER_PROBE_INTERVAL_SECONDS);

        let mut settings = storage.settings().unwrap();
        settings.peer_probe_interval_seconds = Some(120);
        storage.save_settings(&settings).unwrap();
        assert_eq!(interval(), 120);

        let legacy = serde_json::from_value(serde_json::json!({"site_name": "Renamed"})).unwrap();
        storage.save_settings(&legacy).unwrap();
        assert_eq!(storage.settings().unwrap().site_name, "Renamed");
        assert_eq!(interval(), 120);

        for seconds in [
            MIN_PEER_PROBE_INTERVAL_SECONDS - 1,
            MAX_PEER_PROBE_INTERVAL_SECONDS + 1,
        ] {
            settings.peer_probe_interval_seconds = Some(seconds);
            assert!(matches!(
                storage.save_settings(&settings),
                Err(StorageError::InvalidInput(_))
            ));
        }
        assert_eq!(interval(), 120);
    }

    fn report_sample(
        storage: &Storage,
        node: &EnrollmentResponse,
        target: PeerProbeTarget,
        issued_at: u64,
        now: u64,
        latency: Option<f64>,
    ) {
        storage
            .record_peer_probes(
                &node.agent_token,
                PeerProbeBatch {
                    schema_version: PEER_PROBE_SCHEMA_VERSION,
                    issued_at_unix_ms: issued_at,
                    samples: vec![PeerProbeSample {
                        node_id: target.node_id,
                        address: target.address,
                        kind: target.kind,
                        status: if latency.is_some() {
                            PeerProbeStatus::Ready
                        } else {
                            PeerProbeStatus::Timeout
                        },
                        latency_ms: latency,
                    }],
                },
                now,
                60,
            )
            .unwrap();
    }

    #[test]
    fn auto_accepts_either_registered_address_and_reports_the_measured_family() {
        let (_dir, storage, source) = fixture();
        let now = unix_time_ms().unwrap();
        heartbeat(&storage, &source, now);
        let target = enroll(&storage, now);
        configure(
            &storage,
            &target,
            PeerProbeKind::Tcp,
            PeerAddressFamily::Auto,
            now,
        );
        let mut planned = storage
            .peer_probe_plan(&source.agent_token, "", now, 60)
            .unwrap()
            .targets
            .remove(0);
        assert!(planned.address.is_ipv4());
        let fallback = planned.fallback_address.unwrap();
        assert!(fallback.is_ipv6());
        planned.address = fallback;
        let received = now + 150_000;
        heartbeat(&storage, &source, received);
        heartbeat(&storage, &target, received);
        report_sample(
            &storage,
            &source,
            planned.clone(),
            now,
            received,
            Some(11.0),
        );

        planned.address = "2606:4700:4700::1001".parse().unwrap();
        report_sample(
            &storage,
            &source,
            planned,
            received + 1,
            received + 1,
            Some(1.0),
        );
        let rows = storage
            .peer_latencies(&source.node_id, received + 1, 60)
            .unwrap();
        assert_eq!(rows[0].family, Some(6));
        assert_eq!(rows[0].latency_ms, Some(11.0));
        let db = storage.connection().unwrap();
        let peers = directory(&db, received + 1, 60).unwrap();
        let lowest = best(&db, &peers, &source.node_id, received + 1)
            .unwrap()
            .unwrap();
        assert_eq!(lowest.family, Some(6));
    }

    #[test]
    fn caps_plans_and_results_at_99_assigned_targets() {
        const TARGETS: usize = MAX_PEER_PROBE_TARGETS + 2;
        let (_dir, storage, source) = fixture();
        let now = unix_time_ms().unwrap();
        heartbeat(&storage, &source, now);
        let mut target_ids = BTreeSet::new();
        for _ in 0..TARGETS {
            let target = enroll(&storage, now);
            configure(
                &storage,
                &target,
                PeerProbeKind::Tcp,
                PeerAddressFamily::Ipv4,
                now,
            );
            target_ids.insert(target.node_id);
        }
        let mut plan = storage
            .peer_probe_plan(&source.agent_token, "", now, 60)
            .unwrap();
        let fastest = plan.targets[0].node_id.clone();
        let mut issued = now;
        loop {
            for target in &plan.targets {
                assert!(target_ids.remove(&target.node_id));
            }
            let samples = plan
                .targets
                .into_iter()
                .map(|target| PeerProbeSample {
                    latency_ms: Some(if target.node_id == fastest { 1.0 } else { 10.0 }),
                    node_id: target.node_id,
                    address: target.address,
                    kind: target.kind,
                    status: PeerProbeStatus::Ready,
                })
                .collect();
            storage
                .record_peer_probes(
                    &source.agent_token,
                    PeerProbeBatch {
                        schema_version: PEER_PROBE_SCHEMA_VERSION,
                        issued_at_unix_ms: plan.issued_at_unix_ms,
                        samples,
                    },
                    issued,
                    60,
                )
                .unwrap();
            let Some(cursor) = plan.next_after else { break };
            issued += 1;
            plan = storage
                .peer_probe_plan(&source.agent_token, &cursor, issued, 60)
                .unwrap();
        }
        assert_eq!(target_ids.len(), TARGETS - MAX_PEER_PROBE_TARGETS);
        let unassigned = {
            let db = storage.connection().unwrap();
            let peers = directory(&db, issued, 60).unwrap();
            let target = target_ids.first().unwrap();
            peers[target].target.clone().unwrap()
        };
        report_sample(
            &storage,
            &source,
            unassigned,
            issued + 1,
            issued + 1,
            Some(0.0),
        );
        let rows = storage.peer_latencies(&source.node_id, issued, 60).unwrap();
        assert_eq!(rows.len(), MAX_PEER_PROBE_TARGETS);
        assert!(rows.iter().all(|row| row.status == "ready"));
        let db = storage.connection().unwrap();
        assert_eq!(
            samples(&db, &source.node_id).unwrap().len(),
            MAX_PEER_PROBE_TARGETS
        );
        let peers = directory(&db, issued, 60).unwrap();
        let lowest = best(&db, &peers, &source.node_id, issued).unwrap().unwrap();
        assert_eq!(lowest.node_id, fastest);
    }

    #[test]
    fn node_changes_and_maintenance_survive_unavailable_peer_cache() {
        let (_dir, storage, source) = fixture();
        let now = unix_time_ms().unwrap();
        let target = enroll(&storage, now);
        configure(
            &storage,
            &target,
            PeerProbeKind::Tcp,
            PeerAddressFamily::Auto,
            now,
        );
        storage
            .connection()
            .unwrap()
            .execute_batch("DROP TABLE peer_cache.peer_latencies")
            .unwrap();

        let now = now + 1_000;
        configure(
            &storage,
            &target,
            PeerProbeKind::Icmp,
            PeerAddressFamily::Ipv6,
            now,
        );
        storage.revoke_node(&source.node_id, now).unwrap();
        storage.delete_node(&target.node_id, now).unwrap();
        storage.prune_expired(now, 7).unwrap();
        let db = storage.connection().unwrap();
        for node in [&source, &target] {
            assert!(matches!(
                authenticated_node(&db, &node.agent_token),
                Err(StorageError::Unauthorized)
            ));
        }
    }

    #[test]
    fn peer_results_are_directional_and_old_reports_cannot_replace_failures() {
        let (_dir, storage, a) = fixture();
        let now = unix_time_ms().unwrap();
        let b = enroll(&storage, now);
        configure(
            &storage,
            &a,
            PeerProbeKind::Tcp,
            PeerAddressFamily::Ipv4,
            now,
        );
        configure(
            &storage,
            &b,
            PeerProbeKind::Icmp,
            PeerAddressFamily::Ipv6,
            now,
        );
        let to_b = storage
            .peer_probe_plan(&a.agent_token, "", now, 60)
            .unwrap()
            .targets;
        assert_eq!(to_b.len(), 1);
        assert_eq!(to_b[0].node_id, b.node_id);
        assert_eq!(to_b[0].kind, PeerProbeKind::Icmp);
        assert!(to_b[0].address.is_ipv6());
        assert_eq!(to_b[0].fallback_address, None);
        let to_a = storage
            .peer_probe_plan(&b.agent_token, "", now, 60)
            .unwrap()
            .targets;
        assert_eq!(to_a.len(), 1);
        assert_eq!(to_a[0].kind, PeerProbeKind::Tcp);
        assert!(to_a[0].address.is_ipv4());
        assert_eq!(to_a[0].fallback_address, None);
        report_sample(&storage, &a, to_b[0].clone(), now, now, Some(11.0));
        report_sample(&storage, &b, to_a[0].clone(), now, now, Some(45.0));
        for (node, expected) in [(&a, 11.0), (&b, 45.0)] {
            let rows = storage.peer_latencies(&node.node_id, now, 60).unwrap();
            assert_eq!(rows[0].latency_ms, Some(expected));
        }
        report_sample(&storage, &a, to_b[0].clone(), now + 1, now + 1_000, None);
        report_sample(&storage, &a, to_b[0].clone(), now, now + 1_000, Some(11.0));
        let failed = storage.peer_latencies(&a.node_id, now + 1_000, 60).unwrap();
        assert_eq!(failed[0].status, "timeout");
        assert_eq!(failed[0].latency_ms, None);
        assert_eq!(
            storage.peer_latencies(&b.node_id, now + 1_000, 60).unwrap()[0].latency_ms,
            Some(45.0)
        );

        let expired = now + FRESHNESS_MS;
        heartbeat(&storage, &a, expired);
        heartbeat(&storage, &b, expired);
        assert_eq!(
            storage.peer_latencies(&b.node_id, expired, 60).unwrap()[0].status,
            "stale"
        );
        let db = storage.connection().unwrap();
        let peers = directory(&db, expired, 60).unwrap();
        assert!(best(&db, &peers, &b.node_id, expired).unwrap().is_none());
    }

    #[test]
    fn peer_plans_rotate_and_exclude_offline_targets() {
        let (_dir, storage, source) = fixture();
        let now = unix_time_ms().unwrap();
        heartbeat(&storage, &source, now);
        let mut target_ids = Vec::new();
        for _ in 0..=MAX_PEER_PROBE_BATCH {
            let target = enroll(&storage, now);
            configure(
                &storage,
                &target,
                PeerProbeKind::Tcp,
                PeerAddressFamily::Auto,
                now,
            );
            target_ids.push(target.node_id);
        }
        target_ids.sort();
        let first = storage
            .peer_probe_plan(&source.agent_token, "", now, 60)
            .unwrap();
        assert_eq!(
            first
                .targets
                .iter()
                .map(|target| &target.node_id)
                .collect::<Vec<_>>(),
            target_ids[..MAX_PEER_PROBE_BATCH]
                .iter()
                .collect::<Vec<_>>(),
        );
        let cursor = first.next_after.as_deref().unwrap();
        assert_eq!(cursor, first.targets[MAX_PEER_PROBE_BATCH - 1].node_id);
        let second = storage
            .peer_probe_plan(&source.agent_token, cursor, now, 60)
            .unwrap();
        assert_eq!(second.targets.len(), 1);
        assert_eq!(second.targets[0].node_id, target_ids[MAX_PEER_PROBE_BATCH]);
        assert_eq!(second.next_after, None);
        let exhausted = storage
            .peer_probe_plan(&source.agent_token, &second.targets[0].node_id, now, 60)
            .unwrap();
        assert_eq!(exhausted.targets, Vec::<PeerProbeTarget>::new());
        assert_eq!(exhausted.next_after, None);
        assert_eq!(
            storage
                .peer_probe_plan(&source.agent_token, "", now, 60)
                .unwrap()
                .targets,
            first.targets
        );
        let later = now + 60_001;
        heartbeat(&storage, &source, later);
        assert_eq!(
            storage
                .peer_probe_plan(&source.agent_token, "", later, 60)
                .unwrap()
                .targets,
            Vec::<PeerProbeTarget>::new(),
        );
        assert!(matches!(
            storage.peer_probe_plan("invalid", "", now, 60),
            Err(StorageError::Unauthorized)
        ));
    }
    #[test]
    fn best_peer_breaks_ties_by_id_and_invalidates_changed_targets() {
        let (_dir, storage, source) = fixture();
        let now = unix_time_ms().unwrap();
        heartbeat(&storage, &source, now);
        let first = enroll(&storage, now);
        let second = enroll(&storage, now);
        for node in [&first, &second] {
            configure(
                &storage,
                node,
                PeerProbeKind::Tcp,
                PeerAddressFamily::Ipv4,
                now,
            );
        }
        let plan = storage
            .peer_probe_plan(&source.agent_token, "", now, 60)
            .unwrap();
        for target in &plan.targets {
            report_sample(&storage, &source, target.clone(), now, now, Some(4.0));
        }
        let rows = storage.peer_latencies(&source.node_id, now, 60).unwrap();
        assert_eq!(rows[0].node_id, plan.targets[0].node_id);
        {
            let db = storage.connection().unwrap();
            let peers = directory(&db, now, 60).unwrap();
            let lowest = best(&db, &peers, &source.node_id, now).unwrap().unwrap();
            assert_eq!(lowest.node_id, rows[0].node_id);
        }
        let changed = if first.node_id == rows[0].node_id {
            &first
        } else {
            &second
        };
        configure(
            &storage,
            changed,
            PeerProbeKind::Icmp,
            PeerAddressFamily::Ipv6,
            now + 1_000,
        );
        report_sample(
            &storage,
            &source,
            plan.targets[0].clone(),
            now + 1,
            now + 1_000,
            Some(1.0),
        );
        let rows = storage
            .peer_latencies(&source.node_id, now + 1_000, 60)
            .unwrap();
        assert_eq!(rows[0].node_id, plan.targets[1].node_id);
        assert_eq!(rows[1].status, "pending");
        assert_eq!(rows[1].latency_ms, None);
    }
}
