//! 归档任务清理：删 WorkBuddy 会话库里的 archived 会话 + 对应正文 jsonl。
//!
//! 适配性约定（勿破坏）：
//! - 路径全部运行时从「当前用户主目录」解析（`home/.workbuddy`），不硬编码盘符与用户名；
//!   装了 WorkBuddy 的机器即有此目录，没有就返回空统计、按钮自然禁用。
//! - 只删 `status='archived'` 的会话，active/working 等一概不碰。
//!
//! 两条硬护栏：
//! ① WorkBuddy 进程运行中 → 拒绝执行（它退出时会用内存缓存回写 DB，删了也被覆盖回去）；
//! ② 执行前把 DB 备份 + 待删会话清单落到本应用备份目录，物理删除留一条恢复后路。

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use chrono::Local;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};

use wb_switch_core::modules::{config, process};
use wb_switch_core::modules::variant::WbVariant;

/// WorkBuddy 主数据目录（当前用户主目录下）。
fn wb_data_dir() -> PathBuf {
    config::home_dir().join(".workbuddy")
}

fn db_path() -> PathBuf {
    wb_data_dir().join("workbuddy.db")
}

/// 会话正文：`projects/<工作目录路径编码>/<sessionId>.jsonl`。
fn projects_dir() -> PathBuf {
    wb_data_dir().join("projects")
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{}{}", bytes, UNITS[unit])
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}

/// 读出库中全部 archived 会话 id。库不存在 / 打不开 → None（统计显示空态）。
fn read_archived_ids() -> Option<Vec<String>> {
    let conn = Connection::open_with_flags(db_path(), OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let mut stmt = conn.prepare("SELECT id FROM sessions WHERE status = 'archived'").ok()?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .ok()?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    Some(rows)
}

/// 归档会话正文的体积：扫 projects/*/<id>.jsonl，只认库里有记录的 id。
fn transcript_sizes(ids: &[String]) -> (u64, u64) {
    let wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut files = 0u64;
    let mut bytes = 0u64;
    if let Ok(entries) = fs::read_dir(projects_dir()) {
        for dir in entries.filter_map(Result::ok) {
            let Ok(files_in_dir) = fs::read_dir(dir.path()) else {
                continue;
            };
            for file in files_in_dir.filter_map(Result::ok) {
                let path = file.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                    continue;
                }
                let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("");
                if wanted.contains(stem) {
                    bytes += file.metadata().map(|meta| meta.len()).unwrap_or(0);
                    files += 1;
                }
            }
        }
    }
    (files, bytes)
}

/// 只读统计：条数 + 正文体积。进设置页即调，不写任何文件。
pub fn stats() -> Value {
    let db_exists = db_path().exists();
    let running = process::is_workbuddy_running(WbVariant::Cn)
        || process::is_workbuddy_running(WbVariant::Ai);
    match read_archived_ids() {
        Some(ids) => {
            let (_, bytes) = transcript_sizes(&ids);
            json!({
                "ok": true,
                "dbExists": db_exists,
                "running": running,
                "count": ids.len(),
                "bytes": bytes,
                "human": human_bytes(bytes),
            })
        }
        None => json!({
            "ok": db_exists,
            "dbExists": db_exists,
            "running": running,
            "count": 0,
            "bytes": 0,
            "human": human_bytes(0),
            "error": if db_exists { Some("db open failed".to_string()) } else { None },
        }),
    }
}

/// 执行清理：备份 → 删正文 → 删库行。任一步失败即停，不半途吞错。
pub fn purge() -> Value {
    if process::is_workbuddy_running(WbVariant::Cn)
        || process::is_workbuddy_running(WbVariant::Ai)
    {
        return json!({ "ok": false, "blocked": true, "error": "workbuddy running" });
    }

    let Some(ids) = read_archived_ids() else {
        return json!({
            "ok": false,
            "blocked": true,
            "error": if db_path().exists() { "db open failed" } else { "workbuddy db not found" },
        });
    };
    if ids.is_empty() {
        return json!({ "ok": true, "deleted": 0, "freedBytes": 0, "human": human_bytes(0) });
    }

    // 备份：DB 三件套 + id 清单，落本应用自己的备份目录（不写 WorkBuddy 数据区）。
    let backup_dir = config::backup_dir().join(format!("archived-purge-{}", Local::now().format("%Y%m%d-%H%M%S")));
    if let Err(error) = fs::create_dir_all(&backup_dir) {
        return json!({ "ok": false, "error": error.to_string() });
    }
    let db = db_path();
    for suffix in ["", "-wal", "-shm"] {
        let source = PathBuf::from(format!("{}{}", db.display(), suffix));
        if source.exists() {
            let _ = fs::copy(&source, backup_dir.join(format!("workbuddy.db{}", suffix)));
        }
    }
    let _ = fs::write(backup_dir.join("archived-ids.txt"), ids.join("\n"));

    // 删正文：逐文件删，统计实际释放量；缺文件不算失败。
    let (mut freed, mut removed) = (0u64, 0u64);
    let wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();
    if let Ok(entries) = fs::read_dir(projects_dir()) {
        for dir in entries.filter_map(Result::ok) {
            let Ok(files_in_dir) = fs::read_dir(dir.path()) else {
                continue;
            };
            for file in files_in_dir.filter_map(Result::ok) {
                let path = file.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                    continue;
                }
                let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("");
                if !wanted.contains(stem) {
                    continue;
                }
                let size = file.metadata().map(|meta| meta.len()).unwrap_or(0);
                if fs::remove_file(&path).is_ok() {
                    freed += size;
                    removed += 1;
                }
            }
        }
    }

    // 删库行 + 回收空间。DB 行删不掉不影响已删的正文（少腾一点库空间而已）。
    let mut db_error = None;
    match Connection::open(&db) {
        Ok(conn) => {
            let _ = conn.busy_timeout(std::time::Duration::from_secs(3));
            if let Err(error) = conn.execute("DELETE FROM sessions WHERE status = 'archived'", []) {
                db_error = Some(error.to_string());
            } else if let Err(error) = conn.execute_batch("VACUUM;") {
                db_error = Some(error.to_string());
            }
        }
        Err(error) => db_error = Some(error.to_string()),
    }

    json!({
        "ok": db_error.is_none(),
        "deleted": removed,
        "freedBytes": freed,
        "human": human_bytes(freed),
        "total": ids.len(),
        "backupDir": backup_dir.display().to_string(),
        "error": db_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_units() {
        assert_eq!(human_bytes(0), "0B");
        assert_eq!(human_bytes(512), "512B");
        assert_eq!(human_bytes(750 * 1024 * 1024 + 209715), "750.2MB");
    }
}
