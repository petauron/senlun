//! One fixed, opt-in, one-shot Shanghai TCP monitoring task at a time.
//! Existing Agent authentication stays in the HTTP client. The reviewed child
//! receives only a parsed address/family, an empty environment and private tmp.
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};

use reqwest::{Client, StatusCode, Url};
use senlun_protocol::{
    CarrierRoute, RETURN_ROUTE_LEASE_MS, RETURN_ROUTE_SCHEMA_VERSION, ReturnRouteClaim,
    ReturnRouteOutcome, ReturnRouteReport, ReturnRouteTask,
};
use tokio::{io::AsyncReadExt, process::Command, task::JoinHandle};

use crate::task_lease::{
    ClaimValidation, ResultDelivery, TaskLease, post_result, validate_claimed_task,
};

const COLLECTOR: &str = include_str!("shanghai-routes.sh");
const REVISION: &str = "db52fc60523320fc4a34b5c369f28be38df21d13";
const POLL: Duration = Duration::from_secs(15);
const AVAILABILITY_REFRESH: Duration = Duration::from_mins(1);
const EGRESS_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const EGRESS_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_EGRESS_BODY: usize = 128;
const MAX_RESPONSE: usize = 16 * 1024;
const COLLECT_TIMEOUT: Duration = Duration::from_secs(410);
const MAX_SCRIPT_TIMEOUT_SECONDS: u64 = 390;
// Reserve GNU `timeout -k 5s` kill grace plus 1s scheduling margin before outer deadline.
const SCRIPT_KILL_MARGIN_SECONDS: u64 = 6;

pub struct RouteRunner(JoinHandle<()>);
impl RouteRunner {
    #[must_use]
    pub fn start(client: Client, url: Url, token: String) -> Self {
        Self(tokio::spawn(run(client, url, token)))
    }
}
impl Drop for RouteRunner {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct AvailabilityCache {
    checked_at: Option<Instant>,
    available: bool,
}

impl AvailabilityCache {
    fn new() -> Self {
        Self {
            checked_at: None,
            available: false,
        }
    }

    async fn get(&mut self) -> bool {
        if self
            .checked_at
            .is_some_and(|checked_at| checked_at.elapsed() < AVAILABILITY_REFRESH)
        {
            return self.available;
        }
        let available = tokio::task::spawn_blocking(available)
            .await
            .unwrap_or(false);
        self.available = available;
        self.checked_at = Some(Instant::now());
        available
    }
}

fn available() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    let raw = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let capable = raw
        .lines()
        .find_map(|line| {
            line.strip_prefix("CapEff:")
                .and_then(|v| u64::from_str_radix(v.trim(), 16).ok())
        })
        .is_some_and(|bits| bits & (1 << 13) != 0);
    capable
        && [
            "bash",
            "curl",
            "timeout",
            "sha256sum",
            "tar",
            "awk",
            "sed",
            "grep",
            "sort",
            "find",
            "head",
            "date",
        ]
        .iter()
        .all(|name| {
            ["/usr/bin", "/bin"]
                .iter()
                .any(|base| Path::new(base).join(name).is_file())
        })
}

async fn run(client: Client, url: Url, token: String) {
    let Ok(claim_url) = url.join("api/v1/agents/return-routes/claim") else {
        return;
    };
    let Ok(result_url) = url.join("api/v1/agents/return-routes/result") else {
        return;
    };
    let mut pending: Option<(ReturnRouteOutcome, Instant)> = None;
    let mut availability = AvailabilityCache::new();
    loop {
        if let Some((outcome, deadline)) = &pending {
            match post_result(&client, &result_url, &token, outcome, *deadline).await {
                ResultDelivery::Unauthorized => return,
                ResultDelivery::Done => pending = None,
                ResultDelivery::Retry => {}
            }
        }
        if pending.is_none() {
            let claim = ReturnRouteClaim {
                schema_version: RETURN_ROUTE_SCHEMA_VERSION,
                agent_version: env!("CARGO_PKG_VERSION").to_owned(),
                available: availability.get().await,
            };
            let claim_started = Instant::now();
            if let Ok(mut response) = client
                .post(claim_url.clone())
                .bearer_auth(&token)
                .json(&claim)
                .send()
                .await
            {
                if response.status() == StatusCode::UNAUTHORIZED {
                    return;
                }
                if response.status().is_success() {
                    let mut bytes = Vec::new();
                    while let Ok(Some(chunk)) = response.chunk().await {
                        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE {
                            bytes.clear();
                            break;
                        }
                        bytes.extend_from_slice(&chunk);
                    }
                    if let Ok(Some(task)) =
                        serde_json::from_slice::<Option<ReturnRouteTask>>(&bytes)
                    {
                        let claim_received = Instant::now();
                        match validate_claimed_task(
                            &task.id,
                            task.expires_at_unix_ms,
                            RETURN_ROUTE_LEASE_MS,
                            claim_started,
                            claim_received,
                        ) {
                            ClaimValidation::Ignore => {}
                            ClaimValidation::InvalidTask { deadline } => {
                                pending = Some((
                                    ReturnRouteOutcome {
                                        id: task.id,
                                        report: None,
                                        error: Some("invalid_task".to_owned()),
                                    },
                                    deadline,
                                ));
                                continue;
                            }
                            ClaimValidation::Ready(lease) => {
                                let result = collect(&task, lease).await;
                                if Instant::now() < lease.deadline {
                                    pending = Some((
                                        ReturnRouteOutcome {
                                            id: task.id,
                                            report: result.as_ref().ok().cloned(),
                                            error: result.err().map(str::to_owned),
                                        },
                                        lease.deadline,
                                    ));
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
        }
        tokio::time::sleep(POLL).await;
    }
}

fn parse_egress_ip(body: &[u8], expected: IpAddr) -> Result<(), &'static str> {
    let text = std::str::from_utf8(body).map_err(|_| "collection_failed")?;
    let actual: IpAddr = text.trim().parse().map_err(|_| "collection_failed")?;
    if actual != expected {
        return Err("collection_failed");
    }
    Ok(())
}

async fn verify_egress(family: u8, expected: IpAddr) -> Result<(), &'static str> {
    let (bind, url) = match family {
        4 => (IpAddr::V4(Ipv4Addr::UNSPECIFIED), "https://api.ipify.org"),
        6 => (IpAddr::V6(Ipv6Addr::UNSPECIFIED), "https://api6.ipify.org"),
        _ => return Err("invalid_task"),
    };
    let client = Client::builder()
        .https_only(true)
        .no_proxy()
        .local_address(bind)
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(EGRESS_CONNECT_TIMEOUT)
        .timeout(EGRESS_REQUEST_TIMEOUT)
        .pool_max_idle_per_host(0)
        .user_agent(concat!("senlun-agent/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "unavailable")?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| "collection_failed")?;
    if !response.status().is_success() {
        return Err("collection_failed");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "collection_failed")? {
        if body.len().saturating_add(chunk.len()) > MAX_EGRESS_BODY {
            return Err("collection_failed");
        }
        body.extend_from_slice(&chunk);
    }
    parse_egress_ip(&body, expected)
}

async fn verify_egress_by(
    family: u8,
    expected: IpAddr,
    deadline: Instant,
) -> Result<(), &'static str> {
    tokio::time::timeout_at(deadline.into(), verify_egress(family, expected))
        .await
        .map_err(|_| "timeout")?
}

fn script_timeout_seconds(remaining: Duration) -> Option<u64> {
    remaining
        .as_secs()
        .checked_sub(SCRIPT_KILL_MARGIN_SECONDS)
        .filter(|seconds| *seconds > 0)
        .map(|seconds| seconds.min(MAX_SCRIPT_TIMEOUT_SECONDS))
}

struct ChildProcessGroup {
    child: tokio::process::Child,
    #[cfg(unix)]
    pgid: Option<rustix::process::Pid>,
}

impl ChildProcessGroup {
    fn spawn(command: &mut Command) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        let child = command.spawn()?;
        #[cfg(unix)]
        let pgid = child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .and_then(rustix::process::Pid::from_raw);
        Ok(Self {
            child,
            #[cfg(unix)]
            pgid,
        })
    }

    fn take_stdout(&mut self) -> Option<tokio::process::ChildStdout> {
        self.child.stdout.take()
    }

    #[cfg(unix)]
    fn kill_group(&self) {
        if let Some(pgid) = self.pgid {
            let _ = rustix::process::kill_process_group(pgid, rustix::process::Signal::KILL);
        }
    }

    async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        let status = self.child.wait().await;
        #[cfg(unix)]
        if status.is_ok() {
            // Once wait() reaps the leader PID, disarm immediately so Drop never
            // signals a potentially reused PID.
            self.pgid = None;
        }
        status
    }

    async fn terminate(&mut self) {
        #[cfg(unix)]
        {
            self.kill_group();
            self.pgid = None;
        }
        let _ = self.child.kill().await;
    }
}

impl Drop for ChildProcessGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        self.kill_group();
    }
}

async fn collect(
    task: &ReturnRouteTask,
    lease: TaskLease,
) -> Result<ReturnRouteReport, &'static str> {
    let address: IpAddr = task.address.parse().map_err(|_| "invalid_task")?;
    if !matches!(
        (task.family, address),
        (4, IpAddr::V4(_)) | (6, IpAddr::V6(_))
    ) {
        return Err("invalid_task");
    }
    let collection_deadline = lease.collection_deadline(Instant::now(), COLLECT_TIMEOUT);
    verify_egress_by(task.family, address, collection_deadline).await?;
    let work = tempfile::tempdir().map_err(|_| "unavailable")?;
    let script = work.path().join("shanghai-routes.sh");
    tokio::time::timeout_at(
        collection_deadline.into(),
        tokio::fs::write(&script, COLLECTOR),
    )
    .await
    .map_err(|_| "timeout")?
    .map_err(|_| "unavailable")?;
    let Some(script_timeout_secs) = collection_deadline
        .checked_duration_since(Instant::now())
        .and_then(script_timeout_seconds)
    else {
        return Err("timeout");
    };
    let executable = ["/usr/bin/bash", "/bin/bash"]
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .ok_or("unavailable")?;
    let timeout = ["/usr/bin/timeout", "/bin/timeout"]
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .ok_or("unavailable")?;
    let script_timeout = format!("{script_timeout_secs}s");
    let mut command = Command::new(timeout);
    command
        .args(["-k", "5s", &script_timeout, executable])
        .arg(&script)
        .arg(task.family.to_string())
        .arg(address.to_string())
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C.UTF-8")
        .env("HOME", work.path())
        .env("TMPDIR", work.path())
        .env("TERM", "xterm")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = ChildProcessGroup::spawn(&mut command).map_err(|_| "unavailable")?;
    let output = child.take_stdout().ok_or("unavailable")?;
    let mut bytes = Vec::new();
    let result = tokio::time::timeout_at(collection_deadline.into(), async {
        output
            .take((MAX_RESPONSE + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| "collection_failed")?;
        if bytes.len() > MAX_RESPONSE {
            return Err("response_too_large");
        }
        if !child
            .wait()
            .await
            .map_err(|_| "collection_failed")?
            .success()
        {
            return Err("collection_failed");
        }
        Ok(())
    })
    .await;
    match result {
        Ok(Ok(())) => {
            verify_egress_by(task.family, address, collection_deadline).await?;
            parse(&bytes, task, lease.checked_at_unix_ms(Instant::now()))
        }
        Ok(Err(error)) => {
            child.terminate().await;
            Err(error)
        }
        Err(_) => {
            child.terminate().await;
            Err("timeout")
        }
    }
}

fn is_valid_carrier_target(target: &str) -> bool {
    !target.is_empty()
        && target.len() <= 253
        && target
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b".-:".contains(&c))
}

fn is_valid_carrier_label(label: &str) -> bool {
    !label.is_empty() && label.len() <= 64 && !label.chars().any(char::is_control)
}

fn parse(
    bytes: &[u8],
    task: &ReturnRouteTask,
    checked_at_unix_ms: u64,
) -> Result<ReturnRouteReport, &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "invalid_result")?;
    let mut lines = text.lines();
    let _checked = lines
        .next()
        .and_then(|v| v.strip_prefix("CHECKED_AT="))
        .and_then(|v| v.parse::<u64>().ok())
        .ok_or("invalid_result")?;
    let mut carriers = Vec::new();
    for line in lines {
        let values: Vec<_> = line.splitn(6, '|').collect();
        if values.len() != 6 || values[1] != "上海" || values[3] != "tcp" {
            return Err("invalid_result");
        }
        let carrier = match values[2] {
            "电信" => "telecom",
            "联通" => "unicom",
            "移动" => "mobile",
            _ => return Err("invalid_result"),
        };
        if carriers
            .iter()
            .any(|old: &CarrierRoute| old.carrier == carrier)
        {
            return Err("invalid_result");
        }
        let known =
            values[0] == "OK" && !matches!(values[5], "Hidden" | "Unknown" | "未知" | "-" | "");
        let label = if known { values[5] } else { "未知" };
        if !is_valid_carrier_target(values[4]) || !is_valid_carrier_label(label) {
            return Err("invalid_result");
        }
        carriers.push(CarrierRoute {
            carrier: carrier.to_owned(),
            target: values[4].to_owned(),
            status: if known { "ok" } else { "unknown" }.to_owned(),
            label: label.to_owned(),
        });
    }
    if carriers.len() != 3 {
        return Err("invalid_result");
    }
    Ok(ReturnRouteReport {
        family: task.family,
        address: task.address.clone(),
        region: "SH".to_owned(),
        protocol: "tcp".to_owned(),
        source: "TcpQuality".to_owned(),
        revision: REVISION.to_owned(),
        checked_at_unix_ms,
        carriers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_uses_server_anchored_timestamp_while_validating_header() {
        let task = ReturnRouteTask {
            id: uuid::Uuid::new_v4().to_string(),
            family: 4,
            address: "198.51.100.10".to_owned(),
            expires_at_unix_ms: 1_700_000_480_000,
        };
        let output = concat!(
            "CHECKED_AT=100\n",
            "OK|上海|电信|tcp|telecom.example.test|CN2GIA\n",
            "OK|上海|联通|tcp|unicom.example.test|4837\n",
            "OK|上海|移动|tcp|mobile.example.test|CMI\n"
        );
        let report = parse(output.as_bytes(), &task, 1_700_000_012_345).unwrap();
        assert_eq!(report.checked_at_unix_ms, 1_700_000_012_345);
        assert!(parse(b"missing-header\n", &task, 1_700_000_012_345).is_err());
    }

    #[test]
    fn script_timeout_reserves_kill_grace_before_deadline() {
        assert_eq!(script_timeout_seconds(Duration::from_secs(410)), Some(390));
        assert_eq!(script_timeout_seconds(Duration::from_secs(60)), Some(54));
        assert_eq!(script_timeout_seconds(Duration::from_secs(7)), Some(1));
        assert_eq!(script_timeout_seconds(Duration::from_millis(6_999)), None);
        assert_eq!(script_timeout_seconds(Duration::ZERO), None);
    }

    #[test]
    fn egress_ip_comparison_canonicalizes_ipv6_and_rejects_mismatch() {
        let expected_v6: IpAddr = "2001:db8::1".parse().unwrap();
        let expanded_v6 = b"2001:0DB8:0000:0000:0000:0000:0000:0001\n";
        assert!(parse_egress_ip(expanded_v6, expected_v6).is_ok());
        assert!(parse_egress_ip(b"2001:db8::2\n", expected_v6).is_err());
        assert!(parse_egress_ip(b"not-an-ip", expected_v6).is_err());

        let expected_v4: IpAddr = "198.51.100.10".parse().unwrap();
        assert!(parse_egress_ip(b"198.51.100.10\n", expected_v4).is_ok());
        assert!(parse_egress_ip(b"198.51.100.11", expected_v4).is_err());
    }

    #[test]
    fn parse_matches_service_target_and_label_validation() {
        let task = ReturnRouteTask {
            id: uuid::Uuid::new_v4().to_string(),
            family: 4,
            address: "198.51.100.10".to_owned(),
            expires_at_unix_ms: 1_700_000_480_000,
        };
        let valid = concat!(
            "CHECKED_AT=1700000000\n",
            "OK|上海|电信|tcp|telecom.example.test|CN2GIA\n",
            "OK|上海|联通|tcp|unicom.example.test|4837\n",
            "OK|上海|移动|tcp|2001:db8::1|CMI\n"
        );
        assert!(parse(valid.as_bytes(), &task, 1_700_000_012_345).is_ok());

        for invalid in [
            concat!(
                "CHECKED_AT=1700000000\n",
                "OK|上海|电信|tcp||CN2GIA\n",
                "OK|上海|联通|tcp|unicom.example.test|4837\n",
                "OK|上海|移动|tcp|mobile.example.test|CMI\n"
            ),
            concat!(
                "CHECKED_AT=1700000000\n",
                "OK|上海|电信|tcp|bad target/path|CN2GIA\n",
                "OK|上海|联通|tcp|unicom.example.test|4837\n",
                "OK|上海|移动|tcp|mobile.example.test|CMI\n"
            ),
            concat!(
                "CHECKED_AT=1700000000\n",
                "OK|上海|电信|tcp|telecom.example.test|CN2\x07GIA\n",
                "OK|上海|联通|tcp|unicom.example.test|4837\n",
                "OK|上海|移动|tcp|mobile.example.test|CMI\n"
            ),
        ] {
            assert_eq!(
                parse(invalid.as_bytes(), &task, 1_700_000_012_345).err(),
                Some("invalid_result")
            );
        }
    }

    #[cfg(target_os = "linux")]
    fn is_pid_running(pid: u32) -> bool {
        let status_path = Path::new("/proc").join(pid.to_string()).join("status");
        std::fs::read_to_string(status_path).is_ok_and(|status| {
            !status
                .lines()
                .any(|line| line.starts_with("State:\tZ") || line.starts_with("State:\tX"))
        })
    }

    #[cfg(target_os = "linux")]
    async fn wait_for_pid_exit(pid: u32) -> bool {
        for _ in 0..40 {
            if !is_pid_running(pid) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    #[cfg(target_os = "linux")]
    async fn spawn_hanging_subtree(
        dir: &Path,
        inner_timeout: &str,
    ) -> Option<(ChildProcessGroup, u32)> {
        let bash = ["/usr/bin/bash", "/bin/bash"]
            .into_iter()
            .find(|p| Path::new(p).is_file())?;
        let timeout_bin = ["/usr/bin/timeout", "/bin/timeout"]
            .into_iter()
            .find(|p| Path::new(p).is_file())?;
        let pid_file = dir.join("grandchild.pid");
        let script = dir.join("hang.sh");
        let body = format!(concat!(
            "set -eu\n",
            "{timeout_bin} --foreground -k 1s {inner_timeout} ",
            "{bash} -c 'sleep 30 & printf \"%s\\n\" \"$!\" > \"$1\"; wait' _ \"$1\" || {{\n",
            "  kill -KILL 0 2>/dev/null || true\n",
            "  exit 4\n",
            "}}\n"
        ));
        tokio::fs::write(&script, body).await.unwrap();

        let mut command = Command::new(timeout_bin);
        command
            .args(["-k", "1s", "30s", bash])
            .arg(&script)
            .arg(&pid_file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let guard = ChildProcessGroup::spawn(&mut command).unwrap();

        for _ in 0..40 {
            if let Ok(raw) = tokio::fs::read_to_string(&pid_file).await
                && let Ok(pid) = raw.trim().parse::<u32>()
            {
                return Some((guard, pid));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("grandchild pid file was not written");
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn process_group_drop_cleans_up_hanging_subtree_on_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        let Some((guard, pid)) = spawn_hanging_subtree(dir.path(), "30s").await else {
            return;
        };
        assert!(is_pid_running(pid));
        drop(guard);
        assert!(
            wait_for_pid_exit(pid).await,
            "grandchild should exit when ChildProcessGroup is dropped"
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn process_group_terminate_cleans_up_hanging_subtree_on_timeout_or_error() {
        let dir = tempfile::tempdir().unwrap();
        let Some((mut guard, pid)) = spawn_hanging_subtree(dir.path(), "30s").await else {
            return;
        };
        assert!(is_pid_running(pid));
        guard.terminate().await;
        assert!(
            wait_for_pid_exit(pid).await,
            "grandchild should exit when ChildProcessGroup::terminate runs"
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn inner_foreground_timeout_cleans_up_hanging_grandchild() {
        let dir = tempfile::tempdir().unwrap();
        let Some((mut guard, pid)) = spawn_hanging_subtree(dir.path(), "1s").await else {
            return;
        };
        assert!(is_pid_running(pid));
        let status = tokio::time::timeout(Duration::from_secs(5), guard.wait())
            .await
            .expect("inner timeout should terminate hanging script")
            .unwrap();
        assert!(!status.success());
        assert!(
            wait_for_pid_exit(pid).await,
            "inner timeout handler should terminate hanging grandchild"
        );
    }
}
