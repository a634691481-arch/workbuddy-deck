//! 开发态反代：`tauri dev` 下前端没有落盘产物，只能从 vite dev server 取。
//!
//! 面板 iframe 必须从**本地桥 origin** 加载才能拿到 shim，所以这里做同源转发，
//! 而不是把 iframe 直接指向 1420（那样 `__TAURI_INTERNALS__` 不存在，所有 invoke 失效）。
//!
//! 三条硬约束：
//! - 只在本机开发态启用（生产走嵌入资源，不进这条路径）；
//! - 客户端 `no_proxy()`：主窗口的 WebView2 用 `--no-proxy-server` 直连，桥也必须直连，
//!   否则用户开了系统代理/VPN 时反向代理会去打代理，本地回环反而连不上；
//! - 只转发 GET，且不透传 `Content-Encoding`（我们按明文转发，压缩体会被浏览器当明文解析）。

use std::sync::OnceLock;
use std::time::Duration;

use reqwest::header::{ACCEPT_ENCODING, CONTENT_TYPE};

/// 与 `tauri.conf.json` 的 `build.devUrl` 保持一致（vite.config.ts 里 strictPort 1420）。
const DEFAULT_DEV_URL: &str = "http://127.0.0.1:1420";
/// 反代单请求上限：dev server 偶尔会卡在依赖预构建，别把浏览器请求挂死。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// 可达性探测：本地回环，1.5s 足够区分「没在跑」与「慢」。
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

/// 面板要反代的 dev server 地址。`WB_DECK_DEV_URL` 可覆盖（换端口/远程排障）。
pub(super) fn dev_url() -> String {
    std::env::var("WB_DECK_DEV_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| value.starts_with("http://") || value.starts_with("https://"))
        .unwrap_or_else(|| DEFAULT_DEV_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            // 直连回环：与主窗口 WebView2 的 --no-proxy-server 对齐。
            .no_proxy()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// dev server 是否活着（决定 `tauri dev` 下走反代还是退到 dist）。
pub(super) async fn reachable(base: &str) -> bool {
    match client().get(base).timeout(PROBE_TIMEOUT).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// 是不是一个真 vite dev server：返回的首页带 `/@vite/client`。
///
/// 只判端口通会有误伤（1420 被别的服务占用时会把面板指向它），
/// 这一步是「开发便利」与「生产稳定」之间的闸门。
pub(super) async fn is_vite(base: &str) -> bool {
    // 开发模式下 Deck 往往比 vite 先就绪：一次性探测会把「正在启动」误判成「不是 vite」，
    // 于是回退到生产资源，面板白屏或报错。给 3 次、每次 200ms 的宽限。
    let mut resp = None;
    for attempt in 0..3 {
        match client()
            .get(format!("{base}/"))
            .timeout(PROBE_TIMEOUT)
            .send()
            .await
        {
            Ok(value) => {
                resp = Some(value);
                break;
            }
            Err(err) => {
                if attempt == 2 || !(err.is_connect() || err.is_timeout()) {
                    return false;
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    }
    let Some(resp) = resp else {
        return false;
    };
    if !resp.status().is_success() {
        return false;
    }
    resp.text()
        .await
        .map(|html| html.contains("/@vite/client"))
        .unwrap_or(false)
}

/// 转发一个 GET 请求，返回 `(状态码, Content-Type, 正文)`。
pub(super) async fn get(base: &str, path: &str, query: &str) -> (u16, String, Vec<u8>) {
    let target = if query.is_empty() {
        format!("{base}{path}")
    } else {
        format!("{base}{path}?{query}")
    };
    // 连接类错误重试几次：vite 刚启动时端口还没 bind，直接 502 会让首屏停在错误页。
    let mut attempt = 0;
    let resp = loop {
        match client()
            .get(&target)
            .header(ACCEPT_ENCODING, "identity")
            .send()
            .await
        {
            Ok(resp) => break resp,
            Err(err) => {
                attempt += 1;
                if attempt >= 5 || !(err.is_connect() || err.is_timeout()) {
                    return bad_gateway(format!("vite dev 请求失败: {err}"));
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }
    };
    let status = resp.status().as_u16();
    let mime = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    match resp.bytes().await {
        Ok(bytes) => (status, mime, bytes.to_vec()),
        Err(err) => bad_gateway(format!("读取 vite 响应失败: {err}")),
    }
}

fn bad_gateway(message: String) -> (u16, String, Vec<u8>) {
    (
        502,
        "text/plain; charset=utf-8".to_string(),
        message.into_bytes(),
    )
}

/// WebSocket 基址：面板注入脚本用它把 HMR 连接改回 dev server。
pub(super) fn ws_url(base: &str) -> String {
    if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_url_maps_http_scheme() {
        assert_eq!(ws_url("http://localhost:1420"), "ws://localhost:1420");
        assert_eq!(ws_url("https://dev.local"), "wss://dev.local");
    }

    #[test]
    fn dev_url_has_no_trailing_slash() {
        assert!(!dev_url().ends_with('/'));
    }
}
