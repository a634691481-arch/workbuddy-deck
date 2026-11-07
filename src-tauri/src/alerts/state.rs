//! 提醒状态存档：`alerts_state.json`（dismissed / seen 两张表）。
//!
//! 写入是尽力而为：状态丢了最多重复提醒一次，不能反过来拖垮评估。
//! 读取失败/损坏一律按空状态处理——提醒中心永远不该因为存档坏了而报错。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use wb_switch_core::modules::config::{atomic_write, store_dir};

use super::DISMISS_TTL_MS;

const STATE_VERSION: u32 = 1;
const STATE_FILE: &str = "alerts_state.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AlertsState {
    version: u32,
    #[serde(default)]
    pub(crate) dismissed: HashMap<String, i64>,
    #[serde(default)]
    pub(crate) seen: HashMap<String, i64>,
}

impl Default for AlertsState {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            dismissed: HashMap::new(),
            seen: HashMap::new(),
        }
    }
}

fn state_path_at(root: &Path) -> PathBuf {
    root.join(STATE_FILE)
}

pub(crate) fn load() -> AlertsState {
    load_at(&store_dir())
}

/// 缺文件 / 坏 JSON / 版本不符都回落到默认状态：少一次提醒好过整条链路失败。
pub(crate) fn load_at(root: &Path) -> AlertsState {
    std::fs::read_to_string(state_path_at(root))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub(crate) fn save(state: &AlertsState) {
    save_at(&store_dir(), state);
}

pub(crate) fn save_at(root: &Path, state: &AlertsState) {
    if let Ok(text) = serde_json::to_string_pretty(state) {
        let _ = std::fs::create_dir_all(root);
        let _ = atomic_write(&state_path_at(root), &text);
    }
}

/// 指纹是否仍在「忽略静默期」内。边界按 `<`：正好 7 天视为已过期。
pub(crate) fn is_dismissed(state: &AlertsState, fingerprint: &str, now: i64) -> bool {
    matches!(state.dismissed.get(fingerprint), Some(at) if now - *at < DISMISS_TTL_MS)
}

/// 仍在静默期内的忽略条数（面板显示用）。
pub(crate) fn dismissed_count(state: &AlertsState, now: i64) -> usize {
    state
        .dismissed
        .values()
        .filter(|at| now - **at < DISMISS_TTL_MS)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::tmp::TempDir;

    #[test]
    fn dismiss_is_silent_for_seven_days_then_expires() {
        let now = 10 * DISMISS_TTL_MS;
        let mut state = AlertsState::default();
        state
            .dismissed
            .insert("balance:a".to_string(), now - DISMISS_TTL_MS + 1);
        assert!(is_dismissed(&state, "balance:a", now), "7 天内必须静默");
        assert!(!is_dismissed(&state, "balance:b", now), "别的指纹不受影响");

        state
            .dismissed
            .insert("balance:a".to_string(), now - DISMISS_TTL_MS);
        assert!(!is_dismissed(&state, "balance:a", now), "正好 7 天视为过期");
        assert_eq!(dismissed_count(&state, now), 0);
    }

    #[test]
    fn state_roundtrips_and_tolerates_damaged_file() {
        let dir = TempDir::new("state");
        let mut state = AlertsState::default();
        state.dismissed.insert("x".to_string(), 1);
        state.seen.insert("y".to_string(), 2);
        save_at(dir.path(), &state);

        let loaded = load_at(dir.path());
        assert_eq!(loaded.dismissed.get("x"), Some(&1));
        assert_eq!(loaded.seen.get("y"), Some(&2));

        std::fs::write(dir.path().join(STATE_FILE), b"{not json").unwrap();
        let damaged = load_at(dir.path());
        assert!(damaged.dismissed.is_empty(), "损坏存档按空状态走，不 panic");
        assert!(damaged.seen.is_empty());
    }
}
