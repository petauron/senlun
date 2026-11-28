//! Bounded outbound monitoring: 16 schedules, four active probes, one batch.
//! Configuration and result transport use the Service client; targets use a
//! separate credential-free client with redirects, retries and proxies disabled.

use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    net::IpAddr,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use reqwest::{Client, StatusCode, Url};
use senlun_protocol::{
    AGENT_CONFIG_SCHEMA_VERSION, AgentRuntimeConfig, MAX_PROBE_CONCURRENCY, MAX_PROBE_TASKS,
    PROBE_SCHEMA_VERSION, ProbeBatch, ProbeKind, ProbeResult, ProbeTask,
};
use tokio::{
    net::{TcpStream, lookup_host},
    sync::watch,
    task::{JoinHandle, JoinSet},
};
use uuid::Uuid;

use crate::system_command::{self, SystemCommand};

const CONFIG_REFRESH: Duration = Duration::from_secs(30);
const MAX_HTTP_RESPONSE_BYTES: usize = 64 * 1024;
const TCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const TCP_HIGH_LATENCY_MS: f64 = 1000.0;
const TCP_RETRANSMISSION_DROP_MS: f64 = 800.0;
const TCP_HIGH_LATENCY_RETRIES: usize = 3;
const MAX_PENDING_RESULTS_PER_TASK: usize = 4;

pub struct ProbeRunner {
    pub interval: watch::Receiver<Duration>,
    worker: JoinHandle<()>,
    peer_worker: JoinHandle<()>,
}

impl ProbeRunner {
    pub fn start(
        service_client: Client,
        service_url: Url,
        token: String,
        initial_interval: Duration,
    ) -> Result<Self, reqwest::Error> {
        let target_client = target_client()?;
        let (sender, interval) = watch::channel(initial_interval);
        let peer_worker = tokio::spawn(crate::peer_latency::run(
            service_client.clone(),
            target_client.clone(),
            service_url.clone(),
            token.clone(),
        ));
        let worker = tokio::spawn(run_worker(
            service_client,
            target_client,
            service_url,
            token,
            sender,
        ));
        Ok(Self {
            interval,
            worker,
            peer_worker,
        })
    }
}

impl Drop for ProbeRunner {
    fn drop(&mut self) {
        self.worker.abort();
        self.peer_worker.abort();
    }
}

struct ScheduledProbe {
    task: ProbeTask,
    next_run: Instant,
}

struct QueuedResult {
    task: ProbeTask,
    result: ProbeResult,
    retried: bool,
}

struct RejectedBatch {
    samples: Vec<QueuedResult>,
    /// Minimum configuration fetch epoch started after the batch was rejected.
    required_config_epoch: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryOutcome {
    Accepted,
    Unauthorized,
    /// Service explicitly rejected the batch (HTTP 400) and rolled it back atomically.
    Rejected,
    /// Transport, 429, or 5xx error where server persistence or throttling is uncertain.
    Uncertain(&'static str),
}

async fn stop_active_probes(
    active: &mut JoinSet<(ProbeTask, ProbeResult)>,
    in_flight: &mut HashSet<String>,
) {
    active.abort_all();
    while active.join_next().await.is_some() {}
    in_flight.clear();
}

enum TransportResult {
    Configuration(Result<AgentRuntimeConfig, &'static str>, u64),
    Delivery(DeliveryOutcome, Vec<QueuedResult>),
}

async fn apply_configuration(
    config: AgentRuntimeConfig,
    interval: &watch::Sender<Duration>,
    schedules: &mut Vec<ScheduledProbe>,
    active: &mut JoinSet<(ProbeTask, ProbeResult)>,
    in_flight: &mut HashSet<String>,
    pending: &mut BTreeMap<String, VecDeque<QueuedResult>>,
) {
    interval.send_replace(Duration::from_secs(config.interval_seconds));
    let enabled_tasks: Vec<_> = config
        .probes
        .into_iter()
        .filter(|task| task.enabled)
        .collect();
    if schedules.len() != enabled_tasks.len()
        || schedules
            .iter()
            .any(|scheduled| !enabled_tasks.contains(&scheduled.task))
    {
        stop_active_probes(active, in_flight).await;
        pending.retain(|_, queue| {
            queue.retain(|queued| enabled_tasks.contains(&queued.task));
            !queue.is_empty()
        });
    }
    let old_schedules = std::mem::take(schedules);
    for task in enabled_tasks {
        let next_run = old_schedules
            .iter()
            .find(|old| old.task == task)
            .map_or_else(Instant::now, |old| old.next_run);
        schedules.push(ScheduledProbe { task, next_run });
    }
}

fn launch_due_probes(
    schedules: &mut [ScheduledProbe],
    in_flight: &mut HashSet<String>,
    active: &mut JoinSet<(ProbeTask, ProbeResult)>,
    client: &Client,
) {
    // Oldest due task gets the next slot, so slow targets cannot starve
    // later schedules when all 16 tasks compete for four slots.
    schedules.sort_by_key(|scheduled| scheduled.next_run);
    for scheduled in schedules {
        if active.len() >= MAX_PROBE_CONCURRENCY {
            break;
        }
        if scheduled.next_run <= Instant::now() && !in_flight.contains(&scheduled.task.id) {
            scheduled.next_run =
                Instant::now() + Duration::from_secs(scheduled.task.interval_seconds);
            in_flight.insert(scheduled.task.id.clone());
            let client = client.clone();
            let task = scheduled.task.clone();
            active.spawn(async move {
                let result = run_probe(client, task.clone()).await;
                (task, result)
            });
        }
    }
}

fn queue_result(pending: &mut BTreeMap<String, VecDeque<QueuedResult>>, queued: QueuedResult) {
    let queue = pending.entry(queued.result.task_id.clone()).or_default();
    if queue.len() == MAX_PENDING_RESULTS_PER_TASK {
        queue.pop_front();
        tracing::warn!("probe result delivery lagged; older pending sample replaced");
    }
    queue.push_back(queued);
}

fn recover_rejected_batch(
    rejected: RejectedBatch,
    schedules: &[ScheduledProbe],
    pending: &mut BTreeMap<String, VecDeque<QueuedResult>>,
) -> usize {
    let has_stale_sample = rejected.samples.iter().any(|queued| {
        !schedules
            .iter()
            .any(|scheduled| scheduled.task == queued.task)
    });
    if !has_stale_sample {
        // Fresh config matches every rejected task verbatim, so the 400 was not
        // caused by a removed/changed task; drop instead of replaying an identical batch.
        return 0;
    }
    let mut recovered = 0_usize;
    for mut queued in rejected.samples {
        if queued.retried
            || !schedules
                .iter()
                .any(|scheduled| scheduled.task == queued.task)
        {
            continue;
        }
        queued.retried = true;
        let queue = pending.entry(queued.result.task_id.clone()).or_default();
        if queue.len() < MAX_PENDING_RESULTS_PER_TASK {
            queue.push_front(queued);
            recovered += 1;
        }
    }
    recovered
}

fn take_pending_results(
    pending: &mut BTreeMap<String, VecDeque<QueuedResult>>,
    schedules: &[ScheduledProbe],
    last_delivery: &BTreeMap<String, VecDeque<Instant>>,
    now: Instant,
) -> (Vec<QueuedResult>, Option<Instant>) {
    let mut results = Vec::with_capacity(MAX_PROBE_TASKS);
    let mut next_delivery: Option<Instant> = None;
    for scheduled in schedules {
        let Some(queue) = pending.get_mut(&scheduled.task.id) else {
            continue;
        };
        // The Service accepts at most two results per task in its configured
        // interval. Space catch-up reports even when the network recovers.
        if let Some(first) = last_delivery
            .get(&scheduled.task.id)
            .filter(|times| times.len() == 2)
            .and_then(VecDeque::front)
        {
            let ready = *first
                + Duration::from_secs(scheduled.task.interval_seconds)
                + Duration::from_millis(1);
            if ready > now {
                next_delivery = Some(next_delivery.map_or(ready, |next| next.min(ready)));
                continue;
            }
        }
        if let Some(queued) = queue.pop_front() {
            results.push(queued);
        }
    }
    pending.retain(|_, queue| !queue.is_empty());
    (results, next_delivery)
}

fn start_delivery(
    pending: &mut BTreeMap<String, VecDeque<QueuedResult>>,
    schedules: &[ScheduledProbe],
    last_delivery: &BTreeMap<String, VecDeque<Instant>>,
    transport: &mut JoinSet<TransportResult>,
    service_client: &Client,
    service_url: &Url,
    token: &str,
) -> (bool, Option<Instant>) {
    let (submitted, next_delivery) =
        take_pending_results(pending, schedules, last_delivery, Instant::now());
    if submitted.is_empty() {
        return (false, next_delivery);
    }
    let batch = ProbeBatch {
        schema_version: PROBE_SCHEMA_VERSION,
        results: submitted
            .iter()
            .map(|queued| queued.result.clone())
            .collect(),
    };
    let (client, url, token) = (
        service_client.clone(),
        service_url.clone(),
        token.to_owned(),
    );
    transport.spawn(async move {
        let outcome = send_results(&client, &url, &token, &batch).await;
        TransportResult::Delivery(outcome, submitted)
    });
    (true, None)
}

fn record_deliveries(
    schedules: &[ScheduledProbe],
    last_delivery: &mut BTreeMap<String, VecDeque<Instant>>,
    submitted: &[QueuedResult],
) {
    let completed_at = Instant::now();
    for queued in submitted {
        if schedules
            .iter()
            .any(|scheduled| scheduled.task.id == queued.result.task_id)
        {
            let times = last_delivery
                .entry(queued.result.task_id.clone())
                .or_default();
            if times.len() == 2 {
                times.pop_front();
            }
            times.push_back(completed_at);
        }
    }
}

fn finish_rejected_after_config(
    epoch: u64,
    succeeded: bool,
    schedules: &[ScheduledProbe],
    pending: &mut BTreeMap<String, VecDeque<QueuedResult>>,
    rejected_batch: &mut Option<RejectedBatch>,
) {
    if rejected_batch
        .as_ref()
        .is_none_or(|rejected| epoch < rejected.required_config_epoch)
    {
        return;
    }
    let Some(rejected) = rejected_batch.take() else {
        return;
    };
    if succeeded {
        let recovered = recover_rejected_batch(rejected, schedules, pending);
        if recovered > 0 {
            tracing::info!(
                recovered,
                "re-queued valid sibling probe samples after configuration refresh"
            );
        }
    }
}

struct WorkerState {
    schedules: Vec<ScheduledProbe>,
    active: JoinSet<(ProbeTask, ProbeResult)>,
    in_flight: HashSet<String>,
    pending: BTreeMap<String, VecDeque<QueuedResult>>,
    last_delivery: BTreeMap<String, VecDeque<Instant>>,
    rejected_batch: Option<RejectedBatch>,
}

impl WorkerState {
    fn new() -> Self {
        Self {
            schedules: Vec::new(),
            active: JoinSet::new(),
            in_flight: HashSet::new(),
            pending: BTreeMap::new(),
            last_delivery: BTreeMap::new(),
            rejected_batch: None,
        }
    }

    async fn handle_configuration(
        &mut self,
        result: Result<AgentRuntimeConfig, &'static str>,
        epoch: u64,
        interval: &watch::Sender<Duration>,
    ) -> bool {
        match result {
            Ok(config) => {
                apply_configuration(
                    config,
                    interval,
                    &mut self.schedules,
                    &mut self.active,
                    &mut self.in_flight,
                    &mut self.pending,
                )
                .await;
                self.last_delivery.retain(|id, _| {
                    self.schedules
                        .iter()
                        .any(|scheduled| scheduled.task.id == *id)
                });
                finish_rejected_after_config(
                    epoch,
                    true,
                    &self.schedules,
                    &mut self.pending,
                    &mut self.rejected_batch,
                );
                true
            }
            Err("unauthorized") => {
                tracing::warn!("Agent configuration credential rejected; probe worker stopped");
                false
            }
            Err(reason) => {
                finish_rejected_after_config(
                    epoch,
                    false,
                    &self.schedules,
                    &mut self.pending,
                    &mut self.rejected_batch,
                );
                tracing::warn!(reason, "Agent configuration refresh failed");
                true
            }
        }
    }
}

fn handle_delivery_outcome(
    outcome: DeliveryOutcome,
    submitted: Vec<QueuedResult>,
    schedules: &[ScheduledProbe],
    last_delivery: &mut BTreeMap<String, VecDeque<Instant>>,
    config_epoch: u64,
    next_config: &mut Instant,
    rejected_batch: &mut Option<RejectedBatch>,
) -> bool {
    match outcome {
        DeliveryOutcome::Accepted => {
            record_deliveries(schedules, last_delivery, &submitted);
        }
        DeliveryOutcome::Unauthorized => {
            tracing::warn!("Agent result credential rejected; probe worker stopped");
            return false;
        }
        DeliveryOutcome::Rejected => {
            let recoverable: Vec<_> = submitted
                .into_iter()
                .filter(|queued| !queued.retried)
                .collect();
            if recoverable.is_empty() {
                tracing::warn!("recovered probe batch was rejected again; batch discarded");
            } else {
                *rejected_batch = Some(RejectedBatch {
                    samples: recoverable,
                    required_config_epoch: config_epoch.saturating_add(1),
                });
                *next_config = Instant::now();
                tracing::warn!("probe batch rejected; refreshing configuration");
            }
        }
        DeliveryOutcome::Uncertain(reason) => {
            // Keep conservative per-task pacing on transport/429/5xx so uncertain
            // deliveries never burst queued catch-up samples into Service rate limits.
            record_deliveries(schedules, last_delivery, &submitted);
            tracing::warn!(reason, "probe result delivery failed; batch discarded");
        }
    }
    true
}

fn start_configuration(
    next_config: &mut Instant,
    config_pending: &mut bool,
    config_epoch: &mut u64,
    transport: &mut JoinSet<TransportResult>,
    service_client: &Client,
    service_url: &Url,
    token: &str,
) {
    let now = Instant::now();
    if now < *next_config || *config_pending {
        return;
    }
    *next_config = now + CONFIG_REFRESH;
    *config_pending = true;
    *config_epoch = config_epoch.saturating_add(1);
    let epoch = *config_epoch;
    let (client, url, token) = (
        service_client.clone(),
        service_url.clone(),
        token.to_owned(),
    );
    transport.spawn(async move {
        TransportResult::Configuration(fetch_config(&client, &url, &token).await, epoch)
    });
}

fn config_wake(pending: bool, next: Instant) -> Instant {
    if pending {
        Instant::now() + CONFIG_REFRESH
    } else {
        next
    }
}

async fn collect_completed_probes(
    completion: &mut Option<Result<(ProbeTask, ProbeResult), tokio::task::JoinError>>,
    active: &mut JoinSet<(ProbeTask, ProbeResult)>,
    in_flight: &mut HashSet<String>,
    schedules: &[ScheduledProbe],
    pending: &mut BTreeMap<String, VecDeque<QueuedResult>>,
) {
    while let Some(completed) = completion.take().or_else(|| active.try_join_next()) {
        if let Ok((task, result)) = completed {
            in_flight.remove(&result.task_id);
            if schedules.iter().any(|scheduled| scheduled.task == task) {
                queue_result(
                    pending,
                    QueuedResult {
                        task,
                        result,
                        retried: false,
                    },
                );
            }
        } else {
            // Clear reserved slots if a probe task unexpectedly fails.
            stop_active_probes(active, in_flight).await;
            tracing::warn!("probe execution task failed");
        }
    }
}

async fn run_worker(
    service_client: Client,
    target_client: Client,
    service_url: Url,
    token: String,
    interval: watch::Sender<Duration>,
) {
    let mut state = WorkerState::new();
    let mut next_config = Instant::now();
    let mut transport = JoinSet::new();
    let mut completion = None;
    let mut config_pending = false;
    let mut config_epoch = 0_u64;
    let mut delivery_pending = false;
    loop {
        start_configuration(
            &mut next_config,
            &mut config_pending,
            &mut config_epoch,
            &mut transport,
            &service_client,
            &service_url,
            &token,
        );

        collect_completed_probes(
            &mut completion,
            &mut state.active,
            &mut state.in_flight,
            &state.schedules,
            &mut state.pending,
        )
        .await;
        let mut delivery_wake = None;
        if !delivery_pending && state.rejected_batch.is_none() && !state.pending.is_empty() {
            (delivery_pending, delivery_wake) = start_delivery(
                &mut state.pending,
                &state.schedules,
                &state.last_delivery,
                &mut transport,
                &service_client,
                &service_url,
                &token,
            );
        }
        launch_due_probes(
            &mut state.schedules,
            &mut state.in_flight,
            &mut state.active,
            &target_client,
        );
        // An in-flight task already owns its next run. When every slot is
        // occupied, wait for a completion instead of spinning on overdue work.
        let mut next_wake = next_wake(
            &state.schedules,
            &state.in_flight,
            state.active.len(),
            config_wake(config_pending, next_config),
        );
        if !delivery_pending
            && state.rejected_batch.is_none()
            && let Some(delivery_wake) = delivery_wake
        {
            next_wake = next_wake.min(delivery_wake);
        }
        tokio::select! {
            completed = state.active.join_next(), if !state.active.is_empty() => {
                completion = completed;
            }
            completed = transport.join_next(), if !transport.is_empty() => {
                match completed {
                    Some(Ok(TransportResult::Configuration(result, epoch))) => {
                        config_pending = false;
                        if !state.handle_configuration(result, epoch, &interval).await {
                            return;
                        }
                    }
                    Some(Ok(TransportResult::Delivery(outcome, submitted))) => {
                        delivery_pending = false;
                        if !handle_delivery_outcome(
                            outcome,
                            submitted,
                            &state.schedules,
                            &mut state.last_delivery,
                            config_epoch,
                            &mut next_config,
                            &mut state.rejected_batch,
                        ) {
                            return;
                        }
                    }
                    Some(Err(_)) => {
                        transport.abort_all();
                        while transport.join_next().await.is_some() {}
                        config_pending = false;
                        delivery_pending = false;
                        state.rejected_batch = None;
                        tracing::warn!("probe transport task failed");
                    }
                    None => {}
                }
            }
            () = tokio::time::sleep_until(next_wake.into()) => {}
        }
    }
}

fn next_wake(
    schedules: &[ScheduledProbe],
    in_flight: &HashSet<String>,
    active_count: usize,
    next_config: Instant,
) -> Instant {
    if active_count >= MAX_PROBE_CONCURRENCY {
        return next_config;
    }
    schedules
        .iter()
        .filter(|scheduled| !in_flight.contains(&scheduled.task.id))
        .map(|scheduled| scheduled.next_run)
        .min()
        .map_or(next_config, |next_probe| next_probe.min(next_config))
}

async fn fetch_config(
    client: &Client,
    service_url: &Url,
    token: &str,
) -> Result<AgentRuntimeConfig, &'static str> {
    let endpoint = service_url
        .join("api/v1/agents/config")
        .map_err(|_| "invalid_service_url")?;
    let response = client
        .get(endpoint)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| "transport")?;
    if response.status() == StatusCode::UNAUTHORIZED {
        return Err("unauthorized");
    }
    if !response.status().is_success() {
        return Err("http_status");
    }
    let config: AgentRuntimeConfig = crate::read_json_with_limit(response, 64 * 1024)
        .await
        .map_err(|_| "invalid_config")?;
    validate_config(&config)?;
    Ok(config)
}

async fn send_results(
    client: &Client,
    service_url: &Url,
    token: &str,
    batch: &ProbeBatch,
) -> DeliveryOutcome {
    let Ok(endpoint) = service_url.join("api/v1/agents/probes") else {
        return DeliveryOutcome::Uncertain("invalid_service_url");
    };
    let Ok(response) = client
        .post(endpoint)
        .bearer_auth(token)
        .json(batch)
        .send()
        .await
    else {
        return DeliveryOutcome::Uncertain("transport");
    };
    match response.status() {
        status if status.is_success() => {
            // The acknowledgment has no fields needed by the Agent. Dropping it avoids
            // ever accumulating or reflecting Service response text.
            DeliveryOutcome::Accepted
        }
        StatusCode::UNAUTHORIZED => DeliveryOutcome::Unauthorized,
        StatusCode::BAD_REQUEST => DeliveryOutcome::Rejected,
        StatusCode::TOO_MANY_REQUESTS => DeliveryOutcome::Uncertain("rate_limited"),
        _ => DeliveryOutcome::Uncertain("http_status"),
    }
}

fn validate_config(config: &AgentRuntimeConfig) -> Result<(), &'static str> {
    if config.schema_version != AGENT_CONFIG_SCHEMA_VERSION
        || !(1..=300).contains(&config.interval_seconds)
        || config.probes.len() > MAX_PROBE_TASKS
    {
        return Err("invalid_config");
    }
    let mut ids = HashSet::new();
    for task in &config.probes {
        if Uuid::parse_str(&task.id).is_err()
            || !ids.insert(&task.id)
            || task.name.is_empty()
            || task.name.len() > 128
            || task.name.chars().any(char::is_control)
            || task.target.len() > 2048
            || !(5..=3600).contains(&task.interval_seconds)
            || !(1..=30).contains(&task.timeout_seconds)
            || task.timeout_seconds > task.interval_seconds
        {
            return Err("invalid_config");
        }
        match task.kind {
            ProbeKind::Icmp => {
                validate_host(&task.target)?;
            }
            ProbeKind::Tcp => {
                parse_tcp_target(&task.target)?;
            }
            ProbeKind::Http => {
                parse_http_target(&task.target)?;
            }
        }
    }
    Ok(())
}

fn validate_host(host: &str) -> Result<(), &'static str> {
    if host.parse::<IpAddr>().is_ok() {
        return Ok(());
    }
    if host.is_empty() || host.len() > 253 {
        return Err("invalid_target");
    }
    for label in host.trim_end_matches('.').split('.') {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("invalid_target");
        }
    }
    Ok(())
}

fn parse_tcp_target(target: &str) -> Result<(String, u16), &'static str> {
    let url = Url::parse(&format!("tcp://{target}")).map_err(|_| "invalid_target")?;
    if !url.username().is_empty()
        || url.password().is_some()
        || !url.path().is_empty()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("invalid_target");
    }
    let host = url
        .host_str()
        .ok_or("invalid_target")?
        .trim_matches(['[', ']']);
    validate_host(host)?;
    let port = url
        .port()
        .filter(|port| *port != 0)
        .ok_or("invalid_target")?;
    Ok((host.to_owned(), port))
}

fn parse_http_target(target: &str) -> Result<Url, &'static str> {
    let url = Url::parse(target).map_err(|_| "invalid_target")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || target.chars().any(char::is_control)
    {
        return Err("invalid_target");
    }
    Ok(url)
}

fn target_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .pool_max_idle_per_host(0)
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("senlun-probe/", env!("CARGO_PKG_VERSION")))
        .build()
}

pub(crate) async fn run_probe(client: Client, task: ProbeTask) -> ProbeResult {
    let started = Instant::now();
    let deadline = Duration::from_secs(task.timeout_seconds);
    let outcome = if task.kind == ProbeKind::Tcp {
        // Komari measures only the TCP handshake, then retries suspiciously
        // slow handshakes. Each attempt has its own timeout.
        run_tcp_probe(&task.target, deadline.min(TCP_CONNECT_TIMEOUT))
            .await
            .map(Some)
    } else {
        tokio::time::timeout(deadline, async {
            match task.kind {
                ProbeKind::Icmp => {
                    let address = lookup_host((task.target.as_str(), 0))
                        .await
                        .map_err(|_| "dns")?
                        .next()
                        .ok_or("dns")?
                        .ip();
                    let output =
                        system_command::run(SystemCommand::Ping(address), deadline).await?;
                    parse_ping_latency(&output)
                        .map(Some)
                        .ok_or("response_parse")
                }
                ProbeKind::Tcp => unreachable!("TCP probes use run_tcp_probe"),
                ProbeKind::Http => {
                    let mut response = client
                        .get(parse_http_target(&task.target)?)
                        .timeout(deadline)
                        .send()
                        .await
                        .map_err(|error| {
                            if error.is_timeout() {
                                "timeout"
                            } else {
                                "connect"
                            }
                        })?;
                    if !response.status().is_success() {
                        return Err("http_status");
                    }
                    if response
                        .content_length()
                        .is_some_and(|length| length > MAX_HTTP_RESPONSE_BYTES as u64)
                    {
                        return Err("response_too_large");
                    }
                    let mut received = 0_usize;
                    while let Some(chunk) = response.chunk().await.map_err(|_| "response_read")? {
                        received = received.saturating_add(chunk.len());
                        if received > MAX_HTTP_RESPONSE_BYTES {
                            return Err("response_too_large");
                        }
                    }
                    Ok(Some(started.elapsed().as_secs_f64() * 1000.0))
                }
            }
        })
        .await
        .unwrap_or(Err("timeout"))
    };
    let (success, latency_ms, error) = match outcome {
        Ok(latency) => (true, latency, None),
        Err(reason) => (false, None, Some(reason.to_owned())),
    };
    ProbeResult {
        task_id: task.id,
        sample_id: Uuid::new_v4().to_string(),
        collected_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| {
                duration.as_millis().try_into().unwrap_or(u64::MAX)
            }),
        latency_ms,
        success,
        error,
    }
}

async fn measure_tcp(target: &str, timeout: Duration) -> Result<f64, &'static str> {
    let (host, port) = parse_tcp_target(target)?;
    // Match Komari's first-address choice and exclude name resolution from
    // the measured latency. Bound DNS separately so a stalled resolver cannot
    // hold a probe slot indefinitely.
    let address = tokio::time::timeout(timeout, lookup_host((host.as_str(), port)))
        .await
        .map_err(|_| "timeout")?
        .map_err(|_| "dns")?
        .next()
        .ok_or("dns")?;
    let started = Instant::now();
    tokio::time::timeout(timeout, TcpStream::connect(address))
        .await
        .map_err(|_| "timeout")?
        .map_err(|_| "connect")?;
    Ok((started.elapsed().as_secs_f64() * 1000.0).floor())
}

fn tcp_retry_candidate(first: f64, retry: f64) -> Result<Option<f64>, &'static str> {
    if retry > TCP_HIGH_LATENCY_MS {
        return Ok(None);
    }
    if first - retry > TCP_RETRANSMISSION_DROP_MS {
        return Err("probe_failed");
    }
    Ok(Some(retry))
}

async fn run_tcp_probe(target: &str, timeout: Duration) -> Result<f64, &'static str> {
    let first = measure_tcp(target, timeout).await?;
    if first <= TCP_HIGH_LATENCY_MS {
        return Ok(first);
    }
    for _ in 0..TCP_HIGH_LATENCY_RETRIES {
        if let Some(latency) = tcp_retry_candidate(first, measure_tcp(target, timeout).await?)? {
            return Ok(latency);
        }
    }
    Err("probe_failed")
}

fn parse_ping_latency(output: &[u8]) -> Option<f64> {
    let text = std::str::from_utf8(output).ok()?;
    for line in text.lines() {
        let value = line
            .split_once("time=")
            .map(|(_, suffix)| (suffix, false))
            .or_else(|| line.split_once("time<").map(|(_, suffix)| (suffix, true)));
        if let Some((suffix, upper_bound)) = value {
            let value = suffix.split_whitespace().next()?.parse::<f64>().ok()?;
            if value.is_finite() && value >= 0.0 {
                // Komari records the integer millisecond RTT for each ICMP sample.
                // "time<1" is a strict upper bound, so its integer RTT is zero.
                return if upper_bound {
                    (value > 0.0 && value <= 1.0).then_some(0.0)
                } else {
                    Some(value.floor())
                };
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_results_keep_failures_and_respect_batch_bounds() {
        let now = Instant::now();
        let schedules: Vec<_> = (0..MAX_PROBE_TASKS)
            .map(|id| {
                let mut probe = task(ProbeKind::Tcp, "127.0.0.1:80");
                probe.id = format!("task-{id:02}");
                probe.interval_seconds = 5;
                ScheduledProbe {
                    task: probe,
                    next_run: now,
                }
            })
            .collect();
        let queued = |task_idx: usize, sequence: usize, success: bool| QueuedResult {
            task: schedules[task_idx].task.clone(),
            result: ProbeResult {
                task_id: format!("task-{task_idx:02}"),
                sample_id: format!("{task_idx}-{sequence}"),
                collected_at_unix_ms: sequence as u64,
                latency_ms: success.then_some(1.0),
                success,
                error: (!success).then(|| "timeout".to_owned()),
            },
            retried: false,
        };
        let mut pending = BTreeMap::new();
        for task_idx in 0..MAX_PROBE_TASKS {
            queue_result(&mut pending, queued(task_idx, 0, false));
            queue_result(&mut pending, queued(task_idx, 1, true));
        }
        let mut last_delivery = BTreeMap::new();
        let (first, _) = take_pending_results(&mut pending, &schedules, &last_delivery, now);
        assert_eq!(first.len(), MAX_PROBE_TASKS);
        assert!(first.iter().all(|sample| !sample.result.success));
        for sample in &first {
            last_delivery.insert(sample.result.task_id.clone(), VecDeque::from([now]));
        }
        let (second, _) = take_pending_results(&mut pending, &schedules, &last_delivery, now);
        assert_eq!(second.len(), MAX_PROBE_TASKS);
        assert!(second.iter().all(|sample| sample.result.success));
        assert!(pending.is_empty());

        for sample in &second {
            last_delivery
                .get_mut(&sample.result.task_id)
                .unwrap()
                .push_back(now);
        }
        queue_result(&mut pending, queued(0, 0, false));
        queue_result(&mut pending, queued(0, 1, true));
        queue_result(&mut pending, queued(0, 2, true));
        queue_result(&mut pending, queued(0, 3, true));
        queue_result(&mut pending, queued(0, 4, true));
        let (blocked, wake) = take_pending_results(&mut pending, &schedules, &last_delivery, now);
        assert_eq!(blocked.len(), 0);
        let ready = now + Duration::from_millis(5001);
        assert_eq!(wake, Some(ready));
        let (bounded, _) = take_pending_results(&mut pending, &schedules, &last_delivery, ready);
        assert_eq!(bounded.len(), 1);
        assert_eq!(bounded[0].result.sample_id, "0-1");
        assert_eq!(pending["task-00"].front().unwrap().result.sample_id, "0-2");
    }

    fn scheduled(id: &str, target: &str, now: Instant) -> ScheduledProbe {
        let mut probe = task(ProbeKind::Tcp, target);
        probe.id = id.to_owned();
        probe.interval_seconds = 5;
        ScheduledProbe {
            task: probe,
            next_run: now,
        }
    }

    fn queued_sample(probe: &ProbeTask, sample_id: &str) -> QueuedResult {
        QueuedResult {
            task: probe.clone(),
            result: ProbeResult {
                task_id: probe.id.clone(),
                sample_id: sample_id.to_owned(),
                collected_at_unix_ms: 1,
                latency_ms: Some(12.0),
                success: true,
                error: None,
            },
            retried: false,
        }
    }

    #[test]
    fn rejected_batch_recovery_filters_by_full_task_and_respects_bounds() {
        let now = Instant::now();
        let disabled = scheduled("task-disabled", "127.0.0.1:80", now);
        let modified_old = scheduled("task-modified", "127.0.0.1:81", now);
        let modified_new = scheduled("task-modified", "127.0.0.1:82", now);
        let kept = scheduled("task-kept", "127.0.0.1:83", now);
        let saturated = scheduled("task-full", "127.0.0.1:84", now);

        let build_submitted = || {
            vec![
                queued_sample(&disabled.task, "s-disabled"),
                queued_sample(&modified_old.task, "s-modified"),
                queued_sample(&kept.task, "s-kept"),
                queued_sample(&saturated.task, "s-full-old"),
            ]
        };
        let old_schedules = vec![
            scheduled("task-disabled", "127.0.0.1:80", now),
            scheduled("task-modified", "127.0.0.1:81", now),
            scheduled("task-kept", "127.0.0.1:83", now),
            scheduled("task-full", "127.0.0.1:84", now),
        ];
        let new_schedules = vec![
            modified_new,
            scheduled("task-kept", "127.0.0.1:83", now),
            scheduled("task-full", "127.0.0.1:84", now),
        ];

        // Uncertain failures preserve conservative throttle pacing and do not stage replay.
        let mut last_delivery = BTreeMap::new();
        let mut next_config = now + CONFIG_REFRESH;
        let mut rejected_batch = None;
        assert!(handle_delivery_outcome(
            DeliveryOutcome::Uncertain("rate_limited"),
            build_submitted(),
            &old_schedules,
            &mut last_delivery,
            1,
            &mut next_config,
            &mut rejected_batch,
        ));
        assert_eq!(last_delivery["task-kept"].len(), 1);
        assert!(rejected_batch.is_none());

        // Explicit HTTP 400 stages post-rejection config refresh without consuming throttle.
        last_delivery.clear();
        assert!(handle_delivery_outcome(
            DeliveryOutcome::Rejected,
            build_submitted(),
            &old_schedules,
            &mut last_delivery,
            1,
            &mut next_config,
            &mut rejected_batch,
        ));
        assert!(last_delivery.is_empty());
        assert!(next_config <= Instant::now());
        let rejected = rejected_batch.take().expect("rejected batch staged");
        assert_eq!(rejected.required_config_epoch, 2);

        let mut pending = BTreeMap::new();
        for seq in 0..MAX_PENDING_RESULTS_PER_TASK {
            let sample = queued_sample(&saturated.task, &format!("fresh-{seq}"));
            queue_result(&mut pending, sample);
        }
        queue_result(&mut pending, queued_sample(&kept.task, "s-kept-newer"));

        let recovered = recover_rejected_batch(rejected, &new_schedules, &mut pending);
        assert_eq!(recovered, 1);
        assert!(!pending.contains_key("task-disabled"));
        assert!(!pending.contains_key("task-modified"));
        assert_eq!(pending["task-full"].len(), MAX_PENDING_RESULTS_PER_TASK);
        assert_eq!(
            pending["task-full"].front().unwrap().result.sample_id,
            "fresh-0"
        );
        assert_eq!(pending["task-kept"].len(), 2);
        assert_eq!(pending["task-kept"][0].result.sample_id, "s-kept");
        assert!(pending["task-kept"][0].retried);
        assert_eq!(pending["task-kept"][1].result.sample_id, "s-kept-newer");

        // Already-retried samples are dropped if rejected again, and Accepted updates throttle.
        let retried_sample = pending.get_mut("task-kept").unwrap().pop_front().unwrap();
        assert!(handle_delivery_outcome(
            DeliveryOutcome::Rejected,
            vec![retried_sample],
            &new_schedules,
            &mut last_delivery,
            2,
            &mut next_config,
            &mut rejected_batch,
        ));
        assert!(rejected_batch.is_none());
        assert!(last_delivery.is_empty());
        assert!(handle_delivery_outcome(
            DeliveryOutcome::Accepted,
            vec![queued_sample(&kept.task, "s-accepted")],
            &new_schedules,
            &mut last_delivery,
            2,
            &mut next_config,
            &mut rejected_batch,
        ));
        assert_eq!(last_delivery["task-kept"].len(), 1);
    }

    #[test]
    fn unchanged_config_after_rejection_drops_batch_without_replay() {
        let now = Instant::now();
        let a = scheduled("task-a", "127.0.0.1:80", now);
        let b = scheduled("task-b", "127.0.0.1:81", now);
        let rejected = RejectedBatch {
            samples: vec![queued_sample(&a.task, "s-a"), queued_sample(&b.task, "s-b")],
            required_config_epoch: 2,
        };
        let schedules = vec![a, b];
        let mut pending = BTreeMap::new();
        assert_eq!(
            recover_rejected_batch(rejected, &schedules, &mut pending),
            0
        );
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn send_results_distinguishes_accepted_rejected_unauthorized_and_uncertain() {
        let batch = ProbeBatch {
            schema_version: PROBE_SCHEMA_VERSION,
            results: Vec::new(),
        };
        let cases = [
            (
                "HTTP/1.1 200 OK\r\nContent-Length: 17\r\nConnection: close\r\n\r\n{\"accepted\":true}",
                DeliveryOutcome::Accepted,
            ),
            (
                "HTTP/1.1 400 Bad Request\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                DeliveryOutcome::Rejected,
            ),
            (
                "HTTP/1.1 401 Unauthorized\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                DeliveryOutcome::Unauthorized,
            ),
            (
                "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                DeliveryOutcome::Uncertain("rate_limited"),
            ),
            (
                "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                DeliveryOutcome::Uncertain("http_status"),
            ),
        ];
        for (response, expected) in cases {
            let (url, worker) = http_fixture(response.to_owned()).await;
            let outcome = send_results(
                &target_client().unwrap(),
                &Url::parse(&url).unwrap(),
                "test-token",
                &batch,
            )
            .await;
            assert_eq!(outcome, expected);
            worker.await.unwrap();
        }
    }

    async fn read_http_request(stream: &mut tokio::net::TcpStream) -> Option<(Vec<u8>, Vec<u8>)> {
        use tokio::io::AsyncReadExt;

        let mut headers = Vec::new();
        let mut byte = [0_u8; 1];
        while !headers.ends_with(b"\r\n\r\n") {
            if stream.read_exact(&mut byte).await.is_err() || headers.len() >= 16 * 1024 {
                return None;
            }
            headers.push(byte[0]);
        }
        let content_len = String::from_utf8_lossy(&headers)
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        let mut body = vec![0_u8; content_len];
        if content_len > 0 && stream.read_exact(&mut body).await.is_err() {
            return None;
        }
        Some((headers, body))
    }

    #[tokio::test]
    async fn admin_disabled_task_recovers_valid_sibling_sample_after_config_refresh() {
        use std::sync::{Arc, Mutex};
        use tokio::io::AsyncWriteExt;
        use tokio::sync::Notify;

        #[derive(Default)]
        struct MockState {
            disabled_a: bool,
            rejected_sibling_sample: Option<String>,
            recovered_sibling_sample: Option<String>,
        }

        let target_a = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_b = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut probe_a = task(ProbeKind::Tcp, &target_a.local_addr().unwrap().to_string());
        probe_a.interval_seconds = 5;
        let mut probe_b = task(ProbeKind::Tcp, &target_b.local_addr().unwrap().to_string());
        probe_b.interval_seconds = 5;
        let id_a = probe_a.id.clone();
        let id_b = probe_b.id.clone();

        let initial_config = serde_json::to_string(&AgentRuntimeConfig {
            schema_version: AGENT_CONFIG_SCHEMA_VERSION,
            interval_seconds: 3,
            probes: vec![probe_a, probe_b.clone()],
        })
        .unwrap();
        let updated_config = serde_json::to_string(&AgentRuntimeConfig {
            schema_version: AGENT_CONFIG_SCHEMA_VERSION,
            interval_seconds: 3,
            probes: vec![probe_b],
        })
        .unwrap();

        let state = Arc::new(Mutex::new(MockState::default()));
        let recovered = Arc::new(Notify::new());
        let service = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}/", service.local_addr().unwrap())).unwrap();
        let server = tokio::spawn({
            let state = Arc::clone(&state);
            let recovered = Arc::clone(&recovered);
            async move {
                loop {
                    let Ok((mut stream, _)) = service.accept().await else {
                        return;
                    };
                    let state = Arc::clone(&state);
                    let recovered = Arc::clone(&recovered);
                    let initial_config = initial_config.clone();
                    let updated_config = updated_config.clone();
                    let id_a = id_a.clone();
                    let id_b = id_b.clone();
                    tokio::spawn(async move {
                        let Some((headers, body)) = read_http_request(&mut stream).await else {
                            return;
                        };
                        let (status, response_body, hold) =
                            if headers.starts_with(b"GET /api/v1/agents/config ") {
                                let cfg = if state.lock().unwrap().disabled_a {
                                    updated_config
                                } else {
                                    initial_config
                                };
                                ("200 OK", cfg, false)
                            } else if headers.starts_with(b"POST /api/v1/agents/probes ") {
                                let batch: ProbeBatch = serde_json::from_slice(&body).unwrap();
                                let has_a = batch.results.iter().any(|item| item.task_id == id_a);
                                let sibling = batch
                                    .results
                                    .iter()
                                    .find(|item| item.task_id == id_b)
                                    .map(|item| item.sample_id.clone());
                                let mut guard = state.lock().unwrap();
                                if !guard.disabled_a && has_a && sibling.is_some() {
                                    guard.disabled_a = true;
                                    guard.rejected_sibling_sample = sibling;
                                    (
                                        "400 Bad Request",
                                        "{\"error\":\"probe not assigned\"}".to_owned(),
                                        false,
                                    )
                                } else if guard.disabled_a && has_a {
                                    (
                                        "400 Bad Request",
                                        "{\"error\":\"probe not assigned\"}".to_owned(),
                                        false,
                                    )
                                } else {
                                    if guard.disabled_a
                                        && let Some(sample_id) = sibling
                                        && guard.rejected_sibling_sample.as_deref()
                                            == Some(sample_id.as_str())
                                    {
                                        guard.recovered_sibling_sample = Some(sample_id);
                                        recovered.notify_one();
                                    }
                                    let hold = !guard.disabled_a && batch.results.len() < 2;
                                    ("200 OK", "{\"accepted\":true}".to_owned(), hold)
                                }
                            } else {
                                ("404 Not Found", "{}".to_owned(), false)
                            };
                        if hold {
                            // Hold a single-sample initial delivery until the next 5s cycle
                            // queues both tasks together into one mixed batch.
                            tokio::time::sleep(Duration::from_millis(5500)).await;
                        }
                        let response = format!(
                            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                            response_body.len()
                        );
                        let _ = stream.write_all(response.as_bytes()).await;
                    });
                }
            }
        });

        let runner = ProbeRunner::start(
            Client::new(),
            url,
            "test-token".into(),
            Duration::from_secs(3),
        )
        .unwrap();
        // Must recover well before the routine 30s CONFIG_REFRESH window.
        tokio::time::timeout(Duration::from_secs(15), recovered.notified())
            .await
            .expect("valid sibling sample should be re-delivered after 400 rejection");
        let guard = state.lock().unwrap();
        assert!(guard.disabled_a);
        assert!(guard.rejected_sibling_sample.is_some());
        assert_eq!(
            guard.recovered_sibling_sample,
            guard.rejected_sibling_sample
        );
        drop(guard);
        drop(runner);
        server.abort();
    }

    #[tokio::test]
    async fn slow_result_delivery_does_not_delay_the_next_probe() {
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::sync::{Notify, Semaphore};

        let target = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut probe = task(ProbeKind::Tcp, &target.local_addr().unwrap().to_string());
        probe.interval_seconds = 5;
        let config = serde_json::to_string(&AgentRuntimeConfig {
            schema_version: AGENT_CONFIG_SCHEMA_VERSION,
            interval_seconds: 3,
            probes: vec![probe],
        })
        .unwrap();
        let service = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}/", service.local_addr().unwrap())).unwrap();
        let started = Arc::new(Notify::new());
        let release = Arc::new(Semaphore::new(0));
        let server = tokio::spawn({
            let started = started.clone();
            let release = release.clone();
            async move {
                loop {
                    let Ok((mut stream, _)) = service.accept().await else {
                        return;
                    };
                    let (config, started, release) =
                        (config.clone(), started.clone(), release.clone());
                    tokio::spawn(async move {
                        let mut headers = Vec::new();
                        let mut byte = [0_u8; 1];
                        while !headers.ends_with(b"\r\n\r\n") {
                            if stream.read_exact(&mut byte).await.is_err()
                                || headers.len() >= 16 * 1024
                            {
                                return;
                            }
                            headers.push(byte[0]);
                        }
                        let is_config = headers.starts_with(b"GET /api/v1/agents/config ");
                        let body = if is_config {
                            config
                        } else {
                            started.notify_one();
                            if release.acquire().await.is_err() {
                                return;
                            }
                            "{\"accepted\":true}".to_owned()
                        };
                        let _ = stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
                    });
                }
            }
        });
        let runner = ProbeRunner::start(
            Client::new(),
            url,
            "test-token".into(),
            Duration::from_secs(3),
        )
        .unwrap();
        tokio::time::timeout(Duration::from_secs(3), target.accept())
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), started.notified())
            .await
            .unwrap();
        // The first result response remains blocked while the next probe is due.
        tokio::time::timeout(Duration::from_secs(7), target.accept())
            .await
            .unwrap()
            .unwrap();
        release.add_permits(1);
        drop(runner);
        server.abort();
    }

    fn task(kind: ProbeKind, target: &str) -> ProbeTask {
        ProbeTask {
            id: Uuid::new_v4().to_string(),
            name: "Example".to_owned(),
            kind,
            target: target.to_owned(),
            interval_seconds: 60,
            timeout_seconds: 5,
            enabled: true,
        }
    }

    #[test]
    fn rejects_command_like_targets_and_credentials() {
        for host in ["-c", "localhost;id", "$(id)", "a b", "a/b", "", "a..b"] {
            assert!(validate_host(host).is_err(), "{host}");
        }
        for target in [
            "localhost",
            "localhost:0",
            "user@localhost:80",
            "localhost:80/path",
            "localhost:80?x",
        ] {
            assert!(parse_tcp_target(target).is_err(), "{target}");
        }
        for target in [
            "file:///etc/passwd",
            "http://user:secret@localhost",
            "http://localhost/#token",
        ] {
            assert!(parse_http_target(target).is_err(), "{target}");
        }
        assert_eq!(
            parse_tcp_target("[::1]:443").unwrap(),
            ("::1".to_owned(), 443)
        );
        assert!(validate_host("example.com").is_ok());
    }

    #[test]
    fn enforces_schema_task_count_intervals_and_unique_ids() {
        let mut config = AgentRuntimeConfig {
            schema_version: AGENT_CONFIG_SCHEMA_VERSION,
            interval_seconds: 1,
            probes: vec![task(ProbeKind::Tcp, "localhost:443")],
        };
        assert!(validate_config(&config).is_ok());
        config.interval_seconds = 0;
        assert!(validate_config(&config).is_err());
        config.interval_seconds = 300;
        config.probes.push(config.probes[0].clone());
        assert!(validate_config(&config).is_err());
        config.probes = (0..17)
            .map(|_| task(ProbeKind::Icmp, "127.0.0.1"))
            .collect();
        assert!(validate_config(&config).is_err());
        config.probes.truncate(1);
        config.probes[0].timeout_seconds = 31;
        assert!(validate_config(&config).is_err());
        config.schema_version = 99;
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn parses_only_finite_ping_times() {
        assert_eq!(
            parse_ping_latency(b"64 bytes from 127.0.0.1: icmp_seq=1 ttl=64 time=0.123 ms\n"),
            Some(0.0)
        );
        assert_eq!(parse_ping_latency(b"time=143.987 ms"), Some(143.0));
        assert_eq!(parse_ping_latency(b"time<1 ms"), Some(0.0));
        assert_eq!(parse_ping_latency(b"time<2 ms"), None);
        assert_eq!(parse_ping_latency(b"time=NaN ms"), None);
        assert_eq!(parse_ping_latency(b"unrecognized output"), None);
    }

    #[tokio::test]
    async fn tcp_connect_failure_is_a_fixed_error_category() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let result = run_probe(
            target_client().unwrap(),
            task(ProbeKind::Tcp, &address.to_string()),
        )
        .await;
        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("connect"));
        assert!(result.latency_ms.is_none());
    }

    #[test]
    fn tcp_high_latency_retries_match_komari_thresholds() {
        assert_eq!(tcp_retry_candidate(1001.0, 1001.0), Ok(None));
        assert_eq!(tcp_retry_candidate(1001.0, 1000.0), Ok(Some(1000.0)));
        assert_eq!(tcp_retry_candidate(1800.0, 1000.0), Ok(Some(1000.0)));
        assert_eq!(tcp_retry_candidate(1801.0, 1000.0), Err("probe_failed"));
    }

    async fn http_fixture(response: String) -> (String, JoinHandle<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let worker = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                assert!(request.len() < 4096);
                stream.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
            }
            let _ = stream.write_all(response.as_bytes()).await;
            String::from_utf8(request).unwrap().to_ascii_lowercase()
        });
        (url, worker)
    }

    #[tokio::test]
    async fn http_requests_are_credential_free_and_redirects_fail() {
        let (url, worker) = http_fixture(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_owned(),
        )
        .await;
        let result = run_probe(target_client().unwrap(), task(ProbeKind::Http, &url)).await;
        assert!(result.success);
        assert!(result.latency_ms.is_some());
        let request = worker.await.unwrap();
        assert!(!request.contains("authorization:"));
        assert!(!request.contains("cookie:"));

        let redirect = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (url, worker) = http_fixture(format!("HTTP/1.1 302 Found\r\nLocation: http://{}/secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", redirect.local_addr().unwrap())).await;
        let result = run_probe(target_client().unwrap(), task(ProbeKind::Http, &url)).await;
        assert_eq!(result.error.as_deref(), Some("http_status"));
        worker.await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(20), redirect.accept())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn http_caps_declared_and_streamed_response_sizes() {
        let body = "x".repeat(MAX_HTTP_RESPONSE_BYTES + 1);
        for response in [
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            ),
            format!(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n",
                body.len()
            ),
        ] {
            let (url, worker) = http_fixture(response).await;
            let result = run_probe(target_client().unwrap(), task(ProbeKind::Http, &url)).await;
            assert_eq!(result.error.as_deref(), Some("response_too_large"));
            assert!(result.latency_ms.is_none());
            worker.await.unwrap();
        }
    }

    #[tokio::test]
    async fn configuration_uses_only_the_service_bearer_credential() {
        let config = AgentRuntimeConfig {
            schema_version: AGENT_CONFIG_SCHEMA_VERSION,
            interval_seconds: 3,
            probes: vec![task(ProbeKind::Icmp, "127.0.0.1")],
        };
        let body = serde_json::to_string(&config).unwrap();
        let (url, worker) = http_fixture(format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ))
        .await;
        let received = fetch_config(
            &target_client().unwrap(),
            &Url::parse(&url).unwrap(),
            "test-agent-token",
        )
        .await
        .unwrap();
        assert_eq!(received, config);
        let request = worker.await.unwrap();
        assert!(request.starts_with("get /api/v1/agents/config "));
        assert!(request.contains("authorization: bearer test-agent-token\r\n"));
    }
}
