//! One fixed, opt-in, one-shot Shanghai TCP monitoring task at a time.
//! Existing Agent authentication stays in the HTTP client. The reviewed child
//! receives only a parsed address/family, an empty environment and private tmp.
use std::{
    net::IpAddr,
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
                available: available(),
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

fn script_timeout_seconds(remaining: Duration) -> Option<u64> {
    remaining
        .as_secs()
        .checked_sub(SCRIPT_KILL_MARGIN_SECONDS)
        .filter(|seconds| *seconds > 0)
        .map(|seconds| seconds.min(MAX_SCRIPT_TIMEOUT_SECONDS))
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
    let mut child = command.spawn().map_err(|_| "unavailable")?;
    let output = child.stdout.take().ok_or("unavailable")?;
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
        Ok(Ok(())) => parse(&bytes, task, lease.checked_at_unix_ms(Instant::now())),
        Ok(Err(error)) => {
            let _ = child.kill().await;
            Err(error)
        }
        Err(_) => {
            let _ = child.kill().await;
            Err("timeout")
        }
    }
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
        if values[4].len() > 253 || values[5].len() > 64 {
            return Err("invalid_result");
        }
        carriers.push(CarrierRoute {
            carrier: carrier.to_owned(),
            target: values[4].to_owned(),
            status: if known { "ok" } else { "unknown" }.to_owned(),
            label: if known { values[5] } else { "未知" }.to_owned(),
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
}
