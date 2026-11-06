//! 账号分组与标签：用户自己打的组织维度，不参与任何网络请求。
//!
//! 为什么单独一份文件而不是塞进 `accounts.json`：账号库由登录/导入流程维护，
//! 外部工具（workbuddy-switch）也会写它——分组信息混进去会被上游同步覆盖掉。
//! 独立存 `store_dir()/account_groups.json`，谁也不覆盖谁。
//!
//! 数据形状：
//! - `groups`：分组定义（名字 + 是否排除自动切换）；
//! - `members`：账号 id → { 所属分组, 标签 }。账号删除后残留的成员关系按「惰性清理」
//!   处理：读的时候对不上就忽略，不主动扫库。

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::modules::config::{atomic_write, store_dir};

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
struct GroupsStore {
    version: u32,
    #[serde(default)]
    groups: Vec<GroupDef>,
    #[serde(default)]
    members: HashMap<String, MemberDef>,
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

fn store_file_at(root: &Path) -> PathBuf {
    root.join(STORE_FILE_NAME)
}

/// 损坏 / 版本不符与 switch_history 同口径：拒绝写入，保留原文件。
fn load_at(root: &Path) -> Result<GroupsStore, String> {
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

fn save_at(root: &Path, store: &GroupsStore) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|error| format!("账号分组目录不可写：{error}"))?;
    let text = serde_json::to_string_pretty(store)
        .map_err(|error| format!("账号分组序列化失败：{error}"))?;
    atomic_write(&store_file_at(root), &text).map_err(|error| format!("账号分组写入失败：{error}"))
}

/// 规整标签：去首尾空白、去重、限长限量；空串丢弃。
fn normalize_tags(tags: &[String]) -> Vec<String> {
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

/// 读全量（分组定义 + 成员关系）。
pub fn load() -> serde_json::Value {
    let store = match load_at(&store_dir()) {
        Ok(store) => store,
        Err(error) => {
            return serde_json::json!({ "ok": false, "error": error, "groups": [], "members": {} })
        }
    };
    let members: serde_json::Map<String, serde_json::Value> = store
        .members
        .iter()
        .map(|(id, member)| {
            (
                id.clone(),
                serde_json::json!({ "groups": member.groups, "tags": member.tags }),
            )
        })
        .collect();
    serde_json::json!({
        "ok": true,
        "groups": store.groups,
        "members": members,
    })
}

/// 新建或改名一个分组。`id` 为空 = 新建；返回保存后的分组。
pub fn upsert_group(
    id: Option<&str>,
    name: &str,
    exclude_auto_switch: bool,
    now: i64,
) -> Result<serde_json::Value, String> {
    upsert_group_at(&store_dir(), id, name, exclude_auto_switch, now)
}

fn upsert_group_at(
    root: &Path,
    id: Option<&str>,
    name: &str,
    exclude_auto_switch: bool,
    now: i64,
) -> Result<serde_json::Value, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("分组名不能为空".to_string());
    }
    let name = name.chars().take(MAX_NAME_LEN).collect::<String>();
    let mut store = load_at(root)?;
    if store.groups.len() >= MAX_GROUPS && id.is_none() {
        return Err(format!("分组已达上限 {MAX_GROUPS} 个"));
    }
    let dup = store
        .groups
        .iter()
        .any(|group| group.name == name && Some(group.id.as_str()) != id);
    if dup {
        return Err(format!("已有同名分组「{name}」"));
    }
    let group = match id.filter(|value| !value.trim().is_empty()) {
        Some(id) => {
            let group = store
                .groups
                .iter_mut()
                .find(|group| group.id == id)
                .ok_or_else(|| format!("分组 {id} 不存在"))?;
            group.name = name;
            group.exclude_auto_switch = exclude_auto_switch;
            group.clone()
        }
        None => {
            let group = GroupDef {
                id: format!("g_{}", uuid::Uuid::new_v4().simple()),
                name,
                exclude_auto_switch,
                created_at: now,
            };
            store.groups.push(group.clone());
            group
        }
    };
    save_at(root, &store)?;
    Ok(serde_json::json!({ "ok": true, "group": group }))
}

/// 删除分组：定义删除，成员关系里的引用一并摘除。
pub fn remove_group(id: &str) -> Result<serde_json::Value, String> {
    remove_group_at(&store_dir(), id)
}

fn remove_group_at(root: &Path, id: &str) -> Result<serde_json::Value, String> {
    let mut store = load_at(root)?;
    let before = store.groups.len();
    store.groups.retain(|group| group.id != id);
    if store.groups.len() == before {
        return Err(format!("分组 {id} 不存在"));
    }
    for member in store.members.values_mut() {
        member.groups.retain(|group| group != id);
    }
    save_at(root, &store)?;
    Ok(serde_json::json!({ "ok": true }))
}

/// 写一个账号的归属与标签（整组覆盖）。传空数组 = 清空该账号的分组/标签。
pub fn set_member(
    account_id: &str,
    groups: &[String],
    tags: &[String],
) -> Result<serde_json::Value, String> {
    set_member_at(&store_dir(), account_id, groups, tags)
}

fn set_member_at(
    root: &Path,
    account_id: &str,
    groups: &[String],
    tags: &[String],
) -> Result<serde_json::Value, String> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err("缺少账号 id".to_string());
    }
    let mut store = load_at(root)?;
    let known: HashSet<&str> = store.groups.iter().map(|g| g.id.as_str()).collect();
    let groups: Vec<String> = groups
        .iter()
        .filter(|g| known.contains(g.as_str()))
        .cloned()
        .collect();
    let member = MemberDef {
        groups,
        tags: normalize_tags(tags),
    };
    if member.groups.is_empty() && member.tags.is_empty() {
        // 全空就删键，避免 members 无限膨胀
        store.members.remove(account_id);
    } else {
        store.members.insert(account_id.to_string(), member);
    }
    save_at(root, &store)?;
    Ok(serde_json::json!({ "ok": true }))
}

/// 被「排除自动切换」分组覆盖的账号 id 集合（auto_switch 候选筛选用）。
pub fn excluded_auto_switch_ids() -> HashSet<String> {
    excluded_auto_switch_ids_at(&store_dir())
}

fn excluded_auto_switch_ids_at(root: &Path) -> HashSet<String> {
    let store = load_at(root).unwrap_or_default();
    let excluded_groups: HashSet<&str> = store
        .groups
        .iter()
        .filter(|group| group.exclude_auto_switch)
        .map(|group| group.id.as_str())
        .collect();
    store
        .members
        .iter()
        .filter(|(_, member)| member.groups.iter().any(|g| excluded_groups.contains(g.as_str())))
        .map(|(id, _)| id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "wb_account_groups_{}_{name}",
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

    fn store_at(root: &Path, id: &str) -> MemberDef {
        load_at(root)
            .unwrap()
            .members
            .get(id)
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn upsert_rename_remove_roundtrip() {
        let root = TempDir::new("roundtrip");
        let created = upsert_group_at(&root.0, None, "主力", false, 1000).unwrap();
        let id = created["group"]["id"].as_str().unwrap().to_string();
        assert_eq!(created["group"]["name"], "主力");

        // 改名 + 打开排除
        let renamed = upsert_group_at(&root.0, Some(&id), "主力号", true, 2000).unwrap();
        assert_eq!(renamed["group"]["name"], "主力号");
        assert_eq!(renamed["group"]["excludeAutoSwitch"], true);

        // 同名（别的组）拒绝
        upsert_group_at(&root.0, None, "主力号", false, 3000).unwrap_err();
        // 自己改名成自己不算重复
        upsert_group_at(&root.0, Some(&id), "主力号", true, 3000).unwrap();

        remove_group_at(&root.0, &id).unwrap();
        remove_group_at(&root.0, &id).unwrap_err();
    }

    #[test]
    fn set_member_filters_unknown_group_and_normalizes_tags() {
        let root = TempDir::new("member");
        let created = upsert_group_at(&root.0, None, "备用", true, 1000).unwrap();
        let id = created["group"]["id"].as_str().unwrap().to_string();

        let tags = vec!["  低余额  ".to_string(), String::new(), "低余额".to_string(), "x".to_string()];
        set_member_at(&root.0, "acc1", &["g_unknown".to_string(), id.clone()], &tags).unwrap();
        let store = load_at(&root.0).unwrap();
        let member = store.members.get("acc1").unwrap();
        assert_eq!(member.groups, vec![id.clone()]);
        assert_eq!(member.tags, vec!["低余额", "x"]);

        // 清空 = 删键
        set_member_at(&root.0, "acc1", &[], &[]).unwrap();
        assert!(load_at(&root.0).unwrap().members.get("acc1").is_none());
    }

    #[test]
    fn excluded_auto_switch_follows_group_flag() {
        let root = TempDir::new("excluded");
        let excluded_group = upsert_group_at(&root.0, None, "勿动", true, 1000).unwrap()
            ["group"]["id"].as_str().unwrap().to_string();
        let normal_group = upsert_group_at(&root.0, None, "普通", false, 1000).unwrap()
            ["group"]["id"].as_str().unwrap().to_string();
        set_member_at(&root.0, "acc1", &[excluded_group], &[]).unwrap();
        set_member_at(&root.0, "acc2", &[normal_group], &[]).unwrap();

        let ids = excluded_auto_switch_ids_at(&root.0);
        assert!(ids.contains("acc1"));
        assert!(!ids.contains("acc2"));
    }

    #[test]
    fn corrupted_store_refuses_overwrite() {
        let root = TempDir::new("corrupt");
        std::fs::write(store_file_at(&root.0), "{ not json").unwrap();
        assert!(load_at(&root.0).is_err());
        // 写入路径必须失败而不是用空存档覆盖
        upsert_group_at(&root.0, None, "x", false, 0).unwrap_err();
        assert!(std::fs::read_to_string(store_file_at(&root.0))
            .unwrap()
            .contains("not json"));
    }

    #[test]
    fn tag_limits() {
        let tags: Vec<String> = (0..20).map(|i| i.to_string()).collect();
        assert_eq!(normalize_tags(&tags).len(), MAX_TAGS);
        let long = "a".repeat(50);
        assert_eq!(normalize_tags(&[long]).first().unwrap().chars().count(), MAX_TAG_LEN);
    }
}
