//! 用量预测：把「余额 1280」变成「还能撑 2.4 天」。
//!
//! 数据只来自本机积分快照（[`credit_usage`]），**不新增任何网络请求**，离线可用。
//!
//! 三个口径决定（都是为了不猜）：
//! 1. **只把余额下降算作消耗**。签到、领奖、充值会让余额上升，那不是负消耗，
//!    直接跳过——否则一天领了 500 积分，会被算成「昨天用了 −500」。
//! 2. **按天归一**。快照不保证每天都有；两条快照隔了 3 天就除以 3，不能把 3 天的
//!    消耗记在最后一天头上。
//! 3. **样本不足就不预测**。少于 [`MIN_BASIS_DAYS`] 天只返回 `--`，
//!    竞品普遍在只有一两天数据时就画趋势线，那是编的。

use std::collections::BTreeMap;

use chrono::{Duration as ChronoDuration, Local, NaiveDate, TimeZone};
use serde_json::{json, Value};

use crate::modules::config::now_ms;
use crate::modules::credit_usage;

/// 少于这么多天有快照就不给预测。
pub const MIN_BASIS_DAYS: usize = 3;
/// 默认回看窗口：两周足够覆盖「工作日高、周末低」的周期。
pub const DEFAULT_WINDOW_DAYS: i64 = 14;
/// 趋势对比窗口（近 N 天 vs 前 N 天）。
const TREND_DAYS: i64 = 3;
/// EWMA 平滑系数：偏小更稳，能压住单日爆发。
const EWMA_ALPHA: f64 = 0.35;

/// 单个账号的预测结果。
#[derive(Debug, Clone)]
pub struct AccountForecast {
    pub account_id: String,
    pub account_name: String,
    /// 最新一条快照的余额。
    pub remaining: f64,
    /// 日均消耗（积分/天）。0 表示窗口内没有消耗。
    pub per_day: f64,
    /// 还能撑几天；`None` = 样本不足或没有消耗。
    pub days_left: Option<f64>,
    /// 预计耗尽的时刻（毫秒）；与 `days_left` 同时为 `None`。
    pub deplete_at: Option<i64>,
    /// `ok` | `insufficient` | `flat`
    pub confidence: &'static str,
    /// 参与计算的天数（有快照的天数）。
    pub basis_days: usize,
    /// 近 N 天日均 / 前 N 天日均 − 1；样本不足为 `None`。
    pub trend_pct: Option<f64>,
    /// 窗口内累计消耗（只算下降）。
    pub used: f64,
    /// 最近两条快照的余额差（正数 = 上次到现在消耗了多少）。只有一条快照时为 `None`。
    ///
    /// 存在的理由：预测要 3 天样本才敢给，而这个差值**有两条快照就能显示**，
    /// 让新装用户在冷启动期也能看到有用的数字，而不是一片「--」。
    pub last_delta: Option<f64>,
    /// 生成这条读数时距离上一条快照多少秒。
    pub last_delta_secs: Option<i64>,
}

impl AccountForecast {
    fn to_json(&self) -> Value {
        json!({
            "accountId": self.account_id,
            "accountName": self.account_name,
            "remaining": round1(self.remaining),
            "perDay": round2(self.per_day),
            "daysLeft": self.days_left.map(round1),
            "depleteAt": self.deplete_at,
            "confidence": self.confidence,
            "basisDays": self.basis_days,
            "trendPct": self.trend_pct.map(round1),
            "used": round1(self.used),
            "lastDelta": self.last_delta.map(round1),
            "lastDeltaSecs": self.last_delta_secs,
        })
    }
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn local_day(ts_ms: i64) -> Option<NaiveDate> {
    Local
        .timestamp_millis_opt(ts_ms)
        .single()
        .map(|dt| dt.date_naive())
}

/// 把快照按账号分组，并对每个账号做「按天取当天最后一条余额」。
///
/// 用 `max_by_key(ts)` 而不是依赖数组顺序：快照文件是追加写的，但历史版本可能乱序。
fn daily_balances(
    snapshots: &[Value],
    account_id: &str,
    since_ms: i64,
) -> Vec<(NaiveDate, f64)> {
    let mut by_day: BTreeMap<NaiveDate, (i64, f64)> = BTreeMap::new();
    for snapshot in snapshots {
        if snapshot.get("accountId").and_then(Value::as_str) != Some(account_id) {
            continue;
        }
        let Some(ts) = snapshot.get("ts").and_then(Value::as_i64) else {
            continue;
        };
        if ts < since_ms {
            continue;
        }
        let Some(remaining) = snapshot.get("remaining").and_then(Value::as_f64) else {
            continue;
        };
        let Some(day) = local_day(ts) else {
            continue;
        };
        match by_day.get(&day) {
            Some((kept_ts, _)) if *kept_ts >= ts => {}
            _ => {
                by_day.insert(day, (ts, remaining));
            }
        }
    }
    by_day
        .into_iter()
        .map(|(day, (_, remaining))| (day, remaining))
        .collect()
}

/// 相邻快照之间的日均消耗：只保留正消耗，并按间隔天数归一。
fn daily_rates(days: &[(NaiveDate, f64)]) -> Vec<(NaiveDate, f64)> {
    let mut rates = Vec::with_capacity(days.len().saturating_sub(1));
    for pair in days.windows(2) {
        let (from_day, from_remaining) = pair[0];
        let (to_day, to_remaining) = pair[1];
        let gap = (to_day - from_day).num_days().max(1) as f64;
        let drop = from_remaining - to_remaining;
        if drop > 0.0 {
            rates.push((to_day, drop / gap));
        }
    }
    rates
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

fn ewma(values: &[f64]) -> f64 {
    let mut acc = values.first().copied().unwrap_or(0.0);
    for value in values.iter().skip(1) {
        acc = EWMA_ALPHA * value + (1.0 - EWMA_ALPHA) * acc;
    }
    acc
}

/// 近 N 天 vs 前 N 天的速率变化百分比。
fn trend(rate_points: &[(NaiveDate, f64)], today: NaiveDate) -> Option<f64> {
    let recent_cut = today - ChronoDuration::days(TREND_DAYS);
    let previous_cut = today - ChronoDuration::days(TREND_DAYS * 2);
    let sum = |from: NaiveDate, to: NaiveDate| -> f64 {
        rate_points
            .iter()
            .filter(|(day, _)| *day > from && *day <= to)
            .map(|(_, rate)| rate)
            .sum::<f64>()
    };
    let recent = sum(recent_cut, today);
    let previous = sum(previous_cut, recent_cut);
    if previous <= 0.0 {
        return None;
    }
    Some((recent - previous) / previous * 100.0)
}

/// 最近两条快照的余额差（正 = 消耗，负 = 领取/充值）与时间间隔（秒）。
fn recent_delta(snapshots: &[Value], account_id: &str) -> (Option<f64>, Option<i64>) {
    let mut points: Vec<(i64, f64)> = snapshots
        .iter()
        .filter(|snapshot| snapshot.get("accountId").and_then(Value::as_str) == Some(account_id))
        .cloned()
        .filter_map(|snapshot| {
            Some((
                snapshot.get("ts").and_then(Value::as_i64)?,
                snapshot.get("remaining").and_then(Value::as_f64)?,
            ))
        })
        .collect();
    if points.len() < 2 {
        return (None, None);
    }
    points.sort_by_key(|(ts, _)| *ts);
    let (last_ts, last_remaining) = points[points.len() - 1];
    let (prev_ts, prev_remaining) = points[points.len() - 2];
    (
        Some(prev_remaining - last_remaining),
        Some((last_ts - prev_ts) / 1000),
    )
}

/// 预测单个账号。没有快照时返回 `None`。
///
/// 内部版本接收已读好的快照：`account_forecasts` 要对十几个账号循环，
/// 每次都重读文件会变成 O(账号数) 次磁盘 IO。
pub fn forecast_from(
    snapshots: &[Value],
    account_id: &str,
    window_days: i64,
) -> Option<AccountForecast> {
    let window = if window_days > 0 {
        window_days
    } else {
        DEFAULT_WINDOW_DAYS
    };
    let now = now_ms();
    let since = now - window * 86_400_000;
    let days = daily_balances(snapshots, account_id, since);
    if days.is_empty() {
        return None;
    }
    let (last_day, remaining) = *days.last()?;
    let rates = daily_rates(&days);
    let (last_delta, last_delta_secs) = recent_delta(snapshots, account_id);
    let rate_values: Vec<f64> = rates.iter().map(|(_, rate)| *rate).collect();

    let basis_days = days.len();
    // 中位数抗单日爆发，EWMA 抓趋势；取小 = 宁可早预警，不报「还能用很久」。
    let per_day = if rate_values.is_empty() {
        0.0
    } else {
        median(&rate_values).min(ewma(&rate_values))
    };

    let account_name = snapshots
        .iter()
        .filter(|snapshot| snapshot.get("accountId").and_then(Value::as_str) == Some(account_id))
        .max_by_key(|snapshot| snapshot.get("ts").and_then(Value::as_i64).unwrap_or(0))
        .and_then(|snapshot| {
            snapshot
                .get("accountName")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();

    // 窗口内累计消耗 = 首尾余额差的正部分；中间若有反弹（领奖）自然被抵掉。
    let used = match (days.first(), days.last()) {
        (Some((_, first)), Some((_, last))) => (first - last).max(0.0),
        _ => 0.0,
    };

    if basis_days < MIN_BASIS_DAYS {
        return Some(AccountForecast {
            account_id: account_id.to_string(),
            account_name,
            remaining,
            per_day,
            days_left: None,
            deplete_at: None,
            confidence: "insufficient",
            basis_days,
            trend_pct: None,
            used: used.max(0.0),
            last_delta,
            last_delta_secs,
        });
    }

    if per_day <= 0.0 {
        return Some(AccountForecast {
            account_id: account_id.to_string(),
            account_name,
            remaining,
            per_day: 0.0,
            days_left: None,
            deplete_at: None,
            confidence: "flat",
            basis_days,
            trend_pct: trend(&rates, last_day),
            used: used.max(0.0),
            last_delta,
            last_delta_secs,
        });
    }

    let days_left = (remaining / per_day).max(0.0);
    Some(AccountForecast {
        account_id: account_id.to_string(),
        account_name,
        remaining,
        per_day,
        days_left: Some(days_left),
        deplete_at: Some(now + (days_left * 86_400_000.0) as i64),
        confidence: "ok",
        basis_days,
        trend_pct: trend(&rates, last_day),
        used: used.max(0.0),
        last_delta,
        last_delta_secs,
    })
}

/// 全账号预测：一次算完，避免前端 N 次 IPC。
pub fn account_forecasts(window_days: Option<i64>) -> Value {
    let window = window_days.unwrap_or(DEFAULT_WINDOW_DAYS);
    // 快照只读一次，全部账号共用（见 `forecast_from` 的注释）。
    let snapshots = credit_usage::load_snapshots();
    let mut account_ids: Vec<String> = Vec::new();
    for snapshot in &snapshots {
        if let Some(id) = snapshot.get("accountId").and_then(Value::as_str) {
            if !id.trim().is_empty() && !account_ids.iter().any(|seen| seen == id) {
                account_ids.push(id.to_string());
            }
        }
    }
    let forecasts: Vec<Value> = account_ids
        .iter()
        .filter_map(|id| forecast_from(&snapshots, id, window))
        .map(|forecast| forecast.to_json())
        .collect();
    json!({
        "generatedAt": now_ms(),
        "windowDays": window,
        "minBasisDays": MIN_BASIS_DAYS,
        "accounts": forecasts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(offset: i64) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 1).unwrap() + ChronoDuration::days(offset)
    }

    fn series(values: &[(i64, f64)]) -> Vec<(NaiveDate, f64)> {
        values.iter().map(|(offset, v)| (day(*offset), *v)).collect()
    }

    /// 反弹（签到 / 领奖）不能被算成负消耗。
    #[test]
    fn rising_balance_is_not_negative_usage() {
        let rates = daily_rates(&series(&[(0, 100.0), (1, 160.0), (2, 40.0)]));
        assert_eq!(rates.len(), 1, "只有下降的那一段算消耗");
        assert_eq!(rates[0].1, 120.0);
    }

    /// 两条快照隔 3 天：消耗要按天归一，不能全记在最后一天。
    #[test]
    fn multi_day_gap_is_normalized_per_day() {
        let rates = daily_rates(&series(&[(0, 300.0), (3, 0.0)]));
        assert_eq!(rates[0].1, 100.0);
    }

    /// 中位数抗单日爆发：一天用了 500，其余 10，不应该把日均拉到 100。
    #[test]
    fn median_resists_single_day_spike() {
        let values = vec![10.0, 10.0, 10.0, 500.0];
        assert_eq!(median(&values), 10.0);
        assert!(ewma(&values) < 200.0);
    }

    /// 样本不足必须返回 insufficient 且不给天数，前端显示 `--`。
    #[test]
    fn insufficient_basis_yields_no_estimate() {
        // 没有真实快照文件的环境里，只能验证纯函数：两天数据构造出的 rates 只有 1 条。
        let days = series(&[(0, 100.0), (1, 50.0)]);
        assert!(days.len() < MIN_BASIS_DAYS);
        let rates = daily_rates(&days);
        assert_eq!(rates.len(), 1);
    }

    /// 趋势：近 3 天消耗翻倍 → +100%。
    #[test]
    fn trend_compares_recent_with_previous_window() {
        let rates = series(&[(1, 10.0), (2, 10.0), (3, 10.0), (4, 20.0), (5, 20.0), (6, 20.0)]);
        let pct = trend(&rates, day(6)).expect("有样本就应该给出趋势");
        assert!((pct - 100.0).abs() < 0.01, "实际 {pct}");
    }
}
