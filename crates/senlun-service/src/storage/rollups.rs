//! Ten-minute exact hot window and sealed UTC-minute history.
//!
//! Hot tables use SQLite's bounded in-memory database, so savepoints and failed
//! commits roll back hot samples together with the durable traffic ledger.
use super::{StorageError, format_timestamp, history_row, to_i64};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

mod migration;
mod queries;
#[cfg(test)]
mod tests;
pub(crate) use queries::{
    ProbeTotals, history, probe_comparison, probe_history, probe_overview, probe_summaries,
};

pub(crate) const MINUTE: u64 = 60_000;
const HOT_WINDOW: u64 = 10 * MINUTE;
const FLUSH_ROWS: i64 = 4096;
const HOT_BYTES: u64 = 64 * 1024 * 1024;

fn read_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    value.try_into().map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

pub(super) use migration::migrate;

pub(crate) const SNAPSHOT_COLUMNS: &str = "node_id,received_at_ms,collected_at_ms,cpu_usage_percent,
 memory_used_bytes,memory_total_bytes,swap_used_bytes,swap_total_bytes,
 load_one,load_five,load_fifteen,disk_used_bytes,disk_total_bytes,
 network_receive_bytes_per_second,network_transmit_bytes_per_second,
 network_total_received_bytes,network_total_transmitted_bytes,process_count,extensions,uptime_seconds";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Statistic {
    pub count: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
}
impl Statistic {
    fn one(value: f64) -> Self {
        Self {
            count: 1,
            sum: value,
            min: value,
            max: value,
        }
    }
    fn merge(&mut self, other: &Self) {
        self.count += other.count;
        self.sum += other.sum;
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct SnapshotMinute {
    pub samples: u64,
    pub first_received: u64,
    pub last_received: u64,
    pub metrics: BTreeMap<String, Statistic>,
    pub last: Value,
    pub recent_ids: BTreeSet<String>,
}
impl SnapshotMinute {
    fn one(record: Value, received: u64) -> Self {
        let metrics = record
            .as_object()
            .into_iter()
            .flat_map(|v| v.iter())
            .filter_map(|(name, value)| value.as_f64().map(|v| (name.clone(), Statistic::one(v))))
            .collect();
        Self {
            samples: 1,
            first_received: received,
            last_received: received,
            metrics,
            last: record,
            recent_ids: BTreeSet::new(),
        }
    }
    fn merge(&mut self, other: &Self) {
        self.samples += other.samples;
        self.recent_ids.extend(other.recent_ids.iter().cloned());
        self.first_received = self.first_received.min(other.first_received);
        for (name, stat) in &other.metrics {
            self.metrics
                .entry(name.clone())
                .and_modify(|v| v.merge(stat))
                .or_insert_with(|| stat.clone());
        }
        if other.last_received >= self.last_received {
            self.last_received = other.last_received;
            self.last.clone_from(&other.last);
        }
    }
    // Display aggregates may round byte-valued gauges; the independent traffic
    // ledger and the last cumulative counter values remain integers.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    pub(crate) fn record(&self, resolution: u64) -> Value {
        let mut value = self.last.clone();
        if self.samples > 1 || resolution > 0 {
            for (name, stat) in &self.metrics {
                // Cumulative counters are last values, never averaged or summed.
                if matches!(
                    name.as_str(),
                    "net_total_up" | "net_total_down" | "clock_skew_ms"
                ) {
                    continue;
                }
                let v = if matches!(
                    name.as_str(),
                    "ram_total" | "swap_total" | "disk_total" | "temp"
                ) {
                    stat.max
                } else {
                    stat.sum / stat.count as f64
                };
                value[name] = if matches!(
                    name.as_str(),
                    "cpu"
                        | "gpu"
                        | "temp"
                        | "load"
                        | "load5"
                        | "load15"
                        | "connections"
                        | "connections_udp"
                ) {
                    json!(v)
                } else {
                    json!(v.max(0.0) as u64)
                };
            }
        }
        value["time"] = json!(format_timestamp(self.last_received));
        value["samples"] = json!(self.samples);
        value["resolution_ms"] = json!(resolution);
        value["peaks"] = json!(
            self.metrics
                .iter()
                .map(|(k, v)| (k.clone(), v.max))
                .collect::<BTreeMap<_, _>>()
        );
        value
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ProbeMinute {
    pub node: String,
    pub task: String,
    pub samples: u64,
    pub successful: u64,
    pub latency_sum: f64,
    // Each contribution holds at most 32 samples. Legacy buckets may contain
    // multiple contributions. Counted sorted values preserve exact linear
    // percentiles without adding a sketch dependency or inventing precision.
    pub latencies: Vec<(f64, u64)>,
    pub errors: BTreeMap<String, u64>,
    pub first_received: u64,
    pub last_received: u64,
    pub last_collected: u64,
    pub last_latency: Option<f64>,
    pub last_error: Option<String>,
    pub last_success: Option<(u64, f64)>,
    pub recent_ids: BTreeSet<String>,
}
impl ProbeMinute {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        let latency: Option<f64> = row.get(4)?;
        let successful: bool = row.get(5)?;
        let error: Option<String> = row.get(6)?;
        let mut errors = BTreeMap::new();
        if let Some(e) = &error {
            errors.insert(e.clone(), 1);
        }
        let received = read_u64(row, 3)?;
        Ok(Self {
            node: row.get(0)?,
            task: row.get(1)?,
            samples: 1,
            successful: u64::from(successful),
            latency_sum: latency.unwrap_or(0.0),
            latencies: latency.map(|v| vec![(v, 1)]).unwrap_or_default(),
            errors,
            first_received: received,
            last_received: received,
            last_collected: read_u64(row, 2)?,
            last_latency: latency,
            last_error: error,
            last_success: latency.map(|value| (received, value)),
            recent_ids: BTreeSet::new(),
        })
    }
    pub(crate) fn merge(&mut self, other: &Self) {
        self.samples += other.samples;
        self.recent_ids.extend(other.recent_ids.iter().cloned());
        self.successful += other.successful;
        self.latency_sum += other.latency_sum;
        self.first_received = self.first_received.min(other.first_received);
        for (error, count) in &other.errors {
            *self.errors.entry(error.clone()).or_default() += count;
        }
        for &(value, count) in &other.latencies {
            match self
                .latencies
                .binary_search_by(|(v, _)| v.total_cmp(&value))
            {
                Ok(i) => self.latencies[i].1 += count,
                Err(i) => self.latencies.insert(i, (value, count)),
            }
        }
        if other.last_received >= self.last_received {
            self.last_received = other.last_received;
            self.last_collected = other.last_collected;
            self.last_latency = other.last_latency;
            self.last_error.clone_from(&other.last_error);
        }
        if other
            .last_success
            .is_some_and(|(at, _)| self.last_success.is_none_or(|(old, _)| at >= old))
        {
            self.last_success = other.last_success;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn average(&self) -> Option<f64> {
        (self.successful > 0).then(|| self.latency_sum / self.successful as f64)
    }
}

pub(super) fn initialize_hot(db: &Connection) -> Result<(), StorageError> {
    db.execute_batch("ATTACH DATABASE ':memory:' AS hot;")?;
    super::metrics::configure_limit(db, "hot", HOT_BYTES)
        .map_err(|_| StorageError::InvalidInput("cannot bound hot metrics database"))?;
    // The released raw schemas are used solely for the exact in-memory window.
    // There is one raw implementation; persistent raw tables are removed by v7.
    db.execute_batch(&format!(
        "{}\n{}",
        migration::HOT_SCHEMA,
        "PRAGMA hot.journal_mode=MEMORY; PRAGMA hot.synchronous=OFF; PRAGMA hot.cache_size=-4096;"
    ))?;
    Ok(())
}

fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, StorageError> {
    serde_json::from_str(value)
        .map_err(|_| StorageError::InvalidInput("invalid persisted minute summary"))
}
fn encode<T: Serialize>(value: &T) -> Result<String, StorageError> {
    serde_json::to_string(value).map_err(|_| StorageError::InvalidInput("invalid minute summary"))
}

pub(crate) fn latest_extensions(
    db: &Connection,
    node: &str,
) -> Result<Option<String>, StorageError> {
    let hot:Option<String>=db.query_row("SELECT extensions FROM hot.snapshots WHERE node_id=?1 ORDER BY received_at_ms DESC,id DESC LIMIT 1",[node],|r|r.get(0)).optional()?;
    if hot.is_some() {
        return Ok(hot);
    }
    let minute: Option<String> = db
        .query_row(
            "SELECT data FROM snapshot_minutes WHERE node_id=?1 ORDER BY bucket_ms DESC,json_extract(data,'$.last_received') DESC,id DESC LIMIT 1",
            [node],
            |r| r.get(0),
        )
        .optional()?;
    minute
        .map(|data| {
            let summary: SnapshotMinute = decode(&data)?;
            encode(&summary.last["extensions"])
        })
        .transpose()
}

pub(crate) fn latest_minutes(
    db: &Connection,
) -> Result<Vec<(SnapshotMinute, Option<u64>)>, StorageError> {
    let mut statement=db.prepare("SELECT m.data,ns.last_seen_at_ms FROM metrics.snapshot_minutes m JOIN nodes n ON n.id=m.node_id
        LEFT JOIN node_state ns ON ns.node_id=m.node_id
        WHERE n.disabled_at_ms IS NULL AND m.id=(SELECT id FROM metrics.snapshot_minutes x WHERE x.node_id=m.node_id ORDER BY bucket_ms DESC,json_extract(data,'$.last_received') DESC,id DESC LIMIT 1)
        AND NOT EXISTS(SELECT 1 FROM hot.snapshots h WHERE h.node_id=m.node_id)")?;
    let mut rows = statement.query([])?;
    let mut records = Vec::new();
    while let Some(row) = rows.next()? {
        let seen = row
            .get::<_, Option<i64>>(1)?
            .map(u64::try_from)
            .transpose()
            .map_err(|_| StorageError::InvalidInput("negative last-seen timestamp"))?;
        records.push((decode(&row.get::<_, String>(0)?)?, seen));
    }
    Ok(records)
}

fn save_snapshot(
    db: &Connection,
    node: &str,
    bucket: u64,
    mut summary: SnapshotMinute,
) -> Result<(), StorageError> {
    let old: Option<(i64, String)> = db
        .query_row(
            "SELECT id,data FROM metrics.snapshot_minutes WHERE node_id=?1 AND bucket_ms=?2 ORDER BY id DESC LIMIT 1",
            params![node, to_i64(bucket)?],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let mut existing_id = None;
    if let Some((id, old)) = old {
        let mut previous: SnapshotMinute = decode(&old)?;
        if previous.samples + summary.samples <= 64 {
            previous.merge(&summary);
            summary = previous;
            existing_id = Some(id);
        }
    }
    if summary.samples == 0
        || summary.samples > 64
        || summary.metrics.values().any(|v| {
            v.count == 0
                || v.count > summary.samples
                || !v.sum.is_finite()
                || !v.min.is_finite()
                || !v.max.is_finite()
                || v.min > v.max
        })
    {
        return Err(StorageError::InvalidInput(
            "invalid or oversized snapshot minute",
        ));
    }
    let until = if summary.recent_ids.is_empty() {
        0
    } else {
        bucket + HOT_WINDOW + MINUTE
    };
    if let Some(id) = existing_id {
        db.execute(
            "UPDATE metrics.snapshot_minutes SET samples=?1,data=?2,dedup_until_ms=?3 WHERE id=?4",
            params![
                to_i64(summary.samples)?,
                encode(&summary)?,
                to_i64(until)?,
                id
            ],
        )?;
    } else {
        db.execute("INSERT INTO metrics.snapshot_minutes(node_id,bucket_ms,samples,data,dedup_until_ms) VALUES(?1,?2,?3,?4,?5)",
            params![node,to_i64(bucket)?,to_i64(summary.samples)?,encode(&summary)?,to_i64(until)?])?;
    }
    Ok(())
}
fn save_probe(db: &Connection, bucket: u64, mut summary: ProbeMinute) -> Result<(), StorageError> {
    let old: Option<(i64,String)> = db.query_row("SELECT id,data FROM metrics.probe_minutes WHERE node_id=?1 AND task_id=?2 AND bucket_ms=?3 ORDER BY id DESC LIMIT 1",params![summary.node,summary.task,to_i64(bucket)?],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    let mut existing_id = None;
    if let Some((id, old)) = old {
        let mut previous: ProbeMinute = decode(&old)?;
        if previous.samples + summary.samples <= 32 {
            previous.merge(&summary);
            summary = previous;
            existing_id = Some(id);
        }
    }
    let histogram_count = summary
        .latencies
        .iter()
        .try_fold(0_u64, |n, (_, count)| n.checked_add(*count));
    if summary.samples == 0
        || summary.samples > 32
        || summary.latencies.len() > 32
        || summary.successful > summary.samples
        || histogram_count != Some(summary.successful)
        || !summary.latency_sum.is_finite()
        || summary
            .latencies
            .iter()
            .any(|(v, count)| !v.is_finite() || !(0.0..=30_000.0).contains(v) || *count == 0)
        || summary
            .latencies
            .windows(2)
            .any(|v| v[0].0.total_cmp(&v[1].0) != std::cmp::Ordering::Less)
    {
        return Err(StorageError::InvalidInput(
            "invalid or oversized probe minute",
        ));
    }
    let until = if summary.recent_ids.is_empty() {
        0
    } else {
        bucket + HOT_WINDOW + MINUTE
    };
    if let Some(id) = existing_id {
        db.execute(
            "UPDATE metrics.probe_minutes SET samples=?1,data=?2,dedup_until_ms=?3 WHERE id=?4",
            params![
                to_i64(summary.samples)?,
                encode(&summary)?,
                to_i64(until)?,
                id
            ],
        )?;
    } else {
        db.execute("INSERT INTO metrics.probe_minutes(node_id,task_id,bucket_ms,samples,data,dedup_until_ms) VALUES(?1,?2,?3,?4,?5,?6)",
            params![summary.node,summary.task,to_i64(bucket)?,to_i64(summary.samples)?,encode(&summary)?,to_i64(until)?])?;
    }
    Ok(())
}

/// Flush only previously unpersisted samples. Raw rows retain a transactional
/// persisted marker, so retries, late arrivals and partial shutdown buckets do
/// not double-count. Normal closed minutes have one write per node/task.
pub(crate) fn flush(db: &Connection, now: u64, include_open: bool) -> Result<(), StorageError> {
    let cutoff = if include_open {
        i64::MAX
    } else {
        to_i64(now / MINUTE * MINUTE)?
    };
    while flush_snapshots(db, "hot.snapshots", cutoff, true)? > 0 {}
    while flush_probes(db, "hot.probe_results", cutoff, true)? > 0 {}
    let before = to_i64(now.saturating_sub(HOT_WINDOW))?;
    db.execute(
        "DELETE FROM hot.snapshots WHERE persisted=1 AND received_at_ms<?1",
        [before],
    )?;
    db.execute(
        "DELETE FROM hot.probe_results WHERE persisted=1 AND received_at_ms<?1",
        [before],
    )?;
    for table in ["snapshot_minutes", "probe_minutes"] {
        db.execute(&format!("UPDATE metrics.{table} SET data=json_set(data,'$.recent_ids',json('[]')),dedup_until_ms=0
            WHERE dedup_until_ms>0 AND dedup_until_ms<=?1"),[to_i64(now)?])?;
    }
    Ok(())
}

fn flush_snapshots(
    db: &Connection,
    table: &str,
    cutoff: i64,
    hot: bool,
) -> Result<usize, StorageError> {
    let predicate = if hot { "AND persisted=0" } else { "" };
    let sql = format!(
        "SELECT {SNAPSHOT_COLUMNS},id,sample_id FROM {table} WHERE received_at_ms<?1 {predicate} ORDER BY id LIMIT {FLUSH_ROWS}"
    );
    let mut statement = db.prepare(&sql)?;
    let mut rows = statement.query([cutoff])?;
    let mut groups: BTreeMap<(String, u64), Vec<SnapshotMinute>> = BTreeMap::new();
    let mut last_id = 0;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        let record = history_row(row)?;
        let node: String = row.get(0)?;
        let received = read_u64(row, 1)?;
        let mut summary = SnapshotMinute::one(record, received);
        if hot || received >= super::unix_time_ms()?.saturating_sub(HOT_WINDOW) {
            summary.recent_ids.insert(row.get(21)?);
        }
        let contributions = groups
            .entry((node, received / MINUTE * MINUTE))
            .or_default();
        if let Some(last) = contributions.last_mut().filter(|v| v.samples < 64) {
            last.merge(&summary);
        } else {
            contributions.push(summary);
        }
        last_id = row.get(20)?;
        count += 1;
    }
    drop(rows);
    drop(statement);
    for ((node, bucket), contributions) in groups {
        for summary in contributions {
            save_snapshot(db, &node, bucket, summary)?;
        }
    }
    if count > 0 {
        if hot {
            db.execute("UPDATE hot.snapshots SET persisted=1 WHERE id<=?1 AND received_at_ms<?2 AND persisted=0",params![last_id,cutoff])?;
        } else {
            db.execute(
                "DELETE FROM metrics.snapshots WHERE id<=?1 AND received_at_ms<?2",
                params![last_id, cutoff],
            )?;
        }
    }
    Ok(count)
}
fn flush_probes(
    db: &Connection,
    table: &str,
    cutoff: i64,
    hot: bool,
) -> Result<usize, StorageError> {
    let predicate = if hot { "AND persisted=0" } else { "" };
    let mut statement=db.prepare(&format!("SELECT node_id,task_id,collected_at_ms,received_at_ms,latency_ms,success,error,id,sample_id FROM {table} WHERE received_at_ms<?1 {predicate} ORDER BY id LIMIT {FLUSH_ROWS}"))?;
    let mut rows = statement.query([cutoff])?;
    let mut groups: BTreeMap<(String, String, u64), Vec<ProbeMinute>> = BTreeMap::new();
    let mut last_id = 0;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        let mut summary = ProbeMinute::from_row(row)?;
        if hot || summary.last_received >= super::unix_time_ms()?.saturating_sub(HOT_WINDOW) {
            summary.recent_ids.insert(row.get(8)?);
        }
        let contributions = groups
            .entry((
                summary.node.clone(),
                summary.task.clone(),
                summary.last_received / MINUTE * MINUTE,
            ))
            .or_default();
        if let Some(last) = contributions.last_mut().filter(|v| v.samples < 32) {
            last.merge(&summary);
        } else {
            contributions.push(summary);
        }
        last_id = row.get(7)?;
        count += 1;
    }
    drop(rows);
    drop(statement);
    for ((_, _, bucket), contributions) in groups {
        for summary in contributions {
            save_probe(db, bucket, summary)?;
        }
    }
    if count > 0 {
        if hot {
            db.execute("UPDATE hot.probe_results SET persisted=1 WHERE id<=?1 AND received_at_ms<?2 AND persisted=0",params![last_id,cutoff])?;
        } else {
            db.execute(
                "DELETE FROM metrics.probe_results WHERE id<=?1 AND received_at_ms<?2",
                params![last_id, cutoff],
            )?;
        }
    }
    Ok(count)
}
