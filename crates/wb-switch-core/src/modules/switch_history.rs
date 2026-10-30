//! 切换历史：每次账号切换一条，滚动保留最近 [`HISTORY_LIMIT`] 条。
//!
//! 为什么单独记一份：切换是**破坏性动作**（替换式同步会物理删目标账号会话），出事后
//! 必须能回答「谁在什么时候把哪个号切成了哪个号、当时余额多少、结果如何、为什么失败」。
//! 运行日志（`notifications` / `auto_daily_logs` / `error.log`）只能看到一句提示，
//! 没有前后对照，也没有结构化字段供前端筛选。
//!
//! 边界：只存本机（`store_dir()/switch_history.json`），明文，含 uid 与昵称。
//! 写入是**尽力而为**：记历史失败绝不能反过来让切换失败（调用方必须忽略 Err）。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::modules::config::{atomic_write, store_dir};

/// 保留的最近条数。切换是低频动作（一天几次到几十次），500 条足够覆盖数月。
pub const HISTORY_LIMIT: usize = 500;
/// 存档格式版本；读到其它版本视为不可用（保留原文件，不覆盖）。
pub const HISTORY_VERSION: u32 = 1;
const STORE_FILE_NAME: &str = "switch_history.json";

/// 会话搬运摘要：让「复制了几条 / 源有几条 / 删了没」在事后可核对。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDigest {
    #[serde(default)]
    pub copied: i64,
    #[serde(default)]
    pub source_total: i64,
    #[serde(default)]
    pub failed: i64,
    #[serde(default)]
    pub replaced: bool,
}

/// 一次切换的完整记录。
///
/// `result` / `stage` 是前端筛选与告警的依据：
/// - `ok` + `done`：写认证成功且身份校验通过
/// - `rolled_back` + `verify`：写后身份不符，已还原切换前认证文件
/// - `blocked` + `session`：会话搬运不完整，未写认证（fail-closed）
/// - `blocked` + `auth_write`：认证写入失败，已还原
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchEntry {
    pub ts: i64,
    /// `ok` | `rolled_back` | `blocked`
    pub result: String,
    /// `done` | `session` | `auth_write` | `verify` | `launch`
    pub stage: String,
    /// 目标账号在账号库中的 id（前端据此跳转/高亮）
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub to_uid: String,
    #[serde(default)]
    pub to_label: String,
    #[serde(default)]
    pub from_uid: String,
    #[serde(default)]
    pub from_label: String,
    #[serde(default)]
    pub variant: String,
    /// `plain` | `copy` | `sync` | `replace`
    #[serde(default)]
    pub mode: String,
    /// `effective` | `identity-mismatch` | `identity-unverified` | `""`
    #[serde(default)]
    pub identity: String,
    #[serde(default)]
    pub cost_ms: i64,
    #[serde(default)]
    pub sessions: Option<SessionDigest>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryStore {
    version: u32,
    #[serde(default)]
    items: Vec<SwitchEntry>,
}

impl Default for HistoryStore {
    fn default() -> Self {
        Self {
            version: HISTORY_VERSION,
            items: Vec::new(),
        }
    }
}

fn store_file_at(root: &Path) -> PathBuf {
    root.join(STORE_FILE_NAME)
}

fn load_at(root: &Path) -> Result<HistoryStore, String> {
    let file = store_file_at(root);
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(HistoryStore::default())
        }
        Err(error) => return Err(format!("切换历史不可读：{error}")),
    };
    let store: HistoryStore =
        serde_json::from_str(&text).map_err(|error| format!("切换历史内容损坏：{error}"))?;
    if store.version != HISTORY_VERSION {
        return Err(format!(
            "切换历史版本 {} 不受支持（当前支持 {}），已保留原文件",
            store.version, HISTORY_VERSION
        ));
    }
    Ok(store)
}

fn save_at(root: &Path, store: &HistoryStore) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|error| format!("切换历史目录不可写：{error}"))?;
    let text = serde_json::to_string_pretty(store)
        .map_err(|error| format!("切换历史序列化失败：{error}"))?;
    atomic_write(&store_file_at(root), &text).map_err(|error| format!("切换历史写入失败：{error}"))
}

/// 追加一条记录（新记录在最前）。超限按尾部截断。
pub fn record(entry: SwitchEntry) -> Result<(), String> {
    record_at(&store_dir(), entry)
}

fn record_at(root: &Path, entry: SwitchEntry) -> Result<(), String> {
    // 存档损坏/版本不符时**拒绝写入**，不用空存档覆盖它：覆盖等于把用户既有历史
    // 静默清空，比这次记不上更糟（切换本身仍会成功，调用方忽略 Err）。
    let mut store = load_at(root)?;
    store.items.insert(0, entry);
    if store.items.len() > HISTORY_LIMIT {
        store.items.truncate(HISTORY_LIMIT);
    }
    save_at(root, &store)
}

/// 读取最近 `limit` 条（`limit` 为 0 表示全量，上限 [`HISTORY_LIMIT`]）。
pub fn load(limit: usize) -> serde_json::Value {
    load_from(&store_dir(), limit)
}

fn load_from(root: &Path, limit: usize) -> serde_json::Value {
    let store = match load_at(root) {
        Ok(store) => store,
        Err(error) => {
            return serde_json::json!({
                "ok": false,
                "error": error,
                "total": 0,
                "entries": [],
            })
        }
    };
    let total = store.items.len();
    let take = if limit == 0 {
        total
    } else {
        limit.min(total)
    };
    let entries: Vec<&SwitchEntry> = store.items.iter().take(take).collect();
    serde_json::json!({
        "ok": true,
        "version": HISTORY_VERSION,
        "total": total,
        "limit": HISTORY_LIMIT,
        "entries": entries,
    })
}

/// 清空历史，返回删除条数。
pub fn clear() -> Result<usize, String> {
    clear_at(&store_dir())
}

fn clear_at(root: &Path) -> Result<usize, String> {
    let store = load_at(root)?;
    let removed = store.items.len();
    save_at(root, &HistoryStore::default())?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "wb_switch_history_{}_{name}",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn entry(ts: i64, result: &str) -> SwitchEntry {
        SwitchEntry {
            ts,
            result: result.to_string(),
            stage: "done".to_string(),
            account_id: "acc-1".to_string(),
            to_uid: "uid-b".to_string(),
            to_label: "B".to_string(),
            from_uid: "uid-a".to_string(),
            from_label: "A".to_string(),
            variant: "cn".to_string(),
            mode: "replace".to_string(),
            identity: "effective".to_string(),
            cost_ms: 1234,
            sessions: Some(SessionDigest {
                copied: 74,
                source_total: 74,
                failed: 0,
                replaced: true,
            }),
            error: None,
        }
    }

    /// 新记录在最前，读取按时间倒序。
    #[test]
    fn newest_record_comes_first() {
        let dir = TempDir::new("order");
        record_at(&dir.0, entry(1, "ok")).unwrap();
        record_at(&dir.0, entry(2, "rolled_back")).unwrap();
        let value = load_from(&dir.0, 0);
        assert_eq!(value["total"], 2);
        assert_eq!(value["entries"][0]["ts"], 2);
        assert_eq!(value["entries"][0]["result"], "rolled_back");
        assert_eq!(value["entries"][1]["result"], "ok");
        // 结构化字段能原样取到（前端按 camelCase 读）
        assert_eq!(value["entries"][0]["sessions"]["copied"], 74);
        assert_eq!(value["entries"][0]["costMs"], 1234);
    }

    /// 超过上限按尾部截断，只保留最新 500 条。
    #[test]
    fn history_is_capped_at_limit() {
        let dir = TempDir::new("cap");
        let mut store = HistoryStore::default();
        for ts in 0..(HISTORY_LIMIT as i64 + 5) {
            store.items.insert(0, entry(ts, "ok"));
        }
        save_at(&dir.0, &store).unwrap();
        record_at(&dir.0, entry(9999, "ok")).unwrap();
        let value = load_from(&dir.0, 0);
        assert_eq!(value["total"], HISTORY_LIMIT);
        assert_eq!(value["entries"][0]["ts"], 9999);
    }

    /// limit 生效且不超过总量；0 表示全量。
    #[test]
    fn limit_is_applied() {
        let dir = TempDir::new("limit");
        for ts in 0..5 {
            record_at(&dir.0, entry(ts, "ok")).unwrap();
        }
        assert_eq!(load_from(&dir.0, 2)["entries"].as_array().unwrap().len(), 2);
        assert_eq!(load_from(&dir.0, 0)["entries"].as_array().unwrap().len(), 5);
        assert_eq!(load_from(&dir.0, 99)["entries"].as_array().unwrap().len(), 5);
    }

    /// 存档损坏时：读取报错但不删文件；写入不覆盖原文件（宁可记不上也不清空）。
    #[test]
    fn corrupted_store_is_reported_and_preserved() {
        let dir = TempDir::new("corrupt");
        let file = store_file_at(&dir.0);
        std::fs::write(&file, "{not json").unwrap();
        let value = load_from(&dir.0, 0);
        assert_eq!(value["ok"], false);
        assert!(value["error"].as_str().unwrap().contains("损坏"));
        assert!(file.exists(), "损坏的存档不得被删除");
        // 记录不会把损坏文件覆盖成只剩一条
        let _ = record_at(&dir.0, entry(1, "ok"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{not json");
    }

    /// 版本不符不覆盖（避免降级后清掉新版历史）。
    #[test]
    fn future_version_is_rejected() {
        let dir = TempDir::new("version");
        let file = store_file_at(&dir.0);
        std::fs::write(
            &file,
            serde_json::json!({"version": 99, "items": []}).to_string(),
        )
        .unwrap();
        let value = load_from(&dir.0, 0);
        assert_eq!(value["ok"], false);
        assert!(value["error"].as_str().unwrap().contains("版本"));
        assert!(std::fs::read_to_string(&file).unwrap().contains("99"));
    }

    /// 清空返回删除条数，之后读取为空。
    #[test]
    fn clear_reports_removed_count() {
        let dir = TempDir::new("clear");
        record_at(&dir.0, entry(1, "ok")).unwrap();
        record_at(&dir.0, entry(2, "ok")).unwrap();
        assert_eq!(clear_at(&dir.0).unwrap(), 2);
        assert_eq!(load_from(&dir.0, 0)["total"], 0);
        // 再次清空不报错
        assert_eq!(clear_at(&dir.0).unwrap(), 0);
    }

    /// 缺省字段容忍（老版本记录 / 手改过的文件不该让整份历史读不出来）。
    #[test]
    fn missing_fields_get_defaults() {
        let dir = TempDir::new("defaults");
        let file = store_file_at(&dir.0);
        std::fs::write(
            &file,
            serde_json::json!({"version": 1, "items": [{"ts": 5, "result": "ok", "stage": "done"}]})
                .to_string(),
        )
        .unwrap();
        let value = load_from(&dir.0, 0);
        assert_eq!(value["ok"], true);
        assert_eq!(value["entries"][0]["toUid"], "");
        assert_eq!(value["entries"][0]["costMs"], 0);
    }
}
