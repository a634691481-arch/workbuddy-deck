//! 分组存档的读写：文件位置、版本校验、原子写。
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::modules::config::atomic_write;

pub const GROUPS_VERSION: u32 = 1;
const STORE_FILE_NAME: &str = "account_groups.json";
/// 单账号标签上限：标签是检索辅助，不是自由文本备注（备注已有独立字段）。
pub const MAX_TAGS: usize = 8;
pub const MAX_TAG_LEN: usize = 12;
pub const MAX_GROUPS: usize = 12;
pub const MAX_NAME_LEN: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupDef {
    pub id: String,
    pub name: String,
    /// 组内账号一律不参与限流自动切换（用户手动切换不受影响）
    #[serde(default)]
    pub exclude_auto_switch: bool,
    #[serde(default)]
    pub created_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberDef {
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GroupsStore {
    version: u32,
    #[serde(default)]
    pub(crate) groups: Vec<GroupDef>,
    #[serde(default)]
    pub(crate) members: HashMap<String, MemberDef>,
}

impl Default for GroupsStore {
    fn default() -> Self {
        Self {
            version: GROUPS_VERSION,
            groups: Vec::new(),
            members: HashMap::new(),
        }
    }
}

pub(crate) fn store_file_at(root: &Path) -> PathBuf {
    root.join(STORE_FILE_NAME)
}

/// 损坏 / 版本不符与 switch_history 同口径：拒绝写入，保留原文件。
pub(crate) fn load_at(root: &Path) -> Result<GroupsStore, String> {
    let file = store_file_at(root);
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(GroupsStore::default())
        }
        Err(error) => return Err(format!("账号分组存档不可读：{error}")),
    };
    let store: GroupsStore =
        serde_json::from_str(&text).map_err(|error| format!("账号分组存档损坏：{error}"))?;
    if store.version != GROUPS_VERSION {
        return Err(format!(
            "账号分组存档版本 {} 不受支持（当前支持 {}），已保留原文件",
            store.version, GROUPS_VERSION
        ));
    }
    Ok(store)
}

pub(crate) fn save_at(root: &Path, store: &GroupsStore) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|error| format!("账号分组目录不可写：{error}"))?;
    let text = serde_json::to_string_pretty(store)
        .map_err(|error| format!("账号分组序列化失败：{error}"))?;
    atomic_write(&store_file_at(root), &text).map_err(|error| format!("账号分组写入失败：{error}"))
}

/// 规整标签：去首尾空白、去重、限长限量；空串丢弃。
pub(crate) fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in tags {
        let tag = raw.trim().chars().take(MAX_TAG_LEN).collect::<String>();
        if tag.is_empty() || !seen.insert(tag.clone()) {
            continue;
        }
        out.push(tag);
        if out.len() >= MAX_TAGS {
            break;
        }
    }
    out
}
