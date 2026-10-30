//! 托盘读数：常驻状态下不打开窗口也能看到「还剩多少额度」。
//!
//! 只读本地积分快照 + 预测，**不发请求**：托盘 tooltip 会在刷新余额时顺手更新，
//! 不该为了一个 tooltip 触发一轮官方接口。
//!
//! 显示口径刻意做成一句话结论，而不是十几个账号的清单——tooltip 只能看一两秒。

use std::collections::HashMap;

use serde_json::Value;

use wb_switch_core::modules::{account, credit_usage, forecast};

/// 生成 tooltip 文本，例如：
/// `WorkBuddy · 14 个账号 · 合计 32,180 · 最紧 兮希汐熙惜 ≈1.2 天`
pub fn readout_text() -> String {
    let accounts = account::load_accounts();
    let snapshots = credit_usage::load_snapshots();
    let mut latest: HashMap<String, &Value> = HashMap::new();
    for snapshot in &snapshots {
        let Some(id) = snapshot.get("accountId").and_then(Value::as_str) else {
            continue;
        };
        let ts = snapshot.get("ts").and_then(Value::as_i64).unwrap_or(0);
        let newer = latest
            .get(id)
            .map(|old| ts > old.get("ts").and_then(Value::as_i64).unwrap_or(0))
            .unwrap_or(true);
        if newer {
            latest.insert(id.to_string(), snapshot);
        }
    }

    let mut total = 0.0_f64;
    let mut with_balance = 0usize;
    for acc in &accounts {
        let Some(id) = acc.get("id").and_then(Value::as_str) else {
            continue;
        };
        if let Some(snapshot) = latest.get(id) {
            total += snapshot.get("remaining").and_then(Value::as_f64).unwrap_or(0.0);
            with_balance += 1;
        }
    }

    let mut head = format!("WorkBuddy · {} 个账号", accounts.len());
    if with_balance > 0 {
        head.push_str(&format!(" · 合计 {}", group_thousands(total)));
    }

    // 「最紧」= 预测剩余天数最少的那个号（只算有预测的）。
    let forecasts = forecast::account_forecasts(None);
    let tightest = forecasts
        .get("accounts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| entry.get("confidence").and_then(Value::as_str) == Some("ok"))
        .filter_map(|entry| {
            let days = entry.get("daysLeft").and_then(Value::as_f64)?;
            let label = entry
                .get("accountName")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            Some((days, label))
        })
        .min_by(|left, right| left.0.partial_cmp(&right.0).unwrap_or(std::cmp::Ordering::Equal));

    match tightest {
        Some((days, label)) if !label.is_empty() => {
            head.push_str(&format!(" · 最紧 {label} ≈{days:.1} 天"));
        }
        _ => {
            // 预测还没攒够样本：如实说，不编一个天数出来。
            head.push_str(" · 预测样本不足");
        }
    }
    head
}

/// 千分位：1280 → 1,280。tooltip 里大数字不分组很难扫。
fn group_thousands(value: f64) -> String {
    let rounded = value.round() as i64;
    let digits = rounded.abs().to_string();
    let mut grouped = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    if rounded < 0 {
        format!("-{grouped}")
    } else {
        grouped
    }
}

/// 刷新托盘 tooltip。取不到托盘就静默跳过（托盘可能被系统禁用）。
pub fn refresh(app: &tauri::AppHandle) {
    let Some(tray) = app.tray_by_id("main-tray") else {
        return;
    };
    let text = readout_text();
    let _ = tray.set_tooltip(Some(&text));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_are_grouped() {
        assert_eq!(group_thousands(0.0), "0");
        assert_eq!(group_thousands(999.0), "999");
        assert_eq!(group_thousands(1280.0), "1,280");
        assert_eq!(group_thousands(1_234_567.0), "1,234,567");
        assert_eq!(group_thousands(-4200.0), "-4,200");
    }
}
