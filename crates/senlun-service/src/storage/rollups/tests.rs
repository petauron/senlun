use super::*;
use crate::{
    control_tests::{fixture, sample},
    storage::{Storage, hash_token, unix_time_ms},
};
use senlun_protocol::{ProbeBatch, ProbeResult};

fn minute() -> u64 {
    unix_time_ms().unwrap() / MINUTE * MINUTE
}
fn total(storage: &Storage) -> u64 {
    storage
        .connection()
        .unwrap()
        .query_row(
            "SELECT coalesce(sum(samples),0) FROM snapshot_minutes",
            [],
            |r| read_u64(r, 0),
        )
        .unwrap()
}

#[test]
fn sealed_history_is_weighted_preserves_peaks_and_never_double_counts_hot_rows() {
    let (_dir, storage, node) = fixture();
    let start = minute();
    let token = hash_token(&node.agent_token);
    for (offset, cpu, up) in [(1000, 10.0, 100), (4000, 90.0, 200), (61_000, 30.0, 400)] {
        let mut snapshot = sample(start + offset, up, up * 2);
        snapshot.cpu_usage_percent = cpu;
        storage.ingest(&token, &snapshot, start + offset).unwrap();
    }
    let before = storage
        .history(&node.node_id, 1, 500, start + 62_000)
        .unwrap();
    assert_eq!(before.coverage.source_points, 3);
    assert_eq!(before.records.len(), 3);
    storage
        .flush_monitoring(start + 12 * MINUTE, false)
        .unwrap();
    assert_eq!(total(&storage), 3);
    let history = storage
        .history(&node.node_id, 1, 500, start + 12 * MINUTE)
        .unwrap();
    assert_eq!(history.coverage.source_points, 3);
    assert_eq!(history.records.len(), 2);
    assert_eq!(history.records[0]["cpu"], 50.0);
    assert_eq!(history.records[0]["peaks"]["cpu"], 90.0);
    let merged = storage
        .history(&node.node_id, 1, 1, start + 12 * MINUTE)
        .unwrap();
    assert!((merged.records[0]["cpu"].as_f64().unwrap() - 130.0 / 3.0).abs() < 1e-10);
    assert_eq!(
        storage.admin_state().unwrap()["nodes"][0]["traffic_used_up"],
        300
    );
    storage
        .flush_monitoring(start + 13 * MINUTE, false)
        .unwrap();
    assert_eq!(total(&storage), 3);
    let db = storage.connection().unwrap();
    assert_eq!(db.query_row("SELECT count(*) FROM metrics.sqlite_schema WHERE name IN ('snapshots','probe_results')",[],|r|r.get::<_,i64>(0)).unwrap(),0);
}

#[test]
fn partial_hot_eviction_switches_the_whole_minute_to_its_summary() {
    let (_dir, storage, node) = fixture();
    let start = minute();
    let token = hash_token(&node.agent_token);
    for (offset, cpu) in [(1000, 10.0), (4000, 90.0)] {
        let mut s = sample(start + offset, 100, 200);
        s.cpu_usage_percent = cpu;
        storage.ingest(&token, &s, start + offset).unwrap();
    }
    storage
        .flush_monitoring(start + HOT_WINDOW + 2000, false)
        .unwrap();
    let db = storage.connection().unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM hot.snapshots", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(db);
    let result = storage
        .history(&node.node_id, 1, 100, start + HOT_WINDOW + 2000)
        .unwrap();
    assert_eq!(result.coverage.source_points, 2);
    assert_eq!(result.records.len(), 1);
    assert_eq!(result.records[0]["cpu"], 50.0);
}

#[test]
fn same_minute_restart_retry_and_continuation_preserve_counts_and_integer_ledger() {
    let (dir, storage, node) = fixture();
    let start = minute();
    let token = hash_token(&node.agent_token);
    let first = sample(start + 1000, 100, 200);
    storage.ingest(&token, &first, start + 1000).unwrap();
    storage
        .ingest(&token, &sample(start + 4000, 200, 400), start + 4000)
        .unwrap();
    storage.flush_monitoring(start + 5000, true).unwrap();
    drop(storage);
    let storage = Storage::open(&dir.path().join("senlun.db"), 64 * 1024 * 1024).unwrap();
    storage.ingest(&token, &first, start + 6000).unwrap();
    storage
        .ingest(&token, &sample(start + 7000, 300, 600), start + 7000)
        .unwrap();
    let before = storage
        .history(&node.node_id, 1, 100, start + 8000)
        .unwrap();
    assert_eq!(before.coverage.source_points, 3);
    storage.flush_monitoring(start + 9000, true).unwrap();
    assert_eq!(total(&storage), 3);
    let after = storage
        .history(&node.node_id, 1, 100, start + 10_000)
        .unwrap();
    assert_eq!(after.coverage.source_points, 3);
    assert_eq!(
        storage.admin_state().unwrap()["nodes"][0]["traffic_used_up"],
        200
    );
}

#[test]
fn abrupt_restart_can_lose_hot_history_but_not_the_committed_traffic_ledger() {
    let (dir, storage, node) = fixture();
    let start = minute();
    let token = hash_token(&node.agent_token);
    storage
        .ingest(&token, &sample(start + 1000, 100, 200), start + 1000)
        .unwrap();
    let last = sample(start + 4000, 200, 400);
    storage.ingest(&token, &last, start + 4000).unwrap();
    drop(storage); // Model an abrupt exit: no shutdown flush.
    let storage = Storage::open(&dir.path().join("senlun.db"), 64 * 1024 * 1024).unwrap();
    assert_eq!(total(&storage), 0);
    storage.ingest(&token, &last, start + 5000).unwrap();
    storage
        .ingest(&token, &sample(start + 7000, 300, 600), start + 7000)
        .unwrap();
    assert_eq!(
        storage.admin_state().unwrap()["nodes"][0]["traffic_used_up"],
        200
    );
    assert_eq!(
        storage
            .history(&node.node_id, 1, 100, start + 8000)
            .unwrap()
            .coverage
            .source_points,
        1
    );
}

#[test]
fn failed_minute_commit_rolls_back_summary_and_hot_persisted_markers() {
    let (_dir, storage, node) = fixture();
    let start = minute();
    storage
        .ingest(
            &hash_token(&node.agent_token),
            &sample(start + 1000, 100, 200),
            start + 1000,
        )
        .unwrap();
    storage.connection().unwrap().execute_batch("CREATE TABLE metrics.flush_parent(id INTEGER PRIMARY KEY);
        CREATE TABLE metrics.flush_child(id INTEGER REFERENCES flush_parent(id) DEFERRABLE INITIALLY DEFERRED);
        CREATE TRIGGER metrics.reject_minute AFTER INSERT ON snapshot_minutes
        BEGIN INSERT INTO flush_child VALUES(999); END;").unwrap();
    assert!(storage.flush_monitoring(start + MINUTE, false).is_err());
    assert_eq!(total(&storage), 0);
    let db = storage.connection().unwrap();
    assert_eq!(
        db.query_row("SELECT sum(persisted) FROM snapshots", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    db.execute_batch("DROP TRIGGER metrics.reject_minute;")
        .unwrap();
    drop(db);
    storage.flush_monitoring(start + MINUTE, false).unwrap();
    assert_eq!(total(&storage), 1);
}

fn task(storage: &Storage, node: &str) -> crate::control::ProbeDefinition {
    let mut task = crate::control::ProbeDefinition {
        id: String::new(),
        name: "synthetic".into(),
        weight: 0,
        kind: senlun_protocol::ProbeKind::Tcp,
        target: "127.0.0.1:8080".into(),
        interval_seconds: 5,
        timeout_seconds: 1,
        enabled: true,
        node_ids: vec![node.to_owned()],
    };
    storage.save_probe(&mut task).unwrap();
    task
}
fn probe(task: &str, now: u64, latency: Option<f64>) -> ProbeResult {
    ProbeResult {
        task_id: task.to_owned(),
        sample_id: uuid::Uuid::new_v4().to_string(),
        collected_at_unix_ms: now,
        latency_ms: latency,
        success: latency.is_some(),
        error: latency.is_none().then(|| "timeout".into()),
    }
}

#[test]
fn distinct_latencies_and_reverse_source_times_keep_exact_percentiles_and_failures() {
    let (_dir, storage, node) = fixture();
    let start = minute();
    let task = task(&storage, &node.node_id);
    let mut values = Vec::new();
    for i in 0..24_u32 {
        let now = start + u64::from(i / 2) * 5000 + u64::from(i % 2);
        let value = 10.001 + f64::from(i) * 0.137;
        values.push(value);
        let mut result = probe(&task.id, now, Some(value));
        result.collected_at_unix_ms = start + MINUTE - u64::from(i); // Receive time owns the bucket.
        let batch = ProbeBatch {
            schema_version: 1,
            results: vec![result.clone()],
        };
        storage
            .ingest_probes(&node.agent_token, &batch, now)
            .unwrap();
        storage
            .ingest_probes(&node.agent_token, &batch, now)
            .unwrap();
    }
    storage
        .ingest_probes(
            &node.agent_token,
            &ProbeBatch {
                schema_version: 1,
                results: vec![probe(&task.id, start + MINUTE, None)],
            },
            start + MINUTE,
        )
        .unwrap();
    storage
        .flush_monitoring(start + 12 * MINUTE, false)
        .unwrap();
    let history = storage
        .probe_history(&node.node_id, Some(&task.id), 1, start + 12 * MINUTE)
        .unwrap();
    let summary = &history["summary"][0];
    assert_eq!(summary["samples"], 25);
    assert!((summary["loss_percent"].as_f64().unwrap() - 4.0).abs() < 1e-10);
    for (name, p) in [
        ("p50_latency_ms", 50),
        ("p95_latency_ms", 95),
        ("p99_latency_ms", 99),
    ] {
        assert!(
            (summary[name].as_f64().unwrap()
                - crate::control::probe_percentile(&values, p).unwrap())
            .abs()
                < 1e-10
        );
    }
    let db = storage.connection().unwrap();
    let data: String = db
        .query_row(
            "SELECT data FROM probe_minutes WHERE node_id=?1 AND bucket_ms=?2",
            params![node.node_id, to_i64(start).unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    let stored: ProbeMinute = decode(&data).unwrap();
    assert_eq!(stored.latencies.len(), 24);
    assert!(stored.recent_ids.is_empty());
    assert!(
        data.len() < 2200,
        "bounded latency-only summary: {} bytes",
        data.len()
    );
}

#[test]
fn repeated_same_minute_restarts_are_bounded_and_retries_do_not_count_twice() {
    let (dir, mut storage, node) = fixture();
    let start = minute();
    let task = task(&storage, &node.node_id);
    for i in 0..32_u32 {
        let now = start + 1000 + u64::from(i) * 10;
        let result = probe(&task.id, now, Some(10.123 + f64::from(i) * 0.321));
        let batch = ProbeBatch {
            schema_version: 1,
            results: vec![result],
        };
        storage
            .ingest_probes(&node.agent_token, &batch, now)
            .unwrap();
        storage.flush_monitoring(now + 1, true).unwrap();
        drop(storage);
        storage = Storage::open(&dir.path().join("senlun.db"), 64 * 1024 * 1024).unwrap();
        storage
            .ingest_probes(&node.agent_token, &batch, now + 2)
            .unwrap();
    }
    let extra = ProbeBatch {
        schema_version: 1,
        results: vec![probe(&task.id, start + 2000, Some(30.0))],
    };
    assert!(matches!(
        storage.ingest_probes(&node.agent_token, &extra, start + 2000),
        Err(StorageError::RateLimited)
    ));
    let history = storage
        .probe_history(&node.node_id, Some(&task.id), 1, start + 3000)
        .unwrap();
    assert_eq!(history["summary"][0]["samples"], 32);
    let before: String = storage
        .connection()
        .unwrap()
        .query_row(
            "SELECT data FROM probe_minutes WHERE node_id=?1 AND task_id=?2",
            params![node.node_id, task.id],
            |r| r.get(0),
        )
        .unwrap();
    let bucket: ProbeMinute = decode(&before).unwrap();
    assert_eq!(bucket.latencies.len(), 32);
    assert_eq!(bucket.recent_ids.len(), 32);
    storage
        .flush_monitoring(start + 12 * MINUTE, false)
        .unwrap();
    let after: String = storage
        .connection()
        .unwrap()
        .query_row(
            "SELECT data FROM probe_minutes WHERE node_id=?1 AND task_id=?2",
            params![node.node_id, task.id],
            |r| r.get(0),
        )
        .unwrap();
    let bucket: ProbeMinute = decode(&after).unwrap();
    assert_eq!(bucket.latencies.len(), 32);
    assert!(bucket.recent_ids.is_empty());
    assert!(after.len() < before.len());
    eprintln!(
        "32-distinct-RTT minute bytes: recent={}, historical={}",
        before.len(),
        after.len()
    );
}

#[test]
fn forward_migration_preserves_counts_peaks_and_ledger_and_keeps_paired_backup() {
    let (dir, path, db) = crate::storage::metrics::tests::version_six_store();
    drop(db);
    let storage = Storage::open(&path, 64 * 1024 * 1024).unwrap();
    assert_eq!(total(&storage), 1);
    let db = storage.connection().unwrap();
    assert_eq!(
        db.query_row("PRAGMA main.user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        7
    );
    assert_eq!(
        db.query_row("PRAGMA metrics.user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    let ledger: (i64, i64) = db
        .query_row("SELECT used_up,used_down FROM traffic_periods", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(ledger, (80, 70));
    assert!(
        std::fs::read_dir(dir.path()).unwrap().any(|p| p
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("backup"))
    );
}

#[test]
fn retry_in_an_earlier_contribution_after_restart_does_not_double_count() {
    let (dir, storage, node) = fixture();
    let task = task(&storage, &node.node_id);
    let start = minute();
    let first = probe(&task.id, start + 1000, Some(1.25));
    {
        let db = storage.connection().unwrap();
        for i in 0..33 {
            // Model a legitimate legacy/imported minute above the runtime cap.
            let id = if i == 0 {
                first.sample_id.clone()
            } else {
                uuid::Uuid::new_v4().to_string()
            };
            db.execute("INSERT INTO hot.probe_results(node_id,task_id,sample_id,collected_at_ms,received_at_ms,latency_ms,success)
                VALUES(?1,?2,?3,?4,?4,1.25,1)",params![node.node_id,task.id,id,to_i64(start+1000).unwrap()]).unwrap();
        }
    }
    storage.flush_monitoring(start + 2000, true).unwrap();
    drop(storage);
    let storage = Storage::open(&dir.path().join("senlun.db"), 64 * 1024 * 1024).unwrap();
    storage
        .ingest_probes(
            &node.agent_token,
            &ProbeBatch {
                schema_version: 1,
                results: vec![first],
            },
            start + 3000,
        )
        .unwrap();
    let history = storage
        .probe_history(&node.node_id, Some(&task.id), 1, start + 4000)
        .unwrap();
    assert_eq!(history["summary"][0]["samples"], 33);
    assert_eq!(history["chart_source_points"], 33);
    let db = storage.connection().unwrap();
    let comparison = probe_comparison(&db, &task, start, start + 4000).unwrap();
    assert_eq!(comparison[0]["samples"], 33);
    assert_eq!(comparison[0]["avg_latency_ms"], 1.25);
}

#[test]
fn invalid_legacy_data_fails_migration_without_removing_raw_history() {
    let (_dir, path, db) = crate::storage::metrics::tests::version_six_store();
    db.execute("UPDATE probe_results SET latency_ms=31000", [])
        .unwrap();
    drop(db);
    assert!(Storage::open(&path, 64 * 1024 * 1024).is_err());
    let db = Connection::open(path.with_file_name("senlun.metrics.db")).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM probe_results", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='probe_minutes'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn legacy_minute_above_runtime_limits_migrates_in_bounded_contributions() {
    let (_dir, path, db) = crate::storage::metrics::tests::version_six_store();
    let mut latencies = vec![5.5];
    db.execute_batch("BEGIN").unwrap();
    for i in 0..8193 {
        let latency = f64::from(i) * 0.137;
        latencies.push(latency);
        db.execute("INSERT INTO probe_results(node_id,task_id,sample_id,collected_at_ms,received_at_ms,latency_ms,success)
        VALUES('node-a','task-a',?1,1000,1000,?2,1)",params![format!("synthetic-{i}"),latency]).unwrap();
        db.execute("INSERT INTO snapshots(node_id,sample_id,collected_at_ms,received_at_ms,uptime_seconds,
            cpu_usage_percent,load_one,load_five,load_fifteen,memory_total_bytes,memory_used_bytes,
            swap_total_bytes,swap_used_bytes,disk_total_bytes,disk_used_bytes,
            network_receive_bytes_per_second,network_transmit_bytes_per_second,
            network_total_received_bytes,network_total_transmitted_bytes,extensions)
            SELECT node_id,?1,collected_at_ms,1000,uptime_seconds,?2,load_one,load_five,load_fifteen,
            memory_total_bytes,memory_used_bytes,swap_total_bytes,swap_used_bytes,disk_total_bytes,
            disk_used_bytes,network_receive_bytes_per_second,network_transmit_bytes_per_second,
            network_total_received_bytes,network_total_transmitted_bytes,extensions FROM snapshots WHERE id=7",
            params![format!("synthetic-{i}"),f64::from(i % 100)]).unwrap();
    }
    db.execute_batch("COMMIT").unwrap();
    drop(db);
    let storage = Storage::open(&path, 64 * 1024 * 1024).unwrap();
    let db = storage.connection().unwrap();
    for (table, maximum) in [("snapshot_minutes", 64), ("probe_minutes", 32)] {
        let (count, largest): (i64, i64) = db
            .query_row(
                &format!("SELECT sum(samples),max(samples) FROM {table}"),
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 8194);
        assert!(largest <= maximum);
    }
    let history = history(&db, "node-a", 0, 60000, 100).unwrap();
    assert_eq!(history.coverage.source_points, 8194);
    assert_eq!(history.records.len(), 1);
    assert_eq!(history.records[0]["peaks"]["cpu"], 99.0);
    let actual = queries::quantiles(&db, "node-a", "task-a", 0, 60000, 8194).unwrap();
    latencies.sort_by(f64::total_cmp);
    for (actual, p) in actual[2..].iter().zip([50, 95, 99]) {
        assert!(
            (actual.unwrap() - crate::control::probe_percentile(&latencies, p).unwrap()).abs()
                < 1e-10
        );
    }
    let latest = latest_minutes(&db).unwrap();
    assert_eq!(latest.len(), 1);
    assert_eq!(latest[0].0.last["cpu"], 92.0);
    drop(db);
    drop(storage);
    let reopened = Storage::open(&path, 64 * 1024 * 1024).unwrap();
    assert_eq!(total(&reopened), 8194);
}
