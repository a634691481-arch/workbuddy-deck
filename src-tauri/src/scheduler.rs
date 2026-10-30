//! 每日定时调度：到点自动执行「全部日常」，程序常驻托盘即可无人值守。
//!
//! 约定沿用主程序 auto_* 家族（AGENTS.md 同款铁律）：
//! - `auto_daily_config.json`：开关默认关闭，含执行时间与最近执行日期；
//! - `auto_daily_logs.json`：日志上限 500 条 / 保留 30 天；
//! - 防重：`lastRunDate` 记当天日期，跨天自动重置；执行器内部 RUNNING 标志
//!   保证与手动触发互斥（撞车时定时器拿到 busy 直接放弃本轮）。
//!
//! 调度策略：每 30s 醒一次；`当前分钟 >= 目标分钟` 且当天没跑过 → 触发。
//! 错过整点（程序没开 / 睡眠）在醒来后补跑一次，而不是当天直接放弃。

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::Emitter;

use chrono::Timelike;

use wb_switch_core::modules::config;

const DEFAULT_TIME: &str = "09:00";
const LOG_MAX: usize = 500;
const LOG_KEEP_DAYS: i64 = 30;
const TICK_SECS: u64 = 30;

fn config_file() -> PathBuf {
    config::store_dir().join("auto_daily_config.json")
}

fn logs_file() -> PathBuf {
    config::store_dir().join("auto_daily_logs.json")
}

fn default_config() -> Value {
    json!({ "enabled": false, "time": DEFAULT_TIME, "lastRunDate": "" })
}

/// 读配置并把缺省字段补齐，避免旧文件缺 key 时下游判空。
pub fn load_config() -> Value {
    let mut cfg = default_config();
    if let Ok(raw) = std::fs::read_to_string(config_file()) {
        if let Ok(saved) = serde_json::from_str::<Value>(&raw) {
            if let Some(obj) = saved.as_object() {
                for (key, value) in obj {
                    cfg[key] = value.clone();
                }
            }
        }
    }
    cfg
}

pub fn save_config(cfg: &Value) -> std::io::Result<()> {
    let content = serde_json::to_string_pretty(cfg).unwrap_or_default();
    config::atomic_write(&config_file(), &content)
}

pub fn load_logs() -> Vec<Value> {
    std::fs::read_to_string(logs_file())
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}

/// 清空自动日常日志（运行日志页的「清空」用）。配置不动。
pub fn clear_logs() -> std::io::Result<()> {
    config::atomic_write(&logs_file(), "[]")
}

fn append_log(entry: &Value) {
    let mut logs = load_logs();
    logs.push(entry.clone());
    // 30 天外的旧日志直接丢弃（ts 是 ISO 字符串，按字典序比较即可）
    let cutoff = format!(
        "{}Z",
        (chrono::Local::now() - chrono::Duration::days(LOG_KEEP_DAYS))
            .format("%Y-%m-%dT%H-%M-%S")
    );
    logs.retain(|row| row.get("ts").and_then(Value::as_str).unwrap_or("") >= cutoff.as_str());
    while logs.len() > LOG_MAX {
        let remove = logs.len() - LOG_MAX;
        logs.drain(0..remove);
    }
    if let Ok(content) = serde_json::to_string_pretty(&logs) {
        let _ = config::atomic_write(&logs_file(), &content);
    }
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 把一轮 daily 的结果压成日志行。ok=签到/领奖/旅行任一命中，error 才算失败。
fn summarize(result: &Value) -> (usize, usize, usize) {
    let rows = result.get("accounts").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut ok = 0;
    let mut fail = 0;
    let mut skip = 0;
    for row in &rows {
        match row.get("result").and_then(Value::as_str).unwrap_or("") {
            "success" => ok += 1,
            "error" => fail += 1,
            _ => skip += 1,
        }
    }
    (rows.len(), ok, fail + skip)
}

/// 后台循环：常驻期间每 30s 检查一次到点情况。
pub fn spawn(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(TICK_SECS)).await;
            tick(&app).await;
        }
    });
}

async fn tick(app: &tauri::AppHandle) {
    let cfg = load_config();
    if !cfg["enabled"].as_bool().unwrap_or(false) {
        return;
    }
    let raw_time = cfg["time"].as_str().unwrap_or(DEFAULT_TIME).to_string();
    let Some((hh, mm)) = config::parse_clock(&raw_time) else {
        return;
    };
    let now = chrono::Local::now();
    let today = today();
    if cfg["lastRunDate"].as_str() == Some(today.as_str()) {
        return;
    }
    let now_minute = now.hour() as u32 * 60 + now.minute();
    let target_minute = hh * 60 + mm;
    if now_minute < target_minute {
        return;
    }

    // 与手动触发共用执行器；撞车（busy）时不标记已跑，下个 tick 再试。
    let result = crate::executor::execute("daily", Vec::new()).await;
    if result["status"].as_str() == Some("busy") {
        return;
    }

    let mut updated = cfg.clone();
    updated["lastRunDate"] = json!(today);
    let _ = save_config(&updated);

    let (total, ok, other) = summarize(&result);
    append_log(&json!({
        "ts": config::utc_iso(),
        "date": today,
        "time": raw_time,
        "status": result["status"].clone(),
        "total": total,
        "ok": ok,
        "other": other,
    }));
    eprintln!(
        "[auto-daily] date={} status={} total={} ok={} other={}",
        today,
        result["status"].as_str().unwrap_or("?"),
        total,
        ok,
        other
    );
    // 通知前端：刷新数据 + 写一行日志
    let _ = app.emit("auto://daily-done", result);
}
