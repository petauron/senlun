//! Administrator-triggered, bounded IP unlock and Google region checks.
//! Agents pull a fixed task; no executable, script or custom target crosses the API.
use crate::control::{integer, unsigned};
use crate::{
    ApiError, AppState,
    storage::{Storage, StorageError},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post},
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use senlun_protocol::{
    IP_UNLOCK_SCHEMA_VERSION, IpUnlockClaim, IpUnlockOutcome, IpUnlockTask, UnlockServiceStatus,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::net::IpAddr;
use uuid::Uuid;

const MAX_AGE_MS: u64 = 7 * 86_400_000;
const SERVICES: [&str; 6] = [
    "youtube",
    "netflix",
    "disney_plus",
    "chatgpt",
    "reddit",
    "tiktok",
];

pub(crate) use senlun_protocol::IpUnlockReport as UnlockReport;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/nodes/{id}/ip-unlock", get(status))
        .route("/api/admin/nodes/{id}/ip-unlock/check", post(start))
        .route("/api/v1/agents/ip-unlock/claim", post(claim))
        .route("/api/v1/agents/ip-unlock/result", post(finish))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartInput {
    family: u8,
}

async fn status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let now = crate::current_time()?;
    Ok(Json(
        state
            .database(move |storage| storage.unlock_status(&id, now))
            .await?,
    ))
}

async fn start(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<StartInput>,
) -> Result<Json<Value>, ApiError> {
    let now = crate::current_time()?;
    let offline = state.offline_after_seconds;
    Ok(Json(
        state
            .database(move |storage| storage.start_unlock_check(&id, input.family, now, offline))
            .await?,
    ))
}

async fn claim(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<IpUnlockClaim>,
) -> Result<Json<Option<IpUnlockTask>>, ApiError> {
    let token = crate::bearer_token(&headers)?.to_owned();
    let now = crate::current_time()?;
    Ok(Json(
        state
            .database(move |storage| storage.claim_unlock_check(&token, &input, now))
            .await?,
    ))
}

async fn finish(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<IpUnlockOutcome>,
) -> Result<Json<Value>, ApiError> {
    let token = crate::bearer_token(&headers)?.to_owned();
    let now = crate::current_time()?;
    state
        .database(move |storage| storage.finish_unlock_check(&token, input, now))
        .await?;
    Ok(Json(json!({"accepted": true})))
}

fn valid_region_opt(region: Option<&str>) -> bool {
    region.is_none_or(|code| code.len() == 2 && code.bytes().all(|b| b.is_ascii_uppercase()))
}

fn valid_access_type(access_type: Option<&str>) -> bool {
    access_type.is_none_or(|kind| matches!(kind, "native" | "dns"))
}

fn validate_report(report: &UnlockReport, now: u64) -> Result<IpAddr, StorageError> {
    let address = report
        .address
        .parse::<IpAddr>()
        .map_err(|_| StorageError::InvalidInput("invalid unlock address"))?;
    let family_valid = match report.family {
        4 => address.is_ipv4(),
        6 => address.is_ipv6(),
        _ => false,
    };
    if !family_valid
        || !crate::ip_info::public_ip(address)
        || report.checked_at_unix_ms == 0
        || report.checked_at_unix_ms > now.saturating_add(60_000)
        || !["normal", "china", "captcha", "unknown"].contains(&report.google.status.as_str())
        || !valid_region_opt(report.google.region.as_deref())
        || report.services.len() != SERVICES.len()
    {
        return Err(StorageError::InvalidInput("invalid IP unlock report"));
    }
    for service_id in SERVICES {
        let matches: Vec<&UnlockServiceStatus> = report
            .services
            .iter()
            .filter(|entry| entry.id == service_id)
            .collect();
        if matches.len() != 1 {
            return Err(StorageError::InvalidInput(
                "all unlock services are required",
            ));
        }
        let entry = matches[0];
        if ![
            "unlocked",
            "originals_only",
            "app_only",
            "china",
            "blocked",
            "unknown",
        ]
        .contains(&entry.status.as_str())
            || !valid_region_opt(entry.region.as_deref())
            || !valid_access_type(entry.access_type.as_deref())
        {
            return Err(StorageError::InvalidInput("invalid unlock service entry"));
        }
    }
    Ok(address)
}

impl Storage {
    fn save_ip_unlock_tx(
        tx: &Transaction<'_>,
        id: &str,
        report: UnlockReport,
        now: u64,
    ) -> Result<(), StorageError> {
        let address = validate_report(&report, now)?;
        let raw: Option<String> = tx
            .query_row(
                "SELECT o.data FROM node_options o JOIN nodes n ON n.id=o.node_id \
                 WHERE o.node_id=?1 AND n.disabled_at_ms IS NULL",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        let raw = raw.ok_or(StorageError::InvalidInput(
            "configure node public addresses before detecting",
        ))?;
        let mut options: Value = serde_json::from_str(&raw)
            .map_err(|_| StorageError::InvalidInput("invalid stored node options"))?;
        let key = if report.family == 4 { "ipv4" } else { "ipv6" };
        if options[key].as_str().and_then(|v| v.parse::<IpAddr>().ok()) != Some(address) {
            return Err(StorageError::InvalidInput(
                "unlock address is not registered to this node",
            ));
        }
        let mut reports: Vec<UnlockReport> = if options["ip_unlocks"].is_null() {
            Vec::new()
        } else {
            serde_json::from_value(options["ip_unlocks"].clone())
                .map_err(|_| StorageError::InvalidInput("invalid stored unlock reports"))?
        };
        reports.retain(|old| old.family != report.family);
        reports.push(report);
        reports.sort_by_key(|entry| entry.family);
        options["ip_unlocks"] = json!(reports);
        tx.execute(
            "UPDATE node_options SET data=?2 WHERE node_id=?1",
            params![id, options.to_string()],
        )?;
        crate::control::configuration_audit(tx, "node.ip_unlock.detected", id)?;
        Ok(())
    }
}

pub(crate) fn decorate(options: &crate::control::NodeOptions, value: &mut Value, now: u64) {
    let reports: Vec<Value> = options
        .ip_unlocks
        .iter()
        .map(|report| {
            let registered = if report.family == 4 {
                &options.ipv4
            } else {
                &options.ipv6
            };
            let matches = registered
                .parse::<IpAddr>()
                .ok()
                .is_some_and(|ip| Some(ip) == report.address.parse::<IpAddr>().ok());
            json!({
                "family": report.family,
                "checked_at_unix_ms": report.checked_at_unix_ms,
                "stale": now.saturating_sub(report.checked_at_unix_ms) > MAX_AGE_MS,
                "address_changed": !matches,
                "google": if matches {
                    json!(report.google)
                } else {
                    json!({"status": "unknown"})
                },
                "services": if matches {
                    json!(report.services)
                } else {
                    json!([])
                },
            })
        })
        .collect();
    value["ip_unlocks"] = json!(reports);
}

pub(crate) fn migrate(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE ip_unlock_collectors (
            node_id TEXT PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
            agent_version TEXT NOT NULL,
            available INTEGER NOT NULL CHECK(available IN (0,1))
        );
        CREATE TABLE ip_unlock_checks (
            node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
            family INTEGER NOT NULL CHECK(family IN(4,6)),
            id TEXT NOT NULL UNIQUE,
            address TEXT NOT NULL,
            state TEXT NOT NULL CHECK(state IN('pending','running','succeeded','failed')),
            error TEXT NOT NULL DEFAULT '',
            created_at_ms INTEGER NOT NULL,
            started_at_ms INTEGER NOT NULL DEFAULT 0,
            finished_at_ms INTEGER NOT NULL DEFAULT 0,
            expires_at_ms INTEGER NOT NULL,
            PRIMARY KEY(node_id,family)
        );",
    )
}

pub(crate) fn decorate_checks(
    db: &Connection,
    node: &str,
    value: &mut Value,
    now: u64,
) -> Result<(), StorageError> {
    let available: Option<i64> = db
        .query_row(
            "SELECT c.available FROM ip_unlock_collectors c \
             JOIN nodes n ON n.id=c.node_id \
             WHERE c.node_id=?1 AND c.agent_version=n.agent_version",
            [node],
            |r| r.get(0),
        )
        .optional()?;
    value["ip_unlock_capability"] = json!(match available {
        Some(1) => "ready",
        Some(_) => "unavailable",
        None => "upgrade_required",
    });
    let mut stmt = db.prepare(
        "SELECT family,state,error,created_at_ms,finished_at_ms,expires_at_ms \
         FROM ip_unlock_checks WHERE node_id=?1 ORDER BY family",
    )?;
    let rows = stmt.query_map([node], |r| {
        Ok((
            r.get::<_, u8>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, i64>(5)?,
        ))
    })?;
    let mut checks = Vec::new();
    for row in rows {
        let (family, state, error, created, finished, lease_end) = row?;
        let timed_out =
            matches!(state.as_str(), "pending" | "running") && unsigned(lease_end) <= now;
        checks.push(json!({
            "family": family,
            "state": if timed_out { "failed" } else { &state },
            "error": if timed_out { "timeout" } else { &error },
            "created_at_unix_ms": unsigned(created),
            "finished_at_unix_ms": unsigned(finished),
        }));
    }
    value["ip_unlock_checks"] = json!(checks);
    Ok(())
}

impl Storage {
    pub(crate) fn unlock_status(&self, node: &str, now: u64) -> Result<Value, StorageError> {
        let db = self.connection()?;
        let mut value = json!({});
        let raw: String = db
            .query_row(
                "SELECT data FROM node_options o JOIN nodes n ON n.id=o.node_id \
                 WHERE node_id=?1 AND n.disabled_at_ms IS NULL",
                [node],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(StorageError::NotFound)?;
        let options: crate::control::NodeOptions = serde_json::from_str(&raw)
            .map_err(|_| StorageError::InvalidInput("invalid node options"))?;
        decorate(&options, &mut value, now);
        decorate_checks(&db, node, &mut value, now)?;
        Ok(value)
    }

    fn start_unlock_check(
        &self,
        node: &str,
        family: u8,
        now: u64,
        offline: u64,
    ) -> Result<Value, StorageError> {
        if !matches!(family, 4 | 6) {
            return Err(StorageError::InvalidInput("invalid address family"));
        }
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let offline_after_ms =
            crate::control::effective_offline_ms(&tx, offline.max(60).saturating_mul(1000))?;
        let ready: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM ip_unlock_collectors c \
             JOIN nodes n ON n.id=c.node_id \
             JOIN metrics.node_state s ON s.node_id=n.id \
             WHERE n.id=?1 AND n.disabled_at_ms IS NULL AND c.available=1 \
             AND c.agent_version=n.agent_version AND s.last_seen_at_ms>?2)",
            params![node, integer(now.saturating_sub(offline_after_ms))],
            |r| r.get(0),
        )?;
        if !ready {
            return Err(StorageError::Conflict("unlock_collector_unavailable"));
        }
        let key = if family == 4 { "ipv4" } else { "ipv6" };
        let raw: String = tx
            .query_row(
                "SELECT data FROM node_options WHERE node_id=?1",
                [node],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(StorageError::InvalidInput("unlock_address_unavailable"))?;
        let options: Value = serde_json::from_str(&raw)
            .map_err(|_| StorageError::InvalidInput("invalid node options"))?;
        let address = options[key]
            .as_str()
            .and_then(|v| v.parse::<IpAddr>().ok())
            .filter(|ip| ip.is_ipv4() == (family == 4) && crate::ip_info::public_ip(*ip))
            .ok_or(StorageError::InvalidInput("unlock_address_unavailable"))?;
        let busy: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM ip_unlock_checks \
             WHERE node_id=?1 AND state IN('pending','running') AND expires_at_ms>?2)",
            params![node, integer(now)],
            |r| r.get(0),
        )?;
        if busy {
            return Err(StorageError::Conflict("unlock_check_busy"));
        }
        let limited: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM ip_unlock_checks \
             WHERE node_id=?1 AND family=?2 AND created_at_ms>?3) \
             OR (SELECT count(*) FROM ip_unlock_checks \
                 WHERE state IN('pending','running') AND expires_at_ms>?4)>=8",
            params![
                node,
                family,
                integer(now.saturating_sub(60_000)),
                integer(now)
            ],
            |r| r.get(0),
        )?;
        if limited {
            return Err(StorageError::RateLimited);
        }
        tx.execute(
            "INSERT INTO ip_unlock_checks(node_id,family,id,address,state,created_at_ms,\
             expires_at_ms) VALUES(?1,?2,?3,?4,'pending',?5,?6) \
             ON CONFLICT(node_id,family) DO UPDATE SET \
               id=excluded.id,address=excluded.address,state='pending',error='',\
               created_at_ms=excluded.created_at_ms,started_at_ms=0,finished_at_ms=0,\
               expires_at_ms=excluded.expires_at_ms",
            params![
                node,
                family,
                Uuid::new_v4().to_string(),
                address.to_string(),
                integer(now),
                integer(now.saturating_add(180_000))
            ],
        )?;
        crate::control::configuration_audit(&tx, "node.ip_unlock.start", node)?;
        tx.commit()?;
        drop(db);
        self.unlock_status(node, now)
    }

    fn claim_unlock_check(
        &self,
        token: &str,
        input: &IpUnlockClaim,
        now: u64,
    ) -> Result<Option<IpUnlockTask>, StorageError> {
        if input.schema_version != IP_UNLOCK_SCHEMA_VERSION
            || input.agent_version.is_empty()
            || input.agent_version.len() > 64
        {
            return Err(StorageError::InvalidInput("invalid unlock collector"));
        }
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let node = crate::control::authenticated_node(&tx, token)?;
        tx.execute(
            "INSERT INTO ip_unlock_collectors(node_id,agent_version,available) VALUES(?1,?2,?3) \
             ON CONFLICT(node_id) DO UPDATE SET \
               agent_version=excluded.agent_version,available=excluded.available \
             WHERE agent_version<>excluded.agent_version OR available<>excluded.available",
            params![node, input.agent_version, input.available],
        )?;
        let active: u32 = tx.query_row(
            "SELECT count(*) FROM ip_unlock_checks WHERE state='running' AND expires_at_ms>?1",
            [integer(now)],
            |r| r.get(0),
        )?;
        let task: Option<IpUnlockTask> = if input.available && active < 4 {
            tx.query_row(
                "SELECT id,family,address FROM ip_unlock_checks \
                 WHERE node_id=?1 AND state='pending' AND expires_at_ms>?2 \
                 ORDER BY created_at_ms LIMIT 1",
                params![node, integer(now)],
                |r| {
                    Ok(IpUnlockTask {
                        id: r.get(0)?,
                        family: r.get(1)?,
                        address: r.get(2)?,
                        expires_at_unix_ms: now.saturating_add(120_000),
                    })
                },
            )
            .optional()?
        } else {
            None
        };
        if let Some(task) = &task {
            tx.execute(
                "UPDATE ip_unlock_checks SET state='running',started_at_ms=?2,expires_at_ms=?3 \
                 WHERE id=?1",
                params![task.id, integer(now), integer(task.expires_at_unix_ms)],
            )?;
        }
        tx.commit()?;
        Ok(task)
    }

    fn finish_unlock_check(
        &self,
        token: &str,
        input: IpUnlockOutcome,
        now: u64,
    ) -> Result<(), StorageError> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let node = crate::control::authenticated_node(&tx, token)?;
        let job: Option<(u8, String, i64)> = tx
            .query_row(
                "SELECT family,address,started_at_ms FROM ip_unlock_checks \
                 WHERE id=?1 AND node_id=?2 AND state='running' AND expires_at_ms>?3",
                params![input.id, node, integer(now)],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (family, address, started) =
            job.ok_or(StorageError::Conflict("unlock_check_expired"))?;
        let mut error = input.error.unwrap_or_default();
        if ![
            "",
            "invalid_task",
            "unavailable",
            "address_mismatch",
            "collection_failed",
            "timeout",
        ]
        .contains(&error.as_str())
        {
            return Err(StorageError::InvalidInput("invalid unlock error"));
        }
        let raw: String = tx.query_row(
            "SELECT data FROM node_options WHERE node_id=?1",
            [&node],
            |r| r.get(0),
        )?;
        let options: Value = serde_json::from_str(&raw)
            .map_err(|_| StorageError::InvalidInput("invalid node options"))?;
        if !options[if family == 4 { "ipv4" } else { "ipv6" }]
            .as_str()
            .and_then(|v| v.parse::<IpAddr>().ok())
            .is_some_and(|ip| Some(ip) == address.parse::<IpAddr>().ok())
        {
            "address_changed".clone_into(&mut error);
        }
        if error.is_empty() {
            let report = input
                .report
                .ok_or(StorageError::InvalidInput("missing unlock result"))?;
            if report.family != family
                || report.address != address
                || report.checked_at_unix_ms.saturating_add(60_000) < unsigned(started)
            {
                return Err(StorageError::InvalidInput(
                    "unlock result does not match task",
                ));
            }
            Self::save_ip_unlock_tx(&tx, &node, report, now)?;
        }
        tx.execute(
            "UPDATE ip_unlock_checks SET state=?2,error=?3,finished_at_ms=?4 WHERE id=?1",
            params![
                input.id,
                if error.is_empty() {
                    "succeeded"
                } else {
                    "failed"
                },
                error,
                integer(now)
            ],
        )?;
        crate::control::configuration_audit(&tx, "node.ip_unlock.finish", &node)?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::NodeOptions;
    use crate::storage::{SCHEMA_VERSION, hash_token};
    use senlun_protocol::{EnrollmentRequest, GoogleUnlockStatus, PROTOCOL_VERSION};
    use std::path::{Path, PathBuf};

    fn migration_backups(directory: &Path) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.is_dir()
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("senlun.db.backup-"))
            })
            .collect();
        paths.sort();
        paths
    }

    fn schema_nine_fixture(directory: &Path) -> PathBuf {
        let path = directory.join("senlun.db");
        let storage = Storage::open(&path, 64 * 1024 * 1024).unwrap();
        drop(storage);
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "DROP TABLE ip_unlock_checks;
             DROP TABLE ip_unlock_collectors;
             PRAGMA user_version=9;",
        )
        .unwrap();
        path
    }

    fn sample_report(address: &str, checked_at_unix_ms: u64) -> UnlockReport {
        let services = SERVICES
            .iter()
            .map(|id| UnlockServiceStatus {
                id: (*id).to_owned(),
                status: if *id == "reddit" {
                    "app_only".to_owned()
                } else {
                    "unlocked".to_owned()
                },
                region: if *id == "reddit" {
                    None
                } else {
                    Some("HK".to_owned())
                },
                access_type: None,
            })
            .collect();
        UnlockReport {
            family: 4,
            address: address.to_owned(),
            checked_at_unix_ms,
            google: GoogleUnlockStatus {
                status: "normal".to_owned(),
                region: Some("HK".to_owned()),
            },
            services,
        }
    }

    #[test]
    fn migrates_v9_to_v10_with_backup_and_fails_closed_on_collision() {
        let fail_dir = tempfile::tempdir().unwrap();
        let fail_path = schema_nine_fixture(fail_dir.path());
        let db = Connection::open(&fail_path).unwrap();
        db.execute_batch("CREATE TABLE ip_unlock_collectors (collision INTEGER);")
            .unwrap();
        drop(db);
        assert!(Storage::open(&fail_path, 64 * 1024 * 1024).is_err());
        let db = Connection::open(&fail_path).unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            9
        );
        assert_eq!(migration_backups(fail_dir.path()).len(), 1);

        let ok_dir = tempfile::tempdir().unwrap();
        let ok_path = schema_nine_fixture(ok_dir.path());
        let storage = Storage::open(&ok_path, 64 * 1024 * 1024).unwrap();
        let version: i64 = storage
            .connection()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        drop(storage);

        let backups = migration_backups(ok_dir.path());
        assert_eq!(backups.len(), 1);
        let bytes = std::fs::read(backups[0].join("manifest.json")).unwrap();
        let manifest: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(manifest["control_schema"], 9);

        let reopened = Storage::open(&ok_path, 64 * 1024 * 1024).unwrap();
        drop(reopened);
        assert_eq!(migration_backups(ok_dir.path()).len(), 1);
    }

    fn setup_node(storage: &Storage, now: u64) -> (String, String, NodeOptions) {
        let secret = storage.create_enrollment(600, now).unwrap();
        let enrolled = storage
            .enroll(
                &EnrollmentRequest {
                    protocol_version: PROTOCOL_VERSION,
                    node_name: "Unlock QA".to_owned(),
                    agent_version: "0.1.0-alpha.70".to_owned(),
                    region: "HK".to_owned(),
                    group: String::new(),
                },
                &hash_token(&secret.token),
                100,
                now,
            )
            .unwrap();
        storage
            .connection()
            .unwrap()
            .execute(
                "INSERT INTO metrics.node_state(node_id,last_seen_at_ms) VALUES(?1,?2)",
                params![enrolled.node_id, integer(now)],
            )
            .unwrap();
        let mut options = NodeOptions {
            id: enrolled.node_id.clone(),
            name: "Unlock QA".to_owned(),
            ipv4: "1.1.1.1".to_owned(),
            currency: "CNY".to_owned(),
            traffic_limit_type: "sum".to_owned(),
            traffic_reset_day: 1,
            ..NodeOptions::default()
        };
        storage
            .save_node_options(&enrolled.node_id, &mut options)
            .unwrap();
        (enrolled.node_id, enrolled.agent_token, options)
    }

    #[test]
    fn unlock_lifecycle_preserves_report_and_invalidates_on_ip_change() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(&dir.path().join("senlun.db"), 64 * 1024 * 1024).unwrap();
        let now = 1_800_000_000_000_u64;
        let (node_id, agent_token, mut options) = setup_node(&storage, now);

        let claim = IpUnlockClaim {
            schema_version: IP_UNLOCK_SCHEMA_VERSION,
            agent_version: "0.1.0-alpha.70".to_owned(),
            available: true,
        };
        assert!(
            storage
                .claim_unlock_check(&agent_token, &claim, now)
                .unwrap()
                .is_none()
        );

        storage.start_unlock_check(&node_id, 4, now, 90).unwrap();
        let task = storage
            .claim_unlock_check(&agent_token, &claim, now + 1000)
            .unwrap()
            .unwrap();
        assert_eq!(task.family, 4);
        assert_eq!(task.address, "1.1.1.1");

        // Reject private IPs and incomplete service sets before persistence.
        let private_report = sample_report("127.0.0.1", now + 2000);
        assert!(validate_report(&private_report, now + 2000).is_err());
        let mut short = sample_report("1.1.1.1", now + 2000);
        short.services.pop();
        assert!(validate_report(&short, now + 2000).is_err());

        storage
            .finish_unlock_check(
                &agent_token,
                IpUnlockOutcome {
                    id: task.id,
                    report: Some(sample_report("1.1.1.1", now + 2000)),
                    error: None,
                },
                now + 3000,
            )
            .unwrap();

        // Ordinary node metadata saves preserve stored ip_unlocks and ignore caller values.
        options.name = "Unlock QA Renamed".to_owned();
        options.ip_unlocks.clear();
        storage.save_node_options(&node_id, &mut options).unwrap();
        let status = storage.unlock_status(&node_id, now + 4000).unwrap();
        assert_eq!(status["ip_unlocks"][0]["google"]["region"], "HK");
        assert!(status["ip_unlocks"][0].get("address").is_none());
        assert_eq!(status["ip_unlocks"][0]["address_changed"], false);

        // Changing registered IP invalidates displayed unlock results without leaking old IP.
        options.ipv4 = "8.8.8.8".to_owned();
        storage.save_node_options(&node_id, &mut options).unwrap();
        let invalidated = storage.unlock_status(&node_id, now + 5000).unwrap();
        assert_eq!(invalidated["ip_unlocks"][0]["address_changed"], true);
        assert_eq!(invalidated["ip_unlocks"][0]["google"]["status"], "unknown");
        assert_eq!(invalidated["ip_unlocks"][0]["services"], json!([]));
    }
}
