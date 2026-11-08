//! 成长中心：补签（补登卡）/ 连登兑换 / 盲盒抽奖 / 能量开 Buddy 盲盒。
//!
//! 接口前缀 `{GROWTH_HOST}/v2/activity/growth`（`www.workbuddy.cn`），纯 Bearer：
//!
//! | 用途 | 方法 + 路径 |
//! | --- | --- |
//! | 连签状态 | GET `/streak` → `makeup_cards` / `makeup_dates` / `launch_date` / `streak.days` |
//! | 活跃日历 | GET `/heatmap` → `today.date` / `cells[].{date,score}` |
//! | 用补登卡 | POST `/makeup-cards/use` `{target_date}` |
//! | 兑换状态 | GET `/redeem/summary` → `starter_status` / `advanced_status` / `legendary_status` |
//! | 连登兑换 | POST `/redeem` `{tier, client_token}` |
//! | 抽奖次数 | GET `/lottery/chances` → `balance` |
//! | 抽奖 | POST `/lottery/draw` `{client_token}` |
//! | Buddy 能量 | GET `/buddy/quota` → `affordable` / `max_open_count` |
//! | 能量开盒 | POST `/buddy/open` `{count, client_token}` |
//! | 能量余额 | GET `/energy` → `balance` |
//! | 徽章资产 | GET `/badges` → `badges[]` |
//!
//! 安全边界：
//! - **写操作不重试**（[`RetryClass::Write`]），每轮每类最多 1 次；
//! - `client_token` 每次新生成 `u-<uuid4>`（缺了 `/lottery/draw` 会 400）；
//! - 「无补登卡 / 无候选日期 / 档位未解锁 / 没有抽奖次数 / 能量不足」都属**常态**，
//!   记为 `skipped` / `locked`，不写失败日志；
//! - 403「天数不足」必须在认证判定之前拦掉，否则会中止整轮。
//!
//! 与 `growth_tasks` 的分工：本模块只做成长中心的「额外奖励」，任务接取/领奖仍在
//! `growth_tasks`。

use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use chrono::{Datelike, NaiveDate};
use serde_json::{json, Value};

use crate::modules::account::{account_display_name, load_accounts, variant_of};
use crate::modules::config::{
    add_growth_center_log, load_growth_center_config, now_ms, RunFlagGuard,
    GROWTH_CENTER_API_PREFIX, GROWTH_CENTER_DEFAULT_BUDGET_SECONDS,
    GROWTH_CENTER_DEFAULT_INTERVAL_MINUTES, GROWTH_CENTER_MAX_WRITES_PER_RUN,
};
use crate::modules::growth_center_crypto::{decrypt_envelope, parse_envelope, TokenError};
use crate::modules::growth_tasks::uid_of;
use crate::modules::net_budget::{payload, request, ApiReply, RetryClass, TimeBudget};
use crate::modules::refresh::ensure_fresh_token;
use crate::modules::variant::WbVariant;
use crate::modules::wb_runtime;

static CENTER_RUNNING: AtomicBool = AtomicBool::new(false);
static ACCOUNT_LEASES: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

/// 埋点头（与 `growth_tasks` 保持一致；UA 由那边维护）。
fn headers(token: &str, uid: &str) -> std::collections::HashMap<String, String> {
    let mut headers = std::collections::HashMap::new();
    headers.insert("Authorization".into(), format!("Bearer {token}"));
    headers.insert("Accept".into(), "application/json, text/plain, */*".into());
    headers.insert("Content-Type".into(), "application/json".into());
    headers.insert("Origin".into(), crate::modules::config::GROWTH_HOST.into());
    headers.insert(
        "Referer".into(),
        format!(
            "{}/profile/growth-center",
            crate::modules::config::GROWTH_HOST
        ),
    );
    headers.insert("User-Agent".into(), crate::modules::growth_tasks::UA.into());
    if !uid.is_empty() {
        headers.insert("X-User-Id".into(), uid.to_string());
    }
    headers.insert("x-codebuddy-request".into(), "1".into());
    headers.insert("x-client-platform".into(), "web".into());
    headers
}

// ---------------------------------------------------------------------------
// 令牌：明文直用，信封按需解密
// ---------------------------------------------------------------------------

/// 取可用令牌。加密信封账号解密失败时返回分类错误（不阻断其它账号）。
pub async fn resolve_token(account: &Value) -> Result<String, TokenError> {
    match account.get("access_token") {
        Some(Value::String(token)) if !token.trim().is_empty() => Ok(token.clone()),
        Some(value @ Value::Object(_)) => {
            let envelope = parse_envelope(value)?;
            let secret = wb_runtime::at_rest_secret().await?;
            match decrypt_envelope(&envelope, &secret) {
                Ok(token) => Ok(token),
                // 客户端换过密钥：清缓存再试一次，仍失败才降级。
                Err(TokenError::KeyMismatch) => {
                    wb_runtime::invalidate_cache();
                    let secret = wb_runtime::at_rest_secret().await?;
                    decrypt_envelope(&envelope, &secret)
                }
                Err(other) => Err(other),
            }
        }
        _ => Err(TokenError::InvalidFormat("缺少 access_token".into())),
    }
}

// ---------------------------------------------------------------------------
// 纯函数：解析与判定（便于单测）
// ---------------------------------------------------------------------------

/// `makeup_cards` 兼容两种形状：裸数字，或 `{balance: n}`。
pub fn makeup_cards(streak: &Value) -> i64 {
    match streak.get("makeup_cards") {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::Object(map)) => map.get("balance").and_then(Value::as_i64).unwrap_or(0),
        _ => 0,
    }
}

/// 连登档位 → 兑换状态字段。
const TIERS: [(&str, &str); 3] = [
    ("7d", "starter_status"),
    ("14d", "advanced_status"),
    ("28d", "legendary_status"),
];

/// 可兑换档位：状态存在且既不是 `claimed` 也不是 `locked`。
///
/// 字段缺失（`None`）时按「不可兑换」处理——参考实现同样跳过，避免拿未解锁档位
/// 去换一个必然 403 的请求。
pub fn redeemable_tiers(summary: &Value) -> Vec<&'static str> {
    TIERS
        .iter()
        .filter(|(_, field)| {
            summary
                .get(field)
                .and_then(Value::as_str)
                .map(|status| !matches!(status, "claimed" | "locked"))
                .unwrap_or(false)
        })
        .map(|(tier, _)| *tier)
        .collect()
}

fn parse_date(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d").ok()
}

fn score_of(cell: &Value) -> Option<f64> {
    cell.get("score")
        .and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|n| n as f64)))
}

/// 计算可补签日期（**倒序**，最近断点优先）。
///
/// 规则：`start = max(当月 1 日, launch_date)` ≤ day < today，且当天分数为 0、
/// 不在 `makeup_dates` 里。`today` 一律取服务端 `heatmap.today.date`。
///
/// 数据结构异常返回 `Err`，调用方据此**整段跳过、不消耗补登卡**。
pub fn makeup_candidates(streak: &Value, heatmap: &Value) -> Result<Vec<String>, String> {
    if makeup_cards(streak) <= 0 {
        return Ok(vec![]);
    }
    let today_raw = heatmap
        .get("today")
        .and_then(|t| t.get("date"))
        .and_then(Value::as_str)
        .ok_or_else(|| "heatmap 缺少 today.date".to_string())?;
    let today = parse_date(today_raw).ok_or_else(|| format!("today.date 非法: {today_raw}"))?;

    let cells = heatmap
        .get("cells")
        .and_then(Value::as_array)
        .ok_or_else(|| "heatmap.cells 不是数组".to_string())?;

    let mut start = today
        .with_day(1)
        .ok_or_else(|| "无法计算当月 1 日".to_string())?;
    if let Some(launch) = streak.get("launch_date").and_then(Value::as_str) {
        if let Some(launch_date) = parse_date(launch) {
            if launch_date > start {
                start = launch_date;
            }
        }
    }

    let makeup_dates: HashSet<String> = streak
        .get("makeup_dates")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.as_str().map(|s| s.trim().to_string()))
                .collect()
        })
        .unwrap_or_default();

    let mut found: Vec<(NaiveDate, String)> = Vec::new();
    for cell in cells {
        let raw = cell
            .get("date")
            .and_then(Value::as_str)
            .ok_or_else(|| "cells[].date 缺失".to_string())?;
        let date = parse_date(raw).ok_or_else(|| format!("cells[].date 非法: {raw}"))?;
        let score = score_of(cell).ok_or_else(|| format!("cells[].score 非法: {raw}"))?;
        if date < start || date >= today {
            continue;
        }
        if score != 0.0 {
            continue;
        }
        let key = date.format("%Y-%m-%d").to_string();
        if makeup_dates.contains(&key) {
            continue;
        }
        found.push((date, key));
    }
    found.sort_by_key(|item| std::cmp::Reverse(item.0));
    Ok(found.into_iter().map(|(_, key)| key).collect())
}

/// `date is not broken, no makeup needed`：并发下的正常竞争结果。
pub fn is_no_makeup_needed(message: &str) -> bool {
    message.to_lowercase().contains("no makeup needed")
}

/// 没有抽奖次数 / 能量不足：常态，不算失败。
pub fn is_no_chance(message: &str) -> bool {
    let text = message.to_lowercase();
    text.contains("no chance")
        || ((text.contains("insufficient") || text.contains("not enough"))
            && (text.contains("chance") || text.contains("balance")))
}

/// 403「天数不足」= 档位未解锁，属常态。
pub fn is_tier_locked(status: u16, message: &str) -> bool {
    if status != 403 {
        return false;
    }
    let text = message.to_lowercase();
    !(text.contains("unauthorized") || text.contains("token") || text.contains("登录"))
}

/// `tier` 参数不被识别：参考实现会退回用天数（7/14/28）再试一次。
pub fn is_unknown_tier(message: &str) -> bool {
    let text = message.to_lowercase();
    text.contains("tier")
        && (text.contains("unknown")
            || text.contains("unsupported")
            || text.contains("invalid")
            || text.contains("not support"))
}

/// 开盒数量：`min(可负担, 单次上限)`，且至少 1。
pub fn open_count(affordable: i64, max_open: i64) -> i64 {
    let cap = if max_open <= 0 { 1 } else { max_open };
    affordable.max(0).min(cap)
}

/// 文本字段：缺失 / 类型不符一律给空串，前端不用再判 `undefined`。
fn text_of(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// 布尔字段兼容 `true/false` 与 `1/0` 两种形状。
fn bool_of(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
        _ => false,
    }
}

/// 整数字段：缺失 / 类型不符给 0（只读展示，不因数据异常报错）。
fn int_of(data: &Value, key: &str) -> i64 {
    data.get(key).and_then(Value::as_i64).unwrap_or(0)
}

/// 单枚徽章 → 前端契约（camelCase）。
pub fn badge_row(badge: &Value) -> Value {
    json!({
        "taskId": text_of(badge.get("task_id")),
        "code": text_of(badge.get("code")),
        "title": text_of(badge.get("title")),
        "badgeName": text_of(badge.get("badge_name")),
        "earned": bool_of(badge.get("earned")),
        "claimed": bool_of(badge.get("claimed")),
        "claimable": bool_of(badge.get("claimable")),
        "url": text_of(badge.get("url")),
        "taskType": text_of(badge.get("task_type")),
    })
}

/// `/badges` 的 `data` → `{badges: [...]}`。
pub fn parse_badges(data: &Value) -> Value {
    let badges: Vec<Value> = data
        .get("badges")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().map(badge_row).collect())
        .unwrap_or_default();
    json!({ "badges": badges })
}

/// `/buddy/quota` 的 `data` → 前端契约（camelCase，缺字段给 0）。
pub fn parse_buddy_quota(data: &Value) -> Value {
    json!({
        "balance": int_of(data, "balance"),
        "costPerOpen": int_of(data, "cost_per_open"),
        "affordable": int_of(data, "affordable"),
        "maxOpenCount": int_of(data, "max_open_count"),
    })
}

fn client_token() -> String {
    format!("u-{}", uuid::Uuid::new_v4())
}

// ---------------------------------------------------------------------------
// 单次调用
// ---------------------------------------------------------------------------

struct Call {
    token: String,
    uid: String,
    budget: TimeBudget,
}

impl Call {
    fn headers(&self) -> std::collections::HashMap<String, String> {
        headers(&self.token, &self.uid)
    }

    async fn get(&self, path: &str) -> ApiReply {
        request(
            &self.budget,
            &format!("{GROWTH_CENTER_API_PREFIX}{path}"),
            "GET",
            None,
            &self.headers(),
            RetryClass::Read,
        )
        .await
    }

    async fn post(&self, path: &str, body: Value) -> ApiReply {
        request(
            &self.budget,
            &format!("{GROWTH_CENTER_API_PREFIX}{path}"),
            "POST",
            Some(body),
            &self.headers(),
            RetryClass::Write,
        )
        .await
    }
}

fn budget_seconds(cfg: &Value) -> u64 {
    cfg.get("budget_seconds")
        .and_then(Value::as_i64)
        .unwrap_or(GROWTH_CENTER_DEFAULT_BUDGET_SECONDS)
        .clamp(60, 540) as u64
}

async fn call_for(account: &Value, cfg: &Value) -> Result<Call, Value> {
    let token = match resolve_token(account).await {
        Ok(token) => token,
        Err(err) => {
            return Err(json!({
                "result": "skipped",
                "reason": err.code(),
                "error": err.message(),
            }))
        }
    };
    Ok(Call {
        token,
        uid: uid_of(account),
        budget: TimeBudget::new(budget_seconds(cfg)),
    })
}

/// 单次资产查询的预算：两个只读接口，30 秒足够，不占用成长中心轮次预算。
const ASSET_BUDGET_SECONDS: u64 = 30;

/// 只读资产查询用的调用上下文（不读配置、不落日志）。
async fn asset_call(account: &Value) -> Result<Call, String> {
    let token = resolve_token(account).await.map_err(|err| err.message())?;
    Ok(Call {
        token,
        uid: uid_of(account),
        budget: TimeBudget::new(ASSET_BUDGET_SECONDS),
    })
}

/// 徽章资产：`GET /badges` → `{badges}`（camelCase）。
pub async fn badges(account: &Value) -> Result<Value, String> {
    let call = asset_call(account).await?;
    let reply = call.get("/badges").await;
    if !reply.ok() {
        return Err(reply.message());
    }
    Ok(parse_badges(payload(&reply)))
}

/// 能量开盒配额：`GET /buddy/quota` → `{balance, costPerOpen, affordable, maxOpenCount}`。
pub async fn buddy_quota(account: &Value) -> Result<Value, String> {
    let call = asset_call(account).await?;
    let reply = call.get("/buddy/quota").await;
    if !reply.ok() {
        return Err(reply.message());
    }
    Ok(parse_buddy_quota(payload(&reply)))
}

// ---------------------------------------------------------------------------
// 四类动作
// ---------------------------------------------------------------------------

/// 六类只读状态的原始值（前端展示用）。
async fn read_state(call: &Call) -> Value {
    let mut out = json!({});
    for (key, path) in [
        ("streak", "/streak"),
        ("heatmap", "/heatmap"),
        ("redeem", "/redeem/summary"),
        ("lottery", "/lottery/chances"),
        ("buddy", "/buddy/quota"),
        ("energy", "/energy"),
    ] {
        let reply = call.get(path).await;
        out[key] = json!({
            "ok": reply.ok(),
            "status": reply.status,
            "code": reply.code(),
            "message": reply.message(),
            "payload": payload(&reply).clone(),
        });
    }
    out
}

fn feature_on(cfg: &Value, key: &str) -> bool {
    cfg.get("features")
        .and_then(|f| f.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn max_per_run(cfg: &Value, key: &str) -> i64 {
    cfg.get(key)
        .and_then(Value::as_i64)
        .unwrap_or(GROWTH_CENTER_MAX_WRITES_PER_RUN)
        .clamp(1, GROWTH_CENTER_MAX_WRITES_PER_RUN)
}

/// 补签一张卡（最多 1 张/轮）。
async fn step_makeup(account: &Value, call: &Call, cfg: &Value) -> Value {
    if !feature_on(cfg, "makeup") {
        return json!({ "action": "makeup", "result": "skipped", "detail": "功能未开启" });
    }
    let streak = call.get("/streak").await;
    if !streak.ok() {
        return json!({ "action": "makeup", "result": "error", "detail": streak.message(),
                       "needsAttention": streak.is_hard_failure() });
    }
    let heatmap = call.get("/heatmap").await;
    if !heatmap.ok() {
        return json!({ "action": "makeup", "result": "error", "detail": heatmap.message(),
                       "needsAttention": heatmap.is_hard_failure() });
    }
    let candidates = match makeup_candidates(payload(&streak), payload(&heatmap)) {
        Ok(list) => list,
        // 数据结构异常：整段跳过，**不消耗补登卡**。
        Err(detail) => {
            return json!({ "action": "makeup", "result": "error", "detail": detail,
                           "needsAttention": true })
        }
    };
    if candidates.is_empty() {
        return json!({ "action": "makeup", "result": "skipped", "detail": "无候选日期" });
    }
    let limit = max_per_run(cfg, "makeup_max_per_run").max(1) as usize;
    let mut done = Vec::new();
    for date in candidates.into_iter().take(limit) {
        let reply = call
            .post("/makeup-cards/use", json!({ "target_date": date }))
            .await;
        if reply.ok() {
            done.push(date.clone());
            write_log(
                account,
                "makeup",
                "success",
                &date,
                None,
                payload(&reply).clone(),
            );
        } else if is_no_makeup_needed(&reply.message()) {
            // 并发下别人刚补过：正常竞争，不当失败。
            break;
        } else {
            write_log(
                account,
                "makeup",
                "error",
                &date,
                Some(&reply.message()),
                Value::Null,
            );
            return json!({ "action": "makeup", "result": "error", "detail": reply.message(),
                           "needsAttention": reply.is_hard_failure(), "dates": done });
        }
    }
    json!({ "action": "makeup", "result": if done.is_empty() { "skipped" } else { "success" },
            "dates": done })
}

/// 连登兑奖：每个档位最多 1 次。
async fn step_redeem(account: &Value, call: &Call, cfg: &Value) -> Value {
    if !feature_on(cfg, "redeem") {
        return json!({ "action": "redeem", "result": "skipped", "detail": "功能未开启" });
    }
    let summary = call.get("/redeem/summary").await;
    if !summary.ok() {
        return json!({ "action": "redeem", "result": "error", "detail": summary.message(),
                       "needsAttention": summary.is_hard_failure() });
    }
    let tiers = redeemable_tiers(payload(&summary));
    if tiers.is_empty() {
        return json!({ "action": "redeem", "result": "skipped", "detail": "无可兑换档位" });
    }
    let mut done = Vec::new();
    for tier in tiers {
        let mut reply = call
            .post(
                "/redeem",
                json!({ "tier": tier, "client_token": client_token() }),
            )
            .await;
        // 兜底：部分版本只认天数。
        if !reply.ok() && is_unknown_tier(&reply.message()) {
            let days = tier.trim_end_matches('d');
            reply = call
                .post(
                    "/redeem",
                    json!({ "tier": days, "client_token": client_token() }),
                )
                .await;
        }
        if reply.ok() {
            done.push(tier);
            write_log(
                account,
                "redeem",
                "success",
                tier,
                None,
                payload(&reply).clone(),
            );
            continue;
        }
        if is_tier_locked(reply.status, &reply.message()) {
            // 未解锁：常态，不写失败日志。
            continue;
        }
        write_log(
            account,
            "redeem",
            "error",
            tier,
            Some(&reply.message()),
            Value::Null,
        );
        return json!({ "action": "redeem", "result": "error", "detail": reply.message(),
                       "needsAttention": reply.is_hard_failure(), "tiers": done });
    }
    json!({ "action": "redeem", "result": if done.is_empty() { "locked" } else { "success" },
            "tiers": done })
}

/// 抽奖盲盒：每轮最多 1 次。
async fn step_lottery(account: &Value, call: &Call, cfg: &Value) -> Value {
    if !feature_on(cfg, "lottery") {
        return json!({ "action": "lottery", "result": "skipped", "detail": "功能未开启" });
    }
    let chances = call.get("/lottery/chances").await;
    if !chances.ok() {
        return json!({ "action": "lottery", "result": "error", "detail": chances.message(),
                       "needsAttention": chances.is_hard_failure() });
    }
    let balance = payload(&chances)
        .get("balance")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if balance <= 0 {
        return json!({ "action": "lottery", "result": "skipped", "detail": "没有抽奖次数" });
    }
    let reply = call
        .post("/lottery/draw", json!({ "client_token": client_token() }))
        .await;
    if reply.ok() {
        let data = payload(&reply).clone();
        let prize = data
            .get("prize_name")
            .or_else(|| data.get("prize"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let need_address = data
            .get("need_address")
            .or_else(|| data.get("require_address"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        write_log(account, "lottery", "success", &prize, None, data.clone());
        return json!({ "action": "lottery", "result": "success", "prize": prize,
                       "needAddress": need_address });
    }
    if is_no_chance(&reply.message()) {
        return json!({ "action": "lottery", "result": "skipped", "detail": reply.message() });
    }
    write_log(
        account,
        "lottery",
        "error",
        "",
        Some(&reply.message()),
        Value::Null,
    );
    json!({ "action": "lottery", "result": "error", "detail": reply.message(),
            "needsAttention": reply.is_hard_failure() })
}

/// 能量开 Buddy 盲盒：每轮最多 1 次。
async fn step_buddy_open(account: &Value, call: &Call, cfg: &Value) -> Value {
    if !feature_on(cfg, "buddy_open") {
        return json!({ "action": "buddy_open", "result": "skipped", "detail": "功能未开启" });
    }
    let quota = call.get("/buddy/quota").await;
    if !quota.ok() {
        return json!({ "action": "buddy_open", "result": "error", "detail": quota.message(),
                       "needsAttention": quota.is_hard_failure() });
    }
    let data = payload(&quota);
    let affordable = data.get("affordable").and_then(Value::as_i64).unwrap_or(0);
    let max_open = data
        .get("max_open_count")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let count = open_count(affordable, max_open).min(max_per_run(cfg, "buddy_open_max_per_run"));
    if count <= 0 {
        return json!({ "action": "buddy_open", "result": "skipped", "detail": "能量不足" });
    }
    let reply = call
        .post(
            "/buddy/open",
            json!({ "count": count, "client_token": client_token() }),
        )
        .await;
    if reply.ok() {
        write_log(
            account,
            "buddy_open",
            "success",
            "",
            None,
            payload(&reply).clone(),
        );
        return json!({ "action": "buddy_open", "result": "success", "count": count });
    }
    if is_no_chance(&reply.message()) {
        return json!({ "action": "buddy_open", "result": "skipped", "detail": reply.message() });
    }
    write_log(
        account,
        "buddy_open",
        "error",
        "",
        Some(&reply.message()),
        Value::Null,
    );
    json!({ "action": "buddy_open", "result": "error", "detail": reply.message(),
            "needsAttention": reply.is_hard_failure() })
}

/// 写一条成长中心日志（只在真正发起写操作或硬失败时调用）。
fn write_log(
    account: &Value,
    action: &str,
    result: &str,
    detail: &str,
    error: Option<&str>,
    value: Value,
) {
    let mut entry = json!({
        "ts": now_ms(),
        "accountId": account.get("id").cloned().unwrap_or(Value::Null),
        "email": account_display_name(account),
        "variant": variant_of(account).as_str(),
        "action": action,
        "result": result,
        "detail": detail,
        "value": value,
        "needsAttention": error.is_some() && result == "error",
    });
    if let Some(text) = error {
        entry["error"] = json!(text);
    }
    add_growth_center_log(&entry);
}

// ---------------------------------------------------------------------------
// 只读状态聚合
// ---------------------------------------------------------------------------

pub async fn growth_center_status(variant: Option<WbVariant>) -> Value {
    let cfg = load_growth_center_config();
    let accounts: Vec<Value> = load_accounts()
        .into_iter()
        .filter(|a| variant.is_none_or(|target| variant_of(a) == target))
        .collect();
    let mut rows = Vec::new();
    for account in &accounts {
        let call = match call_for(account, &cfg).await {
            Ok(call) => call,
            Err(row) => {
                let mut out = row;
                out["accountId"] = account.get("id").cloned().unwrap_or(Value::Null);
                out["email"] = json!(account_display_name(account));
                rows.push(out);
                continue;
            }
        };
        let state = read_state(&call).await;
        let streak = &state["streak"]["payload"];
        let heatmap = &state["heatmap"]["payload"];
        let redeem = &state["redeem"]["payload"];
        let gaps = makeup_candidates(streak, heatmap).unwrap_or_default();
        rows.push(json!({
            "accountId": account.get("id").cloned().unwrap_or(Value::Null),
            "email": account_display_name(account),
            "ok": true,
            "streakDays": streak.get("streak").and_then(|s| s.get("days"))
                .and_then(Value::as_i64).unwrap_or(0),
            "makeupCards": makeup_cards(streak),
            "gaps": gaps,
            "redeemable": redeemable_tiers(redeem),
            "lotteryChances": state["lottery"]["payload"].get("balance")
                .and_then(Value::as_i64).unwrap_or(0),
            "buddyAffordable": state["buddy"]["payload"].get("affordable")
                .and_then(Value::as_i64).unwrap_or(0),
            "buddyMaxOpen": state["buddy"]["payload"].get("max_open_count")
                .and_then(Value::as_i64).unwrap_or(0),
            "energy": state["energy"]["payload"].get("balance")
                .and_then(Value::as_i64).unwrap_or(0),
            "raw": state,
        }));
    }
    json!({ "accounts": rows })
}

// ---------------------------------------------------------------------------
// 编排
// ---------------------------------------------------------------------------

struct Lease(String);

impl Lease {
    fn try_acquire(account: &Value) -> Option<Self> {
        let key = account
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(String::from)
            .unwrap_or_else(|| account_display_name(account));
        let mut leases = ACCOUNT_LEASES
            .get_or_init(|| Mutex::new(HashSet::new()))
            .lock()
            .unwrap();
        leases.insert(key.clone()).then(|| Self(key))
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(leases) = ACCOUNT_LEASES.get() {
            leases
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&self.0);
        }
    }
}

async fn run_account(account: &Value, cfg: &Value) -> Value {
    let Some(_lease) = Lease::try_acquire(account) else {
        return json!({ "accountId": account.get("id").cloned().unwrap_or(Value::Null),
                       "email": account_display_name(account),
                       "result": "skipped", "reason": "already_running" });
    };
    let call = match call_for(account, cfg).await {
        Ok(call) => call,
        Err(row) => {
            // 加密凭据不可用等：单账号降级，不阻断其它账号。
            let mut out = row;
            out["accountId"] = account.get("id").cloned().unwrap_or(Value::Null);
            out["email"] = json!(account_display_name(account));
            return out;
        }
    };
    let mut steps = Vec::new();
    for step in [
        step_makeup(account, &call, cfg).await,
        step_redeem(account, &call, cfg).await,
        step_lottery(account, &call, cfg).await,
        step_buddy_open(account, &call, cfg).await,
    ] {
        steps.push(step);
    }
    let success = steps
        .iter()
        .filter(|s| s.get("result").and_then(Value::as_str) == Some("success"))
        .count();
    let errors = steps
        .iter()
        .filter(|s| s.get("result").and_then(Value::as_str) == Some("error"))
        .count();
    let budget_out = call.budget.exhausted();
    let result = if errors > 0 {
        "partial"
    } else if success > 0 {
        "success"
    } else {
        "none"
    };
    json!({
        "accountId": account.get("id").cloned().unwrap_or(Value::Null),
        "email": account_display_name(account),
        "result": result,
        "success": success,
        "errors": errors,
        "budgetExhausted": budget_out,
        "steps": steps,
    })
}

pub async fn run_growth_center_cycle() -> Value {
    let Some(_guard) = RunFlagGuard::try_acquire(&CENTER_RUNNING) else {
        return json!({ "status": "skipped", "reason": "already_running" });
    };
    let cfg = load_growth_center_config();
    if cfg.get("enabled").and_then(Value::as_bool) != Some(true) {
        return json!({ "status": "disabled" });
    }
    run_all(&cfg, None).await
}

pub async fn run_growth_center_all(variant: Option<WbVariant>) -> Value {
    let Some(_guard) = RunFlagGuard::try_acquire(&CENTER_RUNNING) else {
        return json!({ "accounts": [], "status": "skipped", "reason": "already_running" });
    };
    let cfg = load_growth_center_config();
    run_all(&cfg, variant).await
}

async fn run_all(cfg: &Value, variant: Option<WbVariant>) -> Value {
    let excluded: HashSet<&str> = cfg
        .get("excluded_account_ids")
        .and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let accounts: Vec<Value> = load_accounts()
        .into_iter()
        .filter(|a| variant.is_none_or(|target| variant_of(a) == target))
        .collect();
    let mut rows = Vec::new();
    for account in &accounts {
        let id = account.get("id").and_then(Value::as_str).unwrap_or("");
        if excluded.contains(id) {
            rows.push(json!({
                "accountId": account.get("id").cloned().unwrap_or(Value::Null),
                "email": account_display_name(account),
                "result": "skipped",
                "reason": "auto_growth_center_disabled",
            }));
            continue;
        }
        let fresh = ensure_fresh_token(
            account.clone(),
            &crate::modules::config::load_checkin_config(),
        )
        .await;
        rows.push(run_account(&fresh, cfg).await);
    }
    json!({ "accounts": rows })
}

/// 单账号单动作（界面上的补签 / 兑换 / 抽奖 / 开盒按钮）。
pub async fn run_growth_center_action(account_id: &str, action: &str) -> Value {
    let Some(_guard) = RunFlagGuard::try_acquire(&CENTER_RUNNING) else {
        return json!({ "result": "skipped", "reason": "already_running" });
    };
    let cfg = load_growth_center_config();
    let Some(account) = load_accounts()
        .into_iter()
        .find(|a| a.get("id").and_then(Value::as_str) == Some(account_id))
    else {
        return json!({ "result": "error", "error": "账号不存在" });
    };
    let Some(_lease) = Lease::try_acquire(&account) else {
        return json!({ "result": "skipped", "reason": "already_running" });
    };
    let call = match call_for(&account, &cfg).await {
        Ok(call) => call,
        Err(row) => return row,
    };
    let step = match action {
        "makeup" => step_makeup(&account, &call, &cfg).await,
        "redeem" => step_redeem(&account, &call, &cfg).await,
        "lottery" => step_lottery(&account, &call, &cfg).await,
        "buddy_open" => step_buddy_open(&account, &call, &cfg).await,
        other => json!({ "result": "error", "error": format!("未知动作: {other}") }),
    };
    json!({
        "accountId": account.get("id").cloned().unwrap_or(Value::Null),
        "email": account_display_name(&account),
        "step": step,
    })
}

pub fn growth_center_cycle_interval() -> Duration {
    let minutes = load_growth_center_config()
        .get("interval_minutes")
        .and_then(Value::as_i64)
        .unwrap_or(GROWTH_CENTER_DEFAULT_INTERVAL_MINUTES)
        .clamp(5, 24 * 60);
    Duration::from_secs((minutes as u64) * 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn streak_with(cards: Value, launch: Option<&str>, made: &[&str]) -> Value {
        json!({
            "makeup_cards": cards,
            "launch_date": launch,
            "makeup_dates": made,
            "streak": { "days": 7 },
        })
    }

    fn heatmap_with(today: &str, cells: Vec<(&str, f64)>) -> Value {
        let cells: Vec<Value> = cells
            .into_iter()
            .map(|(date, score)| json!({ "date": date, "score": score }))
            .collect();
        json!({ "today": { "date": today }, "cells": cells })
    }

    #[test]
    fn makeup_cards_accepts_both_shapes() {
        assert_eq!(makeup_cards(&json!({"makeup_cards": 2})), 2);
        assert_eq!(makeup_cards(&json!({"makeup_cards": {"balance": 3}})), 3);
        assert_eq!(makeup_cards(&json!({})), 0);
    }

    #[test]
    fn candidates_are_recent_first_and_skip_used_dates() {
        let streak = streak_with(json!(2), Some("2026-06-17"), &["2026-10-14"]);
        let heatmap = heatmap_with(
            "2026-10-18",
            vec![
                ("2026-10-01", 0.0), // 当月 1 日之后、未补 → 候选
                ("2026-10-14", 0.0), // 已补 → 排除
                ("2026-10-16", 0.0), // 候选（最近断点）
                ("2026-10-17", 5.0), // 有分数 → 排除
                ("2026-10-18", 0.0), // 今天 → 排除
                ("2026-09-30", 0.0), // 上个月 → 排除
            ],
        );
        let got = makeup_candidates(&streak, &heatmap).unwrap();
        assert_eq!(got, vec!["2026-10-16", "2026-10-01"]);
    }

    #[test]
    fn candidates_respect_launch_date() {
        let streak = streak_with(json!(1), Some("2026-10-10"), &[]);
        let heatmap = heatmap_with("2026-10-18", vec![("2026-10-03", 0.0), ("2026-10-12", 0.0)]);
        assert_eq!(
            makeup_candidates(&streak, &heatmap).unwrap(),
            vec!["2026-10-12"]
        );
    }

    #[test]
    fn no_cards_means_no_candidates_without_parsing() {
        let streak = streak_with(json!(0), None, &[]);
        // 故意给一个坏 heatmap：没有卡时应当直接短路，不报错。
        let heatmap = json!({});
        assert!(makeup_candidates(&streak, &heatmap).unwrap().is_empty());
    }

    #[test]
    fn malformed_heatmap_stops_instead_of_guessing() {
        let streak = streak_with(json!(1), None, &[]);
        assert!(makeup_candidates(&streak, &json!({})).is_err());
        assert!(makeup_candidates(&streak, &json!({"today": {"date": "bad"}})).is_err());
        assert!(makeup_candidates(
            &streak,
            &json!({"today": {"date": "2026-10-18"}, "cells": "nope"})
        )
        .is_err());
    }

    #[test]
    fn redeemable_tiers_skip_claimed_and_locked() {
        let summary = json!({
            "starter_status": "available",
            "advanced_status": "claimed",
            "legendary_status": "locked",
        });
        assert_eq!(redeemable_tiers(&summary), vec!["7d"]);
        assert!(redeemable_tiers(&json!({})).is_empty());
    }

    #[test]
    fn normal_business_rejections_are_recognized() {
        assert!(is_no_makeup_needed("date is not broken, no makeup needed"));
        assert!(is_no_chance("no chance"));
        assert!(is_no_chance("insufficient chance balance"));
        assert!(!is_no_chance("insufficient credit"));
        assert!(is_tier_locked(403, "streak days not enough"));
        // 认证类 403 不能被当成"未解锁"。
        assert!(!is_tier_locked(403, "unauthorized"));
        assert!(!is_tier_locked(401, "streak days not enough"));
        assert!(is_unknown_tier("unknown tier"));
        assert!(!is_unknown_tier("tier ok"));
    }

    #[test]
    fn open_count_is_capped() {
        assert_eq!(open_count(5, 2), 2);
        assert_eq!(open_count(1, 0), 1);
        assert_eq!(open_count(0, 3), 0);
        assert_eq!(open_count(-1, 3), 0);
    }

    #[test]
    fn badges_map_to_camel_case() {
        let data = json!({
            "badges": [{
                "task_id": "t-1",
                "code": "first_checkin",
                "title": "首次签到",
                "earned": true,
                "url": "https://x/b.png",
                "share_uuid": "u-1",
                "record_id": "r-1",
                "claimed": false,
                "claimable": 1,
                "badge_name": "早鸟",
                "task_type": "daily",
                "unknown_field": "ignored",
            }],
        });
        let out = parse_badges(&data);
        let badge = &out["badges"][0];
        assert_eq!(badge["taskId"], "t-1");
        assert_eq!(badge["code"], "first_checkin");
        assert_eq!(badge["title"], "首次签到");
        assert_eq!(badge["badgeName"], "早鸟");
        assert_eq!(badge["earned"], true);
        assert_eq!(badge["claimed"], false);
        // 数字 1 也当 true（服务端两种形状都出现过）。
        assert_eq!(badge["claimable"], true);
        assert_eq!(badge["url"], "https://x/b.png");
        assert_eq!(badge["taskType"], "daily");
        assert!(badge.get("task_id").is_none(), "原始下划线字段不外泄");
        // 只有 badges 一个字段，max_cert 不再输出。
        assert!(out.get("maxCert").is_none());
    }

    #[test]
    fn badges_tolerate_missing_fields() {
        let out = parse_badges(&json!({ "badges": [{}, {"task_id": "only-id"}] }));
        let first = &out["badges"][0];
        assert_eq!(first["taskId"], "");
        assert_eq!(first["earned"], false);
        assert_eq!(first["claimable"], false);
        assert_eq!(out["badges"][1]["taskId"], "only-id");
        assert!(parse_badges(&json!({})).get("badges").unwrap().is_array());
        assert!(
            parse_badges(&json!({"badges": "nope", "max_cert": 3}))["badges"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        // 只有 max_cert 没有 badges：同样给空数组，不 panic。
        assert!(
            parse_badges(&json!({"max_cert": 3}))["badges"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn buddy_quota_parses_and_defaults_to_zero() {
        let out = parse_buddy_quota(&json!({
            "affordable": 1,
            "balance": 18,
            "cost_per_open": 10,
            "max_open_count": 5,
        }));
        assert_eq!(out["balance"], 18);
        assert_eq!(out["costPerOpen"], 10);
        assert_eq!(out["affordable"], 1);
        assert_eq!(out["maxOpenCount"], 5);

        // 缺字段：全部给 0，不 panic。
        let empty = parse_buddy_quota(&json!({}));
        assert_eq!(empty["balance"], 0);
        assert_eq!(empty["costPerOpen"], 0);
        assert_eq!(empty["affordable"], 0);
        assert_eq!(empty["maxOpenCount"], 0);
        // 类型不符同样降级为 0。
        let bad = parse_buddy_quota(&json!({"balance": "18", "max_open_count": null}));
        assert_eq!(bad["balance"], 0);
        assert_eq!(bad["maxOpenCount"], 0);
    }

    #[test]
    fn client_token_shape() {
        let token = client_token();
        assert!(token.starts_with("u-"));
        assert!(token.len() > 10);
        assert_ne!(client_token(), client_token());
    }

    #[test]
    fn plain_token_is_returned_as_is() {
        let token = futures_lite_block(resolve_token(&json!({"access_token": "abc.def"}))).unwrap();
        assert_eq!(token, "abc.def");
    }

    /// 只为在同步测试里跑一个「立即就绪」的 future。
    fn futures_lite_block<F: std::future::Future>(future: F) -> F::Output {
        use std::task::{Context, Poll};
        let mut future = std::pin::pin!(future);
        let mut cx = Context::from_waker(std::task::Waker::noop());
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("该 future 不应挂起"),
        }
    }
}
