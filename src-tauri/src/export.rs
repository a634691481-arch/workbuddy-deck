//! 数据导出：把界面上看到的数字写成能带走的 CSV。
//!
//! 设计约束：
//! 1. **只导出已有数据**，不为了「看起来完整」去发请求；导出是快照动作，不是刷新动作。
//! 2. **列名用英文**，值里的中文（账号名、包名）原样保留——Excel 打开不会有编码问题（带 BOM）。
//! 3. 时间统一 `YYYY-MM-DD HH:MM:SS`（本地时区）：CSV 是给人看的，不是给机器解析的 ISO。
//! 4. 金额 / token 一律写原始数值，不写 `1.2万` 这种展示串——导出后要能算。

use std::path::{Path, PathBuf};

use serde_json::Value;

use wb_switch_core::modules::{account, credit_usage, forecast, health, limits, token_stats};

/// 单份导出支持的类型。
pub const KINDS: [&str; 4] = ["accounts", "tokensDaily", "tokensModels", "credits"];

/// 每种导出的默认文件名（前端拼 `save` 对话框的默认路径用）。
pub fn file_name_of(kind: &str) -> String {
    match kind {
        "accounts" => "accounts.csv".to_string(),
        "tokensDaily" => "tokens-daily.csv".to_string(),
        "tokensModels" => "tokens-models.csv".to_string(),
        "credits" => "credits.csv".to_string(),
        other => format!("{other}.csv"),
    }
}

/// CSV 字段转义：含逗号 / 引号 / 换行才加引号，内部引号翻倍。
fn csv_cell(raw: &str) -> String {
    if raw.contains(',') || raw.contains('"') || raw.contains('\n') || raw.contains('\r') {
        format!("\"{}\"", raw.replace('"', "\"\""))
    } else {
        raw.to_string()
    }
}

fn row(cells: &[String]) -> String {
    let mut line = cells.iter().map(|cell| csv_cell(cell)).collect::<Vec<_>>().join(",");
    line.push_str("\r\n");
    line
}

/// 毫秒时间戳 → `YYYY-MM-DD HH:MM:SS`（本地时区）；0 / null → 空串。
fn stamp(ms: Option<i64>) -> String {
    let Some(ms) = ms.filter(|value| *value > 0) else {
        return String::new();
    };
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}

fn number(value: Option<&Value>) -> String {
    match value {
        Some(Value::Number(num)) => {
            if let Some(int) = num.as_i64() {
                int.to_string()
            } else {
                num.as_f64().map(|float| format!("{float}")).unwrap_or_default()
            }
        }
        Some(Value::String(text)) => text.clone(),
        _ => String::new(),
    }
}

/// 数值单元格：整数不带小数点，小数保留原精度；缺值写空串（不是 0）。
fn cell_num(value: Option<f64>) -> String {
    match value {
        Some(num) if num.fract() == 0.0 => format!("{num:.0}"),
        Some(num) => format!("{num}"),
        None => String::new(),
    }
}

fn text(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or("").to_string()
}

/// 账号清单：余额、占比、健康分、预测、限流模型、最新快照时间。
///
/// 口径与界面一致（本地快照 + 预测 + 限流台账），但**不触发官方拉取**：
/// 导出应该是即时的，不该因为点了导出而打一轮接口。
fn accounts_csv() -> String {
    let now = wb_switch_core::modules::config::now_ms();
    let mut output = String::new();
    output.push_str(&row(&[
        "accountId".into(),
        "label".into(),
        "variant".into(),
        "note".into(),
        "remaining".into(),
        "capacity".into(),
        "usedPercent".into(),
        "healthScore".into(),
        "healthGrade".into(),
        "healthReasons".into(),
        "perDay".into(),
        "daysLeft".into(),
        "depleteAt".into(),
        "limitedModels".into(),
        "snapshotAt".into(),
    ]));

    let snapshots = credit_usage::load_snapshots();
    let mut latest: std::collections::HashMap<String, &Value> = std::collections::HashMap::new();
    for snapshot in &snapshots {
        let Some(id) = snapshot.get("accountId").and_then(Value::as_str) else {
            continue;
        };
        let ts = snapshot.get("ts").and_then(Value::as_i64).unwrap_or(0);
        let newer = latest
            .get(id)
            .map(|old| ts > old.get("ts").and_then(Value::as_i64).unwrap_or(0))
            .unwrap_or(true);
        if newer {
            latest.insert(id.to_string(), snapshot);
        }
    }

    // 限流台账：一次取全量，再按账号拆。
    let mut limits_by_account: std::collections::HashMap<String, Vec<Value>> =
        std::collections::HashMap::new();
    if let Some(rows) = limits::get_rate_limits().get("accounts").and_then(Value::as_array) {
        for entry in rows {
            let Some(id) = entry.get("accountId").and_then(Value::as_str) else {
                continue;
            };
            let active: Vec<Value> = entry
                .get("limited")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter(|item| {
                            item.get("resetAt")
                                .and_then(Value::as_i64)
                                .map(|at| at > now)
                                .unwrap_or(false)
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            limits_by_account.insert(id.to_string(), active);
        }
    }

    let forecasts = forecast::account_forecasts(None);
    let forecast_by_id: std::collections::HashMap<String, Value> = forecasts
        .get("accounts")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|entry| {
                    let id = entry.get("accountId").and_then(Value::as_str)?.to_string();
                    Some((id, entry.clone()))
                })
                .collect()
        })
        .unwrap_or_default();

    for acc in account::load_accounts() {
        let id = text(acc.get("id"));
        let snapshot = latest.get(&id).copied();
        let remaining = snapshot
            .and_then(|snap| snap.get("remaining"))
            .and_then(Value::as_f64);
        let capacity = snapshot
            .and_then(|snap| snap.get("total"))
            .and_then(Value::as_f64);
        let used_percent = match (remaining, capacity) {
            (Some(remaining), Some(capacity)) if capacity > 0.0 => {
                format!("{:.1}", (1.0 - remaining / capacity) * 100.0)
            }
            _ => String::new(),
        };

        let rate_limits = limits_by_account.get(&id).cloned().unwrap_or_default();
        let limited_models = rate_limits
            .iter()
            .map(|entry| text(entry.get("model")))
            .filter(|model| !model.is_empty())
            .collect::<Vec<_>>()
            .join("|");
        let health_value = health::account_health(&health::HealthInput {
            remaining,
            capacity,
            expiring_soon_remaining: snapshot
                .and_then(|snap| snap.get("expiringSoonRemaining"))
                .and_then(Value::as_f64),
            rate_limited: !rate_limits.is_empty(),
            error: None,
            days_left: forecast_by_id
                .get(&id)
                .and_then(|entry| entry.get("daysLeft"))
                .and_then(Value::as_f64),
        });
        let reasons = health_value
            .get("reasons")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("|")
            })
            .unwrap_or_default();
        let prediction = forecast_by_id.get(&id);

        output.push_str(&row(&[
            id.clone(),
            account::account_display_name(&acc),
            account::variant_of(&acc).as_str().to_string(),
            text(acc.get("note")),
            cell_num(remaining),
            cell_num(capacity),
            used_percent,
            number(health_value.get("score")),
            text(health_value.get("grade")),
            reasons,
            number(prediction.and_then(|entry| entry.get("perDay"))),
            number(prediction.and_then(|entry| entry.get("daysLeft"))),
            stamp(prediction.and_then(|entry| entry.get("depleteAt")).and_then(Value::as_i64)),
            limited_models,
            stamp(snapshot.and_then(|snap| snap.get("ts")).and_then(Value::as_i64)),
        ]));
    }
    output
}

/// Token 按天：跨来源按日期合并（同一天两个客户端相加），与 Token 页的柱状图同口径。
fn tokens_daily_csv() -> String {
    let stats = token_stats::get_statistics(None);
    let mut by_date: std::collections::BTreeMap<String, [f64; 6]> =
        std::collections::BTreeMap::new();
    for source in stats.get("sources").and_then(Value::as_array).into_iter().flatten() {
        for point in source.get("daily").and_then(Value::as_array).into_iter().flatten() {
            let key = text(point.get("key"));
            if key.is_empty() {
                continue;
            }
            let entry = by_date.entry(key).or_insert([0.0; 6]);
            entry[0] += point.get("total").and_then(Value::as_f64).unwrap_or(0.0);
            entry[1] += point.get("input").and_then(Value::as_f64).unwrap_or(0.0);
            entry[2] += point.get("output").and_then(Value::as_f64).unwrap_or(0.0);
            entry[3] += point.get("cacheRead").and_then(Value::as_f64).unwrap_or(0.0);
            entry[4] += point.get("cacheWrite").and_then(Value::as_f64).unwrap_or(0.0);
            entry[5] += point.get("records").and_then(Value::as_f64).unwrap_or(0.0);
        }
    }
    let mut output = String::new();
    output.push_str(&row(&[
        "date".into(),
        "total".into(),
        "input".into(),
        "output".into(),
        "cacheRead".into(),
        "cacheWrite".into(),
        "records".into(),
    ]));
    for (date, values) in by_date {
        output.push_str(&row(&[
            date,
            (values[0] as i64).to_string(),
            (values[1] as i64).to_string(),
            (values[2] as i64).to_string(),
            (values[3] as i64).to_string(),
            (values[4] as i64).to_string(),
            (values[5] as i64).to_string(),
        ]));
    }
    output
}

/// Token 按模型：跨来源合并，按总量降序，带占比。
fn tokens_models_csv() -> String {
    let stats = token_stats::get_statistics(None);
    let mut by_model: std::collections::BTreeMap<String, [f64; 6]> =
        std::collections::BTreeMap::new();
    for source in stats.get("sources").and_then(Value::as_array).into_iter().flatten() {
        for group in source.get("models").and_then(Value::as_array).into_iter().flatten() {
            let key = text(group.get("key"));
            if key.is_empty() {
                continue;
            }
            let entry = by_model.entry(key).or_insert([0.0; 6]);
            entry[0] += group.get("total").and_then(Value::as_f64).unwrap_or(0.0);
            entry[1] += group.get("input").and_then(Value::as_f64).unwrap_or(0.0);
            entry[2] += group.get("output").and_then(Value::as_f64).unwrap_or(0.0);
            entry[3] += group.get("cacheRead").and_then(Value::as_f64).unwrap_or(0.0);
            entry[4] += group.get("cacheWrite").and_then(Value::as_f64).unwrap_or(0.0);
            entry[5] += group.get("records").and_then(Value::as_f64).unwrap_or(0.0);
        }
    }
    let total_all: f64 = by_model.values().map(|values| values[0]).sum();
    let mut rows: Vec<(String, [f64; 6])> = by_model.into_iter().collect();
    rows.sort_by(|left, right| {
        right.1[0]
            .partial_cmp(&left.1[0])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut output = String::new();
    output.push_str(&row(&[
        "model".into(),
        "total".into(),
        "input".into(),
        "output".into(),
        "cacheRead".into(),
        "cacheWrite".into(),
        "records".into(),
        "sharePercent".into(),
    ]));
    for (model, values) in rows {
        let share = if total_all > 0.0 {
            format!("{:.2}", values[0] / total_all * 100.0)
        } else {
            String::new()
        };
        output.push_str(&row(&[
            model,
            (values[0] as i64).to_string(),
            (values[1] as i64).to_string(),
            (values[2] as i64).to_string(),
            (values[3] as i64).to_string(),
            (values[4] as i64).to_string(),
            (values[5] as i64).to_string(),
            share,
        ]));
    }
    output
}

/// 积分快照逐条，附与**同账号上一条**的差值（正数 = 消耗，负数 = 领取）。
fn credits_csv() -> String {
    let mut snapshots = credit_usage::load_snapshots();
    snapshots.sort_by_key(|snapshot| snapshot.get("ts").and_then(Value::as_i64).unwrap_or(0));
    let mut previous: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    let mut output = String::new();
    output.push_str(&row(&[
        "timestamp".into(),
        "date".into(),
        "accountId".into(),
        "accountName".into(),
        "variant".into(),
        "total".into(),
        "remaining".into(),
        "delta".into(),
    ]));
    for snapshot in &snapshots {
        let id = text(snapshot.get("accountId"));
        let remaining = snapshot.get("remaining").and_then(Value::as_f64).unwrap_or(0.0);
        let delta = previous
            .get(&id)
            .map(|before| before - remaining)
            .filter(|value| value.abs() > f64::EPSILON)
            .map(|value| format!("{value:.1}"))
            .unwrap_or_default();
        previous.insert(id.clone(), remaining);
        let ts = snapshot.get("ts").and_then(Value::as_i64);
        output.push_str(&row(&[
            stamp(ts),
            chrono::DateTime::from_timestamp_millis(ts.unwrap_or(0))
                .map(|date| date.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string())
                .unwrap_or_default(),
            id,
            text(snapshot.get("accountName")),
            text(snapshot.get("variant")),
            number(snapshot.get("total")),
            number(snapshot.get("remaining")),
            delta,
        ]));
    }
    output
}

fn content_of(kind: &str) -> Result<String, String> {
    match kind {
        "accounts" => Ok(accounts_csv()),
        "tokensDaily" => Ok(tokens_daily_csv()),
        "tokensModels" => Ok(tokens_models_csv()),
        "credits" => Ok(credits_csv()),
        other => Err(format!("未知导出类型：{other}")),
    }
}

/// 写 BOM：Excel 直接双击打开中文不乱码。
fn write_csv(path: &Path, content: &str) -> Result<u64, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("目录不可写：{error}"))?;
    }
    let mut bytes = Vec::with_capacity(content.len() + 3);
    bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    bytes.extend_from_slice(content.as_bytes());
    std::fs::write(path, &bytes).map_err(|error| format!("写入失败：{error}"))?;
    Ok((content.len() + 3) as u64)
}

/// 导出一份到指定文件。
pub fn export_one(kind: &str, path: &str) -> Value {
    if !KINDS.contains(&kind) {
        return serde_json::json!({ "ok": false, "error": format!("未知导出类型：{kind}") });
    }
    match content_of(kind).and_then(|content| write_csv(Path::new(path), &content)) {
        Ok(bytes) => serde_json::json!({
            "ok": true,
            "kind": kind,
            "path": path,
            "bytes": bytes,
        }),
        Err(error) => serde_json::json!({ "ok": false, "error": error }),
    }
}

/// 全部导出到一个目录，逐份返回结果（部分失败不影响其它份）。
pub fn export_all(dir: &str) -> Value {
    let base = PathBuf::from(dir);
    let mut written: Vec<Value> = Vec::new();
    let mut failed = 0;
    for kind in KINDS {
        let path = base.join(file_name_of(kind));
        match content_of(kind).and_then(|content| write_csv(&path, &content)) {
            Ok(bytes) => written.push(serde_json::json!({
                "kind": kind,
                "path": path.to_string_lossy(),
                "bytes": bytes,
            })),
            Err(error) => {
                failed += 1;
                written.push(serde_json::json!({ "kind": kind, "error": error }));
            }
        }
    }
    serde_json::json!({
        "ok": failed == 0,
        "dir": dir,
        "files": written,
        "failed": failed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 含逗号 / 引号 / 换行的字段必须转义，否则列会错位。
    #[test]
    fn csv_cell_escapes_separators() {
        assert_eq!(csv_cell("plain"), "plain");
        assert_eq!(csv_cell("a,b"), "\"a,b\"");
        assert_eq!(csv_cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_cell("line1\nline2"), "\"line1\nline2\"");
    }

    /// 时间戳 0 / None 输出空串，不能变成 1970。
    #[test]
    fn stamp_skips_zero() {
        assert_eq!(stamp(None), "");
        assert_eq!(stamp(Some(0)), "");
        assert!(!stamp(Some(1_800_000_000_000)).is_empty());
    }

    /// 未知类型要报错，不能静默写空文件。
    #[test]
    fn unknown_kind_is_rejected() {
        assert!(content_of("nope").is_err());
        assert!(content_of("accounts").is_ok());
    }

    /// 默认文件名稳定（前端 save 对话框依赖它）。
    #[test]
    fn file_names_are_stable() {
        assert_eq!(file_name_of("accounts"), "accounts.csv");
        assert_eq!(file_name_of("tokensDaily"), "tokens-daily.csv");
    }

    /// 手动验收：把四份 CSV 真写到临时目录并打印前几行。
    ///
    /// 跑法：`cargo test --lib manual_export_smoke -- --ignored --nocapture`
    /// （会扫本机会话文件，所以不放进常规测试集。）
    #[test]
    #[ignore]
    fn manual_export_smoke() {
        let dir = std::env::temp_dir().join("wb_export_smoke");
        std::fs::create_dir_all(&dir).unwrap();
        let result = export_all(dir.to_str().unwrap());
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
        for kind in KINDS {
            let path = dir.join(file_name_of(kind));
            if let Ok(text) = std::fs::read_to_string(&path) {
                let body = text.trim_start_matches('\u{feff}');
                let preview: Vec<&str> = body.lines().take(3).collect();
                println!("\n--- {kind} ({} lines) ---", body.lines().count());
                for line in preview {
                    println!("{line}");
                }
            }
        }
    }
}
