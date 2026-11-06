//! 异常提醒中心：把分散在各处的「该让用户知道」的信号集中成一份带级别的清单。
//!
//! 五类信号（全部只读本地数据，不发网络请求）：
//! 1. `balance`    余额按预测 <3 天耗尽（critical）
//! 2. `health`     健康分 critical（数据同账号卡片）
//! 3. `expiring`   积分包 3 天内到期（critical）/ 7 天内（warn）
//! 4. `switch`     24h 内切换失败 / 回滚（rolled_back=critical，blocked=warn）
//! 5. `runfail`    24h 内 ≥3 条错误通知（warn，聚合一条）
//!
//! 账号侧数据来自 `account_overview` 的**结果缓存**（alerts_overview_cache.json）：
//! 评估口径与界面完全一致，不另算一套。缓存由 `account_overview` 在返回前写入。
//!
//! 去重：指纹（kind + 账号/维度）+ `alerts_state.json`（dismissed / seen）。
//! 忽略 7 天内有效——条件还在也不骚扰；7 天后同一指纹再次出现会重新提醒。

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::Emitter;

use wb_switch_core::modules::config::{atomic_write, now_ms, store_dir};
use wb_switch_core::modules::{notifications, switch_history};

const STATE_VERSION: u32 = 1;
const STATE_FILE: &str = "alerts_state.json";
const OVERVIEW_CACHE_FILE: &str = "alerts_overview_cache.json";
const DAY_MS: i64 = 86_400_000;
/// 忽略后的静默窗口：7 天内同一指纹不再提醒。
const DISMISS_TTL_MS: i64 = 7 * DAY_MS;
/// 错误通知聚合阈值：24h 内达到该数量才报「连续失败」。
const RUNFAIL_THRESHOLD: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlertsState {
    version: u32,
    #[serde(default)]
    dismissed: HashMap<String, i64>,
    #[serde(default)]
    seen: HashMap<String, i64>,
}

impl Default for AlertsState {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            dismissed: HashMap::new(),
            seen: HashMap::new(),
        }
    }
}

fn state_path() -> PathBuf {
    store_dir().join(STATE_FILE)
}

fn load_state() -> AlertsState {
    std::fs::read_to_string(state_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// 存档写失败不影响提醒本身（与通知存档同口径：尽力而为）。
fn save_state(state: &AlertsState) {
    if let Ok(text) = serde_json::to_string_pretty(state) {
        let dir = store_dir();
        let _ = std::fs::create_dir_all(&dir);
        let _ = atomic_write(&state_path(), &text);
    }
}

/// `account_overview` 返回前调用：缓存一份账号行（含 health / resources）。
/// 写失败静默——提醒中心拿不到到期数据只是少一类提醒，不能拖垮账号页。
pub fn cache_overview(rows: &[Value], collected_at: i64) {
    let dir = store_dir();
    let _ = std::fs::create_dir_all(&dir);
    let payload = json!({ "ts": collected_at, "accounts": rows });
    if let Ok(text) = serde_json::to_string(&payload) {
        let _ = atomic_write(&dir.join(OVERVIEW_CACHE_FILE), &text);
    }
}

fn load_overview_cache() -> Vec<Value> {
    std::fs::read_to_string(store_dir().join(OVERVIEW_CACHE_FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|payload| {
            payload
                .get("accounts")
                .and_then(Value::as_array)
                .cloned()
        })
        .unwrap_or_default()
}

/// 评估全部信号（不写状态）。返回按级别排序：critical 在前。
pub fn evaluate() -> Vec<Value> {
    let now = now_ms();
    let mut alerts: Vec<Value> = Vec::new();

    for row in load_overview_cache() {
        let id = row.get("accountId").and_then(Value::as_str).unwrap_or("").to_string();
        let label = row.get("label").and_then(Value::as_str).unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }
        let health = row.get("health").cloned().unwrap_or(Value::Null);

        // 1. 余额耗尽预测（<3 天）
        if let Some(days) = health.get("daysLeft").and_then(Value::as_f64) {
            if days < 3.0 {
                alerts.push(json!({
                    "fingerprint": format!("balance:{id}"),
                    "kind": "balance",
                    "level": "critical",
                    "accountId": id,
                    "accountLabel": label,
                    "ts": now,
                    "data": { "daysLeft": days },
                }));
            }
        }
        // 2. 健康分 critical（原因码交给前端翻译，后端不写死中文）
        if health.get("status").and_then(Value::as_str) == Some("critical") {
            let score = health.get("score").and_then(Value::as_f64).unwrap_or(0.0);
            alerts.push(json!({
                "fingerprint": format!("health:{id}"),
                "kind": "health",
                "level": "warn",
                "accountId": id,
                "accountLabel": label,
                "ts": now,
                "data": {
                    "score": score,
                    "reasons": health.get("reasons").cloned().unwrap_or(json!([])),
                },
            }));
        }
        // 3. 即将到期的积分包（只在还有余额时提醒；已耗尽的包没有动作可做）
        if let Some(resources) = row.get("resources").and_then(Value::as_array) {
            for item in resources {
                let remaining = item.get("remaining").and_then(Value::as_f64).unwrap_or(0.0);
                if remaining <= 0.0 {
                    continue;
                }
                let Some(expire_at) = item.get("expireAt").and_then(Value::as_i64) else {
                    continue;
                };
                let left_ms = expire_at - now;
                if !(0..=7 * DAY_MS).contains(&left_ms) {
                    continue;
                }
                alerts.push(json!({
                    "fingerprint": format!("expiring:{id}:{expire_at}"),
                    "kind": "expiring",
                    "level": if left_ms < 3 * DAY_MS { "critical" } else { "warn" },
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
    }

    // 4. 24h 内失败的切换（一台机器一天内切不了几次，翻最近 20 条足够）
    if let Some(entries) = switch_history::load(20).get("entries").and_then(Value::as_array) {
        for entry in entries {
            let at = entry.get("ts").and_then(Value::as_i64).unwrap_or(0);
            if now - at > DAY_MS {
                continue;
            }
            let result = entry.get("result").and_then(Value::as_str).unwrap_or("");
            if result == "ok" || result.is_empty() {
                continue;
            }
            let account_id = entry.get("accountId").and_then(Value::as_str).unwrap_or("").to_string();
            // 同一账号只报最近一次（后写的在前，跳过已见的）
            if alerts
                .iter()
                .any(|item| item.get("fingerprint").and_then(Value::as_str) == Some(&format!("switch:{account_id}")))
            {
                continue;
            }
            alerts.push(json!({
                "fingerprint": format!("switch:{account_id}"),
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
    }

    // 5. 24h 内错误通知聚合
    if let Ok(entries) = notifications::list() {
        let errors = entries
            .iter()
            .filter(|entry| entry.level == "error" && now - entry.at <= DAY_MS)
            .count();
        if errors >= RUNFAIL_THRESHOLD {
            alerts.push(json!({
                "fingerprint": "runfail",
                "kind": "runfail",
                "level": "warn",
                "accountId": "",
                "accountLabel": "",
                "ts": now,
                "data": { "count": errors },
            }));
        }
    }

    // 过滤「7 天内忽略过」的指纹
    let state = load_state();
    alerts.retain(|alert| {
        let Some(fp) = alert.get("fingerprint").and_then(Value::as_str) else {
            return true;
        };
        !matches!(state.dismissed.get(fp), Some(at) if now - *at < DISMISS_TTL_MS)
    });

    alerts.sort_by(|a, b| {
        let level = |v: &Value| if v.get("level").and_then(Value::as_str) == Some("critical") { 0 } else { 1 };
        level(a).cmp(&level(b))
    });
    alerts
}

/// 当前提醒清单（active）+ 已忽略条数。
pub fn list() -> Value {
    let alerts = evaluate();
    let state = load_state();
    let now = now_ms();
    let dismissed_count = state
        .dismissed
        .values()
        .filter(|at| now - **at < DISMISS_TTL_MS)
        .count();
    json!({
        "ok": true,
        "alerts": alerts,
        "dismissedCount": dismissed_count,
        "evaluatedAt": now,
    })
}

pub fn dismiss(fingerprint: &str) -> Value {
    let mut state = load_state();
    state.dismissed.insert(fingerprint.to_string(), now_ms());
    save_state(&state);
    json!({ "ok": true })
}

pub fn dismiss_all() -> Value {
    let alerts = evaluate();
    let mut state = load_state();
    let now = now_ms();
    for alert in &alerts {
        if let Some(fp) = alert.get("fingerprint").and_then(Value::as_str) {
            state.dismissed.insert(fp.to_string(), now);
        }
    }
    save_state(&state);
    json!({ "ok": true, "dismissed": alerts.len() })
}

/// 评估并挑出「第一次出现」的提醒（用于系统通知），同时记 seen。
///
/// 冷启动兜底：`seen` 为空说明是第一次评估（或重装），此时只记 seen 不通知——
/// 否则装机第一天铃铛还没点亮就先弹一串历史问题。
fn evaluate_new(mark_seen: bool) -> Vec<Value> {
    let alerts = evaluate();
    let mut state = load_state();
    let now = now_ms();
    let first_run = state.seen.is_empty();
    let fresh: Vec<Value> = alerts
        .iter()
        .filter(|alert| {
            let Some(fp) = alert.get("fingerprint").and_then(Value::as_str) else {
                return false;
            };
            if first_run {
                return false;
            }
            !state.seen.contains_key(fp)
        })
        .cloned()
        .collect();
    if mark_seen {
        for alert in &alerts {
            if let Some(fp) = alert.get("fingerprint").and_then(Value::as_str) {
                state.seen.insert(fp.to_string(), now);
            }
        }
        save_state(&state);
    }
    fresh
}

/// 后台轮询：启动 20s 后首评（等账号页把缓存写出来），之后每 10 分钟一次。
/// 有新提醒就发 `alerts://new` 事件，前端决定要不要系统通知（失焦才发）。
pub fn spawn_poller(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        let _ = evaluate_new(true);
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(600)).await;
            let fresh = evaluate_new(true);
            if !fresh.is_empty() {
                // 顺带记一条通知存档，保证「跑完 → 日志里能查到」闭环
                let _ = notifications::record(
                    "warning",
                    "异常提醒",
                    Some(&format!("新增 {} 条异常提醒，详见标题栏铃铛", fresh.len())),
                );
                let _ = app.emit("alerts://new", json!({ "alerts": fresh }));
            }
        }
    });
}
