//! 切换前安全检查（preflight）：在用户点「确认切换」**之前**把后果摆清楚。
//!
//! 完全同步切换会**物理删除**目标账号的旧会话（见 switch.rs `replace_sessions_for_switch`）。
//! 确认弹框里只有一段通用警示文案，用户看不到「这次具体要删多少、删哪些、删了能不能救」。
//! 本模块回答三个问题：源账号（当前登录）有多少会话要搬过去；目标账号现有多少会话、
//! 哪些会被删除；删除前生成一份**可校验**的备份（正文 + sha256 清单）。
//!
//! 边界：只读 + 备份，不改任何 WorkBuddy 状态。备份写入 `~/.wb-switch/backups/
//! switch-preflight-<时间戳>/`，每条会话正文一个文件，`manifest.json` 记录逐文件
//! sha256，清单自身的 sha256 也会返回给前端展示。
//!
//! 体积护栏：正文总量超过 [`BODY_BACKUP_CAP_BYTES`] 时**不生成备份**，只在结果里
//! 说明总量（如 X MB）。半个备份比没有备份更危险——用户以为能恢复，实际缺文件。
//! 同理，任一正文读/写/校验失败都判定**整份失败**：先写临时目录，全成功才改名，
//! 失败时删掉临时目录并返回 `skipped:true` + 失败文件名，绝不谎报文件数。

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use wb_switch_core::modules::config::{atomic_write, now_ms, store_dir};
use wb_switch_core::modules::{account, auto_switch, session};

/// 单次备份的正文体积上限。超出就不做备份，结果里明确说明。
const BODY_BACKUP_CAP_BYTES: u64 = 512 * 1024 * 1024;

/// 正式备份目录名撞车时的重试次数（同一秒内连开两次弹框）。
const PROMOTE_ATTEMPTS: u32 = 8;

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// 备份失败结果：`skipped:true` 让弹框红字警示，`stage`/`failedFile`/`error` 说明卡在哪。
/// 所有失败都走这里，绝不再返回「已备份 N 个文件」的半份结果。
fn backup_failed(stage: &str, file: impl std::fmt::Display, detail: impl std::fmt::Display) -> Value {
    let file = file.to_string();
    json!({
        "skipped": true, "reason": "io", "stage": stage,
        "failedFile": file, "error": format!("{stage}失败: {detail}"),
    })
}

/// 在 WorkBuddy 的 projects 目录下找会话正文（`<projects>/<cwd编码>/<sessionId>.jsonl`）。
/// cwd 编码规则不可靠，按 session id 在一级子目录里搜——与归档清理同一套办法。
/// 目录读不了返回 `Err`（与「这条会话没有正文」的 `Ok(None)` 区分开）。
fn find_session_body(projects_dir: &Path, session_id: &str) -> std::io::Result<Option<PathBuf>> {
    for entry in std::fs::read_dir(projects_dir)? {
        let Ok(entry) = entry else { continue };
        let candidate = entry.path().join(format!("{session_id}.jsonl"));
        if candidate.is_file() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// 收集要备份的正文清单与总体积。没有正文文件的会话（任务页等）只是跳过；
/// 目录读不了则是失败——否则整库都读不到时会被当成「没有正文要备份」。
fn collect_bodies(
    projects_dir: &Path,
    sessions: &[Value],
) -> Result<(Vec<(Value, PathBuf)>, u64), Value> {
    let mut bodies: Vec<(Value, PathBuf)> = Vec::new();
    let mut total_bytes: u64 = 0;
    for item in sessions {
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            continue;
        };
        let found = match find_session_body(projects_dir, id) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(backup_failed("扫描正文", projects_dir.display(), e));
            }
            other => other.ok().flatten(),
        };
        if let Some(path) = found {
            total_bytes += std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            bodies.push((item.clone(), path));
        }
    }
    Ok((bodies, total_bytes))
}

/// 把正文与清单都写进 `dir`（临时目录）：任一文件失败立即返回，由调用方丢掉整份目录。
/// 每个文件写完都**读回校验** sha256——写入报成功不等于落盘正确。
fn fill_backup(
    dir: &Path,
    bodies: &[(Value, PathBuf)],
    sessions: &[Value],
    account_id: &str,
    target_label: &str,
    now: i64,
) -> Result<(usize, String), Value> {
    let mut files: Vec<Value> = Vec::with_capacity(bodies.len());
    for (item, source) in bodies {
        let id = item.get("id").and_then(Value::as_str).unwrap_or("unknown");
        let name = format!("{id}.jsonl");
        let bytes = std::fs::read(source).map_err(|e| backup_failed("读取正文", source.display(), e))?;
        let sha = sha256_hex(&bytes);
        let target = dir.join(&name);
        std::fs::write(&target, &bytes).map_err(|e| backup_failed("写入正文", target.display(), e))?;
        let written = std::fs::read(&target).map_err(|e| backup_failed("校验正文", target.display(), e))?;
        if sha256_hex(&written) != sha {
            return Err(backup_failed("校验正文", target.display(), "sha256 不一致"));
        }
        files.push(json!({
            "sessionId": id, "title": item.get("title").cloned().unwrap_or(Value::Null),
            "file": name, "size": bytes.len(), "sha256": sha,
        }));
    }
    let manifest = json!({
        "version": 1, "createdAt": now, "targetAccountId": account_id,
        "targetLabel": target_label, "sessionCount": sessions.len(),
        "missingCount": sessions.len() - files.len(), "files": files,
    });
    let text = serde_json::to_string_pretty(&manifest)
        .map_err(|e| backup_failed("序列化清单", "manifest.json", e))?;
    let sha = sha256_hex(text.as_bytes());
    let path = dir.join("manifest.json");
    atomic_write(&path, &text).map_err(|e| backup_failed("写入清单", path.display(), e))?;
    let written = std::fs::read(&path).map_err(|e| backup_failed("校验清单", path.display(), e))?;
    if written != text.as_bytes() {
        return Err(backup_failed("校验清单", path.display(), "内容与写入值不一致"));
    }
    Ok((files.len(), sha))
}

/// 全部写成功才把临时目录改名成正式目录：中途失败的半份永远进不了 `backups/`。
/// 同一秒内连开两次弹框会撞名，依次试 `-1`、`-2`…
fn promote(staging: &Path, backups_root: &Path, now: i64) -> Result<PathBuf, Value> {
    let stamp = chrono::DateTime::from_timestamp_millis(now)
        .map(|dt| dt.format("%Y%m%d-%H%M%S").to_string())
        .unwrap_or_else(|| now.to_string());
    let base = format!("switch-preflight-{stamp}");
    for attempt in 0..PROMOTE_ATTEMPTS {
        let name = if attempt == 0 { base.clone() } else { format!("{base}-{attempt}") };
        let dest = backups_root.join(name);
        if dest.exists() {
            continue;
        }
        if let Err(e) = std::fs::rename(staging, &dest) {
            return Err(backup_failed("收尾改名", dest.display(), e));
        }
        return Ok(dest);
    }
    Err(backup_failed("收尾改名", base, "同名备份目录已存在"))
}

/// 备份主体：体积护栏 → 写临时目录（逐文件校验）→ 成功才改成正式目录。
fn build_backup_at(
    backups_root: &Path,
    projects_dir: &Path,
    sessions: &[Value],
    account_id: &str,
    target_label: &str,
    now: i64,
) -> Value {
    let (bodies, total_bytes) = match collect_bodies(projects_dir, sessions) {
        Ok(collected) => collected,
        Err(failed) => return failed,
    };
    if total_bytes > BODY_BACKUP_CAP_BYTES {
        return json!({ "skipped": true, "reason": "size", "totalBytes": total_bytes });
    }
    let staging = backups_root.join(format!(".staging-{now}-{}", std::process::id()));
    let written = std::fs::create_dir_all(&staging)
        .map_err(|e| backup_failed("准备目录", staging.display(), e))
        .and_then(|()| fill_backup(&staging, &bodies, sessions, account_id, target_label, now));
    let (file_count, manifest_sha) = match written {
        Ok(value) => value,
        Err(failed) => {
            let _ = std::fs::remove_dir_all(&staging);
            return failed;
        }
    };
    match promote(&staging, backups_root, now) {
        Ok(dir) => json!({
            "skipped": false, "path": dir.to_string_lossy(),
            "fileCount": file_count, "totalBytes": total_bytes, "manifestSha256": manifest_sha,
        }),
        Err(failed) => {
            let _ = std::fs::remove_dir_all(&staging);
            failed
        }
    }
}

/// 源账号（当前登录）的名称与会话数；没登录或账号已不在库里就返回空。
fn source_summary() -> (Option<String>, String, usize) {
    let Some(id) = auto_switch::current_account_id() else {
        return (None, String::new(), 0);
    };
    match account::find_account(&id) {
        Some(acc) => {
            let list = session::list_sessions_for_account(&acc);
            let count = list.get("sessions").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
            (Some(id), account::account_display_name(&acc), count)
        }
        None => (None, String::new(), 0),
    }
}

/// 弹框只露前几条，清单仍给全量。
fn session_rows(sessions: &[Value]) -> Vec<Value> {
    sessions
        .iter()
        .map(|item| {
            json!({
                "id": item.get("id").cloned().unwrap_or(Value::Null),
                "title": item.get("title").cloned().unwrap_or(Value::Null),
                "updatedAt": item.get("updatedAt").cloned().unwrap_or(Value::Null),
                "isPlayground": item.get("isPlayground").cloned().unwrap_or(json!(false)),
            })
        })
        .collect()
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

    let (source_id, source_label, source_sessions) = source_summary();

    // 目标账号现有会话（这些在完全同步切换中会被删除）
    let target_list = session::list_sessions_for_account(&target);
    let sessions = target_list.get("sessions").and_then(Value::as_array).cloned().unwrap_or_default();

    // 备份根目录固定在 `~/.wb-switch/backups/`，正文目录取自目标账号档位
    let projects_dir = session::SessionPaths::for_variant(account::variant_of(&target)).projects_dir();
    let backup = build_backup_at(
        &store_dir().join("backups"),
        &projects_dir,
        &sessions,
        account_id,
        &target_label,
        now,
    );

    json!({
        "ok": true,
        "source": { "accountId": source_id, "label": source_label, "sessionCount": source_sessions },
        "target": {
            "accountId": account_id, "label": target_label,
            "sessionCount": sessions.len(), "sessions": session_rows(&sessions),
        },
        "backup": backup,
        "checkedAt": now,
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
    fn find_session_body_reports_missing_dir() {
        assert!(find_session_body(Path::new("Z:/definitely/not/exist"), "nope").is_err());
    }

    /// 中途写失败必须整份失败：不留半份目录、不留正式备份目录、不说「已备份」。
    #[test]
    fn backup_aborts_as_a_whole_on_write_failure() {
        let root = std::env::temp_dir().join(format!("wb_preflight_{}", now_ms()));
        let body_dir = root.join("projects").join("ws");
        std::fs::create_dir_all(&body_dir).unwrap();
        std::fs::write(body_dir.join("s1.jsonl"), b"{}\n").unwrap();
        let (backups, now) = (root.join("backups"), now_ms());
        // 预先在本次 staging 目录里占位：让第一个正文的写入必然失败
        let staging = backups.join(format!(".staging-{now}-{}", std::process::id()));
        std::fs::create_dir_all(staging.join("s1.jsonl")).unwrap();

        let sessions = vec![json!({ "id": "s1", "title": "one" })];
        let out = build_backup_at(&backups, &root.join("projects"), &sessions, "acc", "label", now);

        assert_eq!(out.get("skipped"), Some(&json!(true)));
        let failed_file = out.get("failedFile").and_then(Value::as_str).unwrap_or_default();
        assert!(failed_file.contains("s1.jsonl"), "失败文件名要能指认：{failed_file}");
        assert!(!staging.exists(), "半份备份目录必须被删除");
        let leftovers = std::fs::read_dir(&backups).map(|entries| entries.flatten().count()).unwrap_or(0);
        assert_eq!(leftovers, 0, "失败时 backups/ 下不能留任何目录");

        let _ = std::fs::remove_dir_all(&root);
    }
}
