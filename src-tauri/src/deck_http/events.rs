//! Tauri 事件 → SSE 的转发层。
//!
//! iframe 里没有原生 IPC 事件通道，前端的 `listen(...)` 由 [`super::shim`] 接管成
//! EventSource；这里负责把主进程 `emit` 的事件推进广播队列。
//!
//! 新增事件时**必须同步这张表**：漏一个的表现是「面板里进度条不动 / 提醒不弹」，
//! 而且不会报错 —— 排查时先对 [`RELAY_EVENTS`]。

use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Listener};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::broadcast;

use super::{write_resp, EVENT_TX};

/// 面板要转发的 Tauri 事件：与前端 `listen(...)` 的事件名一一对应。
///
/// 这张表是**契约快照**：Tauri 2 没有通配订阅，漏一条的表现是「面板里进度条不动 /
/// 更新卡在 downloading / 提醒不弹」，而且不会报错。新增事件时同步三处：
/// 这里、前端 `listen` 调用、以及本文件的 `relay_table_matches_frontend_contract` 单测。
const RELAY_EVENTS: [&str; 14] = [
    "alerts://new",
    "auto-switch",
    "auto-switch-notify",
    "auto-switch-pending",
    "auto://daily-done",
    "purge-progress",
    "switch-finished",
    "switch-progress",
    "tray://done",
    "tray://refresh",
    "update://available",
    "update://checked",
    "update://done",
    "update://progress",
];

/// 注册全部事件监听（`start` 里只调用一次）。
pub(super) fn spawn_relay(app: &AppHandle, tx: &broadcast::Sender<Value>) {
    for name in RELAY_EVENTS {
        let tx = tx.clone();
        let label = name.to_string();
        // tauri 2 的 Event 只带 payload，事件名在注册时就定了 → 用闭包捕获。
        app.listen_any(name, move |ev| {
            let payload: Value = serde_json::from_str(ev.payload()).unwrap_or(Value::Null);
            let _ = tx.send(json!({ "event": label.as_str(), "payload": payload }));
        });
    }
}

/// SSE：一条连接一个订阅者，15s 心跳防中间层掐断。
pub(super) async fn serve(mut stream: TcpStream) -> std::io::Result<()> {
    let Some(tx) = EVENT_TX.get() else {
        return write_resp(
            &mut stream,
            "503 Service Unavailable",
            "text/plain; charset=utf-8",
            b"no bus",
        )
        .await;
    };
    let mut rx = tx.subscribe();
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nCache-Control: no-cache\r\nConnection: close\r\nX-Accel-Buffering: no\r\n\r\n",
        )
        .await?;
    let mut one = [0u8; 1];
    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Ok(value) => {
                    let frame = format!("data: {value}\n\n");
                    if stream.write_all(frame.as_bytes()).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            },
            _ = tokio::time::sleep(Duration::from_secs(15)) => {
                if stream.write_all(b": ping\n\n").await.is_err() {
                    break;
                }
            }
            read = stream.read(&mut one) => {
                // 客户端关闭面板/刷新：read 返回 0 或错误，直接收摊。
                let _ = read;
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 前端 `listen(...)` 的事件清单（`src/**/*.ts|vue` 里以字面量写死的那批）。
    /// 少一条就是面板里那处永远不动 —— 这张表因此必须逐条对齐，不做「大概覆盖」。
    const FRONTEND_CONTRACT: [&str; 14] = [
        "alerts://new",
        "auto-switch",
        "auto-switch-notify",
        "auto-switch-pending",
        "auto://daily-done",
        "purge-progress",
        "switch-finished",
        "switch-progress",
        "tray://done",
        "tray://refresh",
        "update://available",
        "update://checked",
        "update://done",
        "update://progress",
    ];

    #[test]
    fn relay_table_matches_frontend_contract() {
        for name in FRONTEND_CONTRACT {
            assert!(
                RELAY_EVENTS.contains(&name),
                "RELAY_EVENTS 缺少前端在听的事件: {name}"
            );
        }
        assert_eq!(
            RELAY_EVENTS.len(),
            FRONTEND_CONTRACT.len(),
            "表里有前端没在听的多余事件"
        );
    }

    /// 更新流程四条必须齐：缺 `update://done` 会让面板卡在 downloading。
    #[test]
    fn update_flow_is_complete() {
        for name in [
            "update://available",
            "update://checked",
            "update://progress",
            "update://done",
        ] {
            assert!(RELAY_EVENTS.contains(&name), "更新事件缺失: {name}");
        }
    }
}
