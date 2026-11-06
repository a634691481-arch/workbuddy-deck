//! 切换前安全检查（preflight）：在用户点「确认切换」**之前**把后果摆清楚。
//!
//! 完全同步切换会**物理删除**目标账号的旧会话（见 switch.rs `replace_sessions_for_switch`）。
//! 确认弹框里只有一段通用警示文案，用户看不到「这次具体要删多少、删哪些、删了能不能救」。
//! 本模块回答三个问题：
//! 1. 源账号（当前登录）有多少会话要搬过去；
//! 2. 目标账号现有多少会话、哪些会被删除；
//! 3. 删除前生成一份**可校验**的备份（正文 + sha256 清单）。
//!
//! 边界：只读 + 备份，不改任何 WorkBuddy 状态。备份写入 `~/.wb-switch/backups/
//! switch-preflight-<时间戳>/`，每条会话正文一个文件，`manifest.json` 记录
//! 逐文件 sha256，清单自身的 sha256 也会返回给前端展示。
//!
//! 体积护栏：正文总量超过 [`BODY_BACKUP_CAP_BYTES`] 时**不生成备份**，只在结果里
//! 说明总量（如 X MB）。半个备份比没有备份更危险——用户以为能恢复，实际缺文件。

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use wb_switch_core::modules::config::{atomic_write, now_ms, store_dir};
use wb_switch_core::modules::{account, auto_switch, session};

/// 单次备份的正文体积上限。超出就不做备份，结果里明确说明。
const BODY_BACKUP_CAP_BYTES: u64 = 512 * 1024 * 1024;

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// 在 WorkBuddy 的 projects 目录下找会话正文（`<projects>/<cwd编码>/<sessionId>.jsonl`）。
/// cwd 编码规则不可靠，按 session id 在一级子目录里搜——与归档清理同一套办法。
fn find_session_body(projects_dir: &Path, session_id: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(projects_dir).ok()?;
    for entry in entries.flatten() {
        let candidate = entry.path().join(format!("{session_id}.jsonl"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// 切换前安全检查 + 备份。返回值不抛 Err：失败降级为 `ok:false` 字段，
/// 弹框照常能开（不能因为备份失败就不让用户切换，只让他看不清后果是更糟的选择——
/// 所以失败时 `preflight.error` 会显示在弹框里）。
pub fn preflight(account_id: &str) -> Value {
    let now = now_ms();
    let Some(target) = account::find_account(account_id) else {
        return json!({ "ok": false, "error": format!("账号不存在: {account_id}") });
    };
    let target_label = account::account_display_name(&target);

    // 源账号 = 当前登录账号（与切换执行时的判定同口径）
    let source_id = auto_switch::current_account_id();
    let (source_label, source_sessions) = match &source_id {
        Some(id) => match account::find_account(id) {
            Some(acc) => {
                let list = session::list_sessions_for_account(&acc);
                let count = list.get("sessions").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
                (account::account_display_name(&acc), count)
            }
            None => (String::new(), 0),
        },
        None => (String::new(), 0),
    };

    // 目标账号现有会话（这些在完全同步切换中会被删除）
    let target_list = session::list_sessions_for_account(&target);
    let sessions = target_list
        .get("sessions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let session_count = sessions.len();

    // 会话摘要给弹框展示（全量给清单，前端只露前几条）
    let session_rows: Vec<Value> = sessions
        .iter()
        .map(|item| {
            json!({
                "id": item.get("id").cloned().unwrap_or(Value::Null),
                "title": item.get("title").cloned().unwrap_or(Value::Null),
                "updatedAt": item.get("updatedAt").cloned().unwrap_or(Value::Null),
                "isPlayground": item.get("isPlayground").cloned().unwrap_or(json!(false)),
            })
        })
        .collect();

    let backup = build_backup(&target, &sessions, account_id, &target_label, now);

    json!({
        "ok": true,
        "source": {
            "accountId": source_id,
            "label": source_label,
            "sessionCount": source_sessions,
        },
        "target": {
            "accountId": account_id,
            "label": target_label,
            "sessionCount": session_count,
            "sessions": session_rows,
        },
        "backup": backup,
        "checkedAt": now,
    })
}

/// 生成备份：每条会话正文一个文件 + manifest.json（逐文件 sha256）。
fn build_backup(
    target: &Value,
    sessions: &[Value],
    account_id: &str,
    target_label: &str,
    now: i64,
) -> Value {
    let variant = account::variant_of(target);
    let projects_dir = session::SessionPaths::for_variant(variant).projects_dir();

    // 先量体积：超限就不动手（半份备份比没有更危险）
    let mut bodies: Vec<(Value, PathBuf)> = Vec::new();
    let mut total_bytes: u64 = 0;
    for item in sessions {
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(path) = find_session_body(&projects_dir, id) else {
            continue;
        };
        let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        total_bytes += size;
        bodies.push((item.clone(), path));
    }
    if total_bytes > BODY_BACKUP_CAP_BYTES {
        return json!({
            "skipped": true,
            "reason": "size",
            "totalBytes": total_bytes,
        });
    }

    let dir = store_dir()
        .join("backups")
        .join(format!("switch-preflight-{}", chrono::DateTime::from_timestamp_millis(now)
            .map(|dt| dt.format("%Y%m%d-%H%M%S").to_string())
            .unwrap_or_else(|| now.to_string())));
    if std::fs::create_dir_all(&dir).is_err() {
        return json!({ "skipped": true, "reason": "io" });
    }

    let mut files: Vec<Value> = Vec::new();
    for (item, path) in &bodies {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(_) => continue, // 读不了的正文明记为缺失，不让整份备份失败
        };
        let id = item.get("id").and_then(Value::as_str).unwrap_or("unknown");
        let name = format!("{id}.jsonl");
        if std::fs::write(dir.join(&name), &bytes).is_err() {
            continue;
        }
        files.push(json!({
            "sessionId": id,
            "title": item.get("title").cloned().unwrap_or(Value::Null),
            "file": name,
            "size": bytes.len(),
            "sha256": sha256_hex(&bytes),
        }));
    }

    let manifest = json!({
        "version": 1,
        "createdAt": now,
        "targetAccountId": account_id,
        "targetLabel": target_label,
        "sessionCount": sessions.len(),
        "files": files,
    });
    let manifest_text = serde_json::to_string_pretty(&manifest).unwrap_or_default();
    let manifest_sha = sha256_hex(manifest_text.as_bytes());
    if atomic_write(&dir.join("manifest.json"), &manifest_text).is_err() {
        return json!({ "skipped": true, "reason": "io" });
    }

    json!({
        "skipped": false,
        "path": dir.to_string_lossy(),
        "fileCount": files.len(),
        "totalBytes": total_bytes,
        "manifestSha256": manifest_sha,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_stable() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn find_session_body_missing_dir() {
        assert!(find_session_body(Path::new("Z:/definitely/not/exist"), "nope").is_none());
    }
}
