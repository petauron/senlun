//! Administrator-triggered, bounded Shanghai TCP monitoring.
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
use rusqlite::{Connection, Transaction};
use rusqlite::{OptionalExtension, params};
use senlun_protocol::{
    RETURN_ROUTE_SCHEMA_VERSION, ReturnRouteClaim, ReturnRouteOutcome, ReturnRouteTask,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::net::IpAddr;
use uuid::Uuid;

pub(crate) const COLLECTOR_REVISION: &str = "db52fc60523320fc4a34b5c369f28be38df21d13";
const MAX_AGE_MS: u64 = 7 * 86_400_000;

pub(crate) use senlun_protocol::ReturnRouteReport as RouteReport;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/nodes/{id}/return-routes", get(status))
        .route("/api/admin/nodes/{id}/return-routes/check", post(start))
        .route("/api/v1/agents/return-routes/claim", post(claim))
        .route("/api/v1/agents/return-routes/result", post(finish))
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
            .database(move |storage| storage.route_status(&id, now))
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
            .database(move |storage| storage.start_route_check(&id, input.family, now, offline))
            .await?,
    ))
}
async fn claim(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ReturnRouteClaim>,
) -> Result<Json<Option<ReturnRouteTask>>, ApiError> {
    let token = crate::bearer_token(&headers)?.to_owned();
    let now = crate::current_time()?;
    Ok(Json(
        state
            .database(move |storage| storage.claim_route_check(&token, input, now))
            .await?,
    ))
}
async fn finish(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ReturnRouteOutcome>,
) -> Result<Json<Value>, ApiError> {
    let token = crate::bearer_token(&headers)?.to_owned();
    let now = crate::current_time()?;
    state
        .database(move |storage| storage.finish_route_check(&token, input, now))
        .await?;
    Ok(Json(json!({"accepted":true})))
}

impl Storage {
    fn save_return_route_tx(
        tx: &Transaction<'_>,
        id: &str,
        report: RouteReport,
        now: u64,
    ) -> Result<Value, StorageError> {
        let address = report
            .address
            .parse::<IpAddr>()
            .map_err(|_| StorageError::InvalidInput("invalid route address"))?;
        let family_valid = match report.family {
            4 => address.is_ipv4(),
            6 => address.is_ipv6(),
            _ => false,
        };
        if !family_valid
            || report.region != "SH"
            || report.protocol != "tcp"
            || report.source != "TcpQuality"
            || report.revision != COLLECTOR_REVISION
            || report.checked_at_unix_ms == 0
            || report.checked_at_unix_ms > now.saturating_add(60_000)
            || report.carriers.len() != 3
        {
            return Err(StorageError::InvalidInput(
                "invalid Shanghai TCP route report",
            ));
        }
        for carrier in ["telecom", "unicom", "mobile"] {
            let entries: Vec<_> = report
                .carriers
                .iter()
                .filter(|entry| entry.carrier == carrier)
                .collect();
            if entries.len() != 1 {
                return Err(StorageError::InvalidInput(
                    "three unique carriers are required",
                ));
            }
            let entry = entries[0];
            if !["ok", "unknown", "failed"].contains(&entry.status.as_str())
                || entry.label.is_empty()
                || entry.label.len() > 64
                || entry.label.chars().any(char::is_control)
                || entry.target.is_empty()
                || entry.target.len() > 253
                || !entry
                    .target
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b".-:".contains(&c))
            {
                return Err(StorageError::InvalidInput("invalid carrier route"));
            }
        }
        let raw: Option<String> = tx.query_row("SELECT o.data FROM node_options o JOIN nodes n ON n.id=o.node_id WHERE o.node_id=?1 AND n.disabled_at_ms IS NULL", [id], |row| row.get(0)).optional()?;
        let raw = raw.ok_or(StorageError::InvalidInput(
            "configure node public addresses before detecting",
        ))?;
        let mut options: Value = serde_json::from_str(&raw)
            .map_err(|_| StorageError::InvalidInput("invalid stored node options"))?;
        let key = if report.family == 4 { "ipv4" } else { "ipv6" };
        if options[key].as_str().and_then(|v| v.parse::<IpAddr>().ok()) != Some(address) {
            return Err(StorageError::InvalidInput(
                "route address is not registered to this node",
            ));
        }
        let mut reports: Vec<RouteReport> = if options["return_routes"].is_null() {
            Vec::new()
        } else {
            serde_json::from_value(options["return_routes"].clone())
                .map_err(|_| StorageError::InvalidInput("invalid stored route reports"))?
        };
        reports.retain(|old| old.family != report.family);
        reports.push(report);
        reports.sort_by_key(|entry| entry.family);
        options["return_routes"] = json!(reports);
        tx.execute(
            "UPDATE node_options SET data=?2 WHERE node_id=?1",
            params![id, options.to_string()],
        )?;
        crate::control::configuration_audit(tx, "node.return_routes.detected", id)?;
        let saved: crate::control::NodeOptions = serde_json::from_value(options)
            .map_err(|_| StorageError::InvalidInput("invalid saved node options"))?;
        let mut response = json!({});
        decorate(&saved, &mut response, now);
        Ok(response["return_routes"].take())
    }
}

pub(crate) fn decorate(options: &crate::control::NodeOptions, value: &mut Value, now: u64) {
    let reports: Vec<Value> = options.return_routes.iter().map(|report| {
        let registered = if report.family == 4 {&options.ipv4} else {&options.ipv6};
        let matches = registered.parse::<IpAddr>().ok() == report.address.parse::<IpAddr>().ok();
        json!({"family":report.family,"region":"SH","protocol":"tcp","source":"TcpQuality",
            "checked_at_unix_ms":report.checked_at_unix_ms,
            "stale":now.saturating_sub(report.checked_at_unix_ms) > MAX_AGE_MS,
            "address_changed":!matches,"carriers":if matches {json!(report.carriers)}else{json!([])}})
    }).collect();
    value["return_routes"] = json!(reports);
}

pub(crate) fn migrate(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch("CREATE TABLE return_route_collectors (
        node_id TEXT PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
        agent_version TEXT NOT NULL, available INTEGER NOT NULL CHECK(available IN (0,1))
      );
      CREATE TABLE return_route_checks (
        node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
        family INTEGER NOT NULL CHECK(family IN(4,6)), id TEXT NOT NULL UNIQUE,
        address TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN('pending','running','succeeded','failed')),
        error TEXT NOT NULL DEFAULT '', created_at_ms INTEGER NOT NULL,
        started_at_ms INTEGER NOT NULL DEFAULT 0, finished_at_ms INTEGER NOT NULL DEFAULT 0,
        expires_at_ms INTEGER NOT NULL, PRIMARY KEY(node_id,family)
      );")
}

pub(crate) fn decorate_checks(
    db: &Connection,
    node: &str,
    value: &mut Value,
    now: u64,
) -> Result<(), StorageError> {
    let available: Option<i64> = db.query_row(
        "SELECT c.available FROM return_route_collectors c JOIN nodes n ON n.id=c.node_id WHERE c.node_id=?1 AND c.agent_version=n.agent_version", [node], |r| r.get(0)
    ).optional()?;
    value["return_route_capability"] = json!(match available {
        Some(1) => "ready",
        Some(_) => "unavailable",
        None => "upgrade_required",
    });
    let mut stmt = db.prepare("SELECT family,state,error,created_at_ms,finished_at_ms,expires_at_ms FROM return_route_checks WHERE node_id=?1 ORDER BY family")?;
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
        checks.push(json!({"family":family,"state":if timed_out {"failed"}else{&state},"error":if timed_out {"timeout"}else{&error},"created_at_unix_ms":unsigned(created),"finished_at_unix_ms":unsigned(finished)}));
    }
    value["return_route_checks"] = json!(checks);
    Ok(())
}

impl Storage {
    pub(crate) fn route_status(&self, node: &str, now: u64) -> Result<Value, StorageError> {
        let db = self.connection()?;
        let mut value = json!({});
        let raw: String = db.query_row("SELECT data FROM node_options o JOIN nodes n ON n.id=o.node_id WHERE node_id=?1 AND n.disabled_at_ms IS NULL",[node],|r|r.get(0)).optional()?.ok_or(StorageError::NotFound)?;
        let options: crate::control::NodeOptions = serde_json::from_str(&raw)
            .map_err(|_| StorageError::InvalidInput("invalid node options"))?;
        decorate(&options, &mut value, now);
        decorate_checks(&db, node, &mut value, now)?;
        Ok(value)
    }

    fn start_route_check(
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
        let ready: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM return_route_collectors c JOIN nodes n ON n.id=c.node_id JOIN metrics.node_state s ON s.node_id=n.id WHERE n.id=?1 AND n.disabled_at_ms IS NULL AND c.available=1 AND c.agent_version=n.agent_version AND s.last_seen_at_ms>?2)",params![node,integer(now.saturating_sub(offline.max(60)*1000))],|r|r.get(0))?;
        if !ready {
            return Err(StorageError::Conflict("route_collector_unavailable"));
        }
        let key = if family == 4 { "ipv4" } else { "ipv6" };
        let raw: String = tx.query_row(
            "SELECT data FROM node_options WHERE node_id=?1",
            [node],
            |r| r.get(0),
        )?;
        let options: Value = serde_json::from_str(&raw)
            .map_err(|_| StorageError::InvalidInput("invalid node options"))?;
        let address = options[key]
            .as_str()
            .and_then(|v| v.parse::<IpAddr>().ok())
            .filter(|ip| ip.is_ipv4() == (family == 4))
            .ok_or(StorageError::InvalidInput("route_address_unavailable"))?;
        let busy: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM return_route_checks WHERE node_id=?1 AND state IN('pending','running') AND expires_at_ms>?2)",params![node,integer(now)],|r|r.get(0))?;
        if busy {
            return Err(StorageError::Conflict("route_check_busy"));
        }
        let limited: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM return_route_checks WHERE node_id=?1 AND family=?2 AND created_at_ms>?3) OR (SELECT count(*) FROM return_route_checks WHERE state IN('pending','running') AND expires_at_ms>?4)>=8",params![node,family,integer(now.saturating_sub(60_000)),integer(now)],|r|r.get(0))?;
        if limited {
            return Err(StorageError::RateLimited);
        }
        tx.execute("INSERT INTO return_route_checks(node_id,family,id,address,state,created_at_ms,expires_at_ms) VALUES(?1,?2,?3,?4,'pending',?5,?6) ON CONFLICT(node_id,family) DO UPDATE SET id=excluded.id,address=excluded.address,state='pending',error='',created_at_ms=excluded.created_at_ms,started_at_ms=0,finished_at_ms=0,expires_at_ms=excluded.expires_at_ms",params![node,family,Uuid::new_v4().to_string(),address.to_string(),integer(now),integer(now+600_000)])?;
        crate::control::configuration_audit(&tx, "node.return_routes.start", node)?;
        tx.commit()?;
        drop(db);
        self.route_status(node, now)
    }

    fn claim_route_check(
        &self,
        token: &str,
        input: ReturnRouteClaim,
        now: u64,
    ) -> Result<Option<ReturnRouteTask>, StorageError> {
        if input.schema_version != RETURN_ROUTE_SCHEMA_VERSION
            || input.agent_version.is_empty()
            || input.agent_version.len() > 64
        {
            return Err(StorageError::InvalidInput("invalid route collector"));
        }
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let node = crate::control::authenticated_node(&tx, token)?;
        tx.execute("INSERT INTO return_route_collectors(node_id,agent_version,available) VALUES(?1,?2,?3) ON CONFLICT(node_id) DO UPDATE SET agent_version=excluded.agent_version,available=excluded.available WHERE agent_version<>excluded.agent_version OR available<>excluded.available",params![node,input.agent_version,input.available])?;
        let active: u32 = tx.query_row(
            "SELECT count(*) FROM return_route_checks WHERE state='running' AND expires_at_ms>?1",
            [integer(now)],
            |r| r.get(0),
        )?;
        let task: Option<ReturnRouteTask> = if input.available && active < 2 {
            tx.query_row("SELECT id,family,address FROM return_route_checks WHERE node_id=?1 AND state='pending' AND expires_at_ms>?2 ORDER BY created_at_ms LIMIT 1",params![node,integer(now)],|r|Ok(ReturnRouteTask{id:r.get(0)?,family:r.get(1)?,address:r.get(2)?,expires_at_unix_ms:now+480_000})).optional()?
        } else {
            None
        };
        if let Some(task) = &task {
            tx.execute("UPDATE return_route_checks SET state='running',started_at_ms=?2,expires_at_ms=?3 WHERE id=?1",params![task.id,integer(now),integer(task.expires_at_unix_ms)])?;
        }
        tx.commit()?;
        Ok(task)
    }

    fn finish_route_check(
        &self,
        token: &str,
        input: ReturnRouteOutcome,
        now: u64,
    ) -> Result<(), StorageError> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let node = crate::control::authenticated_node(&tx, token)?;
        let job: Option<(u8,String,i64)> = tx.query_row("SELECT family,address,started_at_ms FROM return_route_checks WHERE id=?1 AND node_id=?2 AND state='running' AND expires_at_ms>?3",params![input.id,node,integer(now)],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let (family, address, started) =
            job.ok_or(StorageError::Conflict("route_check_expired"))?;
        let mut error = input.error.unwrap_or_default();
        if ![
            "",
            "invalid_task",
            "unavailable",
            "collection_failed",
            "response_too_large",
            "timeout",
            "invalid_result",
        ]
        .contains(&error.as_str())
        {
            return Err(StorageError::InvalidInput("invalid route error"));
        }
        let raw: String = tx.query_row(
            "SELECT data FROM node_options WHERE node_id=?1",
            [&node],
            |r| r.get(0),
        )?;
        let options: Value = serde_json::from_str(&raw)
            .map_err(|_| StorageError::InvalidInput("invalid node options"))?;
        if options[if family == 4 { "ipv4" } else { "ipv6" }]
            .as_str()
            .and_then(|v| v.parse::<IpAddr>().ok())
            != address.parse::<IpAddr>().ok()
        {
            error = "address_changed".to_owned();
        }
        if error.is_empty() {
            let report = input
                .report
                .ok_or(StorageError::InvalidInput("missing route result"))?;
            if report.family != family
                || report.address != address
                || report.checked_at_unix_ms.saturating_add(60_000) < unsigned(started)
            {
                return Err(StorageError::InvalidInput(
                    "route result does not match task",
                ));
            }
            Self::save_return_route_tx(&tx, &node, report, now)?;
        }
        tx.execute(
            "UPDATE return_route_checks SET state=?2,error=?3,finished_at_ms=?4 WHERE id=?1",
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
        crate::control::configuration_audit(&tx, "node.return_routes.finish", &node)?;
        tx.commit()?;
        Ok(())
    }
}
