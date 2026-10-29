//! Merge exact hot rows with persisted minutes without counting both copies.
use super::{MINUTE, ProbeMinute, SNAPSHOT_COLUMNS, SnapshotMinute, decode, read_u64};
use crate::{
    control::{ProbeDefinition, visible},
    storage::{HistoryCoverage, HistorySeries, StorageError, history_row, to_i64},
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

// A persisted minute is replaced by exact hot rows only while ALL its
// persisted samples remain in memory. After eviction or restart, use its
// summary plus any as-yet-unpersisted hot rows. Partial shutdown buckets and
// late arrivals therefore cannot create gaps or double counting.
const SNAPSHOT_COVERED: &str = "CASE WHEN EXISTS(SELECT 1 FROM hot.snapshots h WHERE h.node_id=m.node_id
 AND h.persisted=1 AND h.received_at_ms>=m.bucket_ms AND h.received_at_ms<m.bucket_ms+60000)
 THEN (SELECT sum(x.samples) FROM metrics.snapshot_minutes x WHERE x.node_id=m.node_id AND x.bucket_ms=m.bucket_ms)
 =(SELECT count(*) FROM hot.snapshots h WHERE h.node_id=m.node_id AND h.persisted=1
 AND h.received_at_ms>=m.bucket_ms AND h.received_at_ms<m.bucket_ms+60000) ELSE 0 END";
const PROBE_COVERED: &str = "CASE WHEN EXISTS(SELECT 1 FROM hot.probe_results h
 WHERE h.node_id=m.node_id AND h.task_id=m.task_id AND h.persisted=1
 AND h.received_at_ms>=m.bucket_ms AND h.received_at_ms<m.bucket_ms+60000)
 THEN (SELECT sum(x.samples) FROM metrics.probe_minutes x WHERE x.node_id=m.node_id
 AND x.task_id=m.task_id AND x.bucket_ms=m.bucket_ms)=(SELECT count(*) FROM hot.probe_results h
 WHERE h.node_id=m.node_id AND h.task_id=m.task_id AND h.persisted=1
 AND h.received_at_ms>=m.bucket_ms AND h.received_at_ms<m.bucket_ms+60000) ELSE 0 END";

fn snapshots<F>(
    db: &Connection,
    node: &str,
    start: u64,
    end: u64,
    mut visit: F,
) -> Result<(), StorageError>
where
    F: FnMut(SnapshotMinute, u64) -> Result<(), StorageError>,
{
    let sql = format!(
        "SELECT {SNAPSHOT_COLUMNS} FROM hot.snapshots s
      WHERE node_id=?1 AND received_at_ms BETWEEN ?2 AND ?3 AND
      (persisted=0 OR NOT EXISTS(SELECT 1 FROM metrics.snapshot_minutes m
        WHERE m.node_id=s.node_id AND m.bucket_ms=(s.received_at_ms/60000)*60000
        AND NOT ({SNAPSHOT_COVERED}))) ORDER BY received_at_ms,id"
    );
    let mut statement = db.prepare(&sql)?;
    let mut rows = statement.query(params![node, to_i64(start)?, to_i64(end)?])?;
    while let Some(row) = rows.next()? {
        visit(SnapshotMinute::one(history_row(row)?, read_u64(row, 1)?), 0)?;
    }
    let mut statement=db.prepare(&format!("SELECT data FROM metrics.snapshot_minutes m
        WHERE node_id=?1 AND bucket_ms BETWEEN ?2 AND ?3 AND NOT ({SNAPSHOT_COVERED}) ORDER BY bucket_ms"))?;
    let mut rows = statement.query(params![
        node,
        to_i64(start / MINUTE * MINUTE)?,
        to_i64(end / MINUTE * MINUTE)?
    ])?;
    while let Some(row) = rows.next()? {
        visit(decode(&row.get::<_, String>(0)?)?, MINUTE)?;
    }
    Ok(())
}

pub(crate) fn history(
    db: &Connection,
    node: &str,
    start: u64,
    end: u64,
    limit: u32,
) -> Result<HistorySeries, StorageError> {
    let mut source = 0;
    let mut first = None::<u64>;
    let mut last = None::<u64>;
    let mut resolution = 0;
    snapshots(db, node, start, end, |summary, r| {
        source += summary.samples;
        first = Some(first.map_or(summary.first_received, |v| v.min(summary.first_received)));
        last = Some(last.map_or(summary.last_received, |v| v.max(summary.last_received)));
        resolution = resolution.max(r);
        Ok(())
    })?;
    let width = if resolution > 0 || source > u64::from(limit) {
        let span = end.saturating_sub(start).saturating_add(1);
        let w = span.div_ceil(u64::from(limit.saturating_sub(1).max(1)));
        if resolution > 0 {
            w.div_ceil(MINUTE) * MINUTE
        } else {
            w.max(1)
        }
    } else {
        1
    };
    let mut groups = BTreeMap::<u64, SnapshotMinute>::new();
    snapshots(db, node, start, end, |summary, _| {
        let key = if limit == 1 {
            0
        } else {
            summary.last_received / width * width
        };
        groups
            .entry(key)
            .and_modify(|v| v.merge(&summary))
            .or_insert(summary);
        Ok(())
    })?;
    let records: Vec<_> = groups
        .into_values()
        .map(|v| v.record(resolution.max(if v.samples > 1 { width } else { 0 })))
        .collect();
    let downsampled = resolution > 0 || source > records.len() as u64;
    Ok(HistorySeries {
        coverage: HistoryCoverage {
            requested_start_unix_ms: start,
            requested_end_unix_ms: end,
            actual_start_unix_ms: first,
            actual_end_unix_ms: last,
            source_points: source,
            returned_points: records.len(),
            bucket_ms: if downsampled { width } else { 0 },
            downsampled,
        },
        records,
    })
}

pub(crate) fn probe_summaries<F>(
    db: &Connection,
    node: Option<&str>,
    task: Option<&str>,
    start: u64,
    end: u64,
    mut visit: F,
) -> Result<(), StorageError>
where
    F: FnMut(ProbeMinute, u64) -> Result<(), StorageError>,
{
    // Fixed predicates let SQLite seek by node and task when provided.
    let hot_node = if node.is_some() {
        " AND s.node_id=?1"
    } else {
        ""
    };
    let hot_task = if task.is_some() {
        " AND s.task_id=?2"
    } else {
        ""
    };
    let mut statement = db.prepare(&format!(
        "SELECT s.node_id,s.task_id,s.collected_at_ms,s.received_at_ms,s.latency_ms,s.success,s.error
        FROM hot.probe_results s WHERE s.received_at_ms BETWEEN ?3 AND ?4{hot_node}{hot_task}
        AND (s.persisted=0 OR NOT EXISTS(
        SELECT 1 FROM metrics.probe_minutes m WHERE m.node_id=s.node_id AND m.task_id=s.task_id
        AND m.bucket_ms=(s.received_at_ms/60000)*60000 AND NOT ({PROBE_COVERED}))) ORDER BY s.received_at_ms,s.id"
    ))?;
    let mut rows = statement.query(params![node, task, to_i64(start)?, to_i64(end)?])?;
    while let Some(row) = rows.next()? {
        visit(ProbeMinute::from_row(row)?, 0)?;
    }
    let minute_node = if node.is_some() {
        " AND m.node_id=?1"
    } else {
        ""
    };
    let minute_task = if task.is_some() {
        " AND m.task_id=?2"
    } else {
        ""
    };
    let mut statement = db.prepare(&format!(
        "SELECT data FROM metrics.probe_minutes m
        WHERE m.bucket_ms BETWEEN ?3 AND ?4{minute_node}{minute_task}
        AND NOT ({PROBE_COVERED}) ORDER BY m.bucket_ms"
    ))?;
    let mut rows = statement.query(params![
        node,
        task,
        to_i64(start / MINUTE * MINUTE)?,
        to_i64(end / MINUTE * MINUTE)?
    ])?;
    while let Some(row) = rows.next()? {
        visit(decode(&row.get::<_, String>(0)?)?, MINUTE)?;
    }
    Ok(())
}

#[derive(Default)]
pub(crate) struct ProbeTotals {
    pub samples: u64,
    pub successful: u64,
    pub sum: f64,
    pub last: u64,
}
impl ProbeTotals {
    pub(crate) fn add(&mut self, p: &ProbeMinute) {
        self.samples += p.samples;
        self.successful += p.successful;
        self.sum += p.latency_sum;
        self.last = self.last.max(p.last_received);
    }
    #[allow(clippy::cast_precision_loss)] // Display-only aggregate values, never the byte ledger.
    pub(crate) fn average(&self) -> Option<f64> {
        (self.successful > 0).then(|| self.sum / self.successful as f64)
    }
    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn loss(&self) -> Option<f64> {
        (self.samples > 0)
            .then(|| (self.samples - self.successful) as f64 * 100.0 / self.samples as f64)
    }
}

fn recent(p: &ProbeMinute, resolution: u64) -> Value {
    json!({"task_id":p.task,"collected_at_unix_ms":p.last_collected,
        "received_at_unix_ms":p.last_received,"latency_ms":if resolution>0 { p.average() } else { p.last_latency },
        "success":if resolution>0 { p.samples==p.successful } else { p.last_latency.is_some() },"error":p.last_error,
        "samples":p.samples,"failed_samples":p.samples-p.successful,
        "resolution_ms":resolution,"aggregated":resolution>0})
}

#[derive(Default)]
struct ProbeStatistics {
    total: ProbeTotals,
    latest_success: Option<(u64, f64)>,
}
impl ProbeStatistics {
    fn add(&mut self, p: &ProbeMinute) {
        self.total.add(p);
        if p.last_success
            .is_some_and(|(at, _)| self.latest_success.is_none_or(|(old, _)| at >= old))
        {
            self.latest_success = p.last_success;
        }
    }
}

// Let SQLite merge/sort counted latency values using its bounded cache and
// mature temporary sorter. Application memory holds only six percentile ranks,
// even when all values differ across a long retained range.
#[allow(clippy::cast_precision_loss)]
pub(super) fn quantiles(
    db: &Connection,
    node: &str,
    task: &str,
    start: u64,
    end: u64,
    total: u64,
) -> Result<[Option<f64>; 5], StorageError> {
    if total == 0 {
        return Ok([None; 5]);
    }
    let sql = format!(
        "WITH samples(value,weight) AS (
        SELECT latency_ms,1 FROM hot.probe_results s WHERE node_id=?1 AND task_id=?2
          AND received_at_ms BETWEEN ?3 AND ?4 AND success=1 AND latency_ms IS NOT NULL
          AND (persisted=0 OR NOT EXISTS(SELECT 1 FROM metrics.probe_minutes m
            WHERE m.node_id=s.node_id AND m.task_id=s.task_id
            AND m.bucket_ms=(s.received_at_ms/60000)*60000 AND NOT ({PROBE_COVERED})))
        UNION ALL
        SELECT json_extract(j.value,'$[0]'),json_extract(j.value,'$[1]')
        FROM metrics.probe_minutes m,json_each(m.data,'$.latencies') j
        WHERE node_id=?1 AND task_id=?2 AND bucket_ms BETWEEN ?5 AND ?6 AND NOT ({PROBE_COVERED})
        ) SELECT value,sum(weight) FROM samples GROUP BY value ORDER BY value"
    );
    let mut statement = db.prepare(&sql)?;
    let mut rows = statement.query(params![
        node,
        task,
        to_i64(start)?,
        to_i64(end)?,
        to_i64(start / MINUTE * MINUTE)?,
        to_i64(end / MINUTE * MINUTE)?
    ])?;
    let ranks = [50_u64, 95, 99].map(|p| (total - 1) * p);
    let mut selected = [[None::<f64>; 2]; 3];
    let mut count = 0;
    let mut min = None;
    let mut max = None;
    while let Some(row) = rows.next()? {
        let value: f64 = row.get(0)?;
        let weight = read_u64(row, 1)?;
        count += weight;
        min.get_or_insert(value);
        max = Some(value);
        for (i, rank) in ranks.iter().copied().enumerate() {
            let lower = rank / 100;
            let upper = lower + u64::from(rank % 100 != 0);
            if count > lower {
                selected[i][0].get_or_insert(value);
            }
            if count > upper {
                selected[i][1].get_or_insert(value);
            }
        }
    }
    if count != total {
        return Err(StorageError::InvalidInput("probe histogram count mismatch"));
    }
    let percentile = |i: usize| {
        selected[i][0]
            .zip(selected[i][1])
            .map(|(a, b)| a + (b - a) * (ranks[i] % 100) as f64 / 100.0)
    };
    Ok([min, max, percentile(0), percentile(1), percentile(2)])
}

pub(crate) fn probe_history(
    db: &Connection,
    node: &str,
    tasks: &[ProbeDefinition],
    start: u64,
    end: u64,
) -> Result<Value, StorageError> {
    let ids: HashSet<_> = tasks.iter().map(|t| t.id.as_str()).collect();
    let points_per_task = 1000_usize.min(12000 / tasks.len().max(1));
    let persisted:bool=db.query_row(&format!("SELECT EXISTS(SELECT 1 FROM metrics.probe_minutes m
        WHERE node_id=?1 AND bucket_ms BETWEEN ?2 AND ?3 AND task_id IN(SELECT value FROM json_each(?4))
        AND NOT ({PROBE_COVERED}))"),params![node,to_i64(start/MINUTE*MINUTE)?,to_i64(end/MINUTE*MINUTE)?,json!(ids).to_string()],|r|r.get(0))?;
    let width = end
        .saturating_sub(start)
        .saturating_add(1)
        .div_ceil(points_per_task.saturating_sub(1).max(1) as u64)
        .max(1);
    let width = if persisted {
        width.div_ceil(MINUTE) * MINUTE
    } else {
        width
    };
    let mut summaries = BTreeMap::<String, ProbeStatistics>::new();
    let mut chart = BTreeMap::<(String, u64), ProbeTotals>::new();
    let mut records = BTreeMap::new();
    let mut ordinal = 0_u64;
    // A single-task detail view can use the existing node/task/time index.
    // Multiple tasks still share one scan so their existing ordering is kept.
    let selected_task = match tasks {
        [task] => Some(task.id.as_str()),
        _ => None,
    };
    probe_summaries(
        db,
        Some(node),
        selected_task,
        start,
        end,
        |p, resolution| {
            if !ids.contains(p.task.as_str()) {
                return Ok(());
            }
            let bucket = p.last_received / width * width;
            chart.entry((p.task.clone(), bucket)).or_default().add(&p);
            records.insert(
                (p.last_received, p.task.clone(), ordinal),
                recent(&p, resolution),
            );
            ordinal += 1;
            if records.len() > 100 {
                records.pop_first();
            }
            summaries.entry(p.task.clone()).or_default().add(&p);
            Ok(())
        },
    )?;
    let mut summary = Vec::new();
    for (task, p) in &summaries {
        let [min, max, p50, p95, p99] = quantiles(db, node, task, start, end, p.total.successful)?;
        let ratio = p50
            .zip(p99)
            .and_then(|(a, b)| (a > 0.0 && b >= a).then(|| (b - a) / a.clamp(10.0, 50.0)));
        summary.push(json!({"task_id":task,"samples":p.total.samples,"loss_percent":p.total.loss(),"avg_latency_ms":p.total.average(),
            "min_latency_ms":min,"max_latency_ms":max,
            "latest_latency_ms":p.latest_success.map(|v|v.1),"p50_latency_ms":p50,"p95_latency_ms":p95,
            "p99_latency_ms":p99,"p99_p50_ratio":ratio}));
    }
    let source: u64 = summaries.values().map(|v| v.total.samples).sum();
    let chart: Vec<_> = chart
        .into_iter()
        .map(|((task, _), v)| {
            json!({"task_id":task,"received_at_unix_ms":v.last,
        "latency_ms":v.average(),"failed_samples":v.samples-v.successful,"samples":v.samples})
        })
        .collect();
    let tasks:Vec<_>=tasks.iter().map(|t|json!({"id":t.id,"name":t.name,"kind":t.kind,"interval_seconds":t.interval_seconds})).collect();
    Ok(
        json!({"tasks":tasks,"records":records.into_values().rev().collect::<Vec<_>>(),"chart":chart,"summary":summary,
        "chart_source_points":source,"chart_bucket_ms":width,"history_resolution_ms":if persisted {MINUTE} else {0},
        "range_start_unix_ms":start,"range_end_unix_ms":end}),
    )
}

pub(crate) fn probe_comparison(
    db: &Connection,
    task: &ProbeDefinition,
    start: u64,
    end: u64,
) -> Result<Vec<Value>, StorageError> {
    let assigned: HashSet<_> = task.node_ids.iter().map(String::as_str).collect();
    let mut totals = BTreeMap::<String, ProbeTotals>::new();
    let mut visibility = BTreeMap::new();
    probe_summaries(db, None, Some(&task.id), start, end, |p, _| {
        if !assigned.is_empty() && !assigned.contains(p.node.as_str()) {
            return Ok(());
        }
        let allowed = if let Some(value) = visibility.get(&p.node) {
            *value
        } else {
            if visibility.len() >= 10000 {
                return Err(StorageError::InvalidInput("comparison node limit exceeded"));
            }
            let value = visible(db, &p.node)?;
            visibility.insert(p.node.clone(), value);
            value
        };
        if allowed {
            if totals.len() >= 10000 && !totals.contains_key(&p.node) {
                return Err(StorageError::InvalidInput("node comparison limit exceeded"));
            }
            totals.entry(p.node.clone()).or_default().add(&p);
        }
        Ok(())
    })?;
    Ok(totals.into_iter().map(|(node,v)|json!({"node_id":node,"samples":v.samples,"loss_percent":v.loss(),"avg_latency_ms":v.average()})).collect())
}

pub(crate) fn probe_overview(
    db: &Connection,
    task: Option<&str>,
    start: u64,
    end: u64,
) -> Result<Vec<Value>, StorageError> {
    let mut records = BTreeMap::new();
    let mut visibility = BTreeMap::new();
    let mut ordinal = 0_u64;
    probe_summaries(db, None, task, start, end, |p, resolution| {
        let allowed = if let Some(v) = visibility.get(&p.node) {
            *v
        } else {
            if visibility.len() >= 10000 {
                return Err(StorageError::InvalidInput("overview node limit exceeded"));
            }
            let v = visible(db, &p.node)?;
            visibility.insert(p.node.clone(), v);
            v
        };
        if allowed {
            records.insert((p.last_received,p.node.clone(),p.task.clone(),ordinal),json!({"node_id":p.node,"task_id":p.task,
                "received_at_unix_ms":p.last_received,"latency_ms":p.average(),"success":p.successful==p.samples,
                "samples":p.samples,"failed_samples":p.samples-p.successful,"resolution_ms":resolution}));
            ordinal += 1;
            if records.len() > 4000 {
                records.pop_first();
            }
        }
        Ok(())
    })?;
    Ok(records.into_values().rev().collect())
}
