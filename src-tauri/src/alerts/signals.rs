//! 五路信号的判定：纯计算，输入由 `eval` 传入，不读盘、不发请求。
//!
//! 账号侧字段口径（以 `account_overview` 缓存行为准）：
//! - 余额预测在 `row.forecast.daysLeft`（core `forecast.rs`），**不在** `health` 里；
//! - 健康分等级在 `row.health.grade`（core `health.rs`），没有 `status` 字段。
//!
//! 旧缓存缺这些字段时一律不触发——宁可少报，也不能读错字段后 panic。

use serde_json::{json, Value};
use wb_switch_core::modules::notifications::NotificationEntry;

use super::DAY_MS;

/// 余额按预测不足这么多天算 critical。
const BALANCE_CRITICAL_DAYS: f64 = 3.0;
/// 积分包 3 天内到期算 critical。
const EXPIRING_CRITICAL_MS: i64 = 3 * DAY_MS;
/// 只看 7 天内到期的包：更远的现在提醒也没有动作可做。
const EXPIRING_WINDOW_MS: i64 = 7 * DAY_MS;
/// 24h 内失败事件达到该数量才报「连续失败」。
const RUNFAIL_THRESHOLD: usize = 3;
/// 失败事件的时间窗口。
const FAILURE_WINDOW_MS: i64 = DAY_MS;
/// 失败语义关键词：只认「动作没做成」。
/// 故意不收「异常」——提醒中心自己会写一条标题为「异常提醒」的 warning，
/// 收进来会让一次提醒反过来抬高下一轮的失败计数（自我放大）。
const FAILURE_KEYWORDS: [&str; 7] = ["失败", "错误", "超时", "fail", "error", "timeout", "timed out"];

fn account_identity(row: &Value) -> (&str, &str) {
    (
        row.get("accountId").and_then(Value::as_str).unwrap_or(""),
        row.get("label").and_then(Value::as_str).unwrap_or(""),
    )
}

/// 账号侧信号：余额 / 健康 / 到期，逐行产出。
pub(super) fn account_alerts(rows: &[Value], now: i64) -> Vec<Value> {
    let mut out = Vec::new();
    for row in rows {
        push_balance(&mut out, row, now);
        push_health(&mut out, row, now);
        push_expiring(&mut out, row, now);
    }
    out
}

/// 余额耗尽预测：`daysLeft` 来自缓存行的 `forecast`，缺字段 = 样本不足，不报。
fn push_balance(out: &mut Vec<Value>, row: &Value, now: i64) {
    let (id, label) = account_identity(row);
    if id.is_empty() {
        return;
    }
    let forecast = row.get("forecast");
    let Some(days) = forecast
        .and_then(|value| value.get("daysLeft"))
        .and_then(Value::as_f64)
    else {
        return;
    };
    if days >= BALANCE_CRITICAL_DAYS {
        return;
    }
    out.push(json!({
        "fingerprint": format!("balance:{id}"),
        "kind": "balance",
        "level": "critical",
        "accountId": id,
        "accountLabel": label,
        "ts": now,
        "data": {
            "daysLeft": days,
            "confidence": forecast
                .and_then(|value| value.get("confidence"))
                .cloned()
                .unwrap_or(Value::Null),
        },
    }));
}

/// 健康分 critical：`grade` 由 core `health.rs` 给出（`status` 是早期误读的字段）。
fn push_health(out: &mut Vec<Value>, row: &Value, now: i64) {
    let (id, label) = account_identity(row);
    if id.is_empty() {
        return;
    }
    let health = row.get("health").cloned().unwrap_or(Value::Null);
    if health.get("grade").and_then(Value::as_str) != Some("critical") {
        return;
    }
    out.push(json!({
        "fingerprint": format!("health:{id}"),
        "kind": "health",
        "level": "critical",
        "accountId": id,
        "accountLabel": label,
        "ts": now,
        "data": {
            "score": health.get("score").and_then(Value::as_f64).unwrap_or(0.0),
            "reasons": health.get("reasons").cloned().unwrap_or_else(|| json!([])),
        },
    }));
}

/// 即将到期的积分包（只在还有余额时提醒；已耗尽的包没有动作可做）。
fn push_expiring(out: &mut Vec<Value>, row: &Value, now: i64) {
    let (id, label) = account_identity(row);
    if id.is_empty() {
        return;
    }
    let Some(resources) = row.get("resources").and_then(Value::as_array) else {
        return;
    };
    for item in resources {
        let remaining = item
            .get("remaining")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        if remaining <= 0.0 {
            continue;
        }
        let Some(expire_at) = item.get("expireAt").and_then(Value::as_i64) else {
            continue;
        };
        let left_ms = expire_at - now;
        if !(0..=EXPIRING_WINDOW_MS).contains(&left_ms) {
            continue;
        }
        out.push(json!({
            "fingerprint": format!("expiring:{id}:{expire_at}"),
            "kind": "expiring",
            "level": if left_ms < EXPIRING_CRITICAL_MS { "critical" } else { "warn" },
            "accountId": id,
            "accountLabel": label,
            "ts": now,
            "data": {
                "packageName": item.get("packageName").cloned().unwrap_or(Value::Null),
                "remaining": remaining,
                "expireAt": expire_at,
            },
        }));
    }
}

/// 切换失败 / 回滚：只报 24h 内、同账号最近一次（entries 新的在前）。
pub(super) fn switch_alerts(entries: &[Value], now: i64) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for entry in entries {
        let at = entry.get("ts").and_then(Value::as_i64).unwrap_or(0);
        if now - at > DAY_MS {
            continue;
        }
        let result = entry.get("result").and_then(Value::as_str).unwrap_or("");
        if result.is_empty() || result == "ok" {
            continue;
        }
        let account_id = entry
            .get("accountId")
            .and_then(Value::as_str)
            .unwrap_or("");
        let fingerprint = format!("switch:{account_id}");
        if out.iter().any(|item| {
            item.get("fingerprint").and_then(Value::as_str) == Some(fingerprint.as_str())
        }) {
            continue;
        }
        out.push(json!({
            "fingerprint": fingerprint,
            "kind": "switch",
            "level": if result == "rolled_back" { "critical" } else { "warn" },
            "accountId": account_id,
            "accountLabel": entry.get("toLabel").and_then(Value::as_str).unwrap_or(""),
            "ts": at,
            "data": {
                "result": result,
                "stage": entry.get("stage").cloned().unwrap_or(Value::Null),
                "error": entry.get("error").cloned().unwrap_or(Value::Null),
            },
        }));
    }
    out
}

/// 24h 内失败事件聚合：达到 [`RUNFAIL_THRESHOLD`] 条只报一条（不逐个刷屏）。
pub(super) fn runfail_alert(entries: &[NotificationEntry], now: i64) -> Option<Value> {
    let count = entries
        .iter()
        .filter(|entry| now - entry.at <= FAILURE_WINDOW_MS && is_failure(entry))
        .count();
    if count < RUNFAIL_THRESHOLD {
        return None;
    }
    Some(json!({
        "fingerprint": "runfail",
        "kind": "runfail",
        "level": "warn",
        "accountId": "",
        "accountLabel": "",
        "ts": now,
        "data": { "count": count },
    }))
}

/// 「连续失败」口径（spec：24h ≥3 条）：
/// 1. `level == "error"` 一律计入（当前写入点只有 warning/info/success，这里给后续留口子）；
/// 2. `level == "warning"` 且标题或详情含失败语义——executor 的「部分失败」、
///    请求超时等实际都写 warning，只看 error 会永远数不到。
fn is_failure(entry: &NotificationEntry) -> bool {
    if entry.level == "error" {
        return true;
    }
    if entry.level != "warning" {
        return false;
    }
    has_failure_text(&entry.title)
        || entry
            .description
            .as_deref()
            .is_some_and(has_failure_text)
}

fn has_failure_text(text: &str) -> bool {
    let lowered = text.to_lowercase();
    FAILURE_KEYWORDS
        .iter()
        .any(|keyword| lowered.contains(keyword))
}

#[cfg(test)]
#[path = "signals_tests.rs"]
mod tests;
