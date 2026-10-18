//! Bounded group commit for agent ingestion. Acknowledgements follow commit;
//! cancelled callers do not discard queued samples or bypass deduplication.
use crate::storage::{Storage, StorageError};
use crate::{ApiError, HANDLER_TIMEOUT, map_storage_error, session_gate};
use senulun_protocol::{ProbeBatch, SystemSnapshot};
use std::{sync::Arc, time::Duration};
use tokio::sync::{Semaphore, mpsc, oneshot};
use tokio::time::{Instant, timeout, timeout_at};

const QUEUE_CAPACITY: usize = 64;
const MAX_BATCH: usize = 64;
const MAX_BATCH_DELAY: Duration = Duration::from_secs(1);

#[cfg(test)]
mod tests;

pub(crate) enum Input {
    Snapshot {
        token_hash: String,
        snapshot: Box<SystemSnapshot>,
        received_at: u64,
    },
    Probes {
        token: String,
        batch: ProbeBatch,
        received_at: u64,
    },
}
impl Input {
    fn received_at(&self) -> u64 {
        match self {
            Self::Snapshot { received_at, .. } | Self::Probes { received_at, .. } => *received_at,
        }
    }
}
struct Pending {
    input: Input,
    reply: oneshot::Sender<Result<(), ApiError>>,
    _lease: Option<Arc<session_gate::SessionLease>>,
}
pub(crate) struct Ingestor {
    sender: mpsc::Sender<Pending>,
}

impl Ingestor {
    pub(crate) fn start(storage: Arc<Storage>, permit: Arc<Semaphore>) -> Self {
        let (sender, mut receiver) = mpsc::channel::<Pending>(QUEUE_CAPACITY);
        tokio::spawn(async move {
            while let Some(first) = receiver.recv().await {
                let mut pending = Vec::with_capacity(MAX_BATCH);
                pending.push(first);
                let deadline = Instant::now() + MAX_BATCH_DELAY;
                while pending.len() < MAX_BATCH {
                    match timeout_at(deadline, receiver.recv()).await {
                        Ok(Some(item)) => pending.push(item),
                        Ok(None) | Err(_) => break,
                    }
                }
                // No sender is owned by this worker. Dropping all AppStates
                // closes the queue and drains accepted partial batches.
                let Ok(Ok(guard)) =
                    timeout(HANDLER_TIMEOUT, Arc::clone(&permit).acquire_owned()).await
                else {
                    for item in pending {
                        let _ = item
                            .reply
                            .send(Err(ApiError::unavailable("database queue timed out")));
                    }
                    continue;
                };
                let storage = Arc::clone(&storage);
                if tokio::task::spawn_blocking(move || {
                    let _guard = guard;
                    let outcome =
                        storage.ingest_batch(&pending.iter().map(|p| &p.input).collect::<Vec<_>>());
                    match outcome {
                        Ok(results) => {
                            for (item, result) in pending.into_iter().zip(results) {
                                let _ = item.reply.send(result.map_err(map_storage_error));
                            }
                        }
                        Err(error) => {
                            let error = map_storage_error(error);
                            for item in pending {
                                let _ = item.reply.send(Err(error.clone()));
                            }
                        }
                    }
                })
                .await
                .is_err()
                {
                    tracing::error!("ingestion database worker failed");
                }
            }
        });
        Self { sender }
    }

    pub(crate) async fn submit(&self, input: Input) -> Result<(), ApiError> {
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(Pending {
                input,
                reply,
                _lease: session_gate::current(),
            })
            .map_err(|_| ApiError::unavailable("ingestion queue is unavailable or full"))?;
        timeout(HANDLER_TIMEOUT, response)
            .await
            .map_err(|_| ApiError::unavailable("ingestion commit timed out"))?
            .map_err(|_| ApiError::unavailable("ingestion worker is unavailable"))?
    }
}

impl Storage {
    // Savepoints isolate validation failures. A SQLite/commit failure fails all
    // requests before acknowledgements, preserving the existing durability rule.
    fn ingest_batch(
        &self,
        inputs: &[&Input],
    ) -> Result<Vec<Result<(), StorageError>>, StorageError> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let mut results = Vec::with_capacity(inputs.len());
        for input in inputs {
            tx.execute_batch("SAVEPOINT ingest_item")?;
            let result = match input {
                Input::Snapshot {
                    token_hash,
                    snapshot,
                    received_at,
                } => Self::ingest_snapshot_tx(&tx, token_hash, snapshot, *received_at),
                Input::Probes {
                    token,
                    batch,
                    received_at,
                } => Self::ingest_probes_tx(&tx, token, batch, *received_at),
            };
            match result {
                Ok(()) => {
                    tx.execute_batch("RELEASE ingest_item")?;
                    results.push(Ok(()));
                }
                Err(error @ StorageError::Database(_)) => return Err(error),
                Err(error) => {
                    tx.execute_batch("ROLLBACK TO ingest_item; RELEASE ingest_item")?;
                    results.push(Err(error));
                }
            }
        }
        if let Some(now) = inputs.iter().map(|input| input.received_at()).max() {
            crate::storage::rollups::flush(&tx, now, false)?;
        }
        tx.commit()?;
        Ok(results)
    }

    #[cfg(test)]
    pub(crate) fn ingest(
        &self,
        token_hash: &str,
        snapshot: &SystemSnapshot,
        received_at: u64,
    ) -> Result<(), StorageError> {
        self.ingest_batch(&[&Input::Snapshot {
            token_hash: token_hash.into(),
            snapshot: Box::new(snapshot.clone()),
            received_at,
        }])?
        .remove(0)
    }
    #[cfg(test)]
    pub(crate) fn ingest_probes(
        &self,
        token: &str,
        batch: &ProbeBatch,
        received_at: u64,
    ) -> Result<(), StorageError> {
        self.ingest_batch(&[&Input::Probes {
            token: token.into(),
            batch: batch.clone(),
            received_at,
        }])?
        .remove(0)
    }
}
