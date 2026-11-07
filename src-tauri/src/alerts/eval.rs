//! 评估编排：五路信号 → 忽略过滤 → critical 优先排序。
//!
//! 纯函数：输入（账号缓存行 / 切换历史 / 通知）与状态都由调用方传入，
//! 单测可以完全脱离磁盘，不读真实用户目录。

use serde_json::Value;
use wb_switch_core::modules::notifications::NotificationEntry;

use super::signals;
use super::state::{self, AlertsState};

/// 一次评估的全部输入。
#[derive(Default)]
pub(super) struct Inputs {
    pub(super) rows: Vec<Value>,
    pub(super) switches: Vec<Value>,
    pub(super) notifications: Vec<NotificationEntry>,
}

/// 评估全部信号。返回按级别排序：critical 在前。
pub(super) fn evaluate(inputs: &Inputs, state: &AlertsState, now: i64) -> Vec<Value> {
    let mut alerts = signals::account_alerts(&inputs.rows, now);
    alerts.extend(signals::switch_alerts(&inputs.switches, now));
    if let Some(alert) = signals::runfail_alert(&inputs.notifications, now) {
        alerts.push(alert);
    }
    alerts.retain(|alert| !silenced(alert, state, now));
    alerts.sort_by_key(level_rank);
    alerts
}

/// 指纹在忽略静默期内 → 整条静默（条件仍在也不骚扰）。
fn silenced(alert: &Value, state: &AlertsState, now: i64) -> bool {
    alert
        .get("fingerprint")
        .and_then(Value::as_str)
        .is_some_and(|fingerprint| state::is_dismissed(state, fingerprint, now))
}

fn level_rank(alert: &Value) -> u8 {
    if alert.get("level").and_then(Value::as_str) == Some("critical") {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::DISMISS_TTL_MS;
    use serde_json::json;

    fn balance_row(days: f64) -> Value {
        json!({
            "accountId": "a1",
            "label": "主力",
            "health": { "score": 90, "grade": "good", "reasons": [] },
            "forecast": { "daysLeft": days, "confidence": "ok" },
        })
    }

    #[test]
    fn empty_inputs_yield_no_alerts() {
        let alerts = evaluate(&Inputs::default(), &AlertsState::default(), 1_000);
        assert!(alerts.is_empty(), "没有任何数据时清单必须为空，不 panic");
    }

    #[test]
    fn dismissed_fingerprint_is_silent_for_seven_days() {
        let now = 10 * DISMISS_TTL_MS;
        let inputs = Inputs {
            rows: vec![balance_row(1.0)],
            ..Inputs::default()
        };
        let mut state = AlertsState::default();
        state.dismissed.insert("balance:a1".to_string(), now - 1_000);
        assert!(
            evaluate(&inputs, &state, now).is_empty(),
            "忽略后 7 天内静默"
        );

        let expired = now + DISMISS_TTL_MS;
        assert_eq!(
            evaluate(&inputs, &state, expired).len(),
            1,
            "7 天后同一指纹重新提醒"
        );
    }

    #[test]
    fn critical_alerts_come_first() {
        let now = 1_000;
        let mut safe = balance_row(1.0);
        safe["accountId"] = json!("warn-only");
        safe["forecast"] = json!({ "daysLeft": 20.0, "confidence": "ok" });
        safe["resources"] = json!([
            { "packageName": "月包", "remaining": 10.0, "expireAt": now + 5 * crate::alerts::DAY_MS },
        ]);
        let mut tight = balance_row(1.0);
        tight["accountId"] = json!("critical-only");

        let alerts = evaluate(
            &Inputs {
                rows: vec![safe, tight],
                ..Inputs::default()
            },
            &AlertsState::default(),
            now,
        );
        assert_eq!(alerts.len(), 2);
        assert_eq!(alerts[0]["level"], "critical");
        assert_eq!(alerts[1]["level"], "warn");
    }
}
