//! 限流自动切换：当前账号被官方限流时，换到一个还能用的账号。
//!
//! 与 [`crate::modules::rotate`] 的分工必须讲清楚，两者不重叠：
//! - `rotate` 是**主动**轮换，按「积分到期紧迫度」决策，需要拉官方接口（秒级不敏感）；
//! - 这里是**被动**响应，按「限流事件」决策，**只用本地台账，不发任何请求**——
//!   被限流时用户正在等结果，多等一轮 8 个账号的接口是无法接受的。
//!
//! 三条安全默认，缺一不可：
//! 1. 默认**关闭**（`enabled=false`）；
//! 2. 默认**只提示不执行**（`notify_only=true`）——自动切换会关掉 WorkBuddy，
//!    等于打断用户正在做的事，得让他先看到「它想切，切到哪」；
//! 3. 冷却期默认 30 分钟，避免多个模型连续限流触发连环切换。

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::modules::account;
use crate::modules::auth_file;
use crate::modules::config::{atomic_write, now_ms, store_dir};
use crate::modules::credit_usage;
use crate::modules::limits;
use crate::modules::notifications;
use crate::modules::variant::WbVariant;

const CONFIG_FILE: &str = "auto_switch_config.json";
const LOGS_FILE: &str = "auto_switch_logs.json";
const STATE_FILE: &str = "auto_switch_state.json";
/// 日志上限：500 条 / 30 天（与 auto_daily 一致）。
const LOG_LIMIT: usize = 500;
const LOG_RETENTION_DAYS: i64 = 30;

/// 自动切换配置。字段全部有默认值，缺字段的旧配置能直接读。
#[derive(Debug, Clone)]
pub struct AutoSwitchConfig {
    /// 总开关，默认关闭。
    pub enabled: bool,
    /// 只提示不执行，默认开启。
    pub notify_only: bool,
    /// 冷却期（分钟）。
    pub cooldown_minutes: i64,
    /// 永不作为目标的账号。
    pub excluded_account_ids: Vec<String>,
    /// 目标账号至少要剩这么多积分，否则不值得切。
    pub min_remaining: f64,
    /// 只切到同档位（cn / ai）账号：跨档位切换会换数据目录，代价大得多。
    pub require_same_variant: bool,
}

impl Default for AutoSwitchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            notify_only: true,
            cooldown_minutes: 30,
            excluded_account_ids: Vec::new(),
            min_remaining: 50.0,
            require_same_variant: true,
        }
    }
}

impl AutoSwitchConfig {
    fn from_value(value: &Value) -> Self {
        let base = Self::default();
        let bool_of = |key: &str, fallback: bool| {
            value.get(key).and_then(Value::as_bool).unwrap_or(fallback)
        };
        Self {
            enabled: bool_of("enabled", base.enabled),
            notify_only: bool_of("notifyOnly", base.notify_only),
            cooldown_minutes: value
                .get("cooldownMinutes")
                .and_then(Value::as_i64)
                .unwrap_or(base.cooldown_minutes)
                .clamp(1, 24 * 60),
            excluded_account_ids: value
                .get("excludedAccountIds")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            min_remaining: value
                .get("minRemaining")
                .and_then(Value::as_f64)
                .unwrap_or(base.min_remaining)
                .max(0.0),
            require_same_variant: bool_of("requireSameVariant", base.require_same_variant),
        }
    }

    fn to_value(&self) -> Value {
        json!({
            "enabled": self.enabled,
            "notifyOnly": self.notify_only,
            "cooldownMinutes": self.cooldown_minutes,
            "excludedAccountIds": self.excluded_account_ids,
            "minRemaining": self.min_remaining,
            "requireSameVariant": self.require_same_variant,
        })
    }

    pub fn is_excluded(&self, account_id: &str) -> bool {
        self.excluded_account_ids
            .iter()
            .any(|id| id == account_id)
    }
}

fn config_path() -> PathBuf {
    store_dir().join(CONFIG_FILE)
}

fn logs_path() -> PathBuf {
    store_dir().join(LOGS_FILE)
}

fn state_path() -> PathBuf {
    store_dir().join(STATE_FILE)
}

/// 读配置。文件缺失或损坏 → 默认值（默认关闭，所以损坏不会导致意外切换）。
pub fn load_config() -> AutoSwitchConfig {
    let Ok(text) = std::fs::read_to_string(config_path()) else {
        return AutoSwitchConfig::default();
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(value) if value.is_object() => AutoSwitchConfig::from_value(&value),
        _ => AutoSwitchConfig::default(),
    }
}

pub fn save_config(cfg: &AutoSwitchConfig) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(&cfg.to_value()).map_err(|error| error.to_string())?;
    atomic_write(&path, &text).map_err(|error| error.to_string())
}

/// 状态文件里「正在切换」标记的有效期：超过它就当上次进程已死，允许再试。
const IN_FLIGHT_TTL_MS: i64 = 10 * 60 * 1000;

fn read_state() -> Value {
    std::fs::read_to_string(state_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({}))
}

fn write_state(value: &Value) {
    let _ = atomic_write(&state_path(), &value.to_string());
}

/// 上一次切换的时刻（冷却期用）。读不出来就当没有。
fn last_switch_at() -> Option<i64> {
    read_state().get("lastSwitchAt").and_then(Value::as_i64)
}

fn set_last_switch_at(ts: i64) {
    let mut state = read_state();
    state["lastSwitchAt"] = json!(ts);
    write_state(&state);
}

/// 抢占「本次切换」的名额：多个入口（1 秒快路径 / 60 秒兜底 / 2 分钟巡检）
/// 可能同时判定要切，必须只有一个能真正动手——否则 16 秒内会连切两次同一目标。
///
/// 只写 `inFlightAt`，不碰 `lastSwitchAt`：冷却语义不能被这次占位提前消耗，
/// 决定是否切换的 `decide()` 仍按真实的上次成功时刻算。
pub fn try_begin_switch(now: i64) -> bool {
    let mut state = read_state();
    if let Some(at) = state.get("inFlightAt").and_then(Value::as_i64) {
        if at > 0 && now - at < IN_FLIGHT_TTL_MS {
            return false;
        }
    }
    state["inFlightAt"] = json!(now);
    write_state(&state);
    true
}

/// 收尾：清掉占位；成功才写冷却时刻（失败/取消不该白吃一次 30 分钟冷却）。
pub fn end_switch(ok: bool, now: i64) {
    let mut state = read_state();
    state["inFlightAt"] = json!(0);
    if ok {
        state["lastSwitchAt"] = json!(now);
    }
    write_state(&state);
}

/// 候选账号（只用本地快照与台账，不发请求）。
#[derive(Debug, Clone)]
pub struct Candidate {
    pub account_id: String,
    pub label: String,
    pub variant: WbVariant,
    /// 本地最新余额；没有快照为 None。
    pub remaining: Option<f64>,
    /// 该账号仍在生效的限流里最晚的恢复时刻；没有限流为 None。
    pub limited_until: Option<i64>,
}

impl Candidate {
    fn is_limited(&self, now: i64) -> bool {
        self.limited_until.map(|at| at > now).unwrap_or(false)
    }
}

/// 决策结果。`Skip` 不是错误——绝大多数轮次都应该是 Skip。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Skip(String),
    /// 有目标，但配置是「只提示」。
    Notify { target_id: String, reason: String },
    /// 有目标且允许执行。
    Switch { target_id: String, reason: String },
}

/// 纯函数决策：所有输入都由调用方给，便于单测覆盖每条分支。
///
/// 判定顺序是有讲究的，不要随意调换：
/// 1. 开关 → 2. 当前账号是否真的被限流（这是唯一触发条件）→ 3. 冷却 →
/// 4. 候选筛选 → 5. 模式。
pub fn decide(
    cfg: &AutoSwitchConfig,
    candidates: &[Candidate],
    current_account_id: Option<&str>,
    last_switch_at_ms: Option<i64>,
    now: i64,
) -> Decision {
    if !cfg.enabled {
        return Decision::Skip("自动切换未启用".to_string());
    }
    let Some(current_id) = current_account_id.filter(|id| !id.trim().is_empty()) else {
        // 不知道当前在用哪个号时切换＝瞎切：可能把用户刚切好的号又换掉。
        return Decision::Skip("无法判定当前 WorkBuddy 账号".to_string());
    };
    let Some(current) = candidates.iter().find(|item| item.account_id == current_id) else {
        return Decision::Skip("当前账号不在账号库中".to_string());
    };
    // 唯一触发条件：当前账号确实被限流。
    if !current.is_limited(now) {
        return Decision::Skip("当前账号未被限流".to_string());
    }
    if let Some(last) = last_switch_at_ms {
        let cooldown_ms = cfg.cooldown_minutes * 60_000;
        if last + cooldown_ms > now {
            let left = (last + cooldown_ms - now) / 60_000 + 1;
            return Decision::Skip(format!("冷却中（还需 {left} 分钟）"));
        }
    }

    let mut usable: Vec<&Candidate> = candidates
        .iter()
        .filter(|item| item.account_id != current_id)
        .filter(|item| !cfg.is_excluded(&item.account_id))
        .filter(|item| !item.is_limited(now))
        .filter(|item| {
            !cfg.require_same_variant || item.variant.as_str() == current.variant.as_str()
        })
        .filter(|item| item.remaining.map(|value| value >= cfg.min_remaining).unwrap_or(false))
        .collect();

    if usable.is_empty() {
        return Decision::Skip("没有可用账号（同档位、未限流、余额达标）".to_string());
    }
    // 余额降序：切过去要能撑得久，而不是刚切完又要再切。
    usable.sort_by(|left, right| {
        right
            .remaining
            .unwrap_or(0.0)
            .partial_cmp(&left.remaining.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let target = usable[0];
    let reason = format!(
        "{} 限流中，切到 {}（余额 {:.0}）",
        current.label,
        target.label,
        target.remaining.unwrap_or(0.0)
    );
    if cfg.notify_only {
        Decision::Notify {
            target_id: target.account_id.clone(),
            reason,
        }
    } else {
        Decision::Switch {
            target_id: target.account_id.clone(),
            reason,
        }
    }
}

/// 当前 WorkBuddy 登录账号在账号库里的 id。
///
/// 判定与 `workbuddy_status` 同口径（uid 优先、退回 id）。取不到就返回 None，
/// 调用方据此跳过——**宁可不动，也不能瞎切**。
pub fn current_account_id() -> Option<String> {
    let accounts = account::load_accounts();
    for variant in WbVariant::ALL {
        let Some(identity) = auth_file::auth_identity_at(&auth_file::auth_file_path(variant)) else {
            continue;
        };
        let hit = accounts.iter().find(|acc| {
            let uid = account::get_str(acc, "uid").unwrap_or_default();
            let id = account::get_str(acc, "id").unwrap_or_default();
            uid.trim() == identity || id == identity
        });
        if let Some(acc) = hit {
            return account::get_str(acc, "id");
        }
    }
    None
}

/// 从本机数据组装候选列表。
pub fn candidates(now: i64) -> Vec<Candidate> {
    let accounts = account::load_accounts();
    // 每个账号的最新余额。
    let mut latest: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for snapshot in credit_usage::load_snapshots() {
        let Some(id) = snapshot.get("accountId").and_then(Value::as_str) else {
            continue;
        };
        let Some(remaining) = snapshot.get("remaining").and_then(Value::as_f64) else {
            continue;
        };
        // 快照按写入顺序追加：后写覆盖前写，取到的就是最新余额。
        latest.insert(id.to_string(), remaining);
    }
    // 每个账号仍在生效的限流里最晚恢复时刻。
    let mut limited_until: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    if let Some(rows) = limits::get_rate_limits().get("accounts").and_then(Value::as_array) {
        for row in rows {
            let Some(id) = row.get("accountId").and_then(Value::as_str) else {
                continue;
            };
            for item in row.get("limited").and_then(Value::as_array).into_iter().flatten() {
                let Some(reset_at) = item.get("resetAt").and_then(Value::as_i64) else {
                    continue;
                };
                if reset_at <= now {
                    continue;
                }
                let entry = limited_until.entry(id.to_string()).or_insert(reset_at);
                if reset_at > *entry {
                    *entry = reset_at;
                }
            }
        }
    }

    accounts
        .iter()
        .filter_map(|acc| {
            let id = acc.get("id").and_then(Value::as_str)?.to_string();
            Some(Candidate {
                label: account::account_display_name(acc),
                variant: account::variant_of(acc),
                remaining: latest.get(&id).copied(),
                limited_until: limited_until.get(&id).copied(),
                account_id: id,
            })
        })
        .collect()
}

/// 执行一轮判定。`apply_switch` 由宿主注入，避免 core 直接依赖重启逻辑。
///
/// 返回结构直接可给前端/日志用：`{ action, reason, targetId, error }`。
pub fn run_once<F>(
    cfg: &AutoSwitchConfig,
    current_account_id: Option<&str>,
    now: i64,
    apply_switch: F,
) -> Value
where
    F: FnOnce(&str) -> Result<(), String>,
{
    let candidates = candidates(now);
    let decision = decide(cfg, &candidates, current_account_id, last_switch_at(), now);
    let payload = match decision {
        Decision::Skip(reason) => json!({ "action": "skip", "reason": reason }),
        Decision::Notify { target_id, reason } => {
            let label = candidates
                .iter()
                .find(|item| item.account_id == target_id)
                .map(|item| item.label.clone())
                .unwrap_or_default();
            let _ = notifications::record(
                "warning",
                "限流：建议切换账号",
                Some(&format!("{reason}（仅提示模式，未自动切换）")),
            );
            json!({
                "action": "notify",
                "reason": reason,
                "targetId": target_id,
                "targetLabel": label,
            })
        }
        Decision::Switch { target_id, reason } => {
            let label = candidates
                .iter()
                .find(|item| item.account_id == target_id)
                .map(|item| item.label.clone())
                .unwrap_or_default();
            match apply_switch(&target_id) {
                Ok(()) => {
                    set_last_switch_at(now);
                    let _ = notifications::record(
                        "info",
                        "已按限流自动切换账号",
                        Some(reason.as_str()),
                    );
                    json!({
                        "action": "switch",
                        "reason": reason,
                        "targetId": target_id,
                        "targetLabel": label,
                        "ok": true,
                    })
                }
                Err(error) => json!({
                    "action": "switch",
                    "reason": reason,
                    "targetId": target_id,
                    "targetLabel": label,
                    "ok": false,
                    "error": error,
                }),
            }
        }
    };
    append_log(&payload, now);
    payload
}

fn append_log(payload: &Value, now: i64) {
    // 只记「做了什么」与「为什么没做」，跳过每次空转的 skip？——不，skip 正是
    // 用户排查「为什么没自动切」的唯一线索，必须留。但用滚动上限控制体积。
    let store = load_logs_value();
    let mut items = store
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut entry = payload.clone();
    entry["ts"] = json!(now);
    items.insert(0, entry);
    let cutoff = now - LOG_RETENTION_DAYS * 86_400_000;
    items.retain(|item| item.get("ts").and_then(Value::as_i64).unwrap_or(0) >= cutoff);
    if items.len() > LOG_LIMIT {
        items.truncate(LOG_LIMIT);
    }
    let text = json!({ "version": 1, "items": items }).to_string();
    let _ = atomic_write(&logs_path(), &text);
}

fn load_logs_value() -> Value {
    let Ok(text) = std::fs::read_to_string(logs_path()) else {
        return json!({ "version": 1, "items": [] });
    };
    serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({ "version": 1, "items": [] }))
}

/// 读取日志（新→旧，`limit=0` 返回全部）。
pub fn logs(limit: usize) -> Value {
    let store = load_logs_value();
    let items = store
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let total = items.len();
    let take = if limit == 0 { total } else { limit.min(total) };
    json!({
        "total": total,
        "items": items.into_iter().take(take).collect::<Vec<_>>(),
    })
}

pub fn clear_logs() -> Result<usize, String> {
    let store = load_logs_value();
    let removed = store
        .get("items")
        .and_then(Value::as_array)
        .map(|items| items.len())
        .unwrap_or(0);
    let _ = atomic_write(&logs_path(), &json!({ "version": 1, "items": [] }).to_string());
    Ok(removed)
}

/// 配置 + 日志摘要，给设置页一次拿全。
pub fn status() -> Value {
    let cfg = load_config();
    let store = load_logs_value();
    let items = store.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
    json!({
        "config": cfg.to_value(),
        "lastSwitchAt": last_switch_at(),
        "lastDecision": items.first().cloned(),
        "totalEvents": items.len(),
    })
}

/// 配置保存入口（宿主用）。
pub fn update_config(patch: &Value) -> Result<Value, String> {
    let mut cfg = load_config();
    if let Some(value) = patch.get("enabled").and_then(Value::as_bool) {
        cfg.enabled = value;
    }
    if let Some(value) = patch.get("notifyOnly").and_then(Value::as_bool) {
        cfg.notify_only = value;
    }
    if let Some(value) = patch.get("cooldownMinutes").and_then(Value::as_i64) {
        cfg.cooldown_minutes = value.clamp(1, 24 * 60);
    }
    if let Some(value) = patch.get("minRemaining").and_then(Value::as_f64) {
        cfg.min_remaining = value.max(0.0);
    }
    if let Some(value) = patch.get("requireSameVariant").and_then(Value::as_bool) {
        cfg.require_same_variant = value;
    }
    if let Some(items) = patch.get("excludedAccountIds").and_then(Value::as_array) {
        cfg.excluded_account_ids = items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
    }
    save_config(&cfg)?;
    Ok(status())
}

/// 供宿主判断「是否需要提示用户」：仅提示模式下有待切换目标时为 true。
pub fn pending_advice() -> Option<Value> {
    let cfg = load_config();
    if !cfg.enabled || !cfg.notify_only {
        return None;
    }
    let now = now_ms();
    let candidates = candidates(now);
    let current = current_account_id();
    match decide(&cfg, &candidates, current.as_deref(), last_switch_at(), now) {
        Decision::Notify { target_id, reason } => {
            let label = candidates
                .iter()
                .find(|item| item.account_id == target_id)
                .map(|item| item.label.clone())
                .unwrap_or_default();
            Some(json!({ "targetId": target_id, "targetLabel": label, "reason": reason }))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;
    const MIN: i64 = 60_000;

    fn candidate(id: &str, remaining: f64, limited_for: Option<i64>, variant: WbVariant) -> Candidate {
        Candidate {
            account_id: id.to_string(),
            label: id.to_string(),
            variant,
            remaining: Some(remaining),
            limited_until: limited_for.map(|minutes| NOW + minutes * MIN),
        }
    }

    fn enabled() -> AutoSwitchConfig {
        AutoSwitchConfig {
            enabled: true,
            notify_only: false,
            cooldown_minutes: 30,
            excluded_account_ids: Vec::new(),
            min_remaining: 50.0,
            require_same_variant: true,
        }
    }

    /// 未启用时任何情况都不动。
    #[test]
    fn disabled_never_acts() {
        let mut cfg = enabled();
        cfg.enabled = false;
        let items = vec![candidate("a", 100.0, Some(60), WbVariant::Cn)];
        assert!(matches!(
            decide(&cfg, &items, Some("a"), None, NOW),
            Decision::Skip(_)
        ));
    }

    /// 当前账号没被限流 → 不切（这是唯一触发条件）。
    #[test]
    fn no_limit_means_no_action() {
        let cfg = enabled();
        let items = vec![
            candidate("a", 100.0, None, WbVariant::Cn),
            candidate("b", 900.0, None, WbVariant::Cn),
        ];
        assert!(matches!(
            decide(&cfg, &items, Some("a"), None, NOW),
            Decision::Skip(_)
        ));
    }

    /// 限流中且有余量账号 → 切到余额最大的那个。
    #[test]
    fn picks_richest_healthy_target() {
        let cfg = enabled();
        let items = vec![
            candidate("a", 100.0, Some(60), WbVariant::Cn),
            candidate("b", 300.0, None, WbVariant::Cn),
            candidate("c", 900.0, None, WbVariant::Cn),
        ];
        assert_eq!(
            decide(&cfg, &items, Some("a"), None, NOW),
            Decision::Switch {
                target_id: "c".to_string(),
                reason: "a 限流中，切到 c（余额 900）".to_string()
            }
        );
    }

    /// 只提示模式：给出目标但不执行。
    #[test]
    fn notify_only_never_switches() {
        let mut cfg = enabled();
        cfg.notify_only = true;
        let items = vec![
            candidate("a", 100.0, Some(60), WbVariant::Cn),
            candidate("b", 900.0, None, WbVariant::Cn),
        ];
        assert!(matches!(
            decide(&cfg, &items, Some("a"), None, NOW),
            Decision::Notify { .. }
        ));
    }

    /// 目标同样在被限流 → 不能切过去（切了照样不能用）。
    #[test]
    fn skips_limited_targets() {
        let cfg = enabled();
        let items = vec![
            candidate("a", 100.0, Some(60), WbVariant::Cn),
            candidate("b", 900.0, Some(120), WbVariant::Cn),
        ];
        assert!(matches!(
            decide(&cfg, &items, Some("a"), None, NOW),
            Decision::Skip(_)
        ));
    }

    /// 跨档位不切：换数据目录的代价远大于限流。
    #[test]
    fn requires_same_variant_by_default() {
        let cfg = enabled();
        let items = vec![
            candidate("a", 100.0, Some(60), WbVariant::Cn),
            candidate("b", 900.0, None, WbVariant::Ai),
        ];
        assert!(matches!(
            decide(&cfg, &items, Some("a"), None, NOW),
            Decision::Skip(_)
        ));
    }

    /// 余额低于阈值的账号不值得作为目标。
    #[test]
    fn ignores_low_balance_targets() {
        let cfg = enabled();
        let items = vec![
            candidate("a", 100.0, Some(60), WbVariant::Cn),
            candidate("b", 10.0, None, WbVariant::Cn),
        ];
        assert!(matches!(
            decide(&cfg, &items, Some("a"), None, NOW),
            Decision::Skip(_)
        ));
    }

    /// 排除列表生效。
    #[test]
    fn excluded_accounts_are_never_targets() {
        let mut cfg = enabled();
        cfg.excluded_account_ids = vec!["b".to_string()];
        let items = vec![
            candidate("a", 100.0, Some(60), WbVariant::Cn),
            candidate("b", 900.0, None, WbVariant::Cn),
        ];
        assert!(matches!(
            decide(&cfg, &items, Some("a"), None, NOW),
            Decision::Skip(_)
        ));
    }

    /// 冷却期内不重复切换。
    #[test]
    fn cooldown_blocks_repeat_switches() {
        let cfg = enabled();
        let items = vec![
            candidate("a", 100.0, Some(60), WbVariant::Cn),
            candidate("b", 900.0, None, WbVariant::Cn),
        ];
        assert!(matches!(
            decide(&cfg, &items, Some("a"), Some(NOW - 5 * MIN), NOW),
            Decision::Skip(_)
        ));
        // 冷却期已过 → 允许
        assert!(matches!(
            decide(&cfg, &items, Some("a"), Some(NOW - 31 * MIN), NOW),
            Decision::Switch { .. }
        ));
    }

    /// 不知道当前账号时绝不切：可能把用户刚切好的号又换掉。
    #[test]
    fn unknown_current_account_is_skipped() {
        let cfg = enabled();
        let items = vec![
            candidate("a", 100.0, Some(60), WbVariant::Cn),
            candidate("b", 900.0, None, WbVariant::Cn),
        ];
        assert!(matches!(decide(&cfg, &items, None, None, NOW), Decision::Skip(_)));
        assert!(matches!(
            decide(&cfg, &items, Some("   "), None, NOW),
            Decision::Skip(_)
        ));
    }

    /// 旧配置文件缺字段时要按默认值补齐（尤其 notify_only 必须是 true）。
    #[test]
    fn partial_config_falls_back_to_safe_defaults() {
        let cfg = AutoSwitchConfig::from_value(&json!({ "enabled": true }));
        assert!(cfg.notify_only, "缺 notifyOnly 时必须默认只提示");
        assert_eq!(cfg.cooldown_minutes, 30);
        assert_eq!(cfg.min_remaining, 50.0);
    }
}
