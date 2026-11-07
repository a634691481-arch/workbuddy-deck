//! 后台轮询：首评只记 seen 不通知，之后每 [`POLL_INTERVAL_SECS`] 评估一次。
//!
//! 新提醒发 `alerts://new` 事件，前端决定要不要系统通知（失焦才发）。
//! 首评兜底：`seen` 为空说明是第一次评估（或重装），此时只记 seen 不通知——
//! 否则装机第一天铃铛还没点亮就先弹一串历史问题。

use std::time::Duration;

use serde_json::{json, Value};
use tauri::Emitter;
use wb_switch_core::modules::config::now_ms;
use wb_switch_core::modules::notifications;

use super::state;
use super::{evaluate, POLL_INTERVAL_SECS, STARTUP_DELAY_SECS};

/// 评估并挑出「第一次出现」的提醒，同时记 seen。
fn evaluate_new(mark_seen: bool) -> Vec<Value> {
    let alerts = evaluate();
    let mut state = state::load();
    let now = now_ms();
    let first_run = state.seen.is_empty();
    let fresh: Vec<Value> = alerts
        .iter()
        .filter(|alert| {
            let Some(fingerprint) = alert.get("fingerprint").and_then(Value::as_str) else {
                return false;
            };
            !first_run && !state.seen.contains_key(fingerprint)
        })
        .cloned()
        .collect();
    if mark_seen {
        for alert in &alerts {
            if let Some(fingerprint) = alert.get("fingerprint").and_then(Value::as_str) {
                state.seen.insert(fingerprint.to_string(), now);
            }
        }
        state::save(&state);
    }
    fresh
}

/// 启动后延迟首评，之后固定间隔评估。
pub fn spawn_poller(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(STARTUP_DELAY_SECS)).await;
        let _ = evaluate_new(true);
        loop {
            tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
            let fresh = evaluate_new(true);
            if fresh.is_empty() {
                continue;
            }
            // 顺带记一条通知存档，保证「评估 → 日志里能查到」闭环
            let _ = notifications::record(
                "warning",
                "异常提醒",
                Some(&format!("新增 {} 条异常提醒，详见标题栏铃铛", fresh.len())),
            );
            let _ = app.emit("alerts://new", json!({ "alerts": fresh }));
        }
    });
}
