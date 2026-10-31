//! Token 统计的两级缓存：进程内按档位分槽 + 落盘，冷启动也能秒开。
//!
//! 为什么需要它：`token_stats::get_statistics` 要遍历几百个会话 jsonl，秒级耗时。
//! 原实现只有「单个槽 + 5 分钟」，切档位（7D→30D→7D）和重启应用都会重扫。
//!
//! 策略：
//! - 内存 `HashMap<档位, (结果, 时刻)>`：同一次运行内任意切档都命中。
//! - 磁盘 `token_stats_cache.json`：重启后仍命中，TTL 由调用方控制。
//! - 写盘时按体积设上限，超过就只留内存（统计明细可能很大，不该塞进磁盘里）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{json, Value};

use wb_switch_core::modules::config;

/// 结构版本：字段变化时 +1，旧缓存自动作废，不会读到不一致的形状。
const SCHEMA: i64 = 1;
/// 单档结果超过这个体积就不落盘（避免把几 MB 的调用明细写进磁盘）。
const MAX_DISK_ENTRY_BYTES: usize = 3 * 1024 * 1024;

static MEM: Mutex<Option<HashMap<i64, (Value, i64)>>> = Mutex::new(None);

fn cache_path() -> PathBuf {
    config::store_dir().join("token_stats_cache.json")
}

fn mem_get(range: i64) -> Option<(Value, i64)> {
    MEM.lock()
        .ok()?
        .as_ref()?
        .get(&range)
        .map(|(value, at)| (value.clone(), *at))
}

fn mem_put(range: i64, value: &Value, at: i64) {
    if let Ok(mut guard) = MEM.lock() {
        guard
            .get_or_insert_with(HashMap::new)
            .insert(range, (value.clone(), at));
    }
}

/// 读缓存；命中且未过期返回结果。
pub fn get(range: i64, ttl_ms: i64) -> Option<Value> {
    let now = config::now_ms();

    if let Some((value, at)) = mem_get(range) {
        if now - at < ttl_ms {
            return Some(value);
        }
    }

    // 内存没有（首次进页面 / 刚重启）：再看磁盘
    let text = std::fs::read_to_string(cache_path()).ok()?;
    let doc: Value = serde_json::from_str(&text).ok()?;
    if doc.get("v").and_then(Value::as_i64) != Some(SCHEMA) {
        return None;
    }
    let entry = doc.get("entries")?.get(range.to_string())?;
    let at = entry.get("at").and_then(Value::as_i64)?;
    if now - at >= ttl_ms {
        return None;
    }
    let value = entry.get("value")?.clone();
    mem_put(range, &value, at);
    Some(value)
}

/// 写缓存：内存必写，磁盘在体积允许时写（保留其它档位）。
pub fn put(range: i64, value: &Value) {
    let now = config::now_ms();
    mem_put(range, value, now);

    let body = value.to_string();
    if body.len() > MAX_DISK_ENTRY_BYTES {
        return;
    }

    let mut doc = std::fs::read_to_string(cache_path())
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(|doc| doc.get("v").and_then(Value::as_i64) == Some(SCHEMA))
        .unwrap_or_else(|| json!({ "v": SCHEMA, "entries": {} }));

    let entries = doc
        .as_object_mut()
        .map(|obj| obj.entry("entries").or_insert_with(|| json!({})));
    if let Some(entries) = entries.and_then(Value::as_object_mut) {
        entries.insert(range.to_string(), json!({ "at": now, "value": value }));
    }

    if let Some(dir) = cache_path().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = config::atomic_write(&cache_path(), &doc.to_string());
}
