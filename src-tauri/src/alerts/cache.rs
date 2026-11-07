//! 账号侧数据缓存：`account_overview` 返回前写入 `alerts_overview_cache.json`。
//!
//! 缓存行 = 界面那一行的副本，**额外**嵌一个 `forecast`（daysLeft / confidence）。
//! 为什么不把这些字段直接加进 `account_overview` 的返回值：那是前端契约，
//! 多字段会牵连 `api.ts` 的类型；提醒中心只需要在缓存里补齐自己缺的输入。
//! `health`（score / grade / reasons）本来就在行里，原样复用。
//!
//! 结构变更向后兼容：旧缓存没有 `forecast`，读取时不补、不报错，对应信号不触发。

use std::collections::HashMap;
use std::path::Path;

use serde_json::{json, Value};
use wb_switch_core::modules::config::{atomic_write, store_dir};

pub(super) const OVERVIEW_CACHE_FILE: &str = "alerts_overview_cache.json";

/// `account_overview` 返回前调用：缓存一份账号行（含 health / resources / forecast）。
/// 写失败静默——提醒中心拿不到到期数据只是少一类提醒，不能拖垮账号页。
pub fn cache_overview(rows: &[Value], forecasts: &HashMap<String, Value>, collected_at: i64) {
    cache_overview_at(&store_dir(), rows, forecasts, collected_at);
}

fn cache_overview_at(
    root: &Path,
    rows: &[Value],
    forecasts: &HashMap<String, Value>,
    collected_at: i64,
) {
    let accounts: Vec<Value> = rows
        .iter()
        .map(|row| with_forecast(row, forecasts))
        .collect();
    let payload = json!({ "ts": collected_at, "accounts": accounts });
    if let Ok(text) = serde_json::to_string(&payload) {
        let _ = std::fs::create_dir_all(root);
        let _ = atomic_write(&root.join(OVERVIEW_CACHE_FILE), &text);
    }
}

/// 把预测的 `daysLeft` / `confidence` 嵌进缓存行；没有预测就写 null（= 样本不足）。
fn with_forecast(row: &Value, forecasts: &HashMap<String, Value>) -> Value {
    let id = row.get("accountId").and_then(Value::as_str).unwrap_or("");
    let forecast = forecasts.get(id);
    let mut row = row.clone();
    row["forecast"] = json!({
        "daysLeft": forecast
            .and_then(|value| value.get("daysLeft"))
            .cloned()
            .unwrap_or(Value::Null),
        "confidence": forecast
            .and_then(|value| value.get("confidence"))
            .cloned()
            .unwrap_or(Value::Null),
    });
    row
}

pub(super) fn load() -> Vec<Value> {
    load_at(&store_dir())
}

/// 缺文件 / 坏 JSON / 旧结构都回落到空清单：评估层按「没有数据」处理。
fn load_at(root: &Path) -> Vec<Value> {
    std::fs::read_to_string(root.join(OVERVIEW_CACHE_FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|payload| payload.get("accounts").and_then(Value::as_array).cloned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::tmp::TempDir;

    #[test]
    fn cached_rows_carry_forecast_fields() {
        let dir = TempDir::new("cache");
        let rows = vec![json!({
            "accountId": "a1",
            "label": "主力",
            "health": { "score": 20, "grade": "critical", "reasons": ["empty"] },
        })];
        let mut forecasts = HashMap::new();
        forecasts.insert("a1".to_string(), json!({ "daysLeft": 1.5, "confidence": "ok" }));
        cache_overview_at(dir.path(), &rows, &forecasts, 123);

        let loaded = load_at(dir.path());
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0]["forecast"]["daysLeft"], 1.5);
        assert_eq!(loaded[0]["forecast"]["confidence"], "ok");
        assert_eq!(loaded[0]["health"]["grade"], "critical");
    }

    #[test]
    fn legacy_cache_without_forecast_still_loads() {
        let dir = TempDir::new("legacy");
        let payload = json!({
            "ts": 1,
            "accounts": [{ "accountId": "a1", "health": { "score": 10 } }],
        });
        std::fs::write(
            dir.path().join(OVERVIEW_CACHE_FILE),
            payload.to_string(),
        )
        .unwrap();

        let loaded = load_at(dir.path());
        assert_eq!(loaded.len(), 1);
        assert!(
            loaded[0].get("forecast").is_none(),
            "旧缓存原样读出，缺少字段不 panic"
        );
    }
}
