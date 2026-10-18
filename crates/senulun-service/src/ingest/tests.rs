use super::*;
use crate::control_tests::{fixture, sample};
use crate::storage::{hash_token, unix_time_ms};
use senulun_protocol::{ProbeBatch, ProbeResult};

fn input(token: &str, now: u64, up: u64, down: u64) -> Input {
    Input::Snapshot {
        token_hash: hash_token(token),
        snapshot: Box::new(sample(now, up, down)),
        received_at: now,
    }
}
fn count(storage: &Storage) -> i64 {
    storage
        .connection()
        .unwrap()
        .query_row("SELECT count(*) FROM snapshots", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn batch_preserves_every_sample_and_exact_deltas_across_duplicates_and_counter_reset() {
    let (_dir, storage, node) = fixture();
    let now = unix_time_ms().unwrap();
    let first = input(&node.agent_token, now, 1000, 2000);
    let next = input(&node.agent_token, now + 3000, 1100, 2200);
    let reset = input(&node.agent_token, now + 6000, 20, 30);
    let last = input(&node.agent_token, now + 9000, 50, 80);
    let bad = input("invalid-credential", now + 3000, 9999, 9999);
    let too_fast = input(&node.agent_token, now + 100, 9999, 9999);
    let results = storage
        .ingest_batch(&[&first, &first, &bad, &too_fast, &next, &reset, &last])
        .unwrap();
    assert!(results[0].is_ok() && results[1].is_ok());
    assert!(matches!(results[2], Err(StorageError::Unauthorized)));
    assert!(matches!(results[3], Err(StorageError::RateLimited)));
    assert!(results[4..].iter().all(Result::is_ok));
    assert_eq!(count(&storage), 4);
    let state = storage.admin_state().unwrap();
    assert_eq!(state["nodes"][0]["traffic_used_up"], 130);
    assert_eq!(state["nodes"][0]["traffic_used_down"], 250);
    let times: Vec<i64> = storage
        .connection()
        .unwrap()
        .prepare("SELECT collected_at_ms FROM snapshots ORDER BY received_at_ms")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        times,
        [now, now + 3000, now + 6000, now + 9000].map(|v| i64::try_from(v).unwrap())
    );
}

#[test]
fn rejected_probe_batch_does_not_remove_other_requests_or_leave_partial_results() {
    let (_dir, storage, node) = fixture();
    let now = unix_time_ms().unwrap();
    let mut task = crate::control::ProbeDefinition {
        id: String::new(),
        name: "synthetic".into(),
        weight: 0,
        kind: senulun_protocol::ProbeKind::Tcp,
        target: "127.0.0.1:8080".into(),
        interval_seconds: 60,
        timeout_seconds: 1,
        enabled: true,
        node_ids: vec![node.node_id.clone()],
    };
    storage.save_probe(&mut task).unwrap();
    let result = ProbeResult {
        task_id: task.id,
        sample_id: uuid::Uuid::new_v4().to_string(),
        collected_at_unix_ms: now,
        latency_ms: Some(1.0),
        success: true,
        error: None,
    };
    let mut invalid = result.clone();
    invalid.task_id = "not-assigned".into();
    invalid.sample_id = uuid::Uuid::new_v4().to_string();
    let probes = Input::Probes {
        token: node.agent_token.clone(),
        batch: ProbeBatch {
            schema_version: 1,
            results: vec![result.clone(), invalid],
        },
        received_at: now,
    };
    let valid = Input::Probes {
        token: node.agent_token.clone(),
        batch: ProbeBatch {
            schema_version: 1,
            results: vec![result],
        },
        received_at: now,
    };
    let snapshot = input(&node.agent_token, now, 100, 200);
    let results = storage
        .ingest_batch(&[&snapshot, &probes, &valid, &valid])
        .unwrap();
    assert!(results[0].is_ok());
    assert!(matches!(results[1], Err(StorageError::InvalidInput(_))));
    assert!(results[2].is_ok() && results[3].is_ok());
    assert_eq!(count(&storage), 1);
    assert_eq!(
        storage
            .connection()
            .unwrap()
            .query_row("SELECT count(*) FROM probe_results", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn commit_failure_rolls_back_all_samples_and_usage_before_any_success() {
    let (_dir, storage, node) = fixture();
    storage.connection().unwrap().execute_batch("CREATE TABLE metrics.commit_parent(id INTEGER PRIMARY KEY);
        CREATE TABLE metrics.commit_child(id INTEGER REFERENCES commit_parent(id) DEFERRABLE INITIALLY DEFERRED);
        CREATE TRIGGER metrics.reject_commit AFTER INSERT ON traffic_periods BEGIN INSERT INTO commit_child VALUES(999); END;").unwrap();
    let now = unix_time_ms().unwrap();
    let a = input(&node.agent_token, now, 100, 200);
    let b = input(&node.agent_token, now + 3000, 150, 250);
    assert!(matches!(
        storage.ingest_batch(&[&a, &b]),
        Err(StorageError::Database(_))
    ));
    assert_eq!(count(&storage), 0);
    assert_eq!(
        storage
            .connection()
            .unwrap()
            .query_row("SELECT count(*) FROM traffic_periods", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn acknowledgement_waits_for_database_commit() {
    let (_dir, storage, node) = fixture();
    let storage = Arc::new(storage);
    let permit = Arc::new(Semaphore::new(1));
    let held = permit.acquire().await.unwrap();
    let writer = Arc::new(Ingestor::start(Arc::clone(&storage), Arc::clone(&permit)));
    let request = input(&node.agent_token, unix_time_ms().unwrap(), 100, 200);
    let cloned = Arc::clone(&writer);
    let waiting = tokio::spawn(async move { cloned.submit(request).await });
    tokio::time::sleep(MAX_BATCH_DELAY + Duration::from_millis(50)).await;
    assert!(!waiting.is_finished());
    assert_eq!(count(&storage), 0);
    drop(held);
    waiting.await.unwrap().unwrap();
    assert_eq!(count(&storage), 1);
}

#[tokio::test]
async fn cancelled_caller_is_committed_and_retry_is_not_counted_twice() {
    let (_dir, storage, node) = fixture();
    let storage = Arc::new(storage);
    let writer = Arc::new(Ingestor::start(
        Arc::clone(&storage),
        Arc::new(Semaphore::new(1)),
    ));
    let now = unix_time_ms().unwrap();
    let first = sample(now, 100, 200);
    let request = Input::Snapshot {
        token_hash: hash_token(&node.agent_token),
        snapshot: Box::new(first.clone()),
        received_at: now,
    };
    let (reply, cancelled) = oneshot::channel();
    writer
        .sender
        .send(Pending {
            input: request,
            reply,
            _lease: None,
        })
        .await
        .ok()
        .unwrap();
    drop(cancelled);
    writer
        .submit(Input::Snapshot {
            token_hash: hash_token(&node.agent_token),
            snapshot: Box::new(first),
            received_at: now + 3000,
        })
        .await
        .unwrap();
    assert_eq!(count(&storage), 1);
}

#[tokio::test]
async fn credential_revoked_while_queued_is_rechecked_before_writing() {
    let (_dir, storage, node) = fixture();
    let storage = Arc::new(storage);
    let writer = Ingestor::start(Arc::clone(&storage), Arc::new(Semaphore::new(1)));
    let now = unix_time_ms().unwrap();
    let (reply, result) = oneshot::channel();
    writer
        .sender
        .try_send(Pending {
            input: input(&node.agent_token, now, 1, 2),
            reply,
            _lease: None,
        })
        .ok()
        .unwrap();
    storage.revoke_node(&node.node_id, now).unwrap();
    assert_eq!(
        result.await.unwrap().unwrap_err().status,
        axum::http::StatusCode::UNAUTHORIZED
    );
    assert_eq!(count(&storage), 0);
}

#[tokio::test]
async fn closing_queue_drains_partial_batch_and_saturated_queue_fails_explicitly() {
    let (_dir, storage, node) = fixture();
    let storage = Arc::new(storage);
    let writer = Ingestor::start(Arc::clone(&storage), Arc::new(Semaphore::new(1)));
    let (reply, result) = oneshot::channel();
    writer
        .sender
        .try_send(Pending {
            input: input(&node.agent_token, unix_time_ms().unwrap(), 1, 2),
            reply,
            _lease: None,
        })
        .ok()
        .unwrap();
    drop(writer);
    result.await.unwrap().unwrap();
    assert_eq!(count(&storage), 1);
    let (sender, _receiver) = mpsc::channel(1);
    let full = Ingestor { sender };
    let (reply, _result) = oneshot::channel();
    full.sender
        .try_send(Pending {
            input: input(&node.agent_token, unix_time_ms().unwrap(), 1, 2),
            reply,
            _lease: None,
        })
        .ok()
        .unwrap();
    assert_eq!(
        full.submit(input(&node.agent_token, unix_time_ms().unwrap(), 1, 2))
            .await
            .unwrap_err()
            .status,
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );
}
