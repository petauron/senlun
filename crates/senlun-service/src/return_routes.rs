//! Administrator-imported, address-bound Shanghai TCP route reports.
//! Collection is an explicit operator task, never an Agent command channel.
use crate::{
    ApiError, AppState,
    storage::{Storage, StorageError},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::net::IpAddr;

pub(crate) const COLLECTOR_REVISION: &str = "db52fc60523320fc4a34b5c369f28be38df21d13";
const MAX_AGE_MS: u64 = 7 * 86_400_000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RouteReport {
    pub family: u8,
    pub address: String,
    pub region: String,
    pub protocol: String,
    pub source: String,
    pub revision: String,
    pub checked_at_unix_ms: u64,
    pub carriers: Vec<CarrierRoute>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CarrierRoute {
    pub carrier: String,
    pub target: String,
    pub status: String,
    pub label: String,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new().route("/api/admin/nodes/{id}/return-routes", post(import))
}
async fn import(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(report): Json<RouteReport>,
) -> Result<Json<Value>, ApiError> {
    let now = crate::current_time()?;
    state
        .database(move |storage| storage.save_return_route(&id, report, now))
        .await?;
    Ok(Json(json!({"ok":true})))
}
impl Storage {
    fn save_return_route(
        &self,
        id: &str,
        report: RouteReport,
        now: u64,
    ) -> Result<(), StorageError> {
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
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let raw: Option<String> = tx.query_row("SELECT o.data FROM node_options o JOIN nodes n ON n.id=o.node_id WHERE o.node_id=?1 AND n.disabled_at_ms IS NULL", [id], |row| row.get(0)).optional()?;
        let raw = raw.ok_or(StorageError::InvalidInput(
            "configure node public addresses before importing",
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
        crate::control::configuration_audit(&tx, "node.return_routes.import", id)?;
        tx.commit()?;
        Ok(())
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
