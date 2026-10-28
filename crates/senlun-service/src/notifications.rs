//! Bounded alert evaluation and a durable webhook outbox. No network operation
//! runs while holding the SQLite connection; the Service owns one sender worker.

use std::{collections::HashMap, time::Duration};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::json;
use uuid::Uuid;

use crate::{
    control::{AlertRule, Channel, NodeOptions, TrafficSource, selected_traffic, validate_webhook},
    storage::{Storage, StorageError},
};

const MAX_NODES: usize = 1_000;
const MAX_RULES: usize = 128;
// Every supported node/rule combination has one row; evaluation reads rows
// individually, so increasing persistent capacity does not cache incidents.
const MAX_INCIDENTS: usize = MAX_NODES * MAX_RULES;
const MAX_DELIVERIES: usize = 256;
const DELIVERY_BATCH: usize = 4;
const MAX_ATTEMPTS: u32 = 5;
const DELIVERY_LEASE_MS: u64 = 30_000;
const MAX_PAYLOAD_BYTES: usize = 8192;

pub(crate) struct Delivery {
    pub id: String,
    // Do not derive Debug: webhook URLs may contain administrator-provided keys.
    url: String,
    payload: String,
}

struct NodeState {
    name: String,
    created_at: u64,
    last_seen: Option<u64>,
    cpu: Option<f64>,
    memory: Option<f64>,
    disk: Option<f64>,
    traffic: Option<f64>,
    traffic_source: TrafficSource,
    expiry: Option<u64>,
}

struct Incident {
    id: String,
    status: String,
    opened_at: u64,
    // -1 is a durable recovery notification waiting for outbox capacity.
    last_sent_at: i64,
}

impl Storage {
    pub(crate) fn evaluate_alerts(
        &self,
        now_ms: u64,
        offline_after_ms: u64,
    ) -> Result<(), StorageError> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let offline_after_ms = crate::control::effective_offline_ms(&tx, offline_after_ms)?;
        let enabled: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM alert_rules WHERE json_extract(data,'$.enabled')=1)",
            [],
            |row| row.get(0),
        )?;
        if !enabled {
            tx.execute("DELETE FROM alert_deliveries", [])?;
            tx.execute("UPDATE alert_incidents SET status='resolved',updated_at_ms=?1 WHERE status NOT IN ('resolved','superseded')", [integer(now_ms)])?;
            tx.commit()?;
            return Ok(());
        }
        let nodes = load_nodes(&tx, now_ms)?;
        let occupied: i64 =
            tx.query_row("SELECT count(*) FROM alert_incidents", [], |row| row.get(0))?;
        let mut incident_slots =
            MAX_INCIDENTS.saturating_sub(usize::try_from(occupied).unwrap_or(MAX_INCIDENTS));
        let mut rules = tx.prepare("SELECT data FROM alert_rules ORDER BY id LIMIT ?1")?;
        let records = rules.query_map(
            [i64::try_from(MAX_RULES).expect("rule limit fits SQLite")],
            |row| row.get::<_, String>(0),
        )?;
        for record in records {
            let rule: AlertRule = decode(&record?)?;
            if !rule.enabled {
                tx.execute("DELETE FROM alert_deliveries WHERE incident_id IN (SELECT id FROM alert_incidents WHERE rule_id=?1)", [&rule.id])?;
                tx.execute("UPDATE alert_incidents SET status='resolved',updated_at_ms=?2 WHERE rule_id=?1 AND status NOT IN ('resolved','superseded')", params![rule.id, integer(now_ms)])?;
                continue;
            }
            retire_unassigned(&tx, &rule, &nodes, now_ms)?;
            if rule.node_ids.is_empty() {
                for (node_id, node) in &nodes {
                    evaluate_node(
                        &tx,
                        &rule,
                        node_id,
                        node,
                        now_ms,
                        offline_after_ms,
                        &mut incident_slots,
                    )?;
                }
            } else {
                for node_id in &rule.node_ids {
                    if let Some(node) = nodes.get(node_id) {
                        evaluate_node(
                            &tx,
                            &rule,
                            node_id,
                            node,
                            now_ms,
                            offline_after_ms,
                            &mut incident_slots,
                        )?;
                    }
                }
            }
        }
        drop(rules);
        tx.execute("DELETE FROM alert_deliveries WHERE incident_id IN (SELECT a.id FROM alert_incidents a JOIN nodes n ON n.id=a.node_id WHERE n.disabled_at_ms IS NOT NULL)", [])?;
        tx.execute("UPDATE alert_incidents SET status='resolved',updated_at_ms=?1 WHERE status NOT IN ('resolved','superseded') AND node_id IN (SELECT id FROM nodes WHERE disabled_at_ms IS NOT NULL)", [integer(now_ms)])?;
        tx.commit()?;
        Ok(())
    }

    /// Claim up to four messages with a bounded lease. A restart makes them
    /// eligible again after 30 seconds, without losing persisted attempt counts.
    pub(crate) fn pending_deliveries(&self, now_ms: u64) -> Result<Vec<Delivery>, StorageError> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let mut statement = tx.prepare(
            "SELECT d.id,d.payload,c.data FROM alert_deliveries d
             JOIN alert_channels c ON c.id=d.channel_id
             WHERE d.next_attempt_ms<=?1 AND d.attempts<?2
             ORDER BY d.next_attempt_ms,d.created_at_ms,d.id LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![
                integer(now_ms),
                MAX_ATTEMPTS,
                i64::try_from(DELIVERY_BATCH).expect("delivery batch fits SQLite")
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )?;
        let mut deliveries = Vec::with_capacity(DELIVERY_BATCH);
        for row in rows {
            let (id, payload, raw) = row?;
            let channel: Channel = decode(&raw)?;
            if !channel.enabled {
                tx.execute("DELETE FROM alert_deliveries WHERE id=?1", [&id])?;
                continue;
            }
            validate_webhook(&channel.url)?;
            if channel.kind != "webhook" || payload.len() > MAX_PAYLOAD_BYTES {
                return Err(StorageError::InvalidInput("invalid stored alert delivery"));
            }
            tx.execute(
                "UPDATE alert_deliveries SET next_attempt_ms=?2 WHERE id=?1",
                params![id, integer(now_ms.saturating_add(DELIVERY_LEASE_MS))],
            )?;
            deliveries.push(Delivery {
                id,
                url: channel.url,
                payload,
            });
        }
        drop(statement);
        tx.commit()?;
        Ok(deliveries)
    }

    pub(crate) fn delivery_result(
        &self,
        id: &str,
        success: bool,
        now_ms: u64,
    ) -> Result<(), StorageError> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let delivery: Option<(u32, String, String)> = tx
            .query_row(
                "SELECT attempts,incident_id,channel_id FROM alert_deliveries WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((attempts, incident_id, channel_id)) = delivery else {
            // A newer firing/resolved transition may have replaced this item.
            return Ok(());
        };
        if success || attempts + 1 >= MAX_ATTEMPTS {
            if !success {
                // Keep durable failure evidence while freeing the bounded
                // outbox. Only identifiers enter the audit, never the URL.
                let subject = format!("{incident_id}/{channel_id}/{id}");
                tx.execute("INSERT INTO audit_events(happened_at_ms,action,subject) VALUES(?1,'notification.failed',?2)", params![integer(now_ms),subject])?;
                tx.execute("DELETE FROM audit_events WHERE id IN (SELECT id FROM audit_events WHERE action='notification.failed' ORDER BY id DESC LIMIT -1 OFFSET 1000)", [])?;
                tracing::warn!("alert delivery exhausted its five attempts");
            }
            tx.execute("DELETE FROM alert_deliveries WHERE id=?1", [id])?;
        } else {
            let retry_ms = retry_delay_ms(attempts);
            tx.execute(
                "UPDATE alert_deliveries SET attempts=attempts+1,next_attempt_ms=?2 WHERE id=?1",
                params![id, integer(now_ms.saturating_add(retry_ms))],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

// Source/resource edits end only current traffic evaluations, never monitoring history.
pub(crate) fn retire_traffic_evaluation(
    tx: &Transaction<'_>,
    node_id: &str,
    now: u64,
) -> Result<(), StorageError> {
    tx.execute("DELETE FROM alert_deliveries WHERE incident_id IN (SELECT i.id FROM alert_incidents i JOIN alert_rules r ON r.id=i.rule_id WHERE i.node_id=?1 AND json_extract(r.data,'$.metric')='traffic')", [node_id])?;
    tx.execute("UPDATE alert_incidents SET status='superseded',message=message || ' [Traffic source changed; previous evaluation ended without recovery]',updated_at_ms=?2,last_sent_at_ms=0 WHERE node_id=?1 AND (status IN ('pending','firing') OR last_sent_at_ms<0) AND rule_id IN (SELECT id FROM alert_rules WHERE json_extract(data,'$.metric')='traffic')", params![node_id, integer(now)])?;
    Ok(())
}

// Percentages are approximate display/threshold values; byte counters stay exact in storage.
#[allow(clippy::cast_precision_loss)]
fn load_nodes(db: &Connection, now_ms: u64) -> Result<HashMap<String, NodeState>, StorageError> {
    let mut statement = db.prepare(
        "SELECT n.id,n.name,n.created_at_ms,ns.last_seen_at_ms,o.data,
                coalesce(s.cpu_usage_percent,json_extract(m.data,'$.last.cpu')),
                coalesce(s.memory_used_bytes,json_extract(m.data,'$.last.ram')),
                coalesce(s.memory_total_bytes,json_extract(m.data,'$.last.ram_total')),
                coalesce(s.disk_used_bytes,json_extract(m.data,'$.last.disk')),
                coalesce(s.disk_total_bytes,json_extract(m.data,'$.last.disk_total'))
         FROM nodes n LEFT JOIN node_options o ON o.node_id=n.id
         LEFT JOIN node_state ns ON ns.node_id=n.id
         LEFT JOIN snapshots s ON s.id=(SELECT latest.id FROM snapshots latest
             WHERE latest.node_id=n.id ORDER BY latest.received_at_ms DESC,latest.id DESC LIMIT 1)
         LEFT JOIN snapshot_minutes m ON m.id=(SELECT id FROM snapshot_minutes WHERE node_id=n.id
             ORDER BY bucket_ms DESC,json_extract(data,'$.last_received') DESC,id DESC LIMIT 1)
         WHERE n.disabled_at_ms IS NULL ORDER BY n.id LIMIT ?1",
    )?;
    let mut rows = statement.query([i64::try_from(MAX_NODES).expect("node limit fits SQLite")])?;
    let mut nodes = HashMap::new();
    while let Some(row) = rows.next()? {
        let options: Option<NodeOptions> = row
            .get::<_, Option<String>>(4)?
            .map(|raw| decode(&raw))
            .transpose()?;
        let selection = selected_traffic(db, &row.get::<_, String>(0)?, options.as_ref(), now_ms)?;
        nodes.insert(
            row.get(0)?,
            NodeState {
                name: options
                    .as_ref()
                    .map(|options| options.name.clone())
                    .unwrap_or(row.get(1)?),
                created_at: unsigned(row.get(2)?),
                last_seen: row.get::<_, Option<i64>>(3)?.map(unsigned),
                cpu: row.get(5)?,
                memory: percentage(row.get(6)?, row.get(7)?),
                disk: percentage(row.get(8)?, row.get(9)?),
                traffic: selection.percentage,
                traffic_source: selection.source,
                expiry: options.and_then(|options| options.expired_at_unix_ms),
            },
        );
    }
    Ok(nodes)
}

// Unknown data breaks pending duration and cancels queued traffic transitions,
// but it cannot prove recovery of an already firing incident.
fn pause_evaluation(
    tx: &Transaction<'_>,
    rule: &AlertRule,
    incident: Option<&Incident>,
    now: u64,
) -> Result<(), StorageError> {
    let Some(incident) = incident else {
        return Ok(());
    };
    if rule.metric == "traffic" {
        let removed = tx.execute(
            "DELETE FROM alert_deliveries WHERE incident_id=?1",
            [&incident.id],
        )?;
        if incident.status == "resolved" && incident.last_sent_at < 0 {
            tx.execute("UPDATE alert_incidents SET status='superseded',updated_at_ms=?2,last_sent_at_ms=0 WHERE id=?1", params![incident.id, integer(now)])?;
        } else if removed > 0 {
            tx.execute(
                "UPDATE alert_incidents SET last_sent_at_ms=0 WHERE id=?1",
                [&incident.id],
            )?;
        }
    }
    if incident.status == "pending" {
        let status = if rule.metric == "traffic" {
            "superseded"
        } else {
            "resolved"
        };
        tx.execute(
            "UPDATE alert_incidents SET status=?3,updated_at_ms=?2 WHERE id=?1",
            params![incident.id, integer(now), status],
        )?;
    }
    Ok(())
}

fn evaluate_node(
    tx: &Transaction<'_>,
    rule: &AlertRule,
    node_id: &str,
    node: &NodeState,
    now: u64,
    offline_after: u64,
    incident_slots: &mut usize,
) -> Result<(), StorageError> {
    let old: Option<Incident> = tx.query_row(
        "SELECT id,status,opened_at_ms,last_sent_at_ms FROM alert_incidents WHERE node_id=?1 AND rule_id=?2",
        params![node_id, rule.id], |row| Ok(Incident { id: row.get(0)?, status: row.get(1)?, opened_at: unsigned(row.get(2)?), last_sent_at: row.get(3)? }),
    ).optional()?;
    let (breached, _) = condition(rule, node, now, offline_after);
    let Some(breached) = breached else {
        pause_evaluation(tx, rule, old.as_ref(), now)?;
        return Ok(());
    };
    if !breached {
        if let Some(incident) = old {
            if incident.status == "superseded" {
                return Ok(());
            }
            let message = format!("{}: {} recovered", node.name, rule.name);
            let needs_recovery = incident.status == "firing"
                || (incident.status == "resolved" && incident.last_sent_at < 0);
            if incident.status != "resolved" {
                tx.execute("UPDATE alert_incidents SET status='resolved',message=?2,updated_at_ms=?3,last_sent_at_ms=?4 WHERE id=?1", params![incident.id, message, integer(now), if needs_recovery { -1 } else { incident.last_sent_at }])?;
            }
            if needs_recovery
                && enqueue(
                    tx,
                    &incident.id,
                    rule,
                    node_id,
                    node,
                    "resolved",
                    &message,
                    now,
                )?
            {
                tx.execute(
                    "UPDATE alert_incidents SET last_sent_at_ms=?2 WHERE id=?1",
                    params![incident.id, integer(now)],
                )?;
            }
        }
        return Ok(());
    }
    let mut incident = if let Some(incident) = old {
        incident
    } else {
        if !reserve_incident(tx, incident_slots)? {
            return Ok(());
        }
        let incident = Incident {
            id: Uuid::new_v4().to_string(),
            status: "pending".into(),
            opened_at: now,
            last_sent_at: 0,
        };
        tx.execute("INSERT INTO alert_incidents(id,node_id,rule_id,status,message,opened_at_ms,updated_at_ms,last_sent_at_ms) VALUES(?1,?2,?3,'pending','',?4,?4,0)", params![incident.id, node_id, rule.id, integer(now)])?;
        incident
    };
    if incident.status == "resolved" || incident.status == "superseded" {
        incident.status = "pending".into();
        incident.opened_at = now;
        if rule.metric == "traffic" {
            incident.last_sent_at = 0;
        }
    }
    let ready =
        now.saturating_sub(incident.opened_at) >= rule.duration_seconds.saturating_mul(1000);
    if incident.status == "pending" && ready {
        incident.status = "firing".into();
    }
    let reason = alert_reason(&rule.metric, node.traffic_source);
    let message = format!("{}: {}: {reason}", node.name, rule.name);
    tx.execute("UPDATE alert_incidents SET status=?2,message=?3,opened_at_ms=?4,updated_at_ms=?5,last_sent_at_ms=?6 WHERE id=?1 AND (status!=?2 OR message!=?3 OR opened_at_ms!=?4 OR last_sent_at_ms!=?6)", params![incident.id, incident.status, message, integer(incident.opened_at), integer(now), incident.last_sent_at])?;
    if incident.status == "firing"
        && (incident.last_sent_at <= 0
            || now.saturating_sub(unsigned(incident.last_sent_at))
                >= rule.cooldown_seconds.saturating_mul(1000))
        && enqueue(
            tx,
            &incident.id,
            rule,
            node_id,
            node,
            "firing",
            &message,
            now,
        )?
    {
        tx.execute(
            "UPDATE alert_incidents SET last_sent_at_ms=?2 WHERE id=?1",
            params![incident.id, integer(now)],
        )?;
    }
    Ok(())
}

fn alert_reason(metric: &str, source: TrafficSource) -> &'static str {
    match metric {
        "offline" => "node is offline",
        "cpu" => "CPU usage exceeded the configured threshold",
        "memory" => "memory usage exceeded the configured threshold",
        "disk" => "disk usage exceeded the configured threshold",
        "traffic" if source == TrafficSource::Provider => {
            "provider traffic usage exceeded the configured threshold"
        }
        "traffic" => "local traffic usage exceeded the configured threshold",
        "expiry" => "node expiry is within the configured warning window",
        _ => "monitoring rule triggered",
    }
}

// Elapsed seconds and fractional days are presentation/threshold values, not stored counters.
#[allow(clippy::cast_precision_loss)]
fn condition(
    rule: &AlertRule,
    node: &NodeState,
    now: u64,
    offline_after: u64,
) -> (Option<bool>, Option<f64>) {
    let online = node
        .last_seen
        .is_some_and(|seen| now.saturating_sub(seen) <= offline_after);
    let value = match rule.metric.as_str() {
        "offline" => {
            let elapsed = now.saturating_sub(node.last_seen.unwrap_or(node.created_at));
            return (Some(elapsed > offline_after), Some(elapsed as f64 / 1000.0));
        }
        "cpu" if online => node.cpu,
        "memory" if online => node.memory,
        "disk" if online => node.disk,
        "traffic" => node.traffic,
        "expiry" => {
            let days = node
                .expiry
                .map(|expiry| (expiry as f64 - now as f64) / 86_400_000.0);
            return (days.map(|days| days <= rule.threshold), days);
        }
        _ => None,
    };
    (value.map(|value| value >= rule.threshold), value)
}

fn reserve_incident(tx: &Transaction<'_>, available: &mut usize) -> Result<bool, StorageError> {
    if *available > 0 {
        *available -= 1;
        return Ok(true);
    }
    let removed = tx.execute("DELETE FROM alert_incidents WHERE id IN (SELECT id FROM alert_incidents WHERE status IN ('resolved','superseded') AND NOT EXISTS(SELECT 1 FROM alert_deliveries WHERE incident_id=alert_incidents.id) ORDER BY updated_at_ms,id LIMIT 1)", [])?;
    Ok(removed > 0)
}

#[allow(clippy::too_many_arguments)]
fn enqueue(
    tx: &Transaction<'_>,
    incident_id: &str,
    rule: &AlertRule,
    node_id: &str,
    node: &NodeState,
    event: &str,
    message: &str,
    now: u64,
) -> Result<bool, StorageError> {
    // This is the administrator-approved disclosure: identifiers, names, rule,
    // state, time and a short reason. Never IPs, keys or raw/individual metrics.
    let payload = json!({ "status":event, "incident_id":incident_id, "node_id":node_id,
        "node_name":node.name, "rule_id":rule.id, "rule_name":rule.name,
        "message":message, "occurred_at_unix_ms":now })
    .to_string();
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(StorageError::InvalidInput(
            "alert payload exceeded its bound",
        ));
    }
    let mut channels = Vec::new();
    let mut required_slots = 0_usize;
    for channel_id in &rule.channel_ids {
        let raw: Option<String> = tx
            .query_row(
                "SELECT data FROM alert_channels WHERE id=?1",
                [channel_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(raw) = raw else {
            continue;
        };
        let channel: Channel = decode(&raw)?;
        if !channel.enabled {
            continue;
        }
        let existing: Option<String> = tx
            .query_row(
                "SELECT payload FROM alert_deliveries WHERE incident_id=?1 AND channel_id=?2",
                params![incident_id, channel_id],
                |row| row.get(0),
            )
            .optional()?;
        // A cooldown reminder must not reset a failing delivery's retry count.
        if existing
            .as_deref()
            .map(decode::<serde_json::Value>)
            .transpose()?
            .is_some_and(|body| body["status"].as_str() == Some(event))
        {
            continue;
        }
        if existing.is_none() {
            required_slots += 1;
        }
        channels.push(channel_id);
    }
    let count: i64 = tx.query_row("SELECT count(*) FROM alert_deliveries", [], |row| {
        row.get(0)
    })?;
    let count = usize::try_from(count).unwrap_or(MAX_DELIVERIES);
    if count.saturating_add(required_slots) > MAX_DELIVERIES {
        return Ok(false);
    }
    for channel_id in channels {
        tx.execute("INSERT INTO alert_deliveries(id,incident_id,channel_id,payload,attempts,next_attempt_ms,created_at_ms) VALUES(?1,?2,?3,?4,0,?5,?5) ON CONFLICT(incident_id,channel_id) DO UPDATE SET id=excluded.id,payload=excluded.payload,attempts=0,next_attempt_ms=excluded.next_attempt_ms,created_at_ms=excluded.created_at_ms", params![Uuid::new_v4().to_string(), incident_id, channel_id, payload, integer(now)])?;
    }
    Ok(true)
}

fn retire_unassigned(
    tx: &Transaction<'_>,
    rule: &AlertRule,
    nodes: &HashMap<String, NodeState>,
    now: u64,
) -> Result<(), StorageError> {
    let mut statement =
        tx.prepare("SELECT id,node_id FROM alert_incidents WHERE rule_id=?1 LIMIT ?2")?;
    let rows = statement.query_map(
        params![
            rule.id,
            i64::try_from(MAX_INCIDENTS).expect("incident limit fits SQLite")
        ],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )?;
    for row in rows {
        let (id, node_id) = row?;
        if !nodes.contains_key(&node_id)
            || (!rule.node_ids.is_empty() && !rule.node_ids.contains(&node_id))
        {
            tx.execute("DELETE FROM alert_deliveries WHERE incident_id=?1", [&id])?;
            tx.execute("UPDATE alert_incidents SET status='resolved',updated_at_ms=?2,last_sent_at_ms=0 WHERE id=?1 AND status!='superseded' AND (status!='resolved' OR last_sent_at_ms!=0)", params![id,integer(now)])?;
        }
    }
    Ok(())
}

pub(crate) async fn send_delivery(delivery: &Delivery) -> bool {
    let Ok(url) = validate_webhook(&delivery.url) else {
        return false;
    };
    let Ok(client) = reqwest::Client::builder()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(5))
        .pool_max_idle_per_host(0)
        .build()
    else {
        return false;
    };
    client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(delivery.payload.clone())
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

fn retry_delay_ms(previous_failures: u32) -> u64 {
    5_000_u64.saturating_mul(1_u64 << previous_failures.min(MAX_ATTEMPTS))
}

fn percentage(used: Option<f64>, total: Option<f64>) -> Option<f64> {
    let (used, total) = (used?, total?);
    (used.is_finite() && total.is_finite() && total > 0.0).then_some(used / total * 100.0)
}

fn decode<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T, StorageError> {
    serde_json::from_str(raw)
        .map_err(|_| StorageError::InvalidInput("stored alert configuration is invalid"))
}

fn integer(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}
fn unsigned(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node() -> NodeState {
        NodeState {
            name: "example".into(),
            created_at: 1,
            last_seen: Some(1000),
            cpu: Some(90.0),
            memory: Some(80.0),
            disk: None,
            traffic: Some(99.0),
            traffic_source: TrafficSource::Local,
            expiry: Some(86_400_000),
        }
    }

    fn rule(metric: &str) -> AlertRule {
        AlertRule {
            id: "rule".into(),
            name: "example".into(),
            node_ids: Vec::new(),
            metric: metric.into(),
            threshold: 85.0,
            duration_seconds: 10,
            cooldown_seconds: 60,
            channel_ids: Vec::new(),
            enabled: true,
        }
    }

    #[test]
    fn stale_or_missing_usage_is_not_a_proven_recovery() {
        assert_eq!(condition(&rule("cpu"), &node(), 1000, 90_000).0, Some(true));
        assert_eq!(condition(&rule("cpu"), &node(), 100_000, 90_000).0, None);
        assert_eq!(condition(&rule("disk"), &node(), 1000, 90_000).0, None);
        assert_eq!(
            condition(&rule("offline"), &node(), 100_000, 90_000).0,
            Some(true)
        );
    }

    #[test]
    fn expiry_uses_days_remaining_and_percentages_require_a_denominator() {
        let mut expiry = rule("expiry");
        expiry.threshold = 1.0;
        assert_eq!(condition(&expiry, &node(), 0, 90_000).0, Some(true));
        assert_eq!(percentage(Some(50.0), Some(100.0)), Some(50.0));
        assert_eq!(percentage(Some(0.0), Some(0.0)), None);
        assert_eq!(percentage(None, Some(100.0)), None);
    }

    #[test]
    fn retries_are_bounded_exponential_delays() {
        assert_eq!(
            (0..4).map(retry_delay_ms).collect::<Vec<_>>(),
            vec![5000, 10_000, 20_000, 40_000]
        );
    }

    #[test]
    fn offline_traffic_rolls_over_on_the_configured_billing_day() {
        let (_directory, storage, node_id) = configured_storage();
        let mut options = NodeOptions {
            id: node_id.clone(),
            name: "test-node".into(),
            currency: "USD".into(),
            traffic_limit_bytes: 100,
            traffic_limit_type: "sum".into(),
            traffic_reset_day: 15,
            ..NodeOptions::default()
        };
        storage.save_node_options(&node_id, &mut options).unwrap();
        let billing_boundary = u64::try_from(
            time::Date::from_calendar_date(2026, time::Month::September, 15)
                .unwrap()
                .midnight()
                .assume_utc()
                .unix_timestamp(),
        )
        .unwrap()
            * 1000;
        let old_cycle =
            crate::control::period_start(billing_boundary - 1, options.traffic_reset_day);
        let db = storage.connection().unwrap();
        db.execute(
            "INSERT INTO traffic_periods(node_id,cycle_start_ms,raw_up,raw_down,used_up,used_down) VALUES(?1,?2,40,50,40,50)",
            params![node_id, integer(old_cycle)],
        )
        .unwrap();
        assert_eq!(
            load_nodes(&db, billing_boundary - 1).unwrap()[&node_id].traffic,
            Some(90.0)
        );
        assert_eq!(
            load_nodes(&db, billing_boundary).unwrap()[&node_id].traffic,
            Some(0.0)
        );
        let retained: i64 = db
            .query_row(
                "SELECT used_up+used_down FROM traffic_periods WHERE node_id=?1",
                [node_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retained, 90);
    }

    fn configured_storage() -> (tempfile::TempDir, Storage, String) {
        let directory = tempfile::tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("senlun.db"), 64 * 1024 * 1024).unwrap();
        let secret = storage.create_enrollment(600, 1000).unwrap();
        let enrolled = storage
            .enroll(
                &senlun_protocol::EnrollmentRequest {
                    protocol_version: senlun_protocol::PROTOCOL_VERSION,
                    node_name: "test-node".into(),
                    agent_version: "test".into(),
                    region: String::new(),
                    group: String::new(),
                },
                &crate::storage::hash_token(&secret.token),
                100,
                1000,
            )
            .unwrap();
        let mut channel = Channel {
            id: String::new(),
            name: "test-channel".into(),
            kind: "webhook".into(),
            url: "https://example.invalid/senlun-test".into(),
            enabled: true,
        };
        storage.save_channel(&mut channel).unwrap();
        let mut offline = rule("offline");
        offline.id = String::new();
        offline.channel_ids = vec![channel.id];
        storage.save_alert_rule(&mut offline).unwrap();
        (directory, storage, enrolled.node_id)
    }

    fn incident_status(storage: &Storage) -> String {
        storage
            .connection()
            .unwrap()
            .query_row("SELECT status FROM alert_incidents", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn duration_and_recovery_replace_an_in_flight_firing_message() {
        let (_directory, storage, node_id) = configured_storage();
        storage.evaluate_alerts(100_000, 90_000).unwrap();
        assert_eq!(incident_status(&storage), "pending");
        assert!(storage.pending_deliveries(100_000).unwrap().is_empty());
        storage.evaluate_alerts(109_999, 90_000).unwrap();
        assert_eq!(incident_status(&storage), "pending");
        storage.evaluate_alerts(110_000, 90_000).unwrap();
        assert_eq!(incident_status(&storage), "firing");
        let firing = storage.pending_deliveries(110_000).unwrap().pop().unwrap();
        let body: serde_json::Value = serde_json::from_str(&firing.payload).unwrap();
        assert_eq!(body["status"], "firing");
        for forbidden in ["ip", "token", "snapshot", "value", "threshold"] {
            assert!(body.get(forbidden).is_none());
        }
        storage
            .connection()
            .unwrap()
            .execute(
                "INSERT INTO node_state(node_id,last_seen_at_ms) VALUES(?1,110001) ON CONFLICT(node_id) DO UPDATE SET last_seen_at_ms=110001",
                [node_id],
            )
            .unwrap();
        storage.evaluate_alerts(110_001, 90_000).unwrap();
        assert_eq!(incident_status(&storage), "resolved");
        storage.delivery_result(&firing.id, true, 110_002).unwrap();
        let recovery = storage.pending_deliveries(110_002).unwrap().pop().unwrap();
        assert_ne!(recovery.id, firing.id);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&recovery.payload).unwrap()["status"],
            "resolved"
        );
    }

    #[test]
    fn delivery_attempts_survive_restart_and_stop_after_five_failures() {
        let (directory, storage, _node_id) = configured_storage();
        storage.evaluate_alerts(100_000, 90_000).unwrap();
        storage.evaluate_alerts(110_000, 90_000).unwrap();
        let delivery = storage.pending_deliveries(110_000).unwrap().pop().unwrap();
        storage
            .delivery_result(&delivery.id, false, 110_000)
            .unwrap();
        drop(storage);
        let storage = Storage::open(&directory.path().join("senlun.db"), 64 * 1024 * 1024).unwrap();
        assert!(storage.pending_deliveries(114_999).unwrap().is_empty());
        for now in [115_000, 125_000, 145_000, 185_000] {
            let retry = storage.pending_deliveries(now).unwrap().pop().unwrap();
            assert_eq!(retry.id, delivery.id);
            storage.delivery_result(&retry.id, false, now).unwrap();
        }
        assert!(storage.pending_deliveries(1_000_000).unwrap().is_empty());
        let failures: Vec<_> = storage
            .audit_events(100)
            .unwrap()
            .into_iter()
            .filter(|event| event.action == "notification.failed")
            .collect();
        assert_eq!(failures.len(), 1);
        assert!(failures[0].subject.ends_with(&delivery.id));
        assert!(!failures[0].subject.contains("https://"));
    }
}
