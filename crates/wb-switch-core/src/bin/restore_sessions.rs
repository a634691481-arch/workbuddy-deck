//! 会话迁移小工具：把 A 账号的会话复制到 B 账号（保留 B 原有会话，不删除任何东西）。
//!
//! 背景：手动「设为当前」是热切换（只换登录态、不搬会话），和「完全同步」不是一回事。
//! 当用户误用热切换、又想把旧号记录搬到新号时，用这个工具做**增量复制**：
//! 走 core 正式复制路径（建副本 + 关联表 + DB 行），不是手写文件拷贝。
//!
//! 用法：
//!   cargo run --release --manifest-path crates/wb-switch-core/Cargo.toml \
//!     --bin restore_sessions -- <source> <target> [--dry-run]
//!
//!   source/target 支持账号 id 或 uid 的前缀（唯一命中即可）。
//!   目标账号不能是当前登录号（App 在跑时写当前号会撞锁）——先切到源账号再执行。

use serde_json::Value;
use wb_switch_core::modules::{account, auto_switch, session};

fn resolve(prefix: &str, accounts: &[Value]) -> Result<Value, String> {
    let needle = prefix.trim().to_lowercase();
    if needle.is_empty() {
        return Err("账号标识不能为空".into());
    }
    let hits: Vec<&Value> = accounts
        .iter()
        .filter(|acc| {
            let id = acc.get("id").and_then(Value::as_str).unwrap_or("").to_lowercase();
            let uid = acc.get("uid").and_then(Value::as_str).unwrap_or("").to_lowercase();
            id.starts_with(&needle) || uid.starts_with(&needle)
        })
        .collect();
    match hits.len() {
        1 => Ok(hits[0].clone()),
        0 => Err(format!("找不到账号：{prefix}")),
        _ => Err(format!("前缀 {prefix} 命中多个账号，请写长一点")),
    }
}

fn label(acc: &Value) -> String {
    acc.get("label")
        .or_else(|| acc.get("nickname"))
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_string()
}

fn ids_of(acc: &Value) -> Vec<String> {
    session::list_sessions_for_account(acc)
        .get("sessions")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.get("id").and_then(Value::as_str).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dry = args.iter().any(|a| a == "--dry-run");
    let positional: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if positional.len() != 2 {
        eprintln!("用法: restore_sessions <source> <target> [--dry-run]");
        std::process::exit(2);
    }

    let accounts = account::load_accounts();
    let source = match resolve(positional[0], &accounts) {
        Ok(acc) => acc,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    let target = match resolve(positional[1], &accounts) {
        Ok(acc) => acc,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    let source_id = source.get("id").and_then(Value::as_str).unwrap_or("").to_string();
    let target_id = target.get("id").and_then(Value::as_str).unwrap_or("").to_string();
    if source_id == target_id {
        eprintln!("源与目标是同一个账号");
        std::process::exit(1);
    }
    let ids = ids_of(&source);
    println!(
        "源 {} 会话 {} 条 → 目标 {}（{}）",
        label(&source),
        ids.len(),
        label(&target),
        if dry { "演练，不写入" } else { "增量复制，不删除目标原有会话" }
    );
    if ids.is_empty() {
        eprintln!("源账号没有可复制的会话");
        std::process::exit(1);
    }
    if dry {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "ids": ids })).unwrap_or_default());
        return;
    }

    // 写目标是当前登录号时，App 正在跑会撞会话存储锁：要求先切到源账号（此时目标不是当前号）。
    if auto_switch::current_account_id().as_deref() == Some(target_id.as_str()) {
        eprintln!("目标账号是当前登录号：请先在应用里切到「{}」再执行", label(&source));
        std::process::exit(1);
    }

    match session::copy_sessions_cross(&source, &target, &ids) {
        Ok(report) => {
            println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
            let log = serde_json::json!({
                "ts": wb_switch_core::modules::config::now_ms(),
                "source": { "id": source_id, "label": label(&source), "count": ids.len() },
                "target": { "id": target_id, "label": label(&target) },
                "report": report,
            });
            let path = wb_switch_core::modules::config::store_dir().join("restore_sessions.log.json");
            let mut rows: Vec<Value> = std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            rows.push(log);
            let _ = std::fs::write(&path, serde_json::to_string_pretty(&rows).unwrap_or_default());
            println!("日志：{}", path.display());
        }
        Err(err) => {
            eprintln!("复制失败：{err}");
            std::process::exit(1);
        }
    }
}
