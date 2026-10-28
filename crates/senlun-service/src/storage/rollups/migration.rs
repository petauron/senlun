//! Forward-only v7 conversion. The caller creates a verified paired backup
//! before reaching this migration. Rollback journals make both version markers
//! and the data conversion atomic across the control/metrics pair.
use super::{flush_probes, flush_snapshots};
use rusqlite::{Connection, TransactionBehavior};
use std::error::Error;

pub(super) const HOT_SCHEMA: &str = "
CREATE TABLE hot.snapshots (
 id INTEGER PRIMARY KEY, node_id TEXT NOT NULL, sample_id TEXT NOT NULL,
 collected_at_ms INTEGER NOT NULL, received_at_ms INTEGER NOT NULL,
 uptime_seconds INTEGER NOT NULL, cpu_usage_percent REAL NOT NULL,
 load_one REAL NOT NULL,load_five REAL NOT NULL,load_fifteen REAL NOT NULL,
 memory_total_bytes INTEGER NOT NULL,memory_used_bytes INTEGER NOT NULL,
 swap_total_bytes INTEGER NOT NULL,swap_used_bytes INTEGER NOT NULL,
 disk_total_bytes INTEGER NOT NULL,disk_used_bytes INTEGER NOT NULL,
 network_receive_bytes_per_second INTEGER NOT NULL,
 network_transmit_bytes_per_second INTEGER NOT NULL,
 network_total_received_bytes INTEGER NOT NULL,
 network_total_transmitted_bytes INTEGER NOT NULL,
 process_count INTEGER,extensions TEXT NOT NULL DEFAULT '{}',
 persisted INTEGER NOT NULL DEFAULT 0,UNIQUE(node_id,sample_id)
);
CREATE INDEX hot.snapshots_node_received ON snapshots(node_id,received_at_ms DESC,id DESC);
CREATE INDEX hot.snapshots_pending ON snapshots(persisted,received_at_ms,id);
CREATE TABLE hot.probe_results (
 id INTEGER PRIMARY KEY,node_id TEXT NOT NULL,task_id TEXT NOT NULL,
 sample_id TEXT NOT NULL,collected_at_ms INTEGER NOT NULL,
 received_at_ms INTEGER NOT NULL,latency_ms REAL,success INTEGER NOT NULL,
 error TEXT,persisted INTEGER NOT NULL DEFAULT 0,UNIQUE(node_id,sample_id)
);
CREATE INDEX hot.probe_node_task_time ON probe_results(node_id,task_id,received_at_ms DESC,id DESC);
CREATE INDEX hot.probe_node_time ON probe_results(node_id,received_at_ms DESC,id DESC);
CREATE INDEX hot.probe_pending ON probe_results(persisted,received_at_ms,id);
";

pub(crate) fn migrate(db: &mut Connection) -> Result<(), Box<dyn Error + Send + Sync>> {
    for schema in ["main", "metrics"] {
        let mode: String =
            db.query_row(&format!("PRAGMA {schema}.journal_mode=DELETE"), [], |r| {
                r.get(0)
            })?;
        if mode != "delete" {
            return Err("minute migration requires exclusive rollback journals".into());
        }
        db.pragma_update(Some(schema), "synchronous", "FULL")?;
    }
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let main: i64 = tx.query_row("PRAGMA main.user_version", [], |r| r.get(0))?;
    let metrics: i64 = tx.query_row("PRAGMA metrics.user_version", [], |r| r.get(0))?;
    if main != 6 || metrics != 1 {
        return Err("unexpected schema pair for minute migration".into());
    }
    let snapshots: i64 =
        tx.query_row("SELECT count(*) FROM metrics.snapshots", [], |r| r.get(0))?;
    let probes: i64 = tx.query_row("SELECT count(*) FROM metrics.probe_results", [], |r| {
        r.get(0)
    })?;
    let failed: i64 = tx.query_row(
        "SELECT count(*) FROM metrics.probe_results WHERE success=0",
        [],
        |r| r.get(0),
    )?;
    tx.execute_batch("CREATE TABLE metrics.snapshot_minutes (
        id INTEGER PRIMARY KEY,node_id TEXT NOT NULL,bucket_ms INTEGER NOT NULL,samples INTEGER NOT NULL CHECK(samples>0 AND samples<=64),
        data TEXT NOT NULL CHECK(json_valid(data)),dedup_until_ms INTEGER NOT NULL DEFAULT 0
      );
      CREATE INDEX metrics.snapshot_minutes_node_time ON snapshot_minutes(node_id,bucket_ms);
      CREATE INDEX metrics.snapshot_minutes_time ON snapshot_minutes(bucket_ms);
      CREATE INDEX metrics.snapshot_minutes_dedup ON snapshot_minutes(dedup_until_ms) WHERE dedup_until_ms>0;
      CREATE TABLE metrics.probe_minutes (
        id INTEGER PRIMARY KEY,node_id TEXT NOT NULL,task_id TEXT NOT NULL,bucket_ms INTEGER NOT NULL,
        samples INTEGER NOT NULL CHECK(samples>0),data TEXT NOT NULL CHECK(json_valid(data)),dedup_until_ms INTEGER NOT NULL DEFAULT 0,
        CHECK(samples<=32)
      );
      CREATE INDEX metrics.probe_minutes_node_task_time ON probe_minutes(node_id,task_id,bucket_ms);
      CREATE INDEX metrics.probe_minutes_time ON probe_minutes(bucket_ms);
      CREATE INDEX metrics.probe_minutes_dedup ON probe_minutes(dedup_until_ms) WHERE dedup_until_ms>0;
      CREATE INDEX metrics.probe_minutes_node_time ON probe_minutes(node_id,bucket_ms);
      ALTER TABLE metrics.node_state ADD COLUMN last_sample_id TEXT;
      UPDATE metrics.node_state SET last_sample_id=(SELECT sample_id FROM metrics.snapshots s
        WHERE s.node_id=node_state.node_id ORDER BY received_at_ms DESC,id DESC LIMIT 1);")?;
    while flush_snapshots(&tx, "metrics.snapshots", i64::MAX, false)? > 0 {}
    while flush_probes(&tx, "metrics.probe_results", i64::MAX, false)? > 0 {}
    let migrated_snapshots: i64 = tx.query_row(
        "SELECT coalesce(sum(samples),0) FROM metrics.snapshot_minutes",
        [],
        |r| r.get(0),
    )?;
    let migrated_probes: i64 = tx.query_row(
        "SELECT coalesce(sum(samples),0) FROM metrics.probe_minutes",
        [],
        |r| r.get(0),
    )?;
    let migrated_failed:i64=tx.query_row("SELECT coalesce(sum(samples-json_extract(data,'$.successful')),0) FROM metrics.probe_minutes",[],|r|r.get(0))?;
    if (snapshots, probes, failed) != (migrated_snapshots, migrated_probes, migrated_failed) {
        return Err("minute migration sample/failed-count verification failed".into());
    }
    tx.execute_batch(
        "DROP TABLE metrics.snapshots; DROP TABLE metrics.probe_results;
        PRAGMA metrics.user_version=2; PRAGMA main.user_version=7;",
    )?;
    let integrity: String = tx.query_row("PRAGMA metrics.integrity_check(1)", [], |r| r.get(0))?;
    if integrity != "ok" {
        return Err("minute migration integrity check failed".into());
    }
    tx.commit()?;
    super::super::metrics::configure_runtime(db)?;
    Ok(())
}
