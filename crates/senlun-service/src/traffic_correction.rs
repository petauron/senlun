//! Browser correction of local billing usage. No provider snapshots or Agent counters are written.
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    control::{TrafficCorrection, options_or_none, period_start, reset_day, verify_nodes},
    storage::{Storage, StorageError},
};

const BASELINE_MAX_AGE_MS: u64 = 120_000;

// Byte counts and the audit cursor cross JavaScript as decimal strings, never f64.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TrafficBaseline {
    pub sample_id: String,
    pub cycle_start_ms: u64,
    pub used_up_bytes: String,
    pub used_down_bytes: String,
    pub audit_cursor: String,
    pub traffic_limit_type: String,
    pub traffic_reset_day: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BrowserTrafficCorrection {
    pub baseline: TrafficBaseline,
    pub used_up_bytes: String,
    pub used_down_bytes: String,
    pub reason: String,
}

fn bytes(raw: &str) -> Result<u64, StorageError> {
    if raw.is_empty() || raw.len() > 19 || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(StorageError::InvalidInput("流量必须是非负的整数字节数"));
    }
    raw.parse::<u64>()
        .ok()
        .filter(|value| *value <= 9_223_372_036_854_775_807)
        .ok_or(StorageError::InvalidInput("流量数值超出可保存范围"))
}

fn counter(value: i64) -> Result<u64, StorageError> {
    u64::try_from(value).map_err(|_| StorageError::InvalidInput("本地流量基准无效"))
}

fn fresh_sample(sampled: u64, now: u64, offline_after_seconds: u64) -> Result<(), StorageError> {
    if sampled > now
        || now - sampled > BASELINE_MAX_AGE_MS.min(offline_after_seconds.saturating_mul(1000))
    {
        return Err(StorageError::Conflict(
            "节点离线或采样已过期，请等待新上报后重新预览",
        ));
    }
    Ok(())
}

fn context(db: &Connection, id: &str, now: u64, offline: u64) -> Result<Value, StorageError> {
    verify_nodes(db, &[id.to_owned()])?;
    let options = options_or_none(db, id)?;
    let reset_day = reset_day(options.as_ref());
    let direction = options
        .as_ref()
        .map_or("sum", |value| value.traffic_limit_type.as_str());
    let (cycle, up, down): (i64, i64, i64) = db
        .query_row(
            "SELECT cycle_start_ms,used_up,used_down FROM traffic_periods WHERE node_id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or(StorageError::Conflict("节点尚无本地流量采样"))?;
    let (sample_id, sampled): (String, i64) = db.query_row(
        "SELECT sample_id,received_at_ms FROM snapshots WHERE node_id=?1 ORDER BY received_at_ms DESC,id DESC LIMIT 1",
        [id], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?.ok_or(StorageError::Conflict("节点尚无本地流量采样"))?;
    let sampled = counter(sampled)?;
    fresh_sample(sampled, now, offline)?;
    if counter(cycle)? != period_start(now, reset_day) {
        return Err(StorageError::Conflict(
            "计费周期已切换，请等待新上报后重新预览",
        ));
    }
    let cursor: i64 = db.query_row("SELECT COALESCE(MAX(id),0) FROM audit_events", [], |row| {
        row.get(0)
    })?;
    let baseline = TrafficBaseline {
        sample_id,
        cycle_start_ms: counter(cycle)?,
        used_up_bytes: counter(up)?.to_string(),
        used_down_bytes: counter(down)?.to_string(),
        audit_cursor: cursor.to_string(),
        traffic_limit_type: direction.to_owned(),
        traffic_reset_day: reset_day,
    };
    Ok(
        json!({"node_id":id,"name":options.as_ref().map_or(id, |value|value.name.as_str()),
        "baseline":baseline,"sampled_at_ms":sampled,
        "traffic_limit_bytes":options.as_ref().map_or(0, |value|value.traffic_limit_bytes).to_string()}),
    )
}

impl Storage {
    pub(crate) fn traffic_correction_context(
        &self,
        id: &str,
        now: u64,
        offline: u64,
    ) -> Result<Value, StorageError> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let result = context(&tx, id, now, offline)?;
        tx.commit()?;
        Ok(result)
    }

    pub(crate) fn correct_browser_traffic(
        &self,
        id: &str,
        input: &BrowserTrafficCorrection,
        actor: &str,
        now: u64,
        offline: u64,
    ) -> Result<Value, StorageError> {
        let reason = input.reason.trim();
        if reason.is_empty() || reason.len() > 512 || reason.chars().any(char::is_control) {
            return Err(StorageError::InvalidInput(
                "请填写校正原因（最多 512 字节，不含控制字符）",
            ));
        }
        if input.baseline.sample_id.len() > 64 {
            return Err(StorageError::InvalidInput("采样标识无效"));
        }
        bytes(&input.used_up_bytes)?;
        bytes(&input.used_down_bytes)?;
        let expected_up = bytes(&input.baseline.used_up_bytes)?;
        let expected_down = bytes(&input.baseline.used_down_bytes)?;
        let cursor = i64::try_from(bytes(&input.baseline.audit_cursor)?)
            .map_err(|_| StorageError::InvalidInput("预览基准无效"))?;
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        // Reuse the bounded audit history as a conservative mutation fence.
        // Missing/pruned evidence fails closed; no idempotency table or cache.
        unchanged_since(&tx, id, cursor)?;
        let current = context(&tx, id, now, offline)?;
        if current["baseline"]["cycle_start_ms"].as_u64() != Some(input.baseline.cycle_start_ms)
            || current["baseline"]["traffic_reset_day"].as_u64()
                != Some(u64::from(input.baseline.traffic_reset_day))
            || current["baseline"]["traffic_limit_type"].as_str()
                != Some(input.baseline.traffic_limit_type.as_str())
        {
            return Err(StorageError::Conflict(
                "计费周期或计数方式已变化，请重新预览",
            ));
        }
        let value = anchored_correction(&tx, id, input, expected_up, expected_down, now)?;
        Self::correct_traffic_in_transaction(&tx, id, &value, now, actor, reason)?;
        let result = context(&tx, id, now, offline)?;
        tx.commit()?;
        Ok(result)
    }
}

fn unchanged_since(db: &Connection, id: &str, cursor: i64) -> Result<(), StorageError> {
    let (oldest, newest): (i64, i64) = db.query_row(
        "SELECT COALESCE(MIN(id),0),COALESCE(MAX(id),0) FROM audit_events",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if cursor > newest || oldest > cursor.saturating_add(1) {
        return Err(StorageError::Conflict("预览证据已失效，请重新预览"));
    }
    let changed: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM audit_events WHERE id>?1 AND (
                (action IN ('node.metadata.update','node.traffic.reset') AND subject=?2)
                OR (action='node.traffic.correct' AND json_extract(CASE WHEN json_valid(subject) THEN subject ELSE '{}' END,'$.node_id')=?2)))",
            rusqlite::params![cursor,id], |row|row.get(0),
        )?;
    if changed {
        return Err(StorageError::Conflict(
            "该节点已被修改或本次请求已执行，请刷新确认后重新预览",
        ));
    }
    Ok(())
}

fn anchored_correction(
    db: &Connection,
    id: &str,
    input: &BrowserTrafficCorrection,
    expected_up: u64,
    expected_down: u64,
    now: u64,
) -> Result<TrafficCorrection, StorageError> {
    // Resolve the original immutable sample in this same transaction. The
    // browser never supplies or edits Agent raw counters.
    let (sampled, raw_up, raw_down): (i64, i64, i64) = db.query_row(
            "SELECT received_at_ms,network_total_transmitted_bytes,network_total_received_bytes FROM snapshots WHERE node_id=?1 AND sample_id=?2",
            rusqlite::params![id,input.baseline.sample_id], |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).optional()?.ok_or(StorageError::Conflict("原始采样已不可用，请重新预览"))?;
    fresh_sample(counter(sampled)?, now, BASELINE_MAX_AGE_MS / 1000)?;
    let (up, down, used_up, used_down): (i64, i64, i64, i64) = db.query_row(
        "SELECT raw_up,raw_down,used_up,used_down FROM traffic_periods WHERE node_id=?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    if up < raw_up
        || down < raw_down
        || i128::from(used_up) - i128::from(up) != i128::from(expected_up) - i128::from(raw_up)
        || i128::from(used_down) - i128::from(down)
            != i128::from(expected_down) - i128::from(raw_down)
    {
        return Err(StorageError::Conflict(
            "计数器已回落或流量基准已变化，请重新预览",
        ));
    }
    Ok(TrafficCorrection {
        cycle_start_ms: input.baseline.cycle_start_ms,
        sampled_at_ms: counter(sampled)?,
        raw_up: counter(raw_up)?,
        raw_down: counter(raw_down)?,
        used_up: bytes(&input.used_up_bytes)?,
        used_down: bytes(&input.used_down_bytes)?,
    })
}
