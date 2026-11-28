//! Shared monotonic lease calculation for Service-issued v1 one-shot tasks.

use std::time::{Duration, Instant};

use reqwest::{Client, StatusCode, Url};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResultDelivery {
    /// Result accepted or permanently rejected/expired; clear pending state.
    Done,
    /// Transient transport/server error before lease expiry; retry on next poll.
    Retry,
    /// Agent credential was rejected by Service; stop the worker.
    Unauthorized,
}

/// Posts a one-shot task outcome strictly within its monotonic lease deadline.
pub(crate) async fn post_result<T: Serialize>(
    client: &Client,
    url: &Url,
    token: &str,
    outcome: &T,
    deadline: Instant,
) -> ResultDelivery {
    if Instant::now() >= deadline {
        return ResultDelivery::Done;
    }
    match tokio::time::timeout_at(
        deadline.into(),
        client
            .post(url.clone())
            .bearer_auth(token)
            .json(outcome)
            .send(),
    )
    .await
    {
        Ok(Ok(response)) if response.status() == StatusCode::UNAUTHORIZED => {
            ResultDelivery::Unauthorized
        }
        Ok(Ok(response))
            if response.status().is_success()
                || response.status() == StatusCode::CONFLICT
                || response.status() == StatusCode::BAD_REQUEST =>
        {
            ResultDelivery::Done
        }
        Err(_) => ResultDelivery::Done,
        Ok(Ok(_) | Err(_)) => ResultDelivery::Retry,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaimValidation {
    /// Task ID is invalid, or the monotonic lease already elapsed before execution.
    Ignore,
    /// Valid task ID with invalid lease metadata; report `invalid_task` within the lease.
    InvalidTask { deadline: Instant },
    /// Ready to collect within `deadline` and stamp reports relative to Service claim time.
    Ready(TaskLease),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TaskLease {
    pub(crate) deadline: Instant,
    claimed_at_unix_ms: u64,
    received_at: Instant,
    lease_ms: u64,
}

impl TaskLease {
    /// Absolute deadline bounded by both the monotonic task lease and a per-collector cap.
    pub(crate) fn collection_deadline(self, now: Instant, cap: Duration) -> Instant {
        now.checked_add(cap)
            .map_or(self.deadline, |capped| capped.min(self.deadline))
    }

    /// Conservative lower bound on Service wall time when collection finishes:
    /// `claimed_at_unix_ms` is the Service's v1 claim timestamp (`expires_at_unix_ms - TTL`),
    /// and `received_at` is strictly after the Service issued the lease, so
    /// `claimed_at_unix_ms <= checked_at_unix_ms <= service_finish_time`.
    pub(crate) fn checked_at_unix_ms(self, finished_at: Instant) -> u64 {
        let elapsed = finished_at
            .saturating_duration_since(self.received_at)
            .as_millis()
            .min(u128::from(self.lease_ms));
        let elapsed_ms = u64::try_from(elapsed).unwrap_or(self.lease_ms);
        self.claimed_at_unix_ms.saturating_add(elapsed_ms)
    }
}

pub(crate) fn validate_claimed_task(
    task_id: &str,
    expires_at_unix_ms: u64,
    lease_ms: u64,
    claim_started: Instant,
    claim_received: Instant,
) -> ClaimValidation {
    let Some(deadline) = claim_started.checked_add(Duration::from_millis(lease_ms)) else {
        return ClaimValidation::Ignore;
    };
    if Uuid::parse_str(task_id).is_err() || claim_received >= deadline {
        return ClaimValidation::Ignore;
    }
    let Some(claimed_at_unix_ms) = expires_at_unix_ms
        .checked_sub(lease_ms)
        .filter(|timestamp| *timestamp > 0)
    else {
        return ClaimValidation::InvalidTask { deadline };
    };
    ClaimValidation::Ready(TaskLease {
        deadline,
        claimed_at_unix_ms,
        received_at: claim_received,
        lease_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use senlun_protocol::{IP_UNLOCK_LEASE_MS, RETURN_ROUTE_LEASE_MS};

    #[test]
    fn valid_claim_computes_monotonic_deadline_and_server_anchored_timestamp() {
        let started = Instant::now();
        let received = started + Duration::from_secs(2);
        let server_claimed_at = 1_700_000_000_000_u64;
        let id = Uuid::new_v4().to_string();

        for lease_ms in [RETURN_ROUTE_LEASE_MS, IP_UNLOCK_LEASE_MS] {
            let ClaimValidation::Ready(lease) = validate_claimed_task(
                &id,
                server_claimed_at + lease_ms,
                lease_ms,
                started,
                received,
            ) else {
                panic!("valid claimed task should produce monotonic lease");
            };
            assert_eq!(lease.deadline, started + Duration::from_millis(lease_ms));
            assert_eq!(lease.claimed_at_unix_ms, server_claimed_at);
            assert_eq!(
                lease.checked_at_unix_ms(received + Duration::from_secs(15)),
                server_claimed_at + 15_000
            );
            assert_eq!(
                lease.checked_at_unix_ms(received + Duration::from_mins(15)),
                server_claimed_at + lease_ms
            );
            assert_eq!(
                lease.collection_deadline(received, Duration::from_secs(35)),
                (received + Duration::from_secs(35)).min(lease.deadline)
            );
            assert_eq!(
                lease.collection_deadline(received, Duration::from_mins(15)),
                lease.deadline
            );
        }
    }

    #[test]
    fn invalid_id_exhausted_claim_and_invalid_expiry_follow_lease_contract() {
        let started = Instant::now();
        let received = started + Duration::from_millis(50);
        let id = Uuid::new_v4().to_string();

        assert_eq!(
            validate_claimed_task(
                "not-a-uuid",
                1_700_000_000_000,
                IP_UNLOCK_LEASE_MS,
                started,
                received,
            ),
            ClaimValidation::Ignore
        );

        let exhausted = started + Duration::from_millis(IP_UNLOCK_LEASE_MS);
        assert_eq!(
            validate_claimed_task(
                &id,
                1_700_000_000_000,
                IP_UNLOCK_LEASE_MS,
                started,
                exhausted,
            ),
            ClaimValidation::Ignore
        );

        for invalid_expires_at in [0, IP_UNLOCK_LEASE_MS] {
            let result = validate_claimed_task(
                &id,
                invalid_expires_at,
                IP_UNLOCK_LEASE_MS,
                started,
                received,
            );
            assert_eq!(
                result,
                ClaimValidation::InvalidTask {
                    deadline: started + Duration::from_millis(IP_UNLOCK_LEASE_MS),
                }
            );
        }
    }
}
