//! One fixed, opt-in, one-shot Shanghai TCP monitoring task at a time.
//! Existing Agent authentication stays in the HTTP client. The reviewed child
//! receives only a parsed address/family, an empty environment and private tmp.
use std::{net::IpAddr, path::Path, process::Stdio, time::Duration};

use reqwest::{Client, StatusCode, Url};
use senlun_protocol::{
    CarrierRoute, RETURN_ROUTE_SCHEMA_VERSION, ReturnRouteClaim, ReturnRouteOutcome,
    ReturnRouteReport, ReturnRouteTask,
};
use tokio::{io::AsyncReadExt, process::Command, task::JoinHandle};
use uuid::Uuid;

const COLLECTOR: &str = include_str!("shanghai-routes.sh");
const REVISION: &str = "db52fc60523320fc4a34b5c369f28be38df21d13";
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(u64::MAX, |d| {
            u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
        })
}

const POLL: Duration = Duration::from_secs(15);
const MAX_RESPONSE: usize = 16 * 1024;

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
    let mut pending: Option<ReturnRouteOutcome> = None;
    let mut deadline = 0;
    loop {
        if let Some(outcome) = &pending {
            if now_ms() >= deadline {
                pending = None;
            } else {
                match client
                    .post(result_url.clone())
                    .bearer_auth(&token)
                    .json(outcome)
                    .send()
                    .await
                {
                    Ok(response) if response.status() == StatusCode::UNAUTHORIZED => return,
                    Ok(response)
                        if response.status().is_success()
                            || response.status() == StatusCode::CONFLICT
                            || response.status() == StatusCode::BAD_REQUEST =>
                    {
                        pending = None;
                    }
                    _ => {}
                }
            }
        }
        if pending.is_none() {
            let claim = ReturnRouteClaim {
                schema_version: RETURN_ROUTE_SCHEMA_VERSION,
                agent_version: env!("CARGO_PKG_VERSION").to_owned(),
                available: available(),
            };
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
                        let now = now_ms();
                        if task.expires_at_unix_ms > now
                            && task.expires_at_unix_ms <= now.saturating_add(540_000)
                            && Uuid::parse_str(&task.id).is_ok()
                        {
                            deadline = task.expires_at_unix_ms;
                            let result = collect(&task).await;
                            pending = Some(ReturnRouteOutcome {
                                id: task.id,
                                report: result.as_ref().ok().cloned(),
                                error: result.err().map(str::to_owned),
                            });
                            continue;
                        }
                    }
                }
            }
        }
        tokio::time::sleep(POLL).await;
    }
}

async fn collect(task: &ReturnRouteTask) -> Result<ReturnRouteReport, &'static str> {
    let address: IpAddr = task.address.parse().map_err(|_| "invalid_task")?;
    if !matches!(
        (task.family, address),
        (4, IpAddr::V4(_)) | (6, IpAddr::V6(_))
    ) {
        return Err("invalid_task");
    }
    let work = tempfile::tempdir().map_err(|_| "unavailable")?;
    let script = work.path().join("shanghai-routes.sh");
    tokio::fs::write(&script, COLLECTOR)
        .await
        .map_err(|_| "unavailable")?;
    let executable = ["/usr/bin/bash", "/bin/bash"]
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .ok_or("unavailable")?;
    let timeout = ["/usr/bin/timeout", "/bin/timeout"]
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .ok_or("unavailable")?;
    let mut command = Command::new(timeout);
    command
        .args(["-k", "5s", "390s", executable])
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
    let result = tokio::time::timeout(Duration::from_secs(410), async {
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
        Ok(Ok(())) => parse(&bytes, task),
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

fn parse(bytes: &[u8], task: &ReturnRouteTask) -> Result<ReturnRouteReport, &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "invalid_result")?;
    let mut lines = text.lines();
    let checked = lines
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
        checked_at_unix_ms: checked.saturating_mul(1000),
        carriers,
    })
}
