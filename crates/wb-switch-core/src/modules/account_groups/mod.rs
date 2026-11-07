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

mod store;
#[cfg(test)]
mod tests;

pub use store::{
    GroupDef, MemberDef, GROUPS_VERSION, MAX_GROUPS, MAX_NAME_LEN, MAX_TAGS, MAX_TAG_LEN,
};
use std::collections::HashSet;
use std::path::Path;

use crate::modules::config::store_dir;
use store::{load_at, normalize_tags, save_at};

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
