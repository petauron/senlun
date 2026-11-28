//! Work through each peer queue independently of the carrier probes.

use std::{
    future::Future,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use reqwest::{Client, StatusCode, Url};
use senlun_protocol::{
    DEFAULT_PEER_PROBE_INTERVAL_SECONDS, MAX_PEER_PROBE_BATCH, MAX_PEER_PROBE_INTERVAL_SECONDS,
    MAX_PEER_PROBE_TARGETS, MIN_PEER_PROBE_INTERVAL_SECONDS, PEER_PROBE_SCHEMA_VERSION,
    PeerProbeBatch, PeerProbeKind, PeerProbePlan, PeerProbeSample, PeerProbeStatus,
    PeerProbeTarget, ProbeKind, ProbeTask,
};

use tokio::{sync::Semaphore, task::JoinSet, time::Instant};

const MAX_CONCURRENT_PEERS: usize = 4;
const MAX_ROUND_PAGES: usize = MAX_PEER_PROBE_TARGETS.div_ceil(MAX_PEER_PROBE_BATCH);
const MAX_PLAN_BYTES: usize = 8 * 1024;
const PROBE_DEADLINE: Duration = Duration::from_secs(3);

#[derive(Debug)]
enum PlanFetch {
    Plan(PeerProbePlan),
    Unauthorized,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReportOutcome {
    Accepted,
    Unauthorized,
    Failed,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct RoundOutcome {
    interval: Option<Duration>,
    unauthorized: bool,
    fetch_failed: bool,
    report_failures: usize,
}

pub(crate) async fn run(client: Client, target_client: Client, service_url: Url, token: String) {
    let Ok(endpoint) = service_url.join("api/v1/agents/peer-latency") else {
        return;
    };
    let mut interval = Duration::from_secs(DEFAULT_PEER_PROBE_INTERVAL_SECONDS);
    loop {
        let started = Instant::now();
        let outcome = run_round(&client, &target_client, &endpoint, &token).await;
        if outcome.unauthorized {
            tracing::warn!("peer latency worker stopping after HTTP 401");
            return;
        }
        if outcome.report_failures > 0 {
            tracing::warn!(
                failed_batches = outcome.report_failures,
                "peer latency batch report failed; the next round will sample again"
            );
        }
        if let Some(updated) = outcome.interval {
            interval = updated;
        }
        tokio::time::sleep_until(started + interval).await;
    }
}

async fn run_round(
    client: &Client,
    target_client: &Client,
    endpoint: &Url,
    token: &str,
) -> RoundOutcome {
    let target_client = target_client.clone();
    let (report_client, report_url, report_token) =
        (client.clone(), endpoint.clone(), token.to_owned());
    run_round_with(
        |after| fetch_plan(client, endpoint, token, after),
        move |target| {
            let client = target_client.clone();
            async move {
                measure(target, |kind, address| {
                    measure_address(&client, kind, address)
                })
                .await
            }
        },
        move |batch| {
            report_batch(
                report_client.clone(),
                report_url.clone(),
                report_token.clone(),
                batch,
            )
        },
    )
    .await
}

async fn fetch_plan(client: &Client, endpoint: &Url, token: &str, after: String) -> PlanFetch {
    let mut plan_url = endpoint.clone();
    if !after.is_empty() {
        plan_url.query_pairs_mut().append_pair("after", &after);
    }
    let Ok(response) = client.get(plan_url).bearer_auth(token).send().await else {
        tracing::warn!(
            reason = "transport",
            "peer latency plan request failed; the next round will sample again"
        );
        return PlanFetch::Failed;
    };
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED {
        return PlanFetch::Unauthorized;
    }
    if !status.is_success() {
        tracing::warn!(
            status = status.as_u16(),
            "peer latency plan request rejected; the next round will sample again"
        );
        return PlanFetch::Failed;
    }
    match crate::read_json_with_limit(response, MAX_PLAN_BYTES).await {
        Ok(plan) => PlanFetch::Plan(plan),
        Err(_) => {
            tracing::warn!(
                reason = "invalid_response",
                "peer latency plan response was invalid; the next round will sample again"
            );
            PlanFetch::Failed
        }
    }
}

async fn report_batch(
    client: Client,
    endpoint: Url,
    token: String,
    batch: PeerProbeBatch,
) -> ReportOutcome {
    match client
        .post(endpoint)
        .bearer_auth(&token)
        .json(&batch)
        .send()
        .await
    {
        Ok(response) if response.status() == StatusCode::UNAUTHORIZED => {
            ReportOutcome::Unauthorized
        }
        Ok(response) if response.status().is_success() => ReportOutcome::Accepted,
        Ok(_) | Err(_) => ReportOutcome::Failed,
    }
}

async fn run_round_with<Fetch, Probe, Report>(
    mut fetch: impl FnMut(String) -> Fetch,
    probe: impl FnMut(PeerProbeTarget) -> Probe + Clone + Send + 'static,
    report: impl FnMut(PeerProbeBatch) -> Report + Clone + Send + 'static,
) -> RoundOutcome
where
    Fetch: Future<Output = PlanFetch>,
    Probe: Future<Output = PeerProbeSample> + Send + 'static,
    Report: Future<Output = ReportOutcome> + Send + 'static,
{
    let mut after = String::new();
    let mut outcome = RoundOutcome::default();
    let mut remaining = MAX_PEER_PROBE_TARGETS;
    let slots = Arc::new(Semaphore::new(MAX_CONCURRENT_PEERS));
    let mut pages = JoinSet::new();
    // At most four pages and 99 targets can be outstanding. Fetch subsequent
    // pages while earlier pages measure and report through the shared slots.
    for _ in 0..MAX_ROUND_PAGES {
        let plan = match fetch(after.clone()).await {
            PlanFetch::Plan(plan) => plan,
            PlanFetch::Unauthorized => {
                outcome.unauthorized = true;
                return outcome;
            }
            PlanFetch::Failed => {
                outcome.fetch_failed = true;
                break;
            }
        };
        if plan.schema_version != PEER_PROBE_SCHEMA_VERSION
            || !(MIN_PEER_PROBE_INTERVAL_SECONDS..=MAX_PEER_PROBE_INTERVAL_SECONDS)
                .contains(&plan.interval_seconds)
            || plan.targets.len() > remaining.min(MAX_PEER_PROBE_BATCH)
            || plan.next_after.as_ref().is_some_and(|cursor| {
                cursor <= &after
                    || plan
                        .targets
                        .last()
                        .is_none_or(|target| &target.node_id != cursor)
            })
        {
            tracing::warn!(
                reason = "invalid_plan",
                "peer latency plan failed validation; skipping remaining pages"
            );
            outcome.fetch_failed = true;
            break;
        }
        if after.is_empty() {
            outcome.interval = Some(Duration::from_secs(plan.interval_seconds));
        }
        if plan.targets.is_empty() {
            break;
        }
        remaining -= plan.targets.len();
        let probe = probe.clone();
        let mut report = report.clone();
        let slots = Arc::clone(&slots);
        pages.spawn(async move {
            let samples = probe_queue(plan.targets, slots, probe).await;
            if samples.is_empty() {
                return ReportOutcome::Accepted;
            }
            report(PeerProbeBatch {
                schema_version: PEER_PROBE_SCHEMA_VERSION,
                issued_at_unix_ms: plan.issued_at_unix_ms,
                samples,
            })
            .await
        });
        if remaining == 0 {
            break;
        }
        let Some(cursor) = plan.next_after else {
            break;
        };
        after = cursor;
    }
    while let Some(joined) = pages.join_next().await {
        match joined {
            Ok(ReportOutcome::Accepted) => {}
            Ok(ReportOutcome::Unauthorized) => {
                outcome.unauthorized = true;
                return outcome;
            }
            Ok(ReportOutcome::Failed) | Err(_) => outcome.report_failures += 1,
        }
    }
    outcome
}

async fn probe_queue<F>(
    targets: Vec<PeerProbeTarget>,
    slots: Arc<Semaphore>,
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
            let slots = Arc::clone(&slots);
            let probe = probe(target);
            active.spawn(async move {
                let _slot = slots.acquire().await.expect("peer slots are never closed");
                probe.await
            });
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

async fn measure<F>(
    target: PeerProbeTarget,
    mut probe: impl FnMut(PeerProbeKind, IpAddr) -> F,
) -> PeerProbeSample
where
    F: Future<Output = (PeerProbeStatus, Option<f64>)>,
{
    let mut address = target.address;
    let (mut status, mut latency_ms) = probe(target.kind, address).await;
    if status != PeerProbeStatus::Ready
        && let Some(fallback) = target.fallback_address
    {
        address = fallback;
        (status, latency_ms) = probe(target.kind, address).await;
    }
    PeerProbeSample {
        node_id: target.node_id,
        address,
        kind: target.kind,
        status,
        latency_ms,
    }
}

async fn measure_address(
    client: &Client,
    kind: PeerProbeKind,
    address: IpAddr,
) -> (PeerProbeStatus, Option<f64>) {
    let (kind, address) = match kind {
        PeerProbeKind::Tcp => (ProbeKind::Tcp, SocketAddr::new(address, 443).to_string()),
        PeerProbeKind::Icmp => (ProbeKind::Icmp, address.to_string()),
    };
    let task = ProbeTask {
        id: String::new(),
        name: "Node latency".into(),
        kind,
        target: address,
        interval_seconds: DEFAULT_PEER_PROBE_INTERVAL_SECONDS,
        timeout_seconds: PROBE_DEADLINE.as_secs(),
        enabled: true,
    };
    match tokio::time::timeout(
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::ready,
        net::{Ipv4Addr, Ipv6Addr},
    };
    use tokio::sync::mpsc;

    fn target(index: usize) -> PeerProbeTarget {
        PeerProbeTarget {
            node_id: index.to_string(),
            address: Ipv4Addr::LOCALHOST.into(),
            fallback_address: None,
            kind: PeerProbeKind::Tcp,
        }
    }

    #[tokio::test]
    async fn free_slot_starts_next_peer_without_waiting_for_other_probes() {
        assert_queue_refills(false).await;
    }

    #[tokio::test]
    async fn free_slot_starts_peer_on_next_page_before_first_page_finishes() {
        assert_queue_refills(true).await;
    }

    #[tokio::test]
    async fn slow_reports_do_not_block_later_pages() {
        let (measuring, mut measured) = mpsc::channel(MAX_ROUND_PAGES);
        let (reporting, mut reported) = mpsc::channel(MAX_ROUND_PAGES);
        let allow_reports = Arc::new(Semaphore::new(0));
        let round = tokio::spawn(run_round_with(
            |after| {
                let index = after.parse::<usize>().unwrap_or(0) + 1;
                ready(PlanFetch::Plan(PeerProbePlan {
                    schema_version: PEER_PROBE_SCHEMA_VERSION,
                    issued_at_unix_ms: 1,
                    interval_seconds: if index == 1 { 75 } else { 60 },
                    targets: vec![target(index)],
                    next_after: (index < MAX_ROUND_PAGES).then(|| index.to_string()),
                }))
            },
            move |target| {
                let measuring = measuring.clone();
                async move {
                    measuring.send(target.node_id.clone()).await.unwrap();
                    measure(target, |_, _| ready((PeerProbeStatus::Ready, Some(1.0)))).await
                }
            },
            {
                let allow_reports = Arc::clone(&allow_reports);
                move |batch| {
                    let reporting = reporting.clone();
                    let allow_reports = Arc::clone(&allow_reports);
                    async move {
                        reporting
                            .send(batch.samples[0].node_id.clone())
                            .await
                            .unwrap();
                        allow_reports.acquire().await.unwrap().forget();
                        ReportOutcome::Accepted
                    }
                }
            },
        ));

        tokio::time::timeout(Duration::from_secs(5), async {
            let mut measurements = Vec::new();
            let mut reports = Vec::new();
            for _ in 0..MAX_ROUND_PAGES {
                measurements.push(measured.recv().await.unwrap());
                reports.push(reported.recv().await.unwrap());
            }
            measurements.sort_unstable();
            reports.sort_unstable();
            assert_eq!(
                measurements,
                (1..=MAX_ROUND_PAGES)
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
            );
            assert_eq!(reports, measurements);
            assert!(!round.is_finished());
            allow_reports.add_permits(MAX_ROUND_PAGES);
            assert_eq!(
                round.await.unwrap(),
                RoundOutcome {
                    interval: Some(Duration::from_secs(75)),
                    unauthorized: false,
                    fetch_failed: false,
                    report_failures: 0,
                }
            );
        })
        .await
        .expect("peer rotation waited for a slow report");
    }

    #[tokio::test]
    async fn empty_plans_still_set_the_round_interval() {
        let outcome = run_round_with(
            |_| {
                ready(PlanFetch::Plan(PeerProbePlan {
                    schema_version: PEER_PROBE_SCHEMA_VERSION,
                    issued_at_unix_ms: 1,
                    interval_seconds: 120,
                    targets: Vec::new(),
                    next_after: None,
                }))
            },
            |_| async { unreachable!("empty plans do not start probes") },
            |_| async { unreachable!("empty plans do not send reports") },
        )
        .await;
        assert_eq!(
            outcome,
            RoundOutcome {
                interval: Some(Duration::from_mins(2)),
                unauthorized: false,
                fetch_failed: false,
                report_failures: 0,
            }
        );
    }

    #[tokio::test]
    async fn unauthorized_plan_stops_peer_latency_worker() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf).await;
            let reply = concat!(
                "HTTP/1.1 401 Unauthorized\r\n",
                "Content-Length: 0\r\n",
                "Connection: close\r\n\r\n"
            );
            stream.write_all(reply.as_bytes()).await.unwrap();
        });

        tokio::time::timeout(
            Duration::from_secs(2),
            run(Client::new(), Client::new(), url, "test-token".to_owned()),
        )
        .await
        .expect("worker should stop after HTTP 401");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn unauthorized_report_aborts_remaining_pages_immediately() {
        let outcome = tokio::time::timeout(
            Duration::from_secs(2),
            run_round_with(
                |after| {
                    let index = after.parse::<usize>().unwrap_or(0) + 1;
                    ready(PlanFetch::Plan(PeerProbePlan {
                        schema_version: PEER_PROBE_SCHEMA_VERSION,
                        issued_at_unix_ms: 1,
                        interval_seconds: 60,
                        targets: vec![target(index)],
                        next_after: (index == 1).then(|| "1".to_owned()),
                    }))
                },
                |item| {
                    ready(PeerProbeSample {
                        node_id: item.node_id,
                        address: item.address,
                        kind: item.kind,
                        status: PeerProbeStatus::Ready,
                        latency_ms: Some(1.0),
                    })
                },
                |batch| async move {
                    if batch.samples[0].node_id == "1" {
                        ReportOutcome::Unauthorized
                    } else {
                        std::future::pending::<ReportOutcome>().await
                    }
                },
            ),
        )
        .await
        .expect("HTTP 401 report should abort remaining in-flight pages");
        assert!(outcome.unauthorized);
    }

    #[tokio::test]
    async fn report_failures_and_unauthorized_are_recorded_per_round() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        async fn serve_status(status: &str) -> Url {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = [0_u8; 1024];
                let _ = stream.read(&mut buf).await;
                let _ = stream.write_all(response.as_bytes()).await;
            });
            url
        }

        fn empty_batch() -> PeerProbeBatch {
            PeerProbeBatch {
                schema_version: PEER_PROBE_SCHEMA_VERSION,
                issued_at_unix_ms: 1,
                samples: Vec::new(),
            }
        }

        let failed_url = serve_status("503 Service Unavailable").await;
        assert!(matches!(
            fetch_plan(&Client::new(), &failed_url, "token", String::new()).await,
            PlanFetch::Failed
        ));

        let unauth_report_url = serve_status("401 Unauthorized").await;
        let failed_report_url = serve_status("500 Internal Server Error").await;
        assert_eq!(
            report_batch(
                Client::new(),
                unauth_report_url,
                "token".to_owned(),
                empty_batch()
            )
            .await,
            ReportOutcome::Unauthorized
        );
        assert_eq!(
            report_batch(
                Client::new(),
                failed_report_url,
                "token".to_owned(),
                empty_batch()
            )
            .await,
            ReportOutcome::Failed
        );

        let outcome = run_round_with(
            |_| {
                ready(PlanFetch::Plan(PeerProbePlan {
                    schema_version: PEER_PROBE_SCHEMA_VERSION,
                    issued_at_unix_ms: 1,
                    interval_seconds: 60,
                    targets: vec![target(1)],
                    next_after: None,
                }))
            },
            |item| {
                ready(PeerProbeSample {
                    node_id: item.node_id,
                    address: item.address,
                    kind: item.kind,
                    status: PeerProbeStatus::Ready,
                    latency_ms: Some(1.0),
                })
            },
            |_| ready(ReportOutcome::Failed),
        )
        .await;
        assert_eq!(
            outcome,
            RoundOutcome {
                interval: Some(Duration::from_mins(1)),
                unauthorized: false,
                fetch_failed: false,
                report_failures: 1,
            }
        );
    }

    async fn assert_queue_refills(two_pages: bool) {
        let count = if two_pages {
            MAX_CONCURRENT_PEERS
        } else {
            MAX_CONCURRENT_PEERS + 1
        };
        let targets = (0..count).map(target).collect();
        let slots = Arc::new(Semaphore::new(MAX_CONCURRENT_PEERS));
        let (started, mut events) = mpsc::channel(count);
        let first = Arc::new(Semaphore::new(0));
        let others = Arc::new(Semaphore::new(0));
        let probe = {
            let first = Arc::clone(&first);
            let others = Arc::clone(&others);
            move |target: PeerProbeTarget| {
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
                        node_id: target.node_id,
                        address: target.address,
                        kind: target.kind,
                        status: PeerProbeStatus::Ready,
                        latency_ms: Some(1.0),
                    }
                }
            }
        };
        let queue = tokio::spawn(probe_queue(targets, Arc::clone(&slots), probe.clone()));

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
            let next_page = two_pages.then(|| {
                tokio::spawn(probe_queue(
                    vec![target(MAX_CONCURRENT_PEERS)],
                    slots,
                    probe,
                ))
            });
            tokio::task::yield_now().await;
            assert!(matches!(
                events.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
            first.add_permits(1);
            // All other probes are still blocked when the fifth starts.
            assert_eq!(events.recv().await, Some(MAX_CONCURRENT_PEERS.to_string()));
            others.add_permits(MAX_CONCURRENT_PEERS);
            assert_eq!(queue.await.unwrap().len(), count);
            if let Some(next_page) = next_page {
                assert_eq!(next_page.await.unwrap().len(), 1);
            }
        })
        .await
        .expect("peer rotation stalled with one free slot");
    }

    #[tokio::test]
    async fn auto_probes_fallback_only_after_primary_fails() {
        let mut dual_stack = target(0);
        let fallback = Ipv6Addr::LOCALHOST.into();
        dual_stack.fallback_address = Some(fallback);
        let mut attempts = Vec::new();
        let sample = measure(dual_stack.clone(), |_, address| {
            attempts.push(address);
            ready(if address == fallback {
                (PeerProbeStatus::Ready, Some(5.0))
            } else {
                (PeerProbeStatus::Timeout, None)
            })
        })
        .await;
        assert_eq!(attempts, [dual_stack.address, fallback]);
        assert_eq!(sample.address, fallback);
        assert_eq!(sample.status, PeerProbeStatus::Ready);
        assert_eq!(sample.latency_ms, Some(5.0));

        let primary = dual_stack.address;
        let sample = measure(dual_stack, |_, address| {
            assert_eq!(address, primary);
            ready((PeerProbeStatus::Ready, Some(1.0)))
        })
        .await;
        assert_eq!(sample.address, primary);
    }
}
