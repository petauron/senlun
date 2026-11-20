//! One peer probe at a time, in a separate rotation from the carrier probes.

use std::{net::SocketAddr, time::Duration};

use reqwest::{Client, Url};
use senlun_protocol::{
    MAX_PEER_PROBE_BATCH, PEER_PROBE_SCHEMA_VERSION, PeerProbeBatch, PeerProbeKind, PeerProbePlan,
    PeerProbeSample, PeerProbeStatus, PeerProbeTarget, ProbeKind, ProbeTask,
};

const REFRESH: Duration = Duration::from_secs(30);
const PROBE_DEADLINE: Duration = Duration::from_secs(3);

pub(crate) async fn run(client: Client, target_client: Client, service_url: Url, token: String) {
    let Ok(endpoint) = service_url.join("api/v1/agents/peer-latency") else {
        return;
    };
    let mut cursor = String::new();
    let mut ticker = tokio::time::interval(REFRESH);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        let mut plan_url = endpoint.clone();
        if !cursor.is_empty() {
            plan_url.query_pairs_mut().append_pair("after", &cursor);
        }
        let Ok(response) = client.get(plan_url).bearer_auth(&token).send().await else {
            continue;
        };
        if !response.status().is_success() {
            continue;
        }
        let Ok(plan) = crate::read_json_with_limit::<PeerProbePlan>(response, 4096).await else {
            continue;
        };
        if plan.schema_version != PEER_PROBE_SCHEMA_VERSION
            || plan.targets.len() > MAX_PEER_PROBE_BATCH
        {
            continue;
        }
        let mut samples = Vec::with_capacity(plan.targets.len());
        for target in plan.targets {
            samples.push(measure(&target_client, target).await);
        }
        cursor = samples
            .last()
            .map_or_else(String::new, |sample| sample.target.node_id.clone());
        if !samples.is_empty() {
            // The next rotation can replace a lost report. Keep no retry backlog.
            let _ = client
                .post(endpoint.clone())
                .bearer_auth(&token)
                .json(&PeerProbeBatch {
                    schema_version: PEER_PROBE_SCHEMA_VERSION,
                    issued_at_unix_ms: plan.issued_at_unix_ms,
                    samples,
                })
                .send()
                .await;
        }
    }
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
        interval_seconds: REFRESH.as_secs(),
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
