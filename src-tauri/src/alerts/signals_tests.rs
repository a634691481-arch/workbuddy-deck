//! `signals.rs` 的单测：信号判定口径（阈值 / 边界 / 旧缓存兼容）。

use super::*;

fn account_row(days_left: Value, grade: &str) -> Value {
    json!({
        "accountId": "a1",
        "label": "主力",
        "health": { "score": 20, "grade": grade, "reasons": ["empty"] },
        "forecast": { "daysLeft": days_left, "confidence": "ok" },
        "resources": [],
    })
}

#[test]
fn balance_alert_only_below_three_days() {
    let now = 1_000_000;
    let tight = account_alerts(&[account_row(json!(2.9), "good")], now);
    assert_eq!(tight.len(), 1, "2.9 天必须报");
    assert_eq!(tight[0]["kind"], "balance");
    assert_eq!(tight[0]["level"], "critical");

    let safe = account_alerts(&[account_row(json!(3.0), "good")], now);
    assert!(safe.is_empty(), ">=3 天不报");

    let mut legacy = account_row(json!(null), "good");
    legacy.as_object_mut().unwrap().remove("forecast");
    assert!(
        account_alerts(&[legacy], now).is_empty(),
        "旧缓存缺 forecast：不触发，也不 panic"
    );
}

#[test]
fn health_alert_reads_grade_not_status() {
    let now = 1_000_000;
    let alerts = account_alerts(&[account_row(json!(null), "critical")], now);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0]["kind"], "health");
    assert_eq!(alerts[0]["level"], "critical");

    // 真实数据里没有 health.status：写了也不能生效（防止再读错字段）。
    let mut stale = account_row(json!(null), "good");
    stale["health"]["status"] = json!("critical");
    assert!(account_alerts(&[stale], now).is_empty());
}

#[test]
fn expiring_window_has_three_and_seven_day_tiers() {
    let now = 1_000_000;
    let mut row = account_row(json!(null), "good");
    row["resources"] = json!([
        { "packageName": "周包", "remaining": 10.0, "expireAt": now + 2 * DAY_MS },
        { "packageName": "月包", "remaining": 10.0, "expireAt": now + 5 * DAY_MS },
        { "packageName": "远包", "remaining": 10.0, "expireAt": now + 30 * DAY_MS },
        { "packageName": "空包", "remaining": 0.0, "expireAt": now + DAY_MS },
    ]);

    let alerts = account_alerts(&[row], now);
    assert_eq!(alerts.len(), 2, "30 天后与已耗尽的不报");
    assert_eq!(alerts[0]["level"], "critical", "3 天内 = critical");
    assert_eq!(alerts[1]["level"], "warn", "7 天内 = warn");
}

#[test]
fn switch_alerts_report_latest_failure_per_account() {
    let now = 10 * DAY_MS;
    let entries = vec![
        json!({ "ts": now - 1_000, "result": "rolled_back", "stage": "verify", "accountId": "a1", "toLabel": "备用" }),
        json!({ "ts": now - 2_000, "result": "blocked", "stage": "session", "accountId": "a1", "toLabel": "备用" }),
        json!({ "ts": now - 3_000, "result": "ok", "stage": "done", "accountId": "a2" }),
        json!({ "ts": now - DAY_MS - 1, "result": "blocked", "stage": "session", "accountId": "a3" }),
    ];

    let alerts = switch_alerts(&entries, now);
    assert_eq!(alerts.len(), 1, "同账号只报最近一次，跳过成功与 24h 外");
    assert_eq!(alerts[0]["level"], "critical");
    assert_eq!(alerts[0]["data"]["result"], "rolled_back");
}

fn notice(level: &str, title: &str, description: Option<&str>, at: i64) -> NotificationEntry {
    NotificationEntry {
        level: level.to_string(),
        title: title.to_string(),
        description: description.map(str::to_string),
        at,
    }
}

#[test]
fn runfail_needs_three_failures_within_24h() {
    let now = 10 * DAY_MS;
    let two = vec![
        notice("warning", "签到 · 部分失败", Some("成功 1 · 失败 2"), now - 1_000),
        notice("error", "拉取失败", None, now - 2_000),
    ];
    assert!(runfail_alert(&two, now).is_none(), "2 条不到阈值");

    let three = vec![
        notice("warning", "签到 · 部分失败", Some("成功 1 · 失败 2"), now - 1_000),
        notice("error", "拉取失败", None, now - 2_000),
        notice("warning", "成长任务 · 部分失败", None, now - 3_000),
    ];
    let alert = runfail_alert(&three, now).expect("3 条必须报警");
    assert_eq!(alert["data"]["count"], 3);
    assert_eq!(alert["level"], "warn");
}

#[test]
fn runfail_ignores_non_failures_and_stale_entries() {
    let now = 10 * DAY_MS;
    let noise = vec![
        notice("success", "签到 · 完成", None, now - 1_000),
        notice("info", "已按限流自动切换账号", None, now - 2_000),
        notice("warning", "异常提醒", Some("新增 3 条异常提醒"), now - 3_000),
        notice("warning", "签到 · 已取消", None, now - 4_000),
    ];
    assert!(
        runfail_alert(&noise, now).is_none(),
        "成功 / 取消 / 提醒自身都不算失败"
    );

    let stale = vec![
        notice("warning", "签到 · 部分失败", None, now - DAY_MS - 1),
        notice("warning", "签到 · 部分失败", None, now - DAY_MS - 2),
        notice("warning", "签到 · 部分失败", None, now - DAY_MS - 3),
    ];
    assert!(runfail_alert(&stale, now).is_none(), "24h 之外不计数");
}
