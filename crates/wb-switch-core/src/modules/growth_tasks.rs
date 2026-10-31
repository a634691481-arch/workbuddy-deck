//! 成长计划任务自动化：自动接取 → 补缺口（遥测埋点 / 真实对话）→ 领奖。
//!
//! 机制（2026-10 社区验证方案，参考 interli3/workbuddy-cloud-worker）：
//! - 进度由服务端按客户端上报的**遥测事件**统计，不校验是否真的操作过：
//!   `POST https://www.workbuddy.cn/v2/report`
//! - 任务清单 `GET /v2/activity/growth/tasks`；接取 `POST /activity/growth/tasks/accept`；
//!   领奖 `POST /activity/growth/tasks/{code}/claim`（同源相对路径，纯 Bearer）。
//! - 两种上报信封不能混：Web 侧直接发**数组**；桌面侧（`Buddy_App`）发
//!   `{common:{...}, events:[...]}` 且 `mode:"LOCAL"`。
//! - `chat_5` / `Model_chat_GLM5.2` / `Expert_team_use_3` **必须真发对话**：
//!   只发埋点进度不动。前两者走 `/console/webchat/conversations` +
//!   `/console/chat/completions`（SSE，本模块用 `http_request_raw` 读全文再按行解析）。
//!   `Expert_team_use_3` 还必须带 `_meta.codebuddy.ai.growthEvent`——它是**字符串**
//!   形式的 JSON 数组，且里面的 `ExpertActualUse`（驼峰）与常规埋点
//!   `expert_actual_use`（下划线）是两个不同事件，**两个都要发**。
//! - 2026-10 批次「体验 XX 专家」（`Expert_lighthouse` / `WorkBuddy_Valorant` /
//!   `expert_use_*`）同样只认真对话，统一走 [`t_expert_use`] 闭环。
//!
//! 注意：`GROWTH_HOST` 是 `www.workbuddy.cn`，与本 crate 其它模块用的
//! `variant.api_endpoint()`（国内版 `www.codebuddy.cn`）不是同一域名。
//!
//! 默认跳过的任务：`Hp_Appearance`（换主题对客户端有真实副作用）。

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use base64::Engine as _;
use sha2::{Digest, Sha256};

use crate::modules::account::{account_display_name, load_accounts, variant_of};
use crate::modules::config::{
    add_growth_log, http_request, http_request_raw, load_growth_config, now_ms, RunFlagGuard,
    GROWTH_DEFAULT_INTERVAL_MINUTES, GROWTH_DEFAULT_MAX_TRIES, GROWTH_HOST,
};
use crate::modules::refresh::ensure_fresh_token;
use crate::modules::variant::WbVariant;

static GROWTH_RUNNING: AtomicBool = AtomicBool::new(false);
/// 当日每任务尝试次数：`{account_id}|{YYYY-MM-DD}` → `{task_code: n}`。
/// 只放内存，进程重启归零（对齐参考实现的内存态；日志里仍有完整记录）。
static TRIES: OnceLock<Mutex<HashMap<String, HashMap<String, i64>>>> = OnceLock::new();

/// 埋点指纹里的客户端版本：官方升级后可能失效，届时需同步更新。
pub(crate) const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) WorkBuddy/5.5.4 Chrome/138.0.7204.251 Electron/37.10.3 Safari/537.36";
const IDE_VERSION: &str = "5.5.6";
const COMMIT: &str = "5f9692923c93033111c51ad7b003eb80204a9b75";
const RELEASE_DATE: i64 = 1_789_036_585_355;
const OS_VERSION: &str = "10.0.26220";
const EXPERT_MARKET: &str = "https://acc-1258344699.cos.accelerate.myqcloud.com/workbuddy/expert-marketplace/expert_center.json";

/// 换主题任务对客户端有真实副作用，默认不做。
const SKIP_TASKS: [&str; 1] = ["Hp_Appearance"];

/// 只能去官网手动完成的任务（2026-10-31 实测判定）。
///
/// `Library_read`：埋点上报返回 `code:0` 但进度恒为 0/1（8 个账号全部不涨），元素名已失效；
/// `Buddy_App`：桌面信封同样全账号 0/1，无一次成功；
/// `wb_wechat_oa_subscribe_task`：关注公众号，需人工授权，任何脚本都不该代按。
/// 继续打点是纯浪费（每天 3 次 × N 账号），所以显式标 manual，UI 直说「需手动」。
const MANUAL_TASKS: [&str; 3] = ["Library_read", "Buddy_App", "wb_wechat_oa_subscribe_task"];

fn is_skipped(code: &str) -> bool {
    SKIP_TASKS.contains(&code)
}

fn is_manual(code: &str) -> bool {
    MANUAL_TASKS.contains(&code)
}

// ---------------------------------------------------------------------------
// 身份与指纹
// ---------------------------------------------------------------------------

/// 账号稳定 id（用于排除名单与日志）。
fn account_key(account: &Value) -> String {
    account
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(String::from)
        .unwrap_or_else(|| account_display_name(account))
}

/// 解析 JWT payload（base64url，无填充）。
fn jwt_payload(token: &str) -> Option<Value> {
    let part = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(part)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// 用户 id：优先账号库里的 `uid`，缺失时从 access_token 的 `sub` 兜底。
pub(crate) fn uid_of(account: &Value) -> String {
    if let Some(uid) = account
        .get("uid")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    {
        return uid.to_string();
    }
    let token = account
        .get("access_token")
        .and_then(Value::as_str)
        .unwrap_or("");
    jwt_payload(token)
        .and_then(|j| j.get("sub").and_then(Value::as_str).map(String::from))
        .unwrap_or_default()
}

fn nick_of(account: &Value) -> String {
    if let Some(nick) = account
        .get("nickname")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    {
        return nick.to_string();
    }
    let token = account
        .get("access_token")
        .and_then(Value::as_str)
        .unwrap_or("");
    jwt_payload(token)
        .and_then(|j| {
            j.get("nickname")
                .or_else(|| j.get("preferred_username"))
                .and_then(Value::as_str)
                .map(String::from)
        })
        .unwrap_or_default()
}

/// 由 uid 稳定派生的设备 / 会话标识（同账号每次一致，保证指纹自洽）。
///
/// 参考实现用 md5 再截断；这里复用已有的 sha2 依赖（同样的「稳定 32 位 hex」语义），
/// 避免为一次摘要再引入新 crate。
fn derive_id(uid: &str, salt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{salt}:{uid}").as_bytes());
    let hex = format!("{:x}", hasher.finalize());
    hex[..32].to_string()
}

fn base_headers(account: &Value, referer: &str) -> HashMap<String, String> {
    let uid = uid_of(account);
    let mut headers = HashMap::new();
    headers.insert(
        "Authorization".into(),
        format!(
            "Bearer {}",
            account
                .get("access_token")
                .and_then(Value::as_str)
                .unwrap_or("")
        ),
    );
    headers.insert("Content-Type".into(), "application/json".into());
    headers.insert("Accept".into(), "application/json, text/plain, */*".into());
    headers.insert("Origin".into(), GROWTH_HOST.into());
    headers.insert("Referer".into(), format!("{GROWTH_HOST}{referer}"));
    headers.insert("User-Agent".into(), UA.into());
    if !uid.is_empty() {
        headers.insert("X-User-Id".into(), uid);
    }
    headers.insert("x-codebuddy-request".into(), "1".into());
    headers.insert("x-client-platform".into(), "web".into());
    headers
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

async fn req(account: &Value, method: &str, path: &str, body: Option<Value>) -> Value {
    let headers = base_headers(account, "/profile/growth-center");
    http_request(
        &format!("{GROWTH_HOST}{path}"),
        method,
        body,
        Some(&headers),
    )
    .await
}

/// Web 侧遥测信封：直接发事件数组。
async fn report(account: &Value, events: Vec<Value>) -> Value {
    let uid = uid_of(account);
    let nick = nick_of(account);
    let now = now_ms();
    let machine = derive_id(&uid, "machine");
    let session = derive_id(&uid, "session");
    let events: Vec<Value> = events
        .into_iter()
        .map(|event| {
            let mut merged = json!({
                "timestamp": now,
                "reportDelay": 0,
                "userId": uid,
                "userNickname": nick,
                "ideName": "WorkBuddy",
                "ideType": "WorkBuddy",
                "ideVersion": IDE_VERSION,
                "machineId": machine,
                "sessionId": session,
                "mode": "CLOUD",
                "userAgent": UA,
                "os": "Win32",
                "arch": "x64",
                "osVersion": OS_VERSION,
                "timezone": "Asia/Shanghai",
                "product": "SaaS",
                "releaseDate": RELEASE_DATE,
                "commit": COMMIT,
                "extName": "workbuddy-desktop",
                "extVersion": IDE_VERSION,
                "cpuCores": 20,
                "memorySize": 24,
            });
            if let (Some(dst), Some(src)) = (merged.as_object_mut(), event.as_object()) {
                for (k, v) in src {
                    dst.insert(k.clone(), v.clone());
                }
            }
            merged
        })
        .collect();
    req(account, "POST", "/v2/report", Some(Value::Array(events))).await
}

/// 桌面侧遥测信封：`{common, events}` 且 `mode:"LOCAL"`（`Buddy_App` 等任务用）。
async fn report_desktop(account: &Value, events: Vec<Value>) -> Value {
    let uid = uid_of(account);
    let nick = nick_of(account);
    let now = now_ms();
    let machine = derive_id(&uid, "machine");
    let session = derive_id(&uid, "session");
    let fingerprint = json!({
        "timezone": "Asia/Shanghai",
        "reportDelay": 2000,
        "userId": uid,
        "username": nick,
        "userNickname": nick,
        "product": "SaaS",
        "releaseDate": RELEASE_DATE,
        "commit": COMMIT,
        "ideName": "WorkBuddy",
        "ideType": "WorkBuddy",
        "ideVersion": IDE_VERSION,
        "machineId": machine,
        "sessionId": session,
        "extName": "workbuddy-desktop",
        "extVersion": IDE_VERSION,
        "os": "win32",
        "arch": "x64",
        "osVersion": OS_VERSION,
        "cpuCores": 20,
        "memorySize": 24,
        "timestamp": now,
        "presentAt": now,
    });
    let events: Vec<Value> = events
        .into_iter()
        .map(|event| {
            let mut merged = fingerprint.clone();
            if let (Some(dst), Some(src)) = (merged.as_object_mut(), event.as_object()) {
                for (k, v) in src {
                    dst.insert(k.clone(), v.clone());
                }
            }
            merged
        })
        .collect();
    let body = json!({
        "common": {
            "userId": uid_of(account),
            "userNickname": nick_of(account),
            "ideName": "WorkBuddy",
            "ideType": "WorkBuddy",
            "machineId": derive_id(&uid_of(account), "machine"),
            "mode": "LOCAL",
            "userAgent": UA,
            "os": "win32",
            "timezone": "Asia/Shanghai",
        },
        "events": events,
    });
    req(account, "POST", "/v2/report", Some(body)).await
}

// ---------------------------------------------------------------------------
// 任务查询 / 接取 / 领奖
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
struct Progress {
    status: String,
    cur: i64,
    tgt: i64,
}

/// 是否还需要推进：已完成 / 已领奖 → 否；目标未知（tgt<=0）→ 是（交给尝试次数兜底）。
fn needs_work(p: &Progress) -> bool {
    if p.status == "completed" || p.status == "claimed" {
        return false;
    }
    if p.tgt <= 0 {
        return true;
    }
    p.cur < p.tgt
}

fn parse_tasks(resp: &Value) -> Vec<Value> {
    resp.get("data")
        .and_then(|d| d.get("tasks"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn parse_progress(task: &Value) -> Progress {
    let progress = task.get("progress").cloned().unwrap_or_else(|| json!({}));
    Progress {
        status: task
            .get("accept_status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        cur: progress.get("current").and_then(Value::as_i64).unwrap_or(0),
        tgt: progress.get("target").and_then(Value::as_i64).unwrap_or(0),
    }
}

async fn list_tasks(account: &Value) -> Vec<Value> {
    parse_tasks(&req(account, "GET", "/v2/activity/growth/tasks", None).await)
}

async fn progress_of(account: &Value, code: &str) -> Option<Progress> {
    let tasks = list_tasks(account).await;
    tasks
        .iter()
        .find(|t| t.get("task_code").and_then(Value::as_str) == Some(code))
        .map(parse_progress)
}

/// 接取全部未接取任务，返回**确实被接受**的 task_code。
///
/// 顶层 `code=0` 不代表逐条成功（prerequisite 不满足会整体 error），只认
/// `data.results[].status == "accepted"`。
async fn accept_missing(account: &Value, tasks: &[Value]) -> Vec<String> {
    let todo: Vec<String> = tasks
        .iter()
        .filter(|t| {
            t.get("accept_status").and_then(Value::as_str) == Some("not_accepted")
                && t.get("task_code")
                    .and_then(Value::as_str)
                    .is_some_and(|c| !is_skipped(c))
        })
        .filter_map(|t| t.get("task_code").and_then(Value::as_str).map(String::from))
        .collect();
    if todo.is_empty() {
        return vec![];
    }
    let resp = req(
        account,
        "POST",
        "/activity/growth/tasks/accept",
        Some(json!({ "task_codes": todo })),
    )
    .await;
    resp.get("data")
        .and_then(|d| d.get("results"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter(|row| row.get("status").and_then(Value::as_str) == Some("accepted"))
                .filter_map(|row| {
                    row.get("task_code")
                        .and_then(Value::as_str)
                        .map(String::from)
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn claim(account: &Value, code: &str) -> Value {
    req(
        account,
        "POST",
        &format!("/activity/growth/tasks/{code}/claim"),
        Some(json!({})),
    )
    .await
}

// ---------------------------------------------------------------------------
// 真实对话（chat 类任务的必要条件）
// ---------------------------------------------------------------------------

/// 建会话并真发一次对话，返回 `(conversationId, 回复全文)`。
///
/// SSE 响应体这里整段读完再按 `data:` 行解析 —— 省掉 reqwest 的 `stream` feature，
/// 对话回复本身很短，内存代价可忽略。
async fn webchat(
    account: &Value,
    prompt: &str,
    meta: Option<Value>,
    conv_name: &str,
) -> (String, String) {
    let conv = req(
        account,
        "POST",
        "/console/webchat/conversations",
        Some(json!({ "name": format!("{conv_name}-{}", short_uuid()) })),
    )
    .await;
    let conv_id = conv
        .get("data")
        .and_then(|d| d.get("conversationId"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if conv_id.is_empty() {
        return (String::new(), String::new());
    }

    let mut payload = json!({
        "messages": [{ "role": "user", "content": prompt }],
        "model": "glm-5.2",
        "stream": true,
        "conversationId": conv_id,
    });
    if let Some(meta) = meta {
        payload["_meta"] = meta;
    }

    let mut headers = base_headers(account, "/");
    headers.insert("Accept".into(), "text/event-stream".into());
    let (status, _, body) = http_request_raw(
        &format!("{GROWTH_HOST}/console/chat/completions"),
        "POST",
        Some(payload),
        Some(&headers),
        None,
        true,
    )
    .await;
    if status == 0 {
        return (conv_id, String::new());
    }

    let mut text = String::new();
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" || data == "[完成]" {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if let Some(choices) = parsed.get("choices").and_then(Value::as_array) {
            for choice in choices {
                if let Some(content) = choice
                    .get("delta")
                    .and_then(|d| d.get("content"))
                    .and_then(Value::as_str)
                {
                    text.push_str(content);
                }
            }
        }
    }
    (conv_id, text)
}

/// 对话三连埋点（`chat_5` / `Model_chat_GLM5.2` 用）。
fn chat_events(account: &Value, conv_id: &str, prompt: &str, text: &str) -> Vec<Value> {
    let uid = uid_of(account);
    let nick = nick_of(account);
    let now = now_ms();
    let request_id = format!("cmb-{}", uuid::Uuid::new_v4());
    let machine = derive_id(&uid, "machine");
    let common = |extra: Value| {
        let mut base = json!({
            "userId": uid,
            "userNickname": nick,
            "ideName": "web-Agents",
            "ideType": "web-Agents",
            "machineId": machine,
            "mode": "CLOUD",
            "userAgent": UA,
            "os": "Win32",
            "timezone": "Asia/Shanghai",
        });
        if let (Some(dst), Some(src)) = (base.as_object_mut(), extra.as_object()) {
            for (k, v) in src {
                dst.insert(k.clone(), v.clone());
            }
        }
        base
    };
    let input_tokens = (prompt.chars().count() / 4).max(1) as i64;
    let output_tokens = (text.chars().count() / 4).max(1) as i64;
    vec![
        common(json!({
            "eventCode": "chat_request_send",
            "conversationId": conv_id,
            "requestId": request_id,
            "requestModelId": "glm-5.2",
            "requestModelName": "GLM-5.2",
            "inputLength": prompt.chars().count(),
            "customAgentName": "",
        })),
        common(json!({
            "eventCode": "chat_request_response",
            "timestamp": now + 100,
            "conversationId": conv_id,
            "requestId": request_id,
            "requestModelId": "glm-5.2",
            "requestModelName": "GLM-5.2",
            "toolCallCount": 0,
            "inputToken": input_tokens,
            "outputToken": output_tokens,
            "totalToken": input_tokens + output_tokens,
        })),
        common(json!({
            "eventCode": "chat_message_send",
            "timestamp": now + 50,
            "conversationId": conv_id,
            "requestId": request_id,
            "messageId": format!("cmb-{}", uuid::Uuid::new_v4()),
            "requestModelId": "glm-5.2",
            "requestModelName": "GLM-5.2",
            "historyCount": 1,
            "isContextTruncated": false,
            "currentStepCount": 1,
            "traceId": request_id,
            "rootRequestId": request_id,
            "parentConversationId": conv_id,
            "agentName": "cli",
            "agentType": "main",
        })),
    ]
}

// ---------------------------------------------------------------------------
// 专家 / 专家团名单（公开市场配置，无需鉴权）
// ---------------------------------------------------------------------------

fn short_uuid() -> String {
    uuid::Uuid::new_v4().to_string()[..8].to_string()
}

/// 市场配置里的多语言字段取中文优先。
fn cn_of(value: &Value) -> String {
    if let Some(obj) = value.as_object() {
        return obj
            .get("zh")
            .or_else(|| obj.get("en"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
    }
    value.as_str().unwrap_or("").to_string()
}

static MARKET_CACHE: OnceLock<Mutex<Option<Value>>> = OnceLock::new();

async fn market() -> Value {
    let cache = MARKET_CACHE.get_or_init(|| Mutex::new(None));
    if let Some(cached) = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
    {
        return cached;
    }
    let mut headers = HashMap::new();
    headers.insert("User-Agent".to_string(), UA.to_string());
    headers.insert("Accept".to_string(), "application/json".to_string());
    let resp = http_request(EXPERT_MARKET, "GET", None, Some(&headers)).await;
    if resp.is_null() {
        return json!({ "experts": [] });
    }
    *cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(resp.clone());
    resp
}

/// 市场条目展示名：`displayName` 优先，回落到 `name`。
fn expert_name(entry: &Value) -> String {
    let display = cn_of(entry.get("displayName").unwrap_or(&Value::Null));
    if display.is_empty() {
        cn_of(entry.get("name").unwrap_or(&Value::Null))
    } else {
        display
    }
}

/// 专家团（`expertType == "team"`）。
async fn expert_teams(count: usize) -> Vec<Value> {
    let market = market().await;
    market
        .get("experts")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter(|e| {
                    e.get("_meta")
                        .and_then(|m| m.get("expertType"))
                        .and_then(Value::as_str)
                        == Some("team")
                        || e.get("expertType").and_then(Value::as_str) == Some("team")
                })
                .map(|e| {
                    let name = expert_name(e);
                    json!({
                        "id": e.get("id").cloned().unwrap_or(Value::Null),
                        "name": name,
                        "profession": cn_of(e.get("profession").unwrap_or(&Value::Null)),
                        "industryId": e.get("_meta").and_then(|m| m.get("industryId"))
                            .or_else(|| e.get("industryId")).cloned().unwrap_or(Value::Null),
                    })
                })
                .collect::<Vec<_>>()
        })
        .map(|list| list.into_iter().take(count).collect())
        .unwrap_or_default()
}

/// 普通专家（含「轻量云 / lighthouse」专家）。
async fn normal_experts(count: usize) -> Vec<Value> {
    let market = market().await;
    market
        .get("experts")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|e| {
                    let name = expert_name(e);
                    json!({
                        "id": e.get("id").cloned().unwrap_or(Value::Null),
                        "name": name,
                        "profession": cn_of(e.get("profession").unwrap_or(&Value::Null)),
                    })
                })
                .collect::<Vec<_>>()
        })
        .map(|list| list.into_iter().take(count).collect())
        .unwrap_or_default()
}

/// 从官方市场按关键词拉专家（`expert_5` / 体验类专家任务用）。
///
/// 真实端点是 **POST** `/v2/operation-platform/market/expert/list`
/// （`{page,page_size,keyword,sort_by:"reco_rank",sort_order:"desc"}`，**免 template_id**，
/// 个人账号专家页搜索就是它）。旧 `GET /api/v2/open-platform/experts/search?q=` 已 404；
/// `POST /api/v2/open-platform/experts/search` 要 template_id（企业场景才可用）。
async fn search_experts(account: &Value, query: &str, count: usize) -> Vec<Value> {
    let resp = req(
        account,
        "POST",
        "/v2/operation-platform/market/expert/list",
        Some(json!({
            "page": 1,
            "page_size": count,
            "keyword": query,
            "sort_by": "reco_rank",
            "sort_order": "desc",
        })),
    )
    .await;
    let list = resp
        .get("data")
        .and_then(|d| d.get("experts"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    list.iter()
        .map(|e| {
            let id = e
                .get("expert_id")
                .or_else(|| e.get("id"))
                .cloned()
                .unwrap_or(Value::Null);
            let name = cn_of(e.get("display_name_zh").unwrap_or(&Value::Null));
            let name = if name.is_empty() {
                cn_of(e.get("profession_zh").unwrap_or(&Value::Null))
            } else {
                name
            };
            json!({
                "id": id,
                "name": name,
                "profession": cn_of(e.get("profession_zh").unwrap_or(&Value::Null)),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 配方（每个 task_code 一段埋点 / 真实动作）
// ---------------------------------------------------------------------------

async fn sleep(secs: u64) {
    tokio::time::sleep(Duration::from_secs(secs)).await;
}

/// 资料库：点一次介绍入口。
async fn t_library(account: &Value) {
    report(
        account,
        vec![json!({
            "eventCode": "web_element_click",
            "page_url": "https://www.workbuddy.cn/library",
            "element_id": "library_doc_intro_click",
            "element_name": "WorkBuddy资料库介绍",
        })],
    )
    .await;
    sleep(6).await;
}

/// 活动专家名单：桌面端专家中心的「精选」接口（POST，无需 template_id）。
///
/// 2026-10 批次任务专家（电商运营 / 商家宣传等）在这里有**真实 expert_id**；
/// 无畏契约等新专家不在名单里，靠 `search_experts`（market/expert/list）按关键词搜。
async fn featured_experts(account: &Value) -> Vec<Value> {
    let resp = req(account, "POST", "/api/v2/open-platform/featured", Some(json!({}))).await;
    let scenes = resp
        .get("data")
        .and_then(|d| d.get("scenes"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out: Vec<Value> = Vec::new();
    for scene in scenes {
        let Some(experts) = scene.get("experts").and_then(Value::as_array) else {
            continue;
        };
        for entry in experts {
            let id = entry
                .get("expert_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if id.is_empty()
                || out
                    .iter()
                    .any(|row| row.get("id").and_then(Value::as_str) == Some(id.as_str()))
            {
                continue;
            }
            let name = entry
                .get("profession_zh")
                .or_else(|| entry.get("display_name_zh"))
                .or_else(|| entry.get("profession_en"))
                .or_else(|| entry.get("display_name_en"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let profession = entry
                .get("profession_zh")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            out.push(json!({ "id": id, "name": name, "profession": profession }));
        }
    }
    out
}

/// 体验指定专家的通用配方（`Expert_lighthouse` / `WorkBuddy_Valorant` / `expert_use_*`）。
///
/// 与专家团同款闭环：**必须真发对话并带 `_meta` growthEvent**，只发埋点进度不动
/// （2026-10 批次任务描述均为「搜索并召唤 XX 专家，完成一次对话」）。
/// 找专家的顺序：精选活动名单（真实 expert_id）→ 市场服务端搜索 → 市场配置按名过滤
/// → 伪 id 兜底。
async fn t_expert_use(account: &Value, code: &str, keywords: &[&str], fallback_name: &str) {
    let uid = uid_of(account);
    let prompt = "你好，请简单介绍一下你能帮我做什么，回答OK即可";
    for _ in 0..2 {
        let Some(p) = progress_of(account, code).await else {
            return;
        };
        if !needs_work(&p) {
            return;
        }
        let name_hit = |expert: &Value| {
            let name = expert.get("name").and_then(Value::as_str).unwrap_or("");
            // 市场配置的 name 是昵称（如「斗音音」），职业名（如「抖音策略师」）在
            // profession 字段里——关键词必须连它一起匹配，否则只会命中精选名单。
            let profession = expert
                .get("profession")
                .and_then(Value::as_str)
                .unwrap_or("");
            let id = expert
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_lowercase();
            keywords
                .iter()
                .any(|kw| name.contains(kw) || profession.contains(kw) || id.contains(&kw.to_lowercase()))
        };
        let mut found = featured_experts(account)
            .await
            .into_iter()
            .find(&name_hit);
        // 市场服务端搜索（market/expert/list，免 template_id）——无畏契约等
        // 活动专家只在这里有真实 expert_id。
        if found.is_none() {
            for kw in keywords {
                if let Some(hit) = search_experts(account, kw, 5).await.into_iter().find(&name_hit) {
                    found = Some(hit);
                    break;
                }
            }
        }
        if found.is_none() {
            found = normal_experts(300).await.into_iter().find(&name_hit);
        }
        let expert = found.unwrap_or_else(|| {
            json!({ "id": format!("expert-{}", short_uuid()), "name": fallback_name, "profession": "" })
        });
        let request_id = uuid::Uuid::new_v4().to_string();
        let message_id = format!("cmb-{}", uuid::Uuid::new_v4());
        // growthEvent 是**字符串**形式的 JSON 数组；驼峰 `ExpertActualUse` 与
        // 常规埋点 `expert_actual_use` 是两个事件，两个都要发。
        let growth_event = json!([{
            "eventCode": "ExpertActualUse",
            "id": expert["id"],
            "expertType": "agent",
            "extra": {
                "name": expert["name"],
                "expertTitle": expert["profession"],
                "type": "agent",
                "expertType": "agent",
                "source": "builtin",
                "version": "",
                "cost": 0,
                "characterCount": prompt.chars().count(),
                "requestId": request_id,
                "messageId": message_id,
                "requestModelId": "glm-5.2",
                "requestModelName": "GLM-5.2",
            },
        }]);
        let meta = json!({
            "codebuddy.ai": {
                "growthEvent": growth_event.to_string(),
                "promptRequestId": request_id,
                "clientSendTime": now_ms(),
                "userId": uid,
                "mode": "craft",
                "model": "glm-5.2",
                "expertId": expert["id"],
                "expert": {
                    "id": expert["id"],
                    "name": expert["name"],
                    "profession": expert["profession"],
                    "prompt": prompt,
                },
                "tags": [format!("expert:{}", expert["id"].as_str().unwrap_or(""))],
            },
        });
        let (conv_id, _text) = webchat(account, prompt, Some(meta), "expert").await;
        report(
            account,
            vec![json!({
                "eventCode": "expert_actual_use", "id": expert["id"], "name": expert["name"],
                "expertTitle": expert["profession"], "type": "agent",
                "expertType": "agent", "source": "builtin", "version": "", "cost": 0,
                "characterCount": prompt.chars().count(), "conversationId": conv_id,
                "requestId": request_id, "messageId": message_id,
                "requestModelId": "glm-5.2", "requestModelName": "GLM-5.2",
            })],
        )
        .await;
        sleep(5).await;
        if let Some(p) = progress_of(account, code).await {
            if !needs_work(&p) {
                return;
            }
        }
        if conv_id.is_empty() {
            // 对话都发不出去（token 失效 / 接口变更），别空转烧当日次数
            return;
        }
    }
}

/// 腾讯轻量云专家：任务要求「召唤专家，授权连接器后对话一次」——走真对话闭环。
async fn t_expert_lighthouse(account: &Value) {
    t_expert_use(
        account,
        "Expert_lighthouse",
        &["轻量云", "轻量", "lighthouse"],
        "腾讯轻量云专家",
    )
    .await;
}

/// 发现应用列表第一个真实应用 `(application_id, application_name)`。
async fn buddy_app_first(account: &Value) -> Option<(String, String)> {
    let resp = req(
        account,
        "GET",
        "/api/v1/open-platform/buddy_application?limit=10",
        None,
    )
    .await;
    let apps = resp["data"]
        .as_array()
        .or_else(|| resp["data"]["applications"].as_array())?;
    let app = apps.first()?;
    Some((
        app.get("application_id")
            .or_else(|| app.get("id"))?
            .as_str()?
            .to_string(),
        app.get("application_name")
            .or_else(|| app.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("Buddy 应用")
            .to_string(),
    ))
}

/// 发现应用：**授权全链路** + 资产使用上报 + 桌面遥测五连。
///
/// 桌面端「进入应用」的真实服务端动作（`runBuddyAppSwitch`）：
/// 1. GET `/workbuddyopen/v1/authorize/status` → 拿 redirect_uris/allowed_scopes
/// 2. 未授权 → POST `authorize/consent` `{application_id, redirect_uri, approve:true, granted_scopes}` → code
/// 3. POST `asset-usage/report` `{asset_id, asset_type:"buddy_app"}`
/// 4. 遥测五连（`buddyapp_discover_click` → `buddyapp_enter_click` → …）
async fn t_buddy_app(account: &Value) {
    let Some((id, name)) = buddy_app_first(account).await else {
        return; // 应用列表都拉不到，事件也没法带真实 id
    };
    // 1) status：拿授权要素
    let status = req(
        account,
        "GET",
        &format!("/workbuddyopen/v1/authorize/status?application_id={id}"),
        None,
    )
    .await;
    let redirect_uri = status["application"]["redirect_uris"]
        .as_array()
        .and_then(|u| u.first())
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let scopes: Vec<Value> = status["application"]["allowed_scopes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let authorized = status["authorized"]
        .as_bool()
        .or_else(|| status["data"]["authorized"].as_bool())
        .unwrap_or(false);
    // 2) 未授权 → consent（approve:true 即「进入应用」的授权动作）
    if !authorized {
        let _ = req(
            account,
            "POST",
            "/workbuddyopen/v1/authorize/consent",
            Some(json!({
                "application_id": id,
                "redirect_uri": redirect_uri,
                "approve": true,
                "granted_scopes": scopes,
            })),
        )
        .await;
    }
    // 3) 资产使用上报
    let _ = req(
        account,
        "POST",
        "/api/v2/open-platform/asset-usage/report",
        Some(json!({ "asset_id": id, "asset_type": "buddy_app" })),
    )
    .await;
    // 4) 桌面遥测五连（带真实应用 id）
    let id = id.as_str();
    let name = name.as_str();
    let mk = |code: &str, extra: Value| {
        let mut base = json!({
            "eventCode": code, "mode": "LOCAL", "buddyId": id, "buddyName": name,
        });
        if let (Some(dst), Some(src)) = (base.as_object_mut(), extra.as_object()) {
            for (k, v) in src {
                dst.insert(k.clone(), v.clone());
            }
        }
        base
    };
    report_desktop(
        account,
        vec![
            mk("buddyapp_discover_click", json!({})),
            mk(
                "buddyapp_show",
                json!({ "elementId": id, "elementName": name, "position": 2 }),
            ),
            mk(
                "buddyapp_enter_click",
                json!({ "elementId": id, "elementName": name, "position": 2, "isFirstPage": "1" }),
            ),
            mk(
                "buddyapp_auth_confirm_click",
                json!({ "elementId": id, "elementName": name }),
            ),
            mk(
                "buddyapp_bindaccount_skip_click",
                json!({ "elementId": id, "elementName": name }),
            ),
        ],
    )
    .await;
    sleep(4).await;
}

/// 画布 / 自动化 / 灵感：三个 task_code 一起推进。
async fn t_canvas_automation_playbook(account: &Value) {
    report(
        account,
        vec![
            json!({
                "eventCode": "agent_task_created", "source": "CLOUD", "name": "",
                "mode": "craft", "requestModelId": "default", "task_mode": "design",
            }),
            json!({ "eventCode": "wbx_design_canvas_task_create" }),
        ],
    )
    .await;
    sleep(3).await;

    report(
        account,
        vec![
            json!({
                "eventCode": "agent_task_created", "source": "CLOUD", "name": "",
                "mode": "craft", "requestModelId": "default", "task_mode": "automation",
                "isAutomationBackground": true,
            }),
            json!({ "eventCode": "automated_task_create_suc", "action": "create" }),
            json!({ "eventCode": "automated_task_execute", "action": "execute" }),
        ],
    )
    .await;
    sleep(3).await;

    report(
        account,
        vec![json!({
            "eventCode": "playbook_prompt_send", "ext1": uuid::Uuid::new_v4().to_string(),
            "requestId": uuid::Uuid::new_v4().to_string(), "id": "01-ProductDesign",
            "name": "产品设计", "type": "other", "promptLength": 30,
            "isOfficial": 1, "source": "growth-center",
        })],
    )
    .await;
    sleep(3).await;
}

/// 尝鲜技能：真装一个市场插件，再补 `skill_info`。
async fn t_skill(account: &Value) {
    let sources = req(account, "GET", "/console/as/marketplace/sources", None).await;
    let marketplace_id = sources
        .get("data")
        .and_then(|d| d.get("sources"))
        .and_then(Value::as_array)
        .and_then(|list| list.first())
        .and_then(|s| s.get("id"))
        .cloned();
    if let Some(mid) = marketplace_id {
        req(
            account,
            "POST",
            "/console/as/user/plugins/install",
            Some(json!({ "plugin_name": "qq-mail", "marketplace_id": mid, "version": "latest" })),
        )
        .await;
    }
    let skill_id = derive_id(&uid_of(account), "skill");
    report(
        account,
        vec![json!({
            "eventCode": "skill_info", "skillId": format!("skill-{}", &skill_id[..12]),
            "skillName": "QQ邮箱", "mode": "LOCAL", "source": "builtin",
        })],
    )
    .await;
    sleep(5).await;
}

/// 桌面端对话：`desktop_chat_sequence` 六连。
async fn t_rich_meow(account: &Value) {
    let uid = uid_of(account);
    let conv = format!("fp-rm-{}", derive_id(&uid, "rm-conv"));
    let request_id = format!("fp-rm-req-{}", derive_id(&uid, "rm-req"));
    let message_id = format!("fp-rm-msg-{}", derive_id(&uid, "rm-msg"));
    let step = |n: i64, action: &str| {
        json!({
            "eventCode": "desktop_chat_sequence", "conversationId": conv,
            "requestId": request_id, "messageId": message_id, "step": n, "action": action,
        })
    };
    report(
        account,
        vec![
            step(1, "open_app"),
            step(2, "new_conversation"),
            step(3, "send_message"),
            step(4, "receive_response"),
            step(5, "conversation_end"),
            step(6, "close"),
        ],
    )
    .await;
    sleep(5).await;
}

/// 召唤 5 次专家：每次取一个不同专家，召唤 + 实际使用。
async fn t_expert_5(account: &Value) {
    let experts = search_experts(account, "", 20).await;
    for i in 0..5 {
        let Some(p) = progress_of(account, "expert_5").await else {
            break;
        };
        if !needs_work(&p) || p.cur >= p.tgt.max(5) {
            break;
        }
        let expert = experts.get(i % experts.len().max(1)).cloned().unwrap_or_else(|| {
            json!({ "id": format!("expert-{}", short_uuid()), "name": "Expert", "profession": "" })
        });
        let conv_id = format!("conv-{}", uuid::Uuid::new_v4());
        let request_id = uuid::Uuid::new_v4().to_string();
        report(
            account,
            vec![
                json!({
                    "eventCode": "expert_summoned", "id": expert["id"], "name": expert["name"],
                    "type": "agent", "expertTitle": expert["profession"], "expertType": "agent",
                }),
                json!({
                    "eventCode": "expert_actual_use", "id": expert["id"], "name": expert["name"],
                    "type": "", "expertType": "agent", "source": "builtin", "version": "", "cost": 0,
                    "characterCount": 12, "conversationId": conv_id, "requestId": request_id,
                    "messageId": uuid::Uuid::new_v4().to_string(),
                    "requestModelId": "deepseek-v4-flash", "requestModelName": "DeepSeek V4 Flash",
                }),
            ],
        )
        .await;
        sleep(3).await;
    }
}

/// 召唤 3 次专家团：**必须真发对话并带 `_meta`**，否则进度不动。
async fn t_expert_team_3(account: &Value) {
    let teams = expert_teams(12).await;
    if teams.is_empty() {
        return;
    }
    let uid = uid_of(account);
    let prompt = "你好，请简单介绍一下你们团队能帮我做什么，回答OK即可";
    for round in 0..3 {
        let Some(p) = progress_of(account, "Expert_team_use_3").await else {
            break;
        };
        if !needs_work(&p) || p.cur >= p.tgt.max(3) {
            break;
        }
        let team = &teams[round % teams.len()];
        let request_id = uuid::Uuid::new_v4().to_string();
        let message_id = format!("cmb-{}", uuid::Uuid::new_v4());
        // growthEvent 是**字符串**形式的 JSON 数组；eventCode 用驼峰 `ExpertActualUse`。
        let growth_event = json!([{
            "eventCode": "ExpertActualUse",
            "id": team["id"],
            "expertType": "team",
            "extra": {
                "name": team["name"],
                "expertTitle": team["profession"],
                "type": team["industryId"],
                "expertType": "team",
                "source": "builtin",
                "version": "",
                "cost": 8,
                "characterCount": prompt.chars().count(),
                "requestId": request_id,
                "messageId": message_id,
                "requestModelId": "glm-5.2",
                "requestModelName": "GLM-5.2",
            },
        }]);
        let meta = json!({
            "codebuddy.ai": {
                "growthEvent": growth_event.to_string(),
                "promptRequestId": request_id,
                "clientSendTime": now_ms(),
                "userId": uid,
                "mode": "craft",
                "model": "glm-5.2",
                "expertId": team["id"],
                "expert": {
                    "id": team["id"],
                    "name": team["name"],
                    "profession": team["profession"],
                    "prompt": prompt,
                },
                "tags": [format!("expert:{}", team["id"].as_str().unwrap_or(""))],
            },
        });
        let (conv_id, text) = webchat(account, prompt, Some(meta), "team").await;
        // 常规埋点：下划线 `expert_actual_use`，与上面的驼峰事件是**两个**事件。
        report(
            account,
            vec![json!({
                "eventCode": "expert_actual_use", "id": team["id"], "name": team["name"],
                "expertTitle": team["profession"], "type": team["industryId"],
                "expertType": "team", "source": "builtin", "version": "", "cost": 8,
                "characterCount": prompt.chars().count(), "conversationId": conv_id,
                "requestId": request_id, "messageId": message_id,
                "requestModelId": "glm-5.2", "requestModelName": "GLM-5.2",
            })],
        )
        .await;
        let _ = text;
        sleep(5).await;
    }
}

/// 使用 5 个模板：五个不同场景各发一次灵感提示。
async fn t_template_5(account: &Value) {
    let scenes = [
        ("01-ProductDesign", "产品设计"),
        ("02-Marketing", "营销文案"),
        ("03-DataAnalysis", "数据分析"),
        ("04-CodeReview", "代码审查"),
        ("05-Report", "报告撰写"),
    ];
    for (id, name) in scenes {
        let Some(p) = progress_of(account, "template_5").await else {
            break;
        };
        if !needs_work(&p) || p.cur >= p.tgt.max(5) {
            break;
        }
        report(
            account,
            vec![
                json!({
                    "eventCode": "agent_task_created", "source": "CLOUD", "name": "",
                    "mode": "craft", "requestModelId": "default", "action": id,
                    "has_template": true, "template_id": id, "template_name": name,
                }),
                json!({
                    "eventCode": "agent_task_created_with_template", "templateId": id,
                    "templateName": name, "isCustomModel": true, "id": id, "name": name,
                }),
                json!({
                    "eventCode": "playbook_prompt_send", "ext1": uuid::Uuid::new_v4().to_string(),
                    "requestId": uuid::Uuid::new_v4().to_string(), "id": id, "name": name,
                    "type": "other", "promptLength": 30, "isOfficial": 1, "source": "growth-center",
                }),
            ],
        )
        .await;
        sleep(2).await;
    }
}

/// 聊天类任务：真发对话，再补三连埋点。
async fn t_chat_n(account: &Value, code: &str, rounds: usize, prompts: &[&str]) {
    for i in 0..rounds {
        let Some(p) = progress_of(account, code).await else {
            break;
        };
        if !needs_work(&p) || p.cur >= p.tgt.max(rounds as i64) {
            break;
        }
        let prompt = prompts[i % prompts.len()];
        let (conv_id, text) = webchat(account, prompt, None, "growth").await;
        if !text.is_empty() {
            report(account, chat_events(account, &conv_id, prompt, &text)).await;
        }
        sleep(4).await;
    }
}

// ---------------------------------------------------------------------------
// 当日尝试次数（内存态，按 账号+日期 归零）
// ---------------------------------------------------------------------------

fn tries_scope(account: &Value) -> String {
    format!(
        "{}|{}",
        account_key(account),
        crate::modules::checkin::date_str(None)
    )
}

fn tries_get(scope: &str, code: &str) -> i64 {
    TRIES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .get(scope)
        .and_then(|m| m.get(code).copied())
        .unwrap_or(0)
}

fn tries_bump(scope: &str, code: &str) {
    let mut guard = TRIES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap();
    // 只保留当天的 scope（跨天归零），避免内存里按天累积。
    if let Some(date) = scope.rsplit('|').next() {
        let suffix = format!("|{date}");
        guard.retain(|key, _| key.ends_with(&suffix));
    }
    *guard
        .entry(scope.to_string())
        .or_default()
        .entry(code.to_string())
        .or_insert(0) += 1;
}

/// 是否该推进这个任务：仍有缺口、未跳过、当日尝试未超限。
async fn gate(account: &Value, scope: &str, code: &str, max_tries: i64) -> bool {
    if is_skipped(code) || is_manual(code) {
        return false;
    }
    if tries_get(scope, code) >= max_tries {
        return false;
    }
    match progress_of(account, code).await {
        Some(p) => needs_work(&p),
        None => false,
    }
}

/// 一个配方跑完后：复查进度，达标即领奖并记日志。
async fn settle(account: &Value, code: &str, results: &mut Vec<Value>, claimed: &mut Vec<Value>) {
    tries_bump(&tries_scope(account), code);
    let Some(p) = progress_of(account, code).await else {
        results.push(json!({ "code": code, "status": "not_found" }));
        return;
    };
    let mut row = json!({ "code": code, "status": p.status, "current": p.cur, "target": p.tgt });
    if p.status == "completed" {
        let resp = claim(account, code).await;
        let data = resp.get("data").cloned().unwrap_or_else(|| json!({}));
        if resp.get("code").and_then(Value::as_i64) == Some(0) {
            let credit = data.get("credit").and_then(Value::as_i64);
            let energy = data.get("energy").and_then(Value::as_i64);
            row["claimed"] = json!(true);
            row["credit"] = json!(credit);
            row["energy"] = json!(energy);
            claimed.push(json!({ "code": code, "credit": credit, "energy": energy }));
            add_growth_log(&json!({
                "ts": now_ms(),
                "accountId": account.get("id").cloned().unwrap_or(Value::Null),
                "email": account_display_name(account),
                "variant": variant_of(account).as_str(),
                "result": "claimed",
                "task": code,
                "credit": credit,
                "energy": energy,
            }));
        } else {
            row["claimed"] = json!(false);
            row["error"] = json!(resp
                .get("message")
                .or_else(|| resp.get("msg"))
                .and_then(Value::as_str)
                .unwrap_or("领奖失败"));
        }
    }
    results.push(row);
}

// ---------------------------------------------------------------------------
// 单账号 / 整轮
// ---------------------------------------------------------------------------

/// 测试探测用：站内专家搜索（只读，不走埋点）。
pub async fn search_experts_pub(account: &Value, query: &str, count: usize) -> Vec<Value> {
    search_experts(account, query, count).await
}

/// 测试探测用：专家搜索接口原始响应（只读），用于核对结构。
/// 桌面端真实端点是 **POST** `{keyword, page, page_size}`（GET q= 会 404）。
pub async fn search_experts_raw_pub(account: &Value, query: &str, count: usize) -> Value {
    req(
        account,
        "POST",
        "/api/v2/open-platform/experts/search",
        Some(json!({
            "keyword": query,
            "page": 1,
            "page_size": count,
        })),
    )
    .await
}

/// 测试探测用：raw 自由体（核对必填字段时暴力试探）。
pub async fn raw_post_pub(account: &Value, path: &str, body: Value) -> Value {
    req(account, "POST", path, Some(body)).await
}

/// 测试探测用：raw GET。
pub async fn raw_get_pub(account: &Value, path: &str) -> Value {
    req(account, "GET", path, None).await
}

/// 对单个账号跑一轮完整流程：接取 → 逐配方补缺口 → 完成即领奖。
pub async fn run_growth_account(account: &Value, cfg: &Value) -> Value {
    let max_tries = cfg
        .get("max_tries")
        .and_then(Value::as_i64)
        .unwrap_or(GROWTH_DEFAULT_MAX_TRIES);
    let scope = tries_scope(account);
    let mut results: Vec<Value> = Vec::new();
    let mut claimed: Vec<Value> = Vec::new();

    // 1) 接取全部未接取任务
    let tasks = list_tasks(account).await;
    if tasks.is_empty() {
        return json!({
            "accountId": account.get("id").cloned().unwrap_or(Value::Null),
            "email": account_display_name(account),
            "result": "error",
            "error": "任务清单为空（token 失效或接口变更）",
        });
    }
    let accepted = accept_missing(account, &tasks).await;

    // 2) 先清账：已完成但未领取的任务直接领奖。
    //    gate() 把 completed 视为「无需推进」→ 配方不跑 → settle 不执行，
    //    这类任务会永久卡住（2026-10-31 实测两个账号各卡 2 个，白丢 400 分）。
    let completed: Vec<String> = tasks
        .iter()
        .filter(|t| {
            t.get("accept_status").and_then(Value::as_str) == Some("completed")
                && t.get("task_code")
                    .and_then(Value::as_str)
                    .is_some_and(|c| !is_skipped(c))
        })
        .filter_map(|t| t.get("task_code").and_then(Value::as_str).map(String::from))
        .collect();
    for code in completed {
        settle(account, &code, &mut results, &mut claimed).await;
    }

    // 2) 逐配方补缺口（顺序对齐参考实现：轻 → 重）
    if gate(account, &scope, "Library_read", max_tries).await {
        t_library(account).await;
        settle(account, "Library_read", &mut results, &mut claimed).await;
    }
    if gate(account, &scope, "Expert_lighthouse", max_tries).await {
        t_expert_lighthouse(account).await;
        settle(account, "Expert_lighthouse", &mut results, &mut claimed).await;
    }
    // 体验类专家任务（2026-10 批次）：都是「搜索并召唤 XX 专家，完成一次对话」，
    // 统一走真对话闭环；找不到专家时对话仍会发，进度不动则等下一轮再试。
    if gate(account, &scope, "WorkBuddy_Valorant", max_tries).await {
        t_expert_use(account, "WorkBuddy_Valorant", &["无畏契约"], "无畏契约拼豆专家").await;
        settle(account, "WorkBuddy_Valorant", &mut results, &mut claimed).await;
    }
    if gate(account, &scope, "expert_use_maidehao", max_tries).await {
        t_expert_use(account, "expert_use_maidehao", &["电商运营"], "中国电商运营专家").await;
        settle(account, "expert_use_maidehao", &mut results, &mut claimed).await;
    }
    if gate(account, &scope, "expert_use_douyinyin", max_tries).await {
        t_expert_use(account, "expert_use_douyinyin", &["抖音"], "抖音策略师").await;
        settle(account, "expert_use_douyinyin", &mut results, &mut claimed).await;
    }
    if gate(account, &scope, "expert_use_shangjia_xuanchuan", max_tries).await {
        t_expert_use(
            account,
            "expert_use_shangjia_xuanchuan",
            &["商家宣传"],
            "商家宣传页与海报设计师",
        )
        .await;
        settle(account, "expert_use_shangjia_xuanchuan", &mut results, &mut claimed).await;
    }
    if gate(account, &scope, "expert_use_bowangwang", max_tries).await {
        t_expert_use(account, "expert_use_bowangwang", &["直播电商"], "直播电商教练").await;
        settle(account, "expert_use_bowangwang", &mut results, &mut claimed).await;
    }
    if gate(account, &scope, "Buddy_App", max_tries).await {
        t_buddy_app(account).await;
        settle(account, "Buddy_App", &mut results, &mut claimed).await;
    }
    // 这一个配方推进三个 task_code，三个都要 settlements。
    let canvas_due = gate(account, &scope, "create_canvas", max_tries).await
        || gate(account, &scope, "automation_1", max_tries).await
        || gate(account, &scope, "playbook_prompt", max_tries).await;
    if canvas_due {
        t_canvas_automation_playbook(account).await;
        for code in ["create_canvas", "automation_1", "playbook_prompt"] {
            settle(account, code, &mut results, &mut claimed).await;
        }
    }
    if gate(account, &scope, "skill_1", max_tries).await {
        t_skill(account).await;
        settle(account, "skill_1", &mut results, &mut claimed).await;
    }
    if gate(account, &scope, "RichMeow_Chat", max_tries).await {
        t_rich_meow(account).await;
        settle(account, "RichMeow_Chat", &mut results, &mut claimed).await;
    }
    if gate(account, &scope, "expert_5", max_tries).await {
        t_expert_5(account).await;
        settle(account, "expert_5", &mut results, &mut claimed).await;
    }
    if gate(account, &scope, "Expert_team_use_3", max_tries).await {
        t_expert_team_3(account).await;
        settle(account, "Expert_team_use_3", &mut results, &mut claimed).await;
    }
    if gate(account, &scope, "template_5", max_tries).await {
        t_template_5(account).await;
        settle(account, "template_5", &mut results, &mut claimed).await;
    }
    if gate(account, &scope, "chat_5", max_tries).await {
        t_chat_n(
            account,
            "chat_5",
            5,
            &[
                "你好",
                "今天天气怎么样？",
                "1+1等于几？",
                "Python是什么？",
                "推荐一本好书",
            ],
        )
        .await;
        settle(account, "chat_5", &mut results, &mut claimed).await;
    }
    if gate(account, &scope, "Model_chat_GLM5.2", max_tries).await {
        t_chat_n(account, "Model_chat_GLM5.2", 1, &["你好，请介绍一下你自己"]).await;
        settle(account, "Model_chat_GLM5.2", &mut results, &mut claimed).await;
    }

    // 3) 收尾：把「没做」的任务补成显式行，UI 才能说清原因。
    //    原来 gate 返回 false 什么都不留，界面只有「跳过 N」，等于不可诊断。
    for task in &tasks {
        let Some(code) = task.get("task_code").and_then(Value::as_str) else {
            continue;
        };
        if code.is_empty()
            || results
                .iter()
                .any(|r| r.get("code").and_then(Value::as_str) == Some(code))
        {
            continue;
        }
        let p = parse_progress(task);
        // 已领完 / 无缺口的不啰嗦，只标「有缺口却没推进」的。
        if !needs_work(&p) {
            continue;
        }
        let reason = if is_skipped(code) {
            "manual_skip"
        } else if is_manual(code) {
            "manual_required"
        } else if tries_get(&scope, code) >= max_tries {
            "tries_exhausted"
        } else {
            "no_recipe"
        };
        results.push(json!({
            "code": code,
            "status": p.status,
            "current": p.cur,
            "target": p.tgt,
            "skipped": reason,
            // 官方给的一键入口（workbuddy:// 深链或活动页），UI 直接给按钮。
            "title": task.get("title").cloned().unwrap_or(Value::Null),
            "jumpUrl": task.get("jump_url").cloned().unwrap_or(Value::Null),
        }));
    }

    let credit_total: i64 = claimed
        .iter()
        .filter_map(|c| c.get("credit").and_then(Value::as_i64))
        .sum();
    // 每账号每轮一条汇总：没做哪些、为什么。claim 明细仍单独记，便于按任务回溯。
    let pending: Vec<Value> = results
        .iter()
        .filter(|r| r.get("skipped").is_some())
        .cloned()
        .collect();
    if !pending.is_empty() {
        add_growth_log(&json!({
            "ts": now_ms(),
            "accountId": account.get("id").cloned().unwrap_or(Value::Null),
            "email": account_display_name(account),
            "variant": variant_of(account).as_str(),
            "result": "summary",
            "claimed": claimed.len(),
            "credit": credit_total,
            "pending": pending,
        }));
    }
    // 只按「真跑过的配方」判成败：skipped 说明行是给人看的，不能算失败证据。
    let attempted = results.iter().filter(|r| r.get("skipped").is_none()).count();
    let result = if claimed.is_empty() {
        if attempted == 0 {
            "none"
        } else {
            "partial"
        }
    } else {
        "success"
    };
    json!({
        "accountId": account.get("id").cloned().unwrap_or(Value::Null),
        "email": account_display_name(account),
        "result": result,
        "accepted": accepted,
        "tasks": results,
        "claimed": claimed,
        "credit": credit_total,
    })
}

fn excluded(account: &Value, cfg: &Value) -> bool {
    account.get("id").and_then(Value::as_str).is_some_and(|id| {
        cfg.get("excluded_account_ids")
            .and_then(Value::as_array)
            .is_some_and(|ids| ids.iter().any(|v| v.as_str() == Some(id)))
    })
}

/// 后台轮次：配置关闭时整轮短路。
pub async fn run_growth_cycle() -> Value {
    let Some(_guard) = RunFlagGuard::try_acquire(&GROWTH_RUNNING) else {
        return json!({ "status": "skipped", "reason": "already_running" });
    };
    let cfg = load_growth_config();
    if cfg.get("enabled").and_then(Value::as_bool) != Some(true) {
        return json!({ "status": "disabled" });
    }
    let accounts: Vec<Value> = load_accounts()
        .into_iter()
        .filter(|a| !excluded(a, &cfg))
        .collect();
    if accounts.is_empty() {
        return json!({ "status": "no_accounts" });
    }
    let mut rows = Vec::new();
    for account in &accounts {
        let acc = ensure_fresh_token(
            account.clone(),
            &crate::modules::config::load_checkin_config(),
        )
        .await;
        rows.push(run_growth_account(&acc, &cfg).await);
    }
    json!({ "status": "ok", "accounts": rows })
}

/// 手动入口：立即对全部（或指定档位）账号跑一轮，不看总开关（对齐签到/领取的
/// 「立即执行」语义），但仍跳过关闭该功能的账号。
pub async fn run_growth_all(variant: Option<WbVariant>) -> Value {
    let Some(_guard) = RunFlagGuard::try_acquire(&GROWTH_RUNNING) else {
        return json!({ "accounts": [], "status": "skipped", "reason": "already_running" });
    };
    let cfg = load_growth_config();
    let accounts: Vec<Value> = load_accounts()
        .into_iter()
        .filter(|a| variant.is_none_or(|target| variant_of(a) == target))
        .collect();
    let mut rows = Vec::new();
    for account in &accounts {
        if excluded(account, &cfg) {
            rows.push(json!({
                "accountId": account.get("id").cloned().unwrap_or(Value::Null),
                "email": account_display_name(account),
                "result": "skipped",
                "reason": "auto_growth_disabled",
            }));
            continue;
        }
        let acc = ensure_fresh_token(
            account.clone(),
            &crate::modules::config::load_checkin_config(),
        )
        .await;
        rows.push(run_growth_account(&acc, &cfg).await);
    }
    json!({ "accounts": rows })
}

/// 只读：返回每个账号的任务清单与进度（前端展示 `x/已完成` 用）。
pub async fn growth_status(variant: Option<WbVariant>) -> Value {
    let accounts: Vec<Value> = load_accounts()
        .into_iter()
        .filter(|a| variant.is_none_or(|target| variant_of(a) == target))
        .collect();
    let mut rows = Vec::new();
    for account in &accounts {
        let tasks = list_tasks(account).await;
        let total = tasks.len();
        let done = tasks
            .iter()
            .filter(|t| {
                matches!(
                    t.get("accept_status").and_then(Value::as_str),
                    Some("completed") | Some("claimed")
                )
            })
            .count();
        let claimable = tasks
            .iter()
            .filter(|t| t.get("accept_status").and_then(Value::as_str) == Some("completed"))
            .filter_map(|t| t.get("task_code").and_then(Value::as_str).map(String::from))
            .collect::<Vec<_>>();
        rows.push(json!({
            "accountId": account.get("id").cloned().unwrap_or(Value::Null),
            "email": account_display_name(account),
            "total": total,
            "done": done,
            "claimable": claimable,
            "tasks": tasks,
        }));
    }
    json!({ "accounts": rows })
}

/// 后台循环间隔（分钟）：读配置并限幅。
pub fn cycle_interval() -> Duration {
    let minutes = load_growth_config()
        .get("interval_minutes")
        .and_then(Value::as_i64)
        .unwrap_or(GROWTH_DEFAULT_INTERVAL_MINUTES)
        .clamp(5, 24 * 60);
    Duration::from_secs((minutes as u64) * 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_needs_work_semantics() {
        let done = Progress {
            status: "completed".into(),
            cur: 5,
            tgt: 5,
        };
        assert!(!needs_work(&done));
        let claimed = Progress {
            status: "claimed".into(),
            cur: 5,
            tgt: 5,
        };
        assert!(!needs_work(&claimed));
        let partial = Progress {
            status: "accepted".into(),
            cur: 2,
            tgt: 5,
        };
        assert!(needs_work(&partial));
        let unknown_target = Progress {
            status: "accepted".into(),
            cur: 0,
            tgt: 0,
        };
        assert!(needs_work(&unknown_target));
    }

    #[test]
    fn task_list_and_progress_parsing() {
        let resp = json!({"code": 0, "data": {"tasks": [
            {"task_code": "chat_5", "accept_status": "accepted", "progress": {"current": 3, "target": 5}},
            {"task_code": "expert_5", "accept_status": "completed", "progress": {"current": 5, "target": 5}}
        ]}});
        let tasks = parse_tasks(&resp);
        assert_eq!(tasks.len(), 2);
        assert_eq!(
            parse_progress(&tasks[0]),
            Progress {
                status: "accepted".into(),
                cur: 3,
                tgt: 5
            }
        );
        // 缺 progress 时按 0/0 处理，不 panic。
        assert_eq!(
            parse_progress(&json!({"task_code": "x"})),
            Progress {
                status: "".into(),
                cur: 0,
                tgt: 0
            }
        );
    }

    #[test]
    fn skipped_task_is_excluded() {
        assert!(is_skipped("Hp_Appearance"));
        assert!(!is_skipped("chat_5"));
    }

    #[test]
    fn derive_id_is_stable_and_32_hex() {
        let a = derive_id("user-1", "machine");
        let b = derive_id("user-1", "machine");
        assert_eq!(a, b);
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        // 不同 salt 必须不同，否则设备与会话指纹会撞。
        assert_ne!(
            derive_id("user-1", "machine"),
            derive_id("user-1", "session")
        );
    }

    #[test]
    fn uid_prefers_account_field_then_jwt() {
        assert_eq!(uid_of(&json!({"uid": "u-1"})), "u-1");
        // 无 uid 且 token 非法 → 空串，不 panic。
        assert_eq!(uid_of(&json!({"access_token": "bad"})), "");
    }

    #[test]
    fn cn_field_prefers_zh() {
        assert_eq!(
            cn_of(&json!({"zh": "软件开发团队", "en": "Dev"})),
            "软件开发团队"
        );
        assert_eq!(cn_of(&json!({"en": "Dev"})), "Dev");
        assert_eq!(cn_of(&json!("plain")), "plain");
    }

    #[test]
    fn chat_events_emit_three_events_with_shared_request_id() {
        let account = json!({"id": "a", "uid": "u", "nickname": "n"});
        let events = chat_events(&account, "conv-1", "你好", "你好呀");
        assert_eq!(events.len(), 3);
        let codes: Vec<&str> = events
            .iter()
            .map(|e| e.get("eventCode").and_then(Value::as_str).unwrap_or(""))
            .collect();
        assert_eq!(
            codes,
            vec![
                "chat_request_send",
                "chat_request_response",
                "chat_message_send"
            ]
        );
        let ids: Vec<&str> = events
            .iter()
            .map(|e| e.get("requestId").and_then(Value::as_str).unwrap_or(""))
            .collect();
        assert!(
            ids.windows(2).all(|w| w[0] == w[1]),
            "requestId 必须一致: {ids:?}"
        );
        // 输出 token 不能为 0，否则服务端可能不计入用量。
        assert!(events[1]["outputToken"].as_i64().unwrap_or(0) > 0);
    }
}
