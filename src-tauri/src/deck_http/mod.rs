//! 面板本地桥：把前端资源、IPC、事件流暴露成 `http://127.0.0.1:<port>`。
//!
//! 为什么要有它：面板是 WorkBuddy 页面里的一个 iframe，而我们的界面是 Vite 构建产物，
//! 必须有 HTTP 源才能被 iframe 加载（`file://` 跨源拿不到 `__TAURI_INTERNALS__`）；
//! 前端那几十个 `invoke` 也需要一个宿主 —— HTTP 是最稳的桥。
//!
//! 端点（全部只绑 127.0.0.1）：
//! - `GET  /`                   → 面板前端；来源见 [`asset`]（嵌入 / dist / vite dev 反代）
//! - `GET  /__deck_shim.js`     → 伪造 `__TAURI_INTERNALS__` 的注入脚本，见 [`shim`]
//! - `GET  /__deck_diag`        → 只读诊断 JSON（白屏排查第一站）
//! - `POST /__ipc?t=<token>`    → `{cmd,args}` 分发到 [`crate::commands`]，见 [`ipc`]
//! - `GET  /__events?t=<token>` → SSE，转发 Tauri `emit` 事件，见 [`events`]
//!
//! token 是进程内随机串，只出现在面板 URL 里：别的网页拿不到，本地进程也猜不到。
//! `/__deck_diag` 故意不校验 token —— 桥自己出问题时用户要能直接打开看，
//! 而且它只输出来源/端口这类公开事实，不返回 token 与任何业务数据。

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::AppHandle;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

mod asset;
mod boot;
mod events;
mod ipc;
mod proxy;
mod shim;

/// 已启动的端口（`start` 只会成功一次）。
static PORT: OnceLock<u16> = OnceLock::new();
/// IPC/事件流的访问令牌。
static TOKEN: OnceLock<String> = OnceLock::new();
/// Tauri 事件 → SSE 的广播口。
static EVENT_TX: OnceLock<broadcast::Sender<Value>> = OnceLock::new();

/// 一个 HTTP 响应：(状态码, Content-Type, 正文)。
pub(super) type Reply = (u16, String, Vec<u8>);

/// 候选端口：必须稳定。面板 origin 就是 localStorage 的作用域，
/// 每次随机端口会让主题/语言/选中账号全部重置。
const PORT_CANDIDATES: [u16; 4] = [17653, 17654, 17655, 17656];

/// 面板 iframe 的完整 URL（未启动时返回 None）。
pub fn panel_url() -> Option<String> {
    match (PORT.get(), TOKEN.get()) {
        (Some(port), Some(token)) => Some(format!("http://127.0.0.1:{port}/?__deck_t={token}")),
        _ => None,
    }
}

/// 端口上是否已有本应用的桥在应答（= 另一个实例在跑）。
///
/// 跨进程没有共享状态，只能靠探端口：能连上就说明有人占着。用途是拦住
/// 「GUI 版还开着、又起一个 headless」——两个 widget daemon 会在 WorkBuddy
/// 页面里各注入一个图标，用户看到的是重复悬浮球。
pub fn already_serving() -> bool {
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;
    PORT_CANDIDATES.iter().any(|port| {
        let addr = SocketAddr::from(([127, 0, 0, 1], *port));
        TcpStream::connect_timeout(&addr, Duration::from_millis(150)).is_ok()
    })
}

/// 启动服务并返回面板 URL。重复调用返回同一个 URL。
///
/// 同步 bind（`setup` 里没有 tokio 上下文，不能用 `TcpListener::bind().await`），
/// 之后把 listener 交给后台 task 接管。
pub fn start(app: AppHandle) -> Option<String> {
    if let Some(url) = panel_url() {
        return Some(url);
    }
    let (listener, port) = match PORT_CANDIDATES.into_iter().find_map(|candidate| {
        std::net::TcpListener::bind(("127.0.0.1", candidate))
            .ok()
            .map(|l| (l, candidate))
    }) {
        Some(pair) => pair,
        None => {
            let fallback = std::net::TcpListener::bind(("127.0.0.1", 0)).ok()?;
            let port = fallback.local_addr().ok()?.port();
            (fallback, port)
        }
    };
    listener.set_nonblocking(true).ok()?;
    let token = make_token();
    let _ = PORT.set(port);
    let _ = TOKEN.set(token.clone());

    // Tauri emit 的事件（进度/通知/更新）统一转 SSE：iframe 里没法用原生 IPC。
    let (tx, _rx) = broadcast::channel::<Value>(512);
    if EVENT_TX.set(tx.clone()).is_ok() {
        events::spawn_relay(&app, &tx);
    }

    let accept_app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Ok(listener) = TcpListener::from_std(listener) else {
            return;
        };
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let app = accept_app.clone();
            let token = token.clone();
            tauri::async_runtime::spawn(async move {
                let _ = handle(stream, app, token).await;
            });
        }
    });

    panel_url()
}

/// 十六进制随机串：够防本地误触发，不是密码学边界（只绑回环）。
fn make_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let stack = &nanos as *const _ as usize;
    format!("{nanos:x}{stack:x}")
}

pub(super) struct Request {
    method: String,
    path: String,
    query: String,
    body: Vec<u8>,
}

async fn handle(mut stream: TcpStream, app: AppHandle, token: String) -> std::io::Result<()> {
    let Some(req) = read_request(&mut stream).await else {
        return Ok(());
    };
    // SSE 是长连接，必须独占 stream（`route` 只产出完整响应体）。
    if req.method == "GET" && req.path == "/__events" {
        if !token_ok(&req.query, &token) {
            return write_reply(&mut stream, forbidden()).await;
        }
        return events::serve(stream).await;
    }
    write_reply(&mut stream, route(&app, &token, &req).await).await
}

fn forbidden() -> Reply {
    (
        403,
        "text/plain; charset=utf-8".to_string(),
        b"forbidden".to_vec(),
    )
}

async fn route(app: &AppHandle, token: &str, req: &Request) -> Reply {
    match (req.method.as_str(), req.path.as_str()) {
        ("POST", "/__ipc") => {
            if !token_ok(&req.query, token) {
                return forbidden();
            }
            let parsed: Value = serde_json::from_slice(&req.body).unwrap_or_else(|_| json!({}));
            let cmd = parsed
                .get("cmd")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let args = parsed.get("args").cloned().unwrap_or_else(|| json!({}));
            let body = match ipc::dispatch(app, cmd, &args).await {
                Ok(value) => json!({ "__ok": value }),
                Err(error) => json!({ "__error": error }),
            };
            let bytes = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
            (200, "application/json".to_string(), bytes)
        }
        ("GET", "/__deck_shim.js") => (
            200,
            "application/javascript; charset=utf-8".to_string(),
            shim::SHIM.as_bytes().to_vec(),
        ),
        ("GET", "/__deck_diag") => (
            200,
            "application/json; charset=utf-8".to_string(),
            serde_json::to_vec_pretty(&asset::diag(app).await).unwrap_or_default(),
        ),
        ("GET", _) => asset::serve(app, &req.path, &req.query).await,
        _ => (
            405,
            "text/plain; charset=utf-8".to_string(),
            b"method not allowed".to_vec(),
        ),
    }
}

pub(super) async fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buf: Vec<u8> = Vec::with_capacity(8192);
    let mut tmp = [0u8; 8192];
    let mut head_end: Option<usize> = None;
    let mut content_len = 0usize;
    loop {
        let n = stream.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
        if head_end.is_none() {
            if let Some(pos) = find(&buf, b"\r\n\r\n") {
                head_end = Some(pos + 4);
                let head = String::from_utf8_lossy(&buf[..pos]).to_ascii_lowercase();
                for line in head.lines() {
                    if let Some(rest) = line.strip_prefix("content-length:") {
                        content_len = rest.trim().parse().unwrap_or(0);
                    }
                }
            }
        }
        if let Some(end) = head_end {
            if buf.len() >= end + content_len {
                break;
            }
        }
        if buf.len() > 32 * 1024 * 1024 {
            return None;
        }
    }
    let end = head_end?;
    let head = String::from_utf8_lossy(&buf[..end - 4]).to_string();
    let mut lines = head.lines();
    let first = lines.next()?;
    let mut parts = first.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target, String::new()),
    };
    // fragment 不会出现在 HTTP 请求行里，但有些客户端会把 `#hash` 拼进 path。
    let path = path.split('#').next().unwrap_or("/").to_string();
    let tail = (end + content_len).min(buf.len());
    Some(Request {
        method,
        path,
        query,
        body: buf[end..tail].to_vec(),
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn token_ok(query: &str, token: &str) -> bool {
    query.split('&').any(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        key == "t" && value == token
    })
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "OK",
    }
}

pub(super) async fn write_reply(stream: &mut TcpStream, reply: Reply) -> std::io::Result<()> {
    let (status, mime, body) = reply;
    write_resp(
        stream,
        &format!("{status} {}", reason(status)),
        &mime,
        &body,
    )
    .await
}

pub(super) async fn write_resp(
    stream: &mut TcpStream,
    status: &str,
    mime: &str,
    body: &[u8],
) -> std::io::Result<()> {
    // 一次连接一个响应（Connection: close）：桥的流量是「一个面板 + 几十个静态资源」，
    // 省掉 keep-alive 状态机的复杂度，换来不会因为半开连接卡住。
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_check_rejects_wrong_value() {
        assert!(token_ok("t=abc&x=1", "abc"));
        assert!(!token_ok("t=abd", "abc"));
        assert!(!token_ok("", "abc"));
    }

    #[test]
    fn find_locates_header_end() {
        assert_eq!(find(b"GET / HTTP/1.1\r\n\r\nbody", b"\r\n\r\n"), Some(14));
        assert_eq!(find(b"no separator", b"\r\n\r\n"), None);
    }

    #[test]
    fn panel_url_needs_both_parts() {
        // 单测进程里没启动服务，只能是 None（防止误以为有默认端口）。
        assert!(panel_url().is_none());
    }
}
