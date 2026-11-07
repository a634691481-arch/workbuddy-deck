//! 分组模块单测：全部走 `*_at(root)` 变体，绝不写真实 `~/.wb-switch`。
use super::store::*;
use super::*;
use std::path::{Path, PathBuf};

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
