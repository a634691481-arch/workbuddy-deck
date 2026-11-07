//! 异常提醒中心：把分散在各处的「该让用户知道」的信号集中成一份带级别的清单。
//!
//! 五类信号（全部只读本地数据，不发网络请求）：
//! 1. `balance`    余额按预测 <3 天耗尽（critical）
//! 2. `health`     健康分 grade=critical（数据同账号卡片）
//! 3. `expiring`   积分包 3 天内到期（critical）/ 7 天内（warn）
//! 4. `switch`     24h 内切换失败 / 回滚（rolled_back=critical，blocked=warn）
//! 5. `runfail`    24h 内 ≥3 条失败事件（warn，聚合一条）
//!
//! 账号侧数据来自 `account_overview` 的**结果缓存**（alerts_overview_cache.json）：
//! 评估口径与界面完全一致，不另算一套。缓存由 `account_overview` 在返回前写入，
//! 评估只认行内的 `forecast`（daysLeft / confidence）与 `health`（score / grade / reasons）——
//! 旧缓存缺这些字段时对应信号不触发，绝不 panic。
//!
//! 去重：指纹（kind + 账号/维度）+ `alerts_state.json`（dismissed / seen）。
//! 忽略 7 天内有效——条件还在也不骚扰；7 天后同一指纹再次出现会重新提醒。
//!
//! 文件按职责拆开（AGENTS.md §2）：`cache` 账号缓存、`state` 忽略状态、
//! `signals` 五路信号、`eval` 编排过滤、`poller` 后台轮询。

mod cache;
mod eval;
mod poller;
mod signals;
mod state;

#[cfg(test)]
mod tmp;

use serde_json::{json, Value};
use wb_switch_core::modules::config::now_ms;
use wb_switch_core::modules::{notifications, switch_history};

pub use cache::cache_overview;
pub use poller::spawn_poller;

/// 一天毫秒数：所有时间窗口都用它换算。
const DAY_MS: i64 = 86_400_000;
/// 忽略后的静默窗口：7 天内同一指纹不再提醒。
const DISMISS_TTL_MS: i64 = 7 * DAY_MS;
/// 启动后延迟首评：等账号页把 `alerts_overview_cache.json` 写出来。
const STARTUP_DELAY_SECS: u64 = 20;
/// 后台评估间隔：10 分钟。账号数据本身就是十分钟级，再密只是空转。
const POLL_INTERVAL_SECS: u64 = 600;
/// 切换历史只翻最近 20 条：一台机器一天切不了几次，足够覆盖 24h。
const SWITCH_HISTORY_LIMIT: usize = 20;

/// 采集一次评估所需的全部本地输入。
fn gather() -> eval::Inputs {
    eval::Inputs {
        rows: cache::load(),
        switches: switch_history::load(SWITCH_HISTORY_LIMIT)
            .get("entries")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        notifications: notifications::list().unwrap_or_default(),
    }
}

/// 评估全部信号（不写状态）。返回按级别排序：critical 在前。
pub fn evaluate() -> Vec<Value> {
    let now = now_ms();
    eval::evaluate(&gather(), &state::load(), now)
}

/// 当前提醒清单（active）+ 已忽略条数。
pub fn list() -> Value {
    let now = now_ms();
    let state = state::load();
    let alerts = eval::evaluate(&gather(), &state, now);
    json!({
        "ok": true,
        "alerts": alerts,
        "dismissedCount": state::dismissed_count(&state, now),
        "evaluatedAt": now,
    })
}

pub fn dismiss(fingerprint: &str) -> Value {
    let mut state = state::load();
    state.dismissed.insert(fingerprint.to_string(), now_ms());
    state::save(&state);
    json!({ "ok": true })
}

pub fn dismiss_all() -> Value {
    let alerts = evaluate();
    let mut state = state::load();
    let now = now_ms();
    for alert in &alerts {
        if let Some(fingerprint) = alert.get("fingerprint").and_then(Value::as_str) {
            state.dismissed.insert(fingerprint.to_string(), now);
        }
    }
    state::save(&state);
    json!({ "ok": true, "dismissed": alerts.len() })
}
