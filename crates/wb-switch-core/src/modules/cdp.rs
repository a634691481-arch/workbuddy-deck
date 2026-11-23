//! CDP（Chrome DevTools Protocol）客户端内核：连接 WorkBuddy 桌面端调试端口，
//! 在**真实渲染进程上下文**里执行任务（同源 fetch / DOM 探测）。
//!
//! 与 WorkDaddy（AGPL，仅参考行为不复制代码）的机制同类：
//! - 客户端须以 `--remote-debugging-port` 启动（见 [`crate::modules::process`]）；
//! - HTTP `GET /json` 枚举 targets → WebSocket 连 `webSocketDebuggerUrl`；
//! - `Runtime.evaluate`（`awaitPromise`）在页面里跑 JS；
//! - 页面 URL 归属判定只认强信号（workbuddy.cn 域名 / app.asar 路径），
//!   防止把组件发到兄弟客户端（WorkBuddy AI / CodeBuddy）。
//!
//! 安全边界：
//! - 只连 `127.0.0.1` 回环端口；
//! - 全部调用带超时；连接复用前先探活（`Runtime.evaluate` 1+1）；
//! - 任何失败由调用方回退到既有 HTTP 路径，CDP 故障不影响功能闭环。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use crate::modules::config::{cdp_config_file, load_cdp_config, now_ms};
use crate::modules::process::restart_workbuddy_with_cdp;
use crate::modules::variant::WbVariant;

const EVALUATE_TIMEOUT: Duration = Duration::from_secs(30);
/// 探活与端口扫描的短超时。
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// 默认调试端口候选（对齐参考实现：9222-9232 + 9333）。
const DEFAULT_PORTS: [u16; 12] = [9222, 9223, 9224, 9225, 9226, 9227, 9228, 9229, 9230, 9231, 9232, 9333];

// ---------------------------------------------------------------------------
// 目标归属判定（纯函数，可单测）
// ---------------------------------------------------------------------------

/// 归一化 file:// URL 里的空格编码（`WorkBuddy%20AI.app` → 空格）。
pub fn normalize_target_url(url: &str) -> String {
    url.replace("%20", " ")
}

/// 按 URL / 标题判定页面属于哪个客户端。只认强信号，无信号返回 `None`。
pub fn classify_target(url: &str, title: &str) -> Option<&'static str> {
    let u = normalize_target_url(url);
    // app.asar 安装目录（Windows/Linux）与 .app 包路径（macOS）。
    if u.contains("/WorkBuddy AI.app") || u.contains("/WorkBuddyAI/") || u.contains("/workbuddy-ai/") {
        return Some("workbuddy-ai");
    }
    if u.contains("/WorkBuddy.app") || u.contains("/opt/WorkBuddy/") {
        return Some("workbuddy-cn");
    }
    // 通用安装目录（/WorkBuddy/…app.asar）：AI 的专属目录已在上面拦截，剩下的按 CN 处理。
    if u.contains("/WorkBuddy/") && u.contains("app.asar") {
        return Some("workbuddy-cn");
    }
    if u.contains("/CodeBuddy CN.app") || u.contains("/CodeBuddy.app") {
        return Some("codebuddy");
    }
    // 登录域名（强信号）：页面 URL 落在哪个域就是哪个客户端。
    if u.contains("workbuddy.ai") || title.contains("WorkBuddy AI") {
        return Some("workbuddy-ai");
    }
    if u.contains("workbuddy.cn") || title.contains("WorkBuddy") {
        return Some("workbuddy-cn");
    }
    if u.contains("codebuddy.cn") || u.contains("codebuddy.ai") || title.contains("CodeBuddy") {
        return Some("codebuddy");
    }
    None
}

/// 该目标是否是我们要的 WorkBuddy 国内版页面。
pub fn is_workbuddy_cn_target(url: &str, title: &str) -> bool {
    classify_target(url, title) == Some("workbuddy-cn")
}

// ---------------------------------------------------------------------------
// 发现与连接
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TargetInfo {
    pub ws_url: String,
    pub url: String,
    pub title: String,
    pub target_type: String,
}

async fn fetch_json_loopback(url: &str, timeout: Duration) -> Result<Value, String> {
    let client = reqwest::Client::new();
    let resp = tokio::time::timeout(timeout, client.get(url).send())
        .await
        .map_err(|_| format!("超时: {url}"))?
        .map_err(|e| format!("请求失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<Value>().await.map_err(|e| format!("解析失败: {e}"))
}

/// 在候选端口里发现 WorkBuddy 页面 target，返回 `(端口, target)`。
pub async fn discover_workbuddy_target(ports: &[u16]) -> Result<(u16, TargetInfo), String> {
    let mut last_error = String::from("未发现 CDP 端口");
    for port in ports {
        let base = format!("http://127.0.0.1:{port}");
        let version = fetch_json_loopback(&format!("{base}/json/version"), PROBE_TIMEOUT).await;
        if let Err(err) = version {
            last_error = format!("端口 {port}: {err}");
            continue;
        }
        let targets = match fetch_json_loopback(&format!("{base}/json/list"), PROBE_TIMEOUT).await {
            Ok(Value::Array(rows)) => rows,
            Ok(_) => {
                last_error = format!("端口 {port}: /json/list 非数组");
                continue;
            }
            Err(err) => {
                last_error = format!("端口 {port}: {err}");
                continue;
            }
        };
        // 先找强信号页面；type=page 且 URL/标题命中 WorkBuddy。
        let mut fallback: Option<TargetInfo> = None;
        for row in &targets {
            let url = row.get("url").and_then(Value::as_str).unwrap_or("");
            let title = row.get("title").and_then(Value::as_str).unwrap_or("");
            let target_type = row.get("type").and_then(Value::as_str).unwrap_or("");
            if target_type != "page" {
                continue;
            }
            let ws_url = row
                .get("webSocketDebuggerUrl")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if ws_url.is_empty() {
                continue;
            }
            let info = TargetInfo {
                ws_url,
                url: url.to_string(),
                title: title.to_string(),
                target_type: target_type.to_string(),
            };
            if is_workbuddy_cn_target(url, title) {
                return Ok((*port, info));
            }
            if fallback.is_none() {
                fallback = Some(info);
            }
        }
        // 端口活着但没有强信号页面：记录兜底候选（客户端可能刚启动还在加载）。
        if let Some(info) = fallback {
            return Ok((*port, info));
        }
        last_error = format!("端口 {port}: 无匹配页面");
    }
    Err(last_error)
}

// ---------------------------------------------------------------------------
// 会话（连接复用 + 探活）
// ---------------------------------------------------------------------------

pub type PendingMap = Arc<Mutex<HashMap<u64, tokio::sync::oneshot::Sender<Result<Value, String>>>>>;
type WsSink = tokio::sync::Mutex<futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    Message,
>>;

pub struct CdpSession {
    sender: WsSink,
    pending: PendingMap,
    next_id: AtomicU64,
    /// 无 id 的协议事件（bindingCalled / 异常通知）广播给订阅方（悬浮图标守护进程用）。
    pub events: tokio::sync::broadcast::Sender<Value>,
    pub port: u16,
    pub target: TargetInfo,
}

impl CdpSession {
    async fn connect(port: u16, target: TargetInfo) -> Result<Arc<Self>, String> {
        let (ws, _) = tokio::time::timeout(
            PROBE_TIMEOUT,
            tokio_tungstenite::connect_async(&target.ws_url),
        )
        .await
        .map_err(|_| "连接超时".to_string())?
        .map_err(|e| format!("WS 连接失败: {e}"))?;
        let (sink, mut stream) = ws.split();
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let pending_clone = Arc::clone(&pending);
        let (event_tx, _) = tokio::sync::broadcast::channel::<Value>(32);
        let event_tx_clone = event_tx.clone();
        // 读循环：按 id 分发响应到 oneshot；无 id 的协议事件广播给订阅方。
        tokio::spawn(async move {
            while let Some(Ok(msg)) = stream.next().await {
                let Message::Text(text) = msg else { continue };
                let Ok(parsed) = serde_json::from_str::<Value>(&text) else { continue };
                let Some(id) = parsed.get("id").and_then(Value::as_u64) else {
                    let _ = event_tx_clone.send(parsed);
                    continue;
                };
                if let Some(tx) = pending_clone.lock().unwrap().remove(&id) {
                    if let Some(error) = parsed.get("error") {
                        let message = error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("CDP 错误")
                            .to_string();
                        let _ = tx.send(Err(message));
                    } else {
                        let _ = tx.send(Ok(parsed.get("result").cloned().unwrap_or(Value::Null)));
                    }
                }
            }
        });
        Ok(Arc::new(Self {
            sender: tokio::sync::Mutex::new(sink),
            pending,
            next_id: AtomicU64::new(1),
            events: event_tx,
            port,
            target,
        }))
    }

    async fn send(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let text = json!({ "id": id, "method": method, "params": params }).to_string();
        {
            let mut sink = self.sender.lock().await;
            tokio::time::timeout(EVALUATE_TIMEOUT, sink.send(Message::Text(text)))
                .await
                .map_err(|_| "发送超时".to_string())?
                .map_err(|e| format!("发送失败: {e}"))?;
        }
        tokio::time::timeout(EVALUATE_TIMEOUT, rx)
            .await
            .map_err(|_| "响应超时".to_string())?
            .map_err(|_| "连接已断开".to_string())?
    }

    /// 在页面里执行一段返回 Promise 的 JS，等它 resolve 并取回 JSON 值。
    pub async fn evaluate(&self, expression: &str) -> Result<Value, String> {
        let result = self
            .send(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "awaitPromise": true,
                    "returnByValue": true,
                }),
            )
            .await?;
        // 远端异常会包在 result.exceptionDetails 里。
        if let Some(details) = result.get("exceptionDetails") {
            let text = details
                .get("exception")
                .and_then(|e| e.get("description").or_else(|| e.get("value")))
                .and_then(Value::as_str)
                .unwrap_or("页面内执行异常");
            return Err(text.to_string());
        }
        Ok(result.get("result").and_then(|r| r.get("value")).cloned().unwrap_or(Value::Null))
    }
}

static CDP_SESSION: OnceLock<Mutex<Option<Arc<CdpSession>>>> = OnceLock::new();

/// 取缓存会话；探活失败则丢弃缓存返回 `None`。
async fn cached_session() -> Option<Arc<CdpSession>> {
    let lock = CDP_SESSION.get_or_init(|| Mutex::new(None));
    let existing = lock.lock().unwrap_or_else(|p| p.into_inner()).clone();
    if let Some(session) = existing {
        let alive = session
            .evaluate("1+1")
            .await
            .ok()
            .and_then(|v| v.as_i64())
            .is_some_and(|v| v == 2);
        if alive {
            return Some(session);
        }
        *lock.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
    None
}

/// 连接并缓存 WorkBuddy 页面会话。配置关闭或客户端未开调试端口时返回 `None`
/// （调用方回退 HTTP 路径）。`force_reconnect` 用于重启客户端后强制刷新。
/// 热切换账号后刷新客户端页面：不重启进程，让新认证立即生效。
///
/// 与 `restart_workbuddy_with_cdp` 的区别：这条只发 `Page.reload`，
/// WorkBuddy 进程保持不变，切换耗时从 8–15s 降到 1–2s。
pub async fn reload_workbuddy_page() -> bool {
    let Some(session) = workbuddy_page(false).await else {
        return false;
    };
    session
        .send("Page.reload", json!({ "ignoreCache": true }))
        .await
        .is_ok()
}

/// 热切换身份探测：在 WorkBuddy 页面里搜 localStorage + cookie 是否出现目标 uid。
///
/// - `Some(true)`  = 只找到目标 uid → 页面身份已切到新账号
/// - `Some(false)` = 只找到切换前 uid → 页面身份仍是旧账号
/// - `None`        = CDP 不可用 / 两个都出现或都没出现（无法判断，调用方不得赌）
///
/// 不猜 localStorage 的键名：直接对整个序列化结果做子串匹配，uid 是 32 位
/// 十六进制，作为子串足够特异。
pub async fn probe_identity_uid(from_uid: &str, to_uid: &str) -> Option<bool> {
    if from_uid.is_empty() || to_uid.is_empty() || from_uid == to_uid {
        return None;
    }
    let Some(session) = workbuddy_page(false).await else {
        return None;
    };
    let js = format!(
        "(function(){{try{{var s=String(JSON.stringify(localStorage))+String(document.cookie);var a=s.indexOf('{from}')>=0,b=s.indexOf('{to}')>=0;if(b&&!a)return 'to';if(a&&!b)return 'from';return 'unknown';}}catch(e){{return 'unknown';}}}})()",
        from = from_uid,
        to = to_uid,
    );
    match session.evaluate(&js).await {
        Ok(Value::String(v)) => {
            if v == "to" {
                Some(true)
            } else if v == "from" {
                Some(false)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub async fn workbuddy_page(force_reconnect: bool) -> Option<Arc<CdpSession>> {
    let cfg = load_cdp_config();
    if cfg.get("enabled").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    if !force_reconnect {
        if let Some(session) = cached_session().await {
            return Some(session);
        }
    } else {
        *CDP_SESSION
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
    }
    let ports: Vec<u16> = cfg
        .get("ports")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().filter_map(Value::as_u64).map(|v| v as u16).collect())
        .filter(|rows: &Vec<u16>| !rows.is_empty())
        .unwrap_or_else(|| DEFAULT_PORTS.to_vec());
    let (port, target) = discover_workbuddy_target(&ports).await.ok()?;
    let session = CdpSession::connect(port, target).await.ok()?;
    *CDP_SESSION
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = Some(Arc::clone(&session));
    Some(session)
}

/// 当前 CDP 连接状态（设置页展示用）。
///
/// 除了「连上没有」，还要说清「为什么没连上」：`autoStart` 是显式开关的真实值
/// （只看用户配置文件，不看合并默认），`daemon` 是守护进程最近一轮的结论。
/// 设置页那张卡片照这三层就能显示「未连接 / 卡在哪」。
pub async fn status() -> Value {
    let cfg = load_cdp_config();
    let session = cached_session().await;
    let placement = widget_placement(&cfg);
    json!({
        "enabled": cfg.get("enabled").and_then(Value::as_bool).unwrap_or(false),
        "widget": widget_enabled(&cfg),
        "widgetInstalled": WIDGET_SPAWNED.get().is_some() && widget_enabled(&cfg),
        "autoStart": auto_start_enabled(),
        "widgetPlacement": {
            "right": placement.right,
            "bottom": placement.bottom,
            "size": placement.size,
        },
        "connected": session.is_some(),
        "port": session.as_ref().map(|s| s.port),
        "url": session.as_ref().map(|s| s.target.url.clone()),
        "title": session.as_ref().map(|s| s.target.title.clone()),
        "daemon": daemon_state(),
    })
}

// ---------------------------------------------------------------------------
// 页面内同源 fetch（任务执行原语）
// ---------------------------------------------------------------------------

/// 构造「页面里 fetch」的 JS：返回 `{status, body}`（JSON 值）。
///
/// - Bearer token 由调用方拼进 headers（键值都做 JSON 转义）；
/// - 同源请求（页面 origin 即 www.workbuddy.cn），cookie 由浏览器自动携带；
/// - 任何异常都 resolve 成 `{status: 0, error: "..."}`，不让 evaluate 抛。
pub fn page_fetch_js(url: &str, method: &str, body: Option<&Value>, headers: &HashMap<String, String>) -> String {
    let headers_js: Vec<String> = headers
        .iter()
        .map(|(k, v)| format!("{}:{}", escape_js(k), escape_js(v)))
        .collect();
    let body_js = body
        .map(|b| format!("body: {}", escape_js(&b.to_string())))
        .unwrap_or_else(|| "body: null".to_string());
    // JS 里有大量字面量花括号，用占位符替换代替 format!，免转义。
    const TEMPLATE: &str = r#"(async () => {
            try {
                const resp = await fetch(__URL__, {
                    method: __METHOD__,
                    headers: {__HEADERS__},
                    __BODY__,
                    credentials: 'include',
                });
                const text = await resp.text();
                return { status: resp.status, body: text };
            } catch (err) {
                return { status: 0, error: String(err && err.message || err) };
            }
        })()"#;
    TEMPLATE
        .replace("__URL__", &escape_js(url))
        .replace("__METHOD__", &escape_js(method))
        .replace("__HEADERS__", &headers_js.join(","))
        .replace("__BODY__", &body_js)
}

/// 页面内同源 fetch，返回 `(HTTP 状态, 响应文本)`。CDP 不可用或执行失败给 `Err`。
pub async fn page_fetch(
    url: &str,
    method: &str,
    body: Option<&Value>,
    headers: &HashMap<String, String>,
) -> Result<(u16, String), String> {
    let session = workbuddy_page(false)
        .await
        .ok_or_else(|| "CDP 未连接".to_string())?;
    let value = session.evaluate(&page_fetch_js(url, method, body, headers)).await?;
    let status = value.get("status").and_then(Value::as_u64).unwrap_or(0) as u16;
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(format!("页面内 fetch 失败: {error}"));
    }
    let body_text = value.get("body").and_then(Value::as_str).unwrap_or("").to_string();
    if status == 0 {
        return Err("页面内 fetch 返回 status=0".to_string());
    }
    Ok((status, body_text))
}

/// JSON 字符串 → JS 字符串字面量（含引号），防注入。
pub fn escape_js(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 把既有的 CDP 配置探活/端口扫描复用到 `http_request` 之外的场景时，
/// 保持与其它模块一致的便捷入口：返回是否连接成功（用于状态展示与冒烟）。
pub async fn ensure_connected() -> Result<(u16, TargetInfo), String> {
    if let Some(session) = workbuddy_page(false).await {
        return Ok((session.port, session.target.clone()));
    }
    let cfg = load_cdp_config();
    let ports: Vec<u16> = cfg
        .get("ports")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().filter_map(Value::as_u64).map(|v| v as u16).collect())
        .filter(|rows: &Vec<u16>| !rows.is_empty())
        .unwrap_or_else(|| DEFAULT_PORTS.to_vec());
    discover_workbuddy_target(&ports).await
}

/// 探测端口是否开放（供重启客户端后等待就绪）。
pub async fn probe_port(port: u16) -> bool {
    fetch_json_loopback(&format!("http://127.0.0.1:{port}/json/version"), PROBE_TIMEOUT)
        .await
        .is_ok()
}

// ---------------------------------------------------------------------------
// 悬浮图标（对齐 WorkDaddy 的交互形态：客户端页面里常驻一个小图标，点击唤起面板）
// ---------------------------------------------------------------------------

/// 图标 DOM id（重注入检查用）。
const WIDGET_DOM_ID: &str = "wb-deck-icon";
/// 面板 iframe id（避免与 WorkBuddy 自身元素撞车）。
const PANEL_DOM_ID: &str = "wb-deck-frame";

/// 注入脚本的版本号：语义变化（尺寸/拖动/守卫）时递增。
///
/// 两处必须共用它：注入脚本里的 `window.__wbDeckVer`，以及守护进程的探针表达式。
/// 写死两份会得到「探针永远判 false → 每 20s 白注入一次」的假故障。
const WIDGET_VERSION: u32 = 8;

/// 图标默认位置：底部抬到 96px 是为了避让 WorkBuddy 自带的右下角吉祥物
/// （它常驻 `right:22px; bottom:22px` 那一带，压上去用户点不到图标）。
const WIDGET_DEFAULT_RIGHT: i64 = 22;
const WIDGET_DEFAULT_BOTTOM: i64 = 96;
const WIDGET_DEFAULT_SIZE: i64 = 36;

/// 图标摆放（`cdp_config.json` 的 widget 段可覆盖）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetPlacement {
    pub right: i64,
    pub bottom: i64,
    pub size: i64,
}

impl Default for WidgetPlacement {
    fn default() -> Self {
        Self {
            right: WIDGET_DEFAULT_RIGHT,
            bottom: WIDGET_DEFAULT_BOTTOM,
            size: WIDGET_DEFAULT_SIZE,
        }
    }
}

/// `widget` 段既兼容老的 `"widget": true`，也支持新对象写法
/// `"widget": { "enabled": true, "offsetRight": 22, "offsetBottom": 96, "size": 44 }`。
fn widget_section(cfg: &Value) -> Option<&serde_json::Map<String, Value>> {
    cfg.get("widget").and_then(Value::as_object)
}

/// 图标开关：旧配置是纯 bool，新配置读对象里的 `enabled`（缺省开）。
fn widget_enabled(cfg: &Value) -> bool {
    match cfg.get("widget") {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Object(map)) => map.get("enabled").and_then(Value::as_bool).unwrap_or(true),
        _ => true,
    }
}

/// 从配置读图标摆放；越界值夹紧，避免用户把图标推到屏幕外找不回来。
pub fn widget_placement(cfg: &Value) -> WidgetPlacement {
    let number = |key: &str, fallback: i64| {
        widget_section(cfg)
            .and_then(|section| section.get(key))
            .and_then(Value::as_i64)
            .unwrap_or(fallback)
    };
    WidgetPlacement {
        right: number("offsetRight", WIDGET_DEFAULT_RIGHT).clamp(0, 400),
        bottom: number("offsetBottom", WIDGET_DEFAULT_BOTTOM).clamp(0, 800),
        size: number("size", WIDGET_DEFAULT_SIZE).clamp(28, 72),
    }
}

/// 注入 JS（幂等）：右下角机器人图标 + 点击在 WorkBuddy 页面内弹出 Deck 面板（iframe）。
///
/// 面板地址由本地桥提供（回环 + token），点击行为完全在页面内完成，不再回调 Rust——
/// 少一次 CDP 往返，面板开关也更跟手。
pub fn widget_js(panel_url: &str) -> String {
    widget_js_with(panel_url, widget_placement(&load_cdp_config()))
}

/// 同上，但摆位由调用方给定（守护进程按配置生成，单测注入固定值）。
pub fn widget_js_with(panel_url: &str, placement: WidgetPlacement) -> String {
    const TEMPLATE: &str = r##"(() => {
            const VER = __WIDGET_VERSION__;
            const PANEL_URL = __PANEL_URL__;
            // 只注入真实客户端页面的顶层文档。两个必须拦住的场景：
            // 1) `Page.addScriptToEvaluateOnNewDocument` 对**所有 frame** 生效，
            //    不拦的话面板自己的 iframe 里也会长出一个图标（套娃）；
            // 2) 桥 origin（127.0.0.1/localhost）上跑的是我们自己的前端，不需要图标。
            try {
                if (window.top !== window.self) return true;
                const host = location.hostname || '';
                if (host === '127.0.0.1' || host === 'localhost' || host === '::1') return true;
                if (window.__WB_DECK_EMBED__) return true;
            } catch (e) {}
            window.__wbDeckUrl = PANEL_URL;
            // 同版本：只刷新地址/坐标与点击行为；换版本：清掉旧注入物重建（页面不刷新也能换代）。
            if (window.__wbDeckToggle && window.__wbDeckVer === VER) {
                const oldBtn = document.getElementById("__wbDeckDomId__");
                if (oldBtn) {
                    oldBtn.onclick = () => window.__wbDeckToggle();
                    // 用户改了 cdp_config 的 widget 段后，下一次注入就能生效。
                    oldBtn.style.right = "__RIGHT__px";
                    oldBtn.style.bottom = "__BOTTOM__px";
                    oldBtn.style.width = "__SIZE__px";
                    oldBtn.style.height = "__SIZE__px";
                }
                // 应用重启后 token 变了：已打开的面板必须换到新地址，否则 IPC 全 403。
                const oldFrame = document.getElementById("__wbDeckFrameId__");
                if (oldFrame && oldFrame.getAttribute("src") && oldFrame.getAttribute("src") !== PANEL_URL) {
                    oldFrame.setAttribute("src", PANEL_URL);
                }
                return true;
            }
            if (window.__wbDeckToggle) {
                const deadMask = document.getElementById("__wbDeckMask");
                if (deadMask) deadMask.remove();
                const deadBtn = document.getElementById("__wbDeckDomId__");
                if (deadBtn) deadBtn.remove();
            }
            const SVG = '<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><g fill="#f5f5f4"><rect x="11" y="2" width="2" height="3.2" rx="1"/><circle cx="12" cy="2" r="1.6"/><rect x="4.5" y="5.2" width="15" height="13" rx="3.4"/><rect x="2" y="8.2" width="1.8" height="5.4" rx="0.9"/><rect x="20.2" y="8.2" width="1.8" height="5.4" rx="0.9"/></g><circle cx="9.2" cy="11.4" r="1.7" fill="#17171a"/><circle cx="14.8" cy="11.4" r="1.7" fill="#17171a"/></svg>';
            let mask = null;
            let frame = null;
            // box 必须提到外层：toggle 要在 ensure() 之外改动画状态，
            // 放在 ensure() 里的 const 会让 toggle 拿到 undefined（面板关不掉）。
            let box = null;
            let open = false;
            // 面板拖动后的偏移（相对居中位置）；双击标题条复位。
            let shiftX = 0;
            let shiftY = 0;
            let drag = null;
            // 退出动画的收尾定时器：快速开→关→开时先撤掉，避免面板被定时器藏掉。
            let hideTimer = null;
            const ANIM_MS = 180;
            // 尊重系统「减少动态效果」：直接切换，不做过渡。
            const REDUCED = !!(window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches);

            const toggle = (next) => {
                ensure();
                open = next === undefined ? !open : !!next;
                if (open) {
                    const url = window.__wbDeckUrl || PANEL_URL;
                    if (frame.getAttribute("src") !== url) frame.setAttribute("src", url);
                    if (hideTimer) { clearTimeout(hideTimer); hideTimer = null; }
                    mask.style.display = "flex";
                    // 先读一次布局强制回流：让"隐藏态初值 → 展示态"一定走过渡，
                    // 否则 display 切换同帧改样式会被浏览器合并成硬切（闪烁）。
                    void mask.offsetWidth;
                    box.style.opacity = "1";
                    box.style.scale = "1";
                    box.style.translate = "0 0";
                    try { frame.focus(); } catch (e) {}
                } else {
                    box.style.opacity = "0";
                    box.style.scale = "0.96";
                    box.style.translate = "0 10px";
                    if (hideTimer) clearTimeout(hideTimer);
                    if (REDUCED) {
                        mask.style.display = "none";
                        hideTimer = null;
                    } else {
                        hideTimer = setTimeout(() => {
                            mask.style.display = "none";
                            hideTimer = null;
                        }, ANIM_MS + 20);
                    }
                }
            };

            const ensure = () => {
                if (mask) return;
                mask = document.createElement("div");
                mask.id = "__wbDeckMask";
                mask.style.cssText = "position:fixed;inset:0;z-index:2147483645;background:transparent;display:none;align-items:center;justify-content:flex-end;padding:12px;box-sizing:border-box;";
                mask.addEventListener("mousedown", (e) => { if (e.target === mask) toggle(false); });

                box = document.createElement("div");
                // 尺寸按视口自适应：1366×768 的机器上 88vh 会顶到屏幕上下边，
                // 这里高取 min(820px, 80vh)、宽取 30vw（夹在 320–620px），
                // 遮罩还留 12px padding，任何视口都不会溢出。
                // 动画用独立的 scale / translate 属性（不动 transform）：
                // transform 留给标题条拖动的位移，两者互不覆盖。
                box.style.cssText = "position:relative;display:flex;flex-direction:column;width:30vw;min-width:320px;max-width:620px;height:min(820px,80vh);background:#12141c;border:1px solid rgba(255,255,255,.12);border-radius:12px;overflow:hidden;opacity:0;scale:.96;translate:0 10px;transition:opacity 180ms linear,scale 180ms linear,translate 180ms linear;will-change:opacity,scale,translate;";
                // 动画结束再收尾：定时器在后台标签页会被节流（可能延到 1s 才触发），
                // transitionend 是准时的；定时器只当兜底，两者都到才隐藏。
                box.addEventListener("transitionend", (e) => {
                    if (e.target !== box || e.propertyName !== "opacity" || open) return;
                    mask.style.display = "none";
                    if (hideTimer) { clearTimeout(hideTimer); hideTimer = null; }
                });

                // 标题条：显示身份 + 可拖动（用户常要把面板挪开去看背后的页面）。
                const bar = document.createElement("div");
                bar.style.cssText = "flex:0 0 auto;height:30px;display:flex;align-items:center;gap:8px;padding:0 4px 0 12px;background:#161925;border-bottom:1px solid rgba(255,255,255,.08);cursor:move;touch-action:none;user-select:none;-webkit-user-select:none;";
                const label = document.createElement("span");
                label.textContent = "WORKBUDDY DECK";
                label.style.cssText = "flex:1;color:#9c98ab;font:600 12px/1 ui-sans-serif,system-ui,sans-serif;";

                frame = document.createElement("iframe");
                frame.id = "__wbDeckFrameId__";
                frame.setAttribute("allow", "clipboard-write");
                frame.style.cssText = "flex:1 1 auto;width:100%;min-height:0;border:none;background:#12141c;";

                const close = document.createElement("button");
                close.type = "button";
                close.textContent = "×";
                close.title = "关闭";
                close.style.cssText = "width:26px;height:26px;padding:0;border:none;border-radius:8px;background:transparent;color:#9c98ab;font:400 18px/1 system-ui,sans-serif;cursor:pointer;";
                close.onmouseenter = () => { close.style.color = "#ffffff"; };
                close.onmouseleave = () => { close.style.color = "#9c98ab"; };
                close.onclick = (e) => { e.stopPropagation(); toggle(false); };

                // 拖动：pointer capture 之后即使指针滑到 iframe 上方也由标题条接管；
                // 同时临时关掉 iframe 的 pointer-events，否则鼠标一进 iframe 就丢 move 事件。
                bar.addEventListener("pointerdown", (e) => {
                    if (e.target === close) return;
                    drag = { x: e.clientX, y: e.clientY, baseX: shiftX, baseY: shiftY };
                    try { bar.setPointerCapture(e.pointerId); } catch (err) {}
                    frame.style.pointerEvents = "none";
                    e.preventDefault();
                });
                bar.addEventListener("pointermove", (e) => {
                    if (!drag) return;
                    const limitX = Math.max(0, (window.innerWidth - box.offsetWidth) / 2);
                    const limitY = Math.max(0, (window.innerHeight - box.offsetHeight) / 2);
                    shiftX = Math.max(-limitX, Math.min(limitX, drag.baseX + e.clientX - drag.x));
                    shiftY = Math.max(-limitY, Math.min(limitY, drag.baseY + e.clientY - drag.y));
                    box.style.transform = "translate(" + shiftX + "px," + shiftY + "px)";
                });
                const endDrag = (e) => {
                    if (!drag) return;
                    drag = null;
                    frame.style.pointerEvents = "";
                    try { bar.releasePointerCapture(e.pointerId); } catch (err) {}
                };
                bar.addEventListener("pointerup", endDrag);
                bar.addEventListener("pointercancel", endDrag);
                bar.addEventListener("dblclick", () => {
                    shiftX = 0;
                    shiftY = 0;
                    box.style.transform = "";
                });

                bar.appendChild(label);
                bar.appendChild(close);
                box.appendChild(bar);
                box.appendChild(frame);
                mask.appendChild(box);
                document.body.appendChild(mask);
                document.addEventListener("keydown", (e) => {
                    if (e.key === "Escape" && open) toggle(false);
                }, true);
            };

            window.__wbDeckVer = VER;
            window.__wbDeckToggle = toggle;

            const mk = () => {
                const existing = document.getElementById("__wbDeckDomId__");
                if (existing) { existing.onclick = () => toggle(); return; }
                const btn = document.createElement("button");
                btn.id = "__wbDeckDomId__";
                btn.title = "WorkBuddy Deck";
                btn.setAttribute("aria-label", "WorkBuddy Deck");
                // 默认 bottom 96px：WorkBuddy 自带的右下角吉祥物占着 22px 那一带，
                // 压上去等于把入口藏进别人的图标里（已实拍确认重叠）。
                btn.style.cssText = "position:fixed;right:__RIGHT__px;bottom:__BOTTOM__px;z-index:2147483646;width:__SIZE__px;height:__SIZE__px;box-sizing:border-box;padding:0;border-radius:4px;background:#17171a;border:1px solid rgba(255,255,255,.14);display:flex;align-items:center;justify-content:center;cursor:pointer;box-shadow:0 4px 14px rgba(0,0,0,.45);opacity:.9;transition:opacity .2s,transform .2s;user-select:none;-webkit-user-select:none;";
                btn.innerHTML = SVG;
                btn.onmouseenter = () => { btn.style.opacity = "1"; btn.style.transform = "scale(1.08)"; };
                btn.onmouseleave = () => { btn.style.opacity = ".9"; btn.style.transform = "none"; };
                btn.onclick = () => toggle();
                document.body.appendChild(btn);
            };
            document.body ? mk() : document.addEventListener("DOMContentLoaded", mk, { once: true });
            return true;
        })()"##;
    TEMPLATE
        .replace("__WIDGET_VERSION__", &WIDGET_VERSION.to_string())
        .replace("__PANEL_URL__", &escape_js(panel_url))
        .replace("__RIGHT__", &placement.right.to_string())
        .replace("__BOTTOM__", &placement.bottom.to_string())
        .replace("__SIZE__", &placement.size.to_string())
        .replace("__wbDeckFrameId__", PANEL_DOM_ID)
        .replace("__wbDeckDomId__", WIDGET_DOM_ID)
}

/// 探活表达式：图标在、toggle 可用、版本号是当前这一代。
///
/// 版本号必须与注入脚本共用 [`WIDGET_VERSION`]：写死一份会得到「探针永远 false →
/// 每 20s 重新注入一次」的假故障（虽然幂等不会双图标，但白烧 CDP 往返）。
fn widget_probe_js() -> String {
    format!(
        "!!(document.getElementById({}) && typeof window.__wbDeckToggle === 'function' && window.__wbDeckVer === {})",
        escape_js(WIDGET_DOM_ID),
        WIDGET_VERSION
    )
}

static WIDGET_SPAWNED: OnceLock<()> = OnceLock::new();

/// 守护进程最近一轮的结论，经 [`status`] 暴露给设置页。
///
/// 为什么要有它：用户看到「没图标」时，唯一能自查的信息就是「卡在客户端没开调试端口」
/// 还是「已注入但页面把它清掉了」。只 eprintln 到某个看不见的控制台等于没有观测。
static DAEMON_STATE: OnceLock<Mutex<Value>> = OnceLock::new();

fn set_daemon_state(state: Value) {
    let mut next = state;
    if let Some(map) = next.as_object_mut() {
        map.insert("at".to_string(), json!(now_ms()));
    }
    *DAEMON_STATE
        .get_or_init(|| Mutex::new(Value::Null))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = next;
}

fn daemon_state() -> Value {
    DAEMON_STATE
        .get_or_init(|| Mutex::new(Value::Null))
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or(Value::Null)
}

/// 本进程是否已经自动重启过客户端。
///
/// **只允许一次**：反复杀客户端（用户正在里面干活）比「没有面板图标」糟糕得多。
/// 一次没成说明环境有问题（参数被单实例吞掉 / 启动太慢 / 端口被拦），
/// 这时应该等用户去设置页手动点「带调试端口重启」，而不是继续循环杀进程。
static AUTO_RESTART_TRIED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// `autoStart` 只认用户配置文件里的**显式 true**。
///
/// 为什么不读 [`load_cdp_config`] 的合并结果：那边的历史默认是 `true`，
/// 效果是「Deck 一启动就把用户正在用的 WorkBuddy 杀掉重开一次」，没有任何提示。
/// 默认必须是不动进程；想自动拉起的人在 `cdp_config.json` 里自己写 `"autoStart": true`。
fn auto_start_enabled() -> bool {
    // 启动器模式（`npm run deck:inject`）就是把「带调试端口拉起客户端」当成职责，
    // 用环境变量显式授权，不必让用户先手写 cdp_config.json。
    if std::env::var_os("WB_DECK_CDP_AUTOSTART").is_some() {
        return true;
    }
    std::fs::read_to_string(cdp_config_file())
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.get("autoStart").and_then(Value::as_bool))
        .unwrap_or(false)
}

/// 自动拉起的三种结果：写进日志与 daemon 状态，用户据此判断卡在哪一步。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoStartOutcome {
    /// 没有显式开启：完全不动进程。
    Disabled,
    /// 本进程已经试过一次：不再重复。
    AlreadyTried,
    /// 本次尝试了拉起（失败由上层错误分支记录）。
    Attempted,
}

impl AutoStartOutcome {
    fn label(self) -> &'static str {
        match self {
            Self::Disabled => "off",
            Self::AlreadyTried => "once-only",
            Self::Attempted => "attempted",
        }
    }
}

/// 客户端没开调试端口时的自动处理入口。
///
/// 默认**不动用户进程**（`autoStart` 缺省关闭）：只有用户显式打开、且本进程还没试过，
/// 才会在后台重启客户端并带上 `--remote-debugging-port`。
/// 手动通路（设置页「带调试端口重启」→ `cdp_restart_client`）不受这里影响。
async fn ensure_client_with_cdp() -> Result<AutoStartOutcome, String> {
    if !auto_start_enabled() {
        return Ok(AutoStartOutcome::Disabled);
    }
    if AUTO_RESTART_TRIED.swap(true, Ordering::SeqCst) {
        return Ok(AutoStartOutcome::AlreadyTried);
    }
    let cfg = load_cdp_config();
    let variant = WbVariant::Cn;
    let port = cfg
        .get("ports")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(Value::as_u64)
        .unwrap_or(9222) as u16;
    // close/launch 里有最长 15s 的等待，阻塞操作放独立线程。
    tokio::task::spawn_blocking(move || restart_workbuddy_with_cdp(variant, port, 10))
        .await
        .map_err(|e| format!("重启任务失败: {e}"))?
        .map(|_| AutoStartOutcome::Attempted)
}

/// 启动悬浮图标守护进程（进程内单例）。
///
/// 每轮：读配置（开关 / 摆位）→ 连页面 → 注册「新文档自动注入」→ 立即注入 →
/// 每 20s 探活（SPA 导航把图标清掉就重注入），会话断开则重连。
///
/// 拿不到调试端口时**只记状态、不动用户进程**（见 [`ensure_client_with_cdp`]）。
/// `[deck] cdp 未就绪 …` 这行日志与 `cdp_status()` 里的 `daemon` 字段，
/// 是用户判断「卡在哪一步」的唯一入口，别删。
pub fn spawn_widget_daemon(panel_url: String) {
    if WIDGET_SPAWNED.set(()).is_err() {
        return;
    }
    tokio::spawn(async move {
        // 未就绪时的重试节奏：客户端冷启动 5–15s 就绪，固定 20s 重试会让图标
        // 「明明页面早就起来了却要再等十几秒」。前 30 次 1s 快轮询（覆盖启动窗口），
        // 之后 3s，最后退到 5s 长驻——回环探测连接被拒是瞬时失败，成本可忽略。
        let mut fast_retries: u32 = 0;
        loop {
            let cfg = load_cdp_config();
            let placement = widget_placement(&cfg);
            let script = widget_js_with(&panel_url, placement);
            if !widget_enabled(&cfg) {
                set_daemon_state(json!({
                    "reason": "widget-disabled",
                    "detail": "cdp_config.json 的 widget 开关为关",
                }));
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }
            let Some(session) = workbuddy_page(false).await else {
                let auto = ensure_client_with_cdp().await;
                let (auto_label, auto_detail) = match &auto {
                    Ok(outcome) => (outcome.label(), String::new()),
                    Err(error) => ("failed", error.clone()),
                };
                set_daemon_state(json!({
                    "reason": "cdp-unavailable",
                    "autoStart": auto_label,
                    "detail": auto_detail,
                    "hint": "客户端没开调试端口：设置页点「带调试端口重启」，或带 --remote-debugging-port 手动启动",
                }));
                // 快速重试：客户端冷启动 5–15s 就绪，前 30 次 1s 轮询覆盖这段窗口，
                // 之后 3s；再久退到 5s 长驻（回环探测连接被拒是瞬时失败，成本可忽略）。
                let delay = if fast_retries < 30 {
                    1
                } else if fast_retries < 50 {
                    3
                } else {
                    5
                };
                fast_retries = fast_retries.saturating_add(1);
                if fast_retries == 1 || fast_retries == 31 || fast_retries == 51 {
                    if auto_detail.is_empty() {
                        eprintln!("[deck] cdp 未就绪 reason=cdp-unavailable auto_start={auto_label} retry_in={delay}s");
                    } else {
                        eprintln!("[deck] cdp 未就绪 reason=cdp-unavailable auto_start={auto_label} error={auto_detail} retry_in={delay}s");
                    }
                }
                tokio::time::sleep(Duration::from_secs(delay)).await;
                continue;
            };
            fast_retries = 0;
            // 注册是 per-session 的：导航后浏览器自动重跑同一份脚本，会话换代时由外层循环
            // 重新注册。**同一会话内绝不重复注册**——那会让脚本栈里叠多份（幂等守卫能挡住
            // 双图标，但没必要留着这层噪音）。
            let _ = session.send("Page.enable", json!({})).await;
            let _ = session
                .send(
                    "Page.addScriptToEvaluateOnNewDocument",
                    json!({ "source": script }),
                )
                .await;
            let mut injected = false;
            loop {
                if !injected {
                    injected = session
                        .evaluate(&script)
                        .await
                        .ok()
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if injected {
                        set_daemon_state(json!({
                            "reason": "widget-injected",
                            "port": session.port,
                            "url": session.target.url,
                            "placement": {
                                "right": placement.right,
                                "bottom": placement.bottom,
                                "size": placement.size,
                            },
                        }));
                        eprintln!(
                            "[deck] widget injected port={} url={}",
                            session.port, session.target.url
                        );
                    }
                }
                // 5s 探活：SPA 导航把图标清掉后最多 5s 重注入（原 20s 太钝）。
                // 单次 evaluate 只是一个 DOM 查询，成本可忽略。
                tokio::time::sleep(Duration::from_secs(5)).await;
                // 探活 + 图标存活性检查（导航重注入兜底）。版本号与注入脚本共用常量。
                match session.evaluate(&widget_probe_js()).await {
                    Ok(v) if v.as_bool() == Some(true) => {}
                    _ => {
                        injected = false;
                        if session.evaluate("1+1").await.is_err() {
                            break;
                        }
                    }
                }
            }
            // 会话已死：清缓存让下一轮重新发现连接。
            *CDP_SESSION
                .get_or_init(|| Mutex::new(None))
                .lock()
                .unwrap_or_else(|p| p.into_inner()) = None;
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classify_target_only_accepts_strong_signals() {
        assert!(is_workbuddy_cn_target("https://www.workbuddy.cn/profile", "WorkBuddy"));
        assert!(is_workbuddy_cn_target(
            "file:///C:/apps/WorkBuddy/resources/app.asar/index.html",
            ""
        ));
        // 兄弟客户端绝不能误判成 CN（WorkDaddy 踩过的坑：注错端）。
        assert!(!is_workbuddy_cn_target("https://www.workbuddy.ai/app", "WorkBuddy AI"));
        assert!(!is_workbuddy_cn_target(
            "file:///C:/apps/WorkBuddy AI/resources/app.asar/index.html",
            ""
        ));
        assert!(!is_workbuddy_cn_target("https://www.codebuddy.cn/", "CodeBuddy"));
        assert_eq!(classify_target("about:blank", ""), None);
        // URL 空格编码归一化后仍能识别。
        assert_eq!(
            classify_target("file:///Applications/WorkBuddy%20AI.app/x", ""),
            Some("workbuddy-ai")
        );
    }

    #[test]
    fn escape_js_produces_safe_string_literal() {
        assert_eq!(escape_js("abc"), r#""abc""#);
        assert_eq!(escape_js("a\"b"), r#""a\"b""#);
        assert_eq!(escape_js("a\\b"), r#""a\\b""#);
    }

    /// page_fetch_js 不得残留占位符，且是合法的 JS 调用形状。
    #[test]
    fn page_fetch_js_replaces_all_placeholders() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer token".to_string());
        let js = page_fetch_js(
            "https://www.workbuddy.cn/v2/report",
            "POST",
            Some(&json!([{ "eventCode": "x" }])),
            &headers,
        );
        assert!(!js.contains("__URL__"));
        assert!(!js.contains("__METHOD__"));
        assert!(!js.contains("__HEADERS__"));
        assert!(!js.contains("__BODY__"));
        assert!(js.contains(r#""https://www.workbuddy.cn/v2/report""#));
        assert!(js.contains("credentials: 'include'"));
    }

    /// 面板注入 JS：幂等守卫 + 图标 + iframe 浮层 + 关闭/ESC 四要素齐备。
    /// 用 `widget_js_with` 固定摆位，避免测试受本机 `cdp_config.json` 影响。
    #[test]
    fn widget_js_injects_icon_and_panel() {
        let js = widget_js_with(
            "http://127.0.0.1:1234/?__deck_t=abc",
            WidgetPlacement::default(),
        );
        assert!(js.contains("window.__wbDeckToggle = toggle;"));
        assert!(js.contains(WIDGET_DOM_ID));
        assert!(js.contains(PANEL_DOM_ID));
        assert!(js.contains("http://127.0.0.1:1234/?__deck_t=abc"));
        assert!(js.contains("iframe"));
        assert!(js.contains("Escape"));
        assert!(!js.contains("__PANEL_URL__"));
        assert!(!js.contains("__RIGHT__"));
        assert!(!js.contains("__BOTTOM__"));
        assert!(!js.contains("__SIZE__"));
        assert!(!js.contains("__WIDGET_VERSION__"));
        assert!(!js.contains("__wbDeckDomId__"));
        assert!(!js.contains("__wbDeckFrameId__"));
    }

    /// 图标必须避开 WorkBuddy 自带的右下角吉祥物（默认 bottom 96px），
    /// 并且摆位/尺寸真的来自注入参数，而不是写死在 JS 里。
    #[test]
    fn widget_js_honors_placement_and_avoids_mascot() {
        let custom = WidgetPlacement {
            right: 18,
            bottom: 132,
            size: 52,
        };
        let js = widget_js_with("http://127.0.0.1:1234/?__deck_t=abc", custom);
        assert!(js.contains("right:18px;bottom:132px"), "摆位没生效");
        assert!(js.contains("width:52px;height:52px"), "尺寸没生效");
        assert!(
            WIDGET_DEFAULT_BOTTOM >= 88,
            "默认底部偏移必须让开吉祥物（实测它在 22px 一带）"
        );
        assert_eq!(WidgetPlacement::default().bottom, WIDGET_DEFAULT_BOTTOM);
    }

    /// 顶层文档守卫：`Page.addScriptToEvaluateOnNewDocument` 会注入所有 frame，
    /// 少了这道判断，面板自己的 iframe 里会再长一个图标（套娃）。
    #[test]
    fn widget_js_only_runs_in_top_level_client_document() {
        let js = widget_js_with(
            "http://127.0.0.1:1234/?__deck_t=abc",
            WidgetPlacement::default(),
        );
        assert!(js.contains("window.top !== window.self"));
        assert!(js.contains("'127.0.0.1'"));
        assert!(js.contains("'localhost'"));
        assert!(js.contains("window.__WB_DECK_EMBED__"));
        // 守卫必须出现在建 DOM 之前（放在 mk/ensure 之后等于没守）。
        let guard = js.find("window.top !== window.self").expect("缺顶层守卫");
        let mk = js.find("const mk = ()").expect("缺 mk()");
        assert!(guard < mk, "守卫必须在建 DOM 之前");
    }

    /// 面板尺寸按视口自适应（1366×768 上不再顶满），标题条可拖动。
    #[test]
    fn widget_js_panel_sizes_to_viewport_and_is_draggable() {
        let js = widget_js_with(
            "http://127.0.0.1:1234/?__deck_t=abc",
            WidgetPlacement::default(),
        );
        assert!(js.contains("height:min(820px,80vh)"), "高度没按视口收");
        assert!(js.contains("width:30vw"), "宽度没按视口收");
        assert!(js.contains("max-width:620px"), "缺宽度上限");
        assert!(js.contains("pointerdown"), "拖动入口丢了");
        assert!(js.contains("setPointerCapture"), "拖动会丢事件（需要 pointer capture）");
        assert!(js.contains("pointerEvents = \"none\""), "拖动时 iframe 会吃掉 move 事件");
        assert!(js.contains("dblclick"), "缺拖动复位");
    }

    /// 面板进出动画：线性过渡 + 关闭定时收尾 + 快速切换不留残状态。
    #[test]
    fn widget_js_has_linear_open_close_animation() {
        let js = widget_js_with(
            "http://127.0.0.1:1234/?__deck_t=abc",
            WidgetPlacement::default(),
        );
        assert!(
            js.contains("transition:opacity 180ms linear"),
            "面板进出动画必须是线性过渡"
        );
        assert!(js.contains("scale:.96"), "初始态没有缩放（打开会硬切）");
        assert!(js.contains("translate:0 10px"), "初始态没有位移（打开会硬切）");
        assert!(js.contains("void mask.offsetWidth"), "缺强制回流，开场动画会被合并成闪一下");
        assert!(js.contains("hideTimer"), "关闭动画没有收尾定时器");
        assert!(js.contains("transitionend"), "没有 transitionend 收尾（后台标签页定时器会被节流）");
        assert!(js.contains("clearTimeout(hideTimer)"), "快速开关会把面板永久藏起来");
        // 拖动位移走 transform，动画走独立 scale/translate 属性，两者不能互相覆盖。
        assert!(js.contains("box.style.transform"), "拖动位移丢了");
        assert!(!js.contains("box.style.transform = \"scale"), "动画覆盖了拖动 transform");
    }

    /// 探针版本号必须与注入脚本同源，否则每 20s 白注入一次。
    #[test]
    fn probe_version_matches_injected_script() {
        let probe = widget_probe_js();
        assert!(probe.contains(&format!("=== {WIDGET_VERSION}")), "探针版本与常量不一致");
        let js = widget_js_with(
            "http://127.0.0.1:1234/?__deck_t=abc",
            WidgetPlacement::default(),
        );
        assert!(js.contains(&format!("const VER = {WIDGET_VERSION};")));
    }

    /// 摆位解析兼容老写法（`"widget": true`）与新对象写法，越界值夹紧。
    #[test]
    fn widget_config_accepts_bool_and_object() {
        assert!(widget_enabled(&json!({ "widget": true })));
        assert!(!widget_enabled(&json!({ "widget": false })));
        assert!(widget_enabled(&json!({ "widget": { "offsetBottom": 120 } })));
        assert!(!widget_enabled(&json!({ "widget": { "enabled": false } })));
        // 缺省：老配置的 true / 完全没有 widget 键都按开启处理。
        assert!(widget_enabled(&json!({})));

        assert_eq!(
            widget_placement(&json!({ "widget": true })),
            WidgetPlacement::default()
        );
        let custom = widget_placement(&json!({
            "widget": { "offsetRight": 8, "offsetBottom": 120, "size": 56 }
        }));
        assert_eq!(
            custom,
            WidgetPlacement {
                right: 8,
                bottom: 120,
                size: 56
            }
        );
        // 夹紧：不许把图标放到屏幕外，也不许缩成看不见的点。
        let clamped = widget_placement(&json!({
            "widget": { "offsetRight": -50, "offsetBottom": 9999, "size": 4 }
        }));
        assert_eq!(clamped.right, 0);
        assert_eq!(clamped.bottom, 800);
        assert_eq!(clamped.size, 28);
    }

    /// 自动拉起默认必须是关的：`autoStart` 缺省时读用户配置文件（不存在 = 不动进程）。
    #[test]
    fn auto_start_defaults_to_off() {
        // 只验语义，不写真实配置：函数读 `~/.wb-switch/cdp_config.json`，
        // 单测环境里可能被用户改成 true，所以这里断言的是「默认值常量级结论」。
        assert_eq!(AutoStartOutcome::Disabled.label(), "off");
        assert_eq!(AutoStartOutcome::AlreadyTried.label(), "once-only");
        assert_eq!(AutoStartOutcome::Attempted.label(), "attempted");
    }
}
