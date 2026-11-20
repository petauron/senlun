//! Work through each peer queue independently of the carrier probes.

use std::{future::Future, net::SocketAddr, time::Duration};

use reqwest::{Client, Url};
use senlun_protocol::{
    MAX_PEER_PROBE_BATCH, PEER_PROBE_SCHEMA_VERSION, PeerProbeBatch, PeerProbeKind, PeerProbePlan,
    PeerProbeSample, PeerProbeStatus, PeerProbeTarget, ProbeKind, ProbeTask,
};

use tokio::task::JoinSet;

const ROUND_PAUSE: Duration = Duration::from_secs(30);
const MAX_CONCURRENT_PEERS: usize = 4;
// Service enrollment supports up to 1,000 active nodes.
const MAX_ROUND_PAGES: usize = 1_000_usize.div_ceil(MAX_PEER_PROBE_BATCH);
const MAX_PLAN_BYTES: usize = 8 * 1024;
const PROBE_DEADLINE: Duration = Duration::from_secs(3);

pub(crate) async fn run(client: Client, target_client: Client, service_url: Url, token: String) {
    let Ok(endpoint) = service_url.join("api/v1/agents/peer-latency") else {
        return;
    };
    loop {
        run_round(&client, &target_client, &endpoint, &token).await;
        tokio::time::sleep(ROUND_PAUSE).await;
    }
}

async fn run_round(client: &Client, target_client: &Client, endpoint: &Url, token: &str) {
    let mut after = String::new();
    let mut delivery = JoinSet::new();
    for _ in 0..MAX_ROUND_PAGES {
        let mut plan_url = endpoint.clone();
        if !after.is_empty() {
            plan_url.query_pairs_mut().append_pair("after", &after);
        }
        let Ok(response) = client.get(plan_url).bearer_auth(token).send().await else {
            break;
        };
        if !response.status().is_success() {
            break;
        }
        let Ok(plan) = crate::read_json_with_limit::<PeerProbePlan>(response, MAX_PLAN_BYTES).await
        else {
            break;
        };
        if plan.schema_version != PEER_PROBE_SCHEMA_VERSION
            || plan.targets.is_empty()
            || plan.targets.len() > MAX_PEER_PROBE_BATCH
            || plan.next_after.as_ref().is_some_and(|cursor| {
                cursor <= &after
                    || plan
                        .targets
                        .last()
                        .is_none_or(|target| &target.node_id != cursor)
            })
        {
            break;
        }
        let samples = probe_queue(plan.targets, |target| {
            let client = target_client.clone();
            async move { measure(&client, target).await }
        })
        .await;
        if !samples.is_empty() {
            // Overlap one report with the next page. Bound pending results
            // while a slow Service processes the previous report.
            let _ = delivery.join_next().await;
            let (client, endpoint, token) = (client.clone(), endpoint.clone(), token.to_owned());
            delivery.spawn(async move {
                let _ = client
                    .post(endpoint)
                    .bearer_auth(token)
                    .json(&PeerProbeBatch {
                        schema_version: PEER_PROBE_SCHEMA_VERSION,
                        issued_at_unix_ms: plan.issued_at_unix_ms,
                        samples,
                    })
                    .send()
                    .await;
            });
        }
        let Some(cursor) = plan.next_after else {
            break;
        };
        after = cursor;
    }
    let _ = delivery.join_next().await;
}

async fn probe_queue<F>(
    targets: Vec<PeerProbeTarget>,
    mut probe: impl FnMut(PeerProbeTarget) -> F,
) -> Vec<PeerProbeSample>
where
    F: Future<Output = PeerProbeSample> + Send + 'static,
{
    let mut samples = Vec::with_capacity(targets.len());
    let mut pending = targets.into_iter();
    let mut active = JoinSet::new();
    loop {
        for target in pending.by_ref().take(MAX_CONCURRENT_PEERS - active.len()) {
            active.spawn(probe(target));
        }
        let Some(outcome) = active.join_next().await else {
            break;
        };
        if let Ok(sample) = outcome {
            samples.push(sample);
        }
    }
    samples
}

async fn measure(client: &Client, target: PeerProbeTarget) -> PeerProbeSample {
    let (kind, address) = match target.kind {
        PeerProbeKind::Tcp => (
            ProbeKind::Tcp,
            SocketAddr::new(target.address, 443).to_string(),
        ),
        PeerProbeKind::Icmp => (ProbeKind::Icmp, target.address.to_string()),
    };
    let task = ProbeTask {
        id: target.node_id.clone(),
        name: "Node latency".into(),
        kind,
        target: address,
        interval_seconds: ROUND_PAUSE.as_secs(),
        timeout_seconds: PROBE_DEADLINE.as_secs(),
        enabled: true,
    };
    let (status, latency_ms) = match tokio::time::timeout(
        PROBE_DEADLINE,
        crate::probes::run_probe(client.clone(), task),
    )
    .await
    {
        Ok(result) if result.success => (PeerProbeStatus::Ready, result.latency_ms),
        Ok(result) if result.error.as_deref() == Some("unavailable") => {
            (PeerProbeStatus::Unavailable, None)
        }
        Ok(result) if result.error.as_deref() != Some("timeout") => (PeerProbeStatus::Failed, None),
        _ => (PeerProbeStatus::Timeout, None),
    };
    PeerProbeSample {
        target,
        status,
        latency_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::Ipv4Addr, sync::Arc};
    use tokio::sync::{Semaphore, mpsc};

    #[tokio::test]
    async fn free_slot_starts_next_peer_without_waiting_for_other_probes() {
        let count = MAX_CONCURRENT_PEERS + 1;
        let targets = (0..count)
            .map(|index| PeerProbeTarget {
                node_id: index.to_string(),
                address: Ipv4Addr::LOCALHOST.into(),
                kind: PeerProbeKind::Tcp,
            })
            .collect();
        let (started, mut events) = mpsc::channel(count);
        let first = Arc::new(Semaphore::new(0));
        let others = Arc::new(Semaphore::new(0));
        let queue = tokio::spawn(probe_queue(targets, {
            let first = Arc::clone(&first);
            let others = Arc::clone(&others);
            move |target| {
                let started = started.clone();
                let gate = Arc::clone(if target.node_id == "0" {
                    &first
                } else {
                    &others
                });
                async move {
                    started.send(target.node_id.clone()).await.unwrap();
                    gate.acquire().await.unwrap().forget();
                    PeerProbeSample {
                        target,
                        status: PeerProbeStatus::Ready,
                        latency_ms: Some(1.0),
                    }
                }
            }
        }));

        tokio::time::timeout(Duration::from_secs(5), async {
            let mut initial = Vec::with_capacity(MAX_CONCURRENT_PEERS);
            for _ in 0..MAX_CONCURRENT_PEERS {
                initial.push(events.recv().await.unwrap());
            }
            initial.sort_unstable();
            assert_eq!(
                initial,
                (0..MAX_CONCURRENT_PEERS)
                    .map(|index| index.to_string())
                    .collect::<Vec<_>>()
            );
            assert!(matches!(
                events.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
            first.add_permits(1);
            // All other probes are still blocked when the fifth starts.
            assert_eq!(events.recv().await, Some(MAX_CONCURRENT_PEERS.to_string()));
            others.add_permits(MAX_CONCURRENT_PEERS);
            assert_eq!(queue.await.unwrap().len(), count);
        })
        .await
        .expect("peer rotation stalled with one free slot");
    }
}
