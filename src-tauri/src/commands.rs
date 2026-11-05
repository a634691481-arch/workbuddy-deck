//! 薄命令层：只做参数校验与转调，业务编排在 `executor` 与 `wb-switch-core`。
//!
//! 约定（见 AGENTS.md）：这里不写业务分支、不拼统计、不做重试；新增动作加一个薄包装即可。

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::path::PathBuf;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use serde_json::{json, Value};

use tauri::Emitter;

use wb_switch_core::modules::config;
use wb_switch_core::modules::credit_usage;
use wb_switch_core::modules::growth_tasks;
use wb_switch_core::modules::variant::WbVariant;
use wb_switch_core::modules::{
    account, auth_file, checkin, credits, error_log, export_import, forecast, limits,
    auto_switch, health, notifications, oauth, process, refresh, switch, switch_history,
};

use crate::executor;
use crate::scheduler;

/// 账号摘要：只给界面需要的字段，令牌不出后端（AGENTS.md §5）。
fn summary(acc: &Value) -> Value {
    json!({
        "id": acc.get("id").cloned().unwrap_or(Value::Null),
        "label": account::account_display_name(acc),
        "variant": account::variant_of(acc).as_str(),
        "note": acc.get("note").cloned().unwrap_or(Value::Null),
    })
}

async fn fresh(acc: Value) -> Value {
    refresh::ensure_fresh_token(acc, &config::load_checkin_config()).await
}

#[tauri::command]
pub fn list_accounts() -> Value {
    let rows: Vec<Value> = account::cn_accounts().iter().map(summary).collect();
    json!({ "total": rows.len(), "accounts": rows })
}

/// 从本机 WorkBuddy 登录态导入当前账号（cn / ai 两档位都试一遍）。
///
/// 为什么需要它：账号列表读的是 `~/.wb-switch/accounts.json`（上游 workbuddy-switch
/// 的库）。没装过那个工具的用户，这个文件根本不存在 → 账号库永远为空，即便他
/// 已经在 WorkBuddy 里登录过。登录态其实在 WorkBuddy 自己的 auth 文件里，这里把它读出来落库。
///
/// 核心逻辑（信封 token 保护 / 同 uid 去重 / 保留本地备注）在 core `account::import_local`，
/// 这里只负责遍历档位并把失败摊平成可读结果——单档位失败不影响另一档位。
#[tauri::command]
pub fn import_local_accounts() -> Value {
    let mut imported: Vec<Value> = Vec::new();
    let mut errors: Vec<Value> = Vec::new();
    let mut skipped = 0;
    // 只要国内版登录态：国际版不导入
    for variant in [WbVariant::Cn] {
        match account::import_local_if_absent(variant) {
            Ok(Some(meta)) => imported.push(meta),
            // 库里已有同 uid：装过 workbuddy-switch 的机器走到这里，必须空转
            Ok(None) => skipped += 1,
            Err(error) => errors.push(json!({ "variant": variant.as_str(), "error": error })),
        }
    }
    json!({
        "ok": true,
        "imported": imported.len(),
        "skipped": skipped,
        "accounts": imported,
        "errors": errors,
    })
}

/// 单账号详情：签到状态 + 成长任务清单（清单实时拉，不写死）。
#[tauri::command]
pub async fn account_detail(account_id: String) -> Result<Value, String> {
    if account_id.trim().is_empty() {
        return Err("缺少账号 id".to_string());
    }
    let acc = account::find_account(&account_id).ok_or_else(|| "账号不存在".to_string())?;
    let variant = account::variant_of(&acc);
    let acc = fresh(acc).await;

    let checkin_status = checkin::get_checkin_status_for_display(&acc).await;
    let growth = growth_tasks::growth_status(Some(variant)).await;
    let row = growth
        .get("accounts")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("accountId").and_then(Value::as_str) == Some(account_id.as_str())
            })
        })
        .cloned()
        .unwrap_or_else(|| json!({ "tasks": [] }));

    Ok(json!({
        "accountId": account_id,
        "variant": variant.as_str(),
        "checkin": checkin_status,
        "growth": row,
        "today": checkin::date_str(None),
    }))
}

#[tauri::command]
pub async fn checkin_now(account_id: String) -> Result<Value, String> {
    if account_id.trim().is_empty() {
        return Err("缺少账号 id".to_string());
    }
    let acc = account::find_account(&account_id).ok_or_else(|| "账号不存在".to_string())?;
    Ok(checkin::checkin_account(&fresh(acc).await).await)
}

/// 用系统默认浏览器打开 URL。
///
/// Windows 上**不能用 `explorer.exe`**：它把参数当「要浏览的路径」，遇到完整的
/// `https://…?state=…` 时经常落到「打开方式 / 选择文件」分支（用户看到的是文件选择框），
/// 而不是启动默认浏览器。这里改走 `url.dll,FileProtocolHandler`（系统 URL 协议处理器），
/// 它只认协议、不做文件路径解析；失败再退回 `cmd /C start`（空标题占位，否则 URL 里的
/// `&` 会被当成命令分隔符）。macOS / Linux 交给各自的打开器。
/// 失败不致命，调用方会把 URL 回传给界面供手动复制。
fn open_in_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        // CREATE_NO_WINDOW：不要为这次调用闪一个控制台窗口。
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let launched = std::process::Command::new("rundll32.exe")
            .arg("url.dll,FileProtocolHandler")
            .arg(url)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
        if launched.is_ok() {
            return Ok(());
        }
        let mut fallback = std::process::Command::new("cmd");
        fallback
            .raw_arg(format!("/C start \"\" \"{url}\""))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// 发起浏览器登录：申请 state → 打开授权页 → 前端拿 loginId 轮询。
///
/// 不再拉起主程序：用户要的是「点一下就在浏览器登录」，多跳一步是多余动作。
#[tauri::command]
pub async fn start_login(variant: Option<String>) -> Value {
    // 只做国内版：入参里的 intl/ai 一律按国内版处理（前端已无档位入口）
    let variant = match variant.as_deref() {
        _ => WbVariant::Cn,
    };
    let res = match oauth::oauth_start(variant).await {
        Ok(res) => res,
        Err(error) => return json!({ "ok": false, "error": error }),
    };
    let url = res
        .get("verificationUri")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if url.is_empty() {
        return json!({ "ok": false, "error": "服务端未返回授权地址" });
    }
    let opened = open_in_browser(&url);
    json!({
        "ok": true,
        "loginId": res.get("loginId").cloned().unwrap_or(Value::Null),
        "url": url,
        "expiresIn": res.get("expiresIn").cloned().unwrap_or(Value::Null),
        "browserOpened": opened.is_ok(),
        "browserError": opened.err(),
    })
}

/// 轮询一次登录结果；成功后 core 已把账号写进账号库。
#[tauri::command]
pub async fn poll_login(login_id: String) -> Value {
    if login_id.trim().is_empty() {
        return json!({ "done": true, "error": "缺少 loginId" });
    }
    oauth::oauth_poll(&login_id).await
}

/// 全账号积分汇总：并发受限地逐个查余额，返回总额与逐号明细。
///
/// 为什么限制并发：每个账号在国内版要打 3 个资源接口，8 个号全并发等于 24 个请求，
/// 容易被网关限流；这里固定 4 路上限，失败账号单独记，不拖垮整次汇总。
#[tauri::command]
pub async fn total_balance() -> Value {
    const CONCURRENCY: usize = 4;

    let all = account::load_accounts();
    if all.is_empty() {
        return json!({ "ok": true, "total": 0.0, "accounts": [], "failed": 0 });
    }

    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(CONCURRENCY));
    let mut handles = Vec::with_capacity(all.len());
    for acc in all {
        let permit = semaphore.clone();
        handles.push(tokio::spawn(async move {
            let _guard = permit.acquire().await;
            let id = acc.get("id").cloned().unwrap_or(Value::Null);
            let label = account::account_display_name(&acc);
            let res = credits::get_credit_expiry(&acc).await;
            let ok = res.get("ok").and_then(Value::as_bool).unwrap_or(false);
            json!({
                "accountId": id,
                "label": label,
                "ok": ok,
                "remaining": res.get("totalRemaining").cloned().unwrap_or(Value::Null),
                "error": res.get("error").cloned().unwrap_or(Value::Null),
            })
        }));
    }

    let mut rows = Vec::with_capacity(handles.len());
    for handle in handles {
        if let Ok(row) = handle.await {
            rows.push(row);
        }
    }

    let total: f64 = rows
        .iter()
        .filter(|row| row.get("ok").and_then(Value::as_bool) == Some(true))
        .filter_map(|row| row.get("remaining").and_then(Value::as_f64))
        .sum();
    let failed = rows
        .iter()
        .filter(|row| row.get("ok").and_then(Value::as_bool) != Some(true))
        .count();

    json!({ "ok": true, "total": total, "accounts": rows, "failed": failed })
}

/// 导出账号到指定路径。
///
/// 未指定 id 时**默认导出全部账号**（本工具的语义是「备份整库」，不需要先勾选；
/// core 的 `select_export_records` 在空 id 时会报「请先选择」，故在这里补全）。
///
/// 安全提示必须回给界面：导出文件含**明文 access/refresh token**，
/// 等同于把账号直接交出去，界面上必须显式告知（AGENTS.md §5）。
#[tauri::command]
pub fn export_accounts_file(path: String, account_ids: Option<Vec<String>>) -> Value {
    if path.trim().is_empty() {
        return json!({ "ok": false, "error": "缺少导出路径" });
    }
    let mut ids = account_ids.unwrap_or_default();
    if ids.is_empty() {
        ids = account::load_accounts()
            .iter()
            .filter_map(|acc| acc.get("id").and_then(Value::as_str).map(String::from))
            .collect();
    }
    if ids.is_empty() {
        return json!({ "ok": false, "error": "账号库为空，没有可导出的账号" });
    }
    let count = ids.len();
    match export_import::export_accounts_to_path(&ids, &path) {
        Ok(written) => json!({ "ok": true, "path": written, "count": count }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

/// 预览待导入文件：只解析不入库，让用户先看到会导入几个账号。
#[tauri::command]
pub fn preview_import_file(path: String) -> Value {
    if path.trim().is_empty() {
        return json!({ "ok": false, "error": "缺少文件路径" });
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => return json!({ "ok": false, "error": format!("读取文件失败：{error}") }),
    };
    match export_import::preview_accounts(&text) {
        Ok(preview) => json!({ "ok": true, "preview": preview }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

/// 执行导入：全部条目入库（core 内部按 uid 合并，不会产生重复账号）。
#[tauri::command]
pub fn import_accounts_file(path: String) -> Value {
    if path.trim().is_empty() {
        return json!({ "ok": false, "error": "缺少文件路径" });
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => return json!({ "ok": false, "error": format!("读取文件失败：{error}") }),
    };
    let preview = match export_import::preview_accounts(&text) {
        Ok(preview) => preview,
        Err(error) => return json!({ "ok": false, "error": error }),
    };
    let total = preview
        .get("accounts")
        .and_then(Value::as_array)
        .map(|rows| rows.len())
        .unwrap_or(0);
    if total == 0 {
        return json!({ "ok": false, "error": "文件里没有可用账号" });
    }
    let indexes: Vec<usize> = (0..total).collect();
    match export_import::import_accounts(&text, &indexes) {
        // ImportResult 未实现 Serialize，逐字段取出，避免为此改 core 的公共结构。
        Ok(result) => json!({
            "ok": true,
            "imported": result.imported,
            "skipped": result.skipped,
            "overwritten": result.overwritten,
        }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

/// 积分余额：只取交付字段（剩余/总量），resources 明细不传前端（AGENTS.md §5 最小暴露）。
#[tauri::command]
pub async fn account_balance(account_id: String) -> Value {
    if account_id.trim().is_empty() {
        return json!({ "ok": false, "error": "缺少账号 id" });
    }
    let Some(acc) = account::find_account(&account_id) else {
        return json!({ "ok": false, "error": "账号不存在" });
    };
    let res = credits::get_credit_expiry(&fresh(acc).await).await;
    json!({
        "ok": res.get("ok").and_then(Value::as_bool).unwrap_or(false),
        "remaining": res.get("totalRemaining").cloned().unwrap_or(Value::Null),
        "capacity": res.get("totalCapacity").cloned().unwrap_or(Value::Null),
        "error": res.get("error").cloned().unwrap_or(Value::Null),
    })
}

#[tauri::command]
pub fn run_status() -> Value {
    executor::status()
}

#[tauri::command]
pub fn cancel_run() -> Value {
    executor::cancel()
}

macro_rules! run_command {
    ($name:ident, $action:literal) => {
        #[tauri::command]
        pub async fn $name(account_ids: Option<Vec<String>>) -> Value {
            executor::execute($action, account_ids.unwrap_or_default()).await
        }
    };
}

run_command!(run_checkin, "checkin");
run_command!(run_growth, "growth");
run_command!(run_travel, "travel");
run_command!(run_daily, "daily");

/// 每日自动执行：读配置 + 最近日志一起返回，前端一次请求画完整面板。
#[tauri::command]
pub fn auto_daily_get() -> Value {
    json!({ "config": scheduler::load_config(), "logs": scheduler::load_logs() })
}

/// 每日自动执行：改开关 / 时间。时间不合法拒绝落盘，避免静默失效。
#[tauri::command]
pub fn auto_daily_set(enabled: bool, time: String) -> Value {
    if config::parse_clock(&time).is_none() {
        return json!({ "ok": false, "error": "时间格式应为 HH:MM" });
    }
    let mut cfg = scheduler::load_config();
    cfg["enabled"] = json!(enabled);
    cfg["time"] = json!(time);
    match scheduler::save_config(&cfg) {
        Ok(()) => json!({ "ok": true, "config": cfg }),
        Err(error) => json!({ "ok": false, "error": error.to_string() }),
    }
}

/// 拉起主程序去添加账号：本工具不自己实现登录（AGENTS.md §5 白名单）。
#[tauri::command]
pub fn launch_main_app() -> Value {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(raw) = std::env::var("WB_SWITCH_EXE") {
        if !raw.trim().is_empty() {
            candidates.push(PathBuf::from(raw));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("wb-switch-rust.exe"));
            candidates.push(dir.join("..").join("wb-switch-rust.exe"));
            if let Ok(entries) = std::fs::read_dir(dir.join("..").join("dist-portable")) {
                for entry in entries.flatten() {
                    candidates.push(entry.path().join("wb-switch-rust.exe"));
                }
            }
        }
    }
    for path in candidates {
        if path.is_file() {
            return match std::process::Command::new(&path).spawn() {
                Ok(_) => json!({ "started": true, "path": path.to_string_lossy() }),
                Err(error) => json!({ "started": false, "error": error.to_string() }),
            };
        }
    }
    json!({
        "started": false,
        "error": "未找到 workbuddy-switch 主程序（可用 WB_SWITCH_EXE 指定路径）",
    })
}

// ─────────────────────────── 统计（积分 / Token） ───────────────────────────

/// 积分统计：直接转调 core 的统计聚合（含官方用量，`refresh` 才走网络）。
#[tauri::command]
pub async fn credit_statistics(refresh: Option<bool>) -> Value {
    wb_switch_core::modules::credit_usage::get_statistics(refresh.unwrap_or(false)).await
}

/// Token 统计：扫描本地会话 jsonl（同步重活，放阻塞线程避免卡 IPC）。
///
/// 缓存交给 `token_cache`：按档位分槽 + 落盘，切档位与重启应用都不再重扫。
/// TTL 30 分钟——统计的是历史量，没必要更实时；「重新扫描」按钮带 refresh=true 绕过。
#[tauri::command]
pub async fn token_statistics(days: Option<i64>, refresh: Option<bool>) -> Value {
    const TTL_MS: i64 = 30 * 60 * 1000;

    let days = days.unwrap_or(0);
    if !refresh.unwrap_or(false) {
        if let Some(mut cached) = crate::token_cache::get(days, TTL_MS) {
            cached["cached"] = json!(true);
            return cached;
        }
    }

    let range = if days > 0 { Some(days) } else { None };
    match tokio::task::spawn_blocking(move || {
        wb_switch_core::modules::token_stats::get_statistics(range)
    })
    .await
    {
        Ok(value) => {
            if value.get("error").is_none() {
                crate::token_cache::put(days, &value);
            }
            value
        }
        Err(error) => json!({ "error": error.to_string(), "sources": [] }),
    }
}

// ─────────────────────────── 账号管理 ───────────────────────────

/// 删除账号（写回 accounts.json，不动会话/磁盘其它数据）。
#[tauri::command]
pub fn account_delete(account_id: String) -> Value {
    if account_id.trim().is_empty() {
        return json!({ "ok": false, "error": "缺少账号 id" });
    }
    match account::delete_account(account_id.trim()) {
        Ok(()) => json!({ "ok": true, "id": account_id }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

/// 设置账号备注（页面展示名会优先用它，脱敏在前端做）。
#[tauri::command]
pub fn account_set_note(account_id: String, note: String) -> Value {
    if account_id.trim().is_empty() {
        return json!({ "ok": false, "error": "缺少账号 id" });
    }
    let mut accounts = account::load_accounts();
    let Some(target) = accounts
        .iter_mut()
        .find(|acc| acc.get("id").and_then(Value::as_str) == Some(account_id.trim()))
    else {
        return json!({ "ok": false, "error": "账号不存在" });
    };
    let note = note.trim();
    if note.is_empty() {
        target.as_object_mut().map(|obj| obj.remove("note"));
    } else {
        target["note"] = json!(note);
    }
    match account::save_accounts(&accounts) {
        Ok(()) => json!({ "ok": true }),
        Err(error) => json!({ "ok": false, "error": error.to_string() }),
    }
}

/// 用量预测：按账号返回「余额还能撑几天」。
///
/// 纯本地计算（积分快照 + 中位数/EWMA），不发请求；样本不足 3 天的账号会带
/// `confidence=insufficient` 且 `daysLeft=null`，前端显示样本进度而不是瞎猜。
#[tauri::command(rename_all = "camelCase")]
pub fn account_forecast(window_days: Option<i64>) -> Value {
    forecast::account_forecasts(window_days)
}

/// 一键刷新全部账号的 access token。
///
/// 与 `refresh::run_keepalive_cycle` 的区别：保活只刷「临期」账号，用户手动点这个按钮
/// 要的是**全部重刷**，所以这里不做有效期判断，逐号强刷。并发上限 4，避免官方 auth
/// 接口被同一 IP 连续打满；单号失败只记录，不影响其它账号（失败原因回给界面）。
#[tauri::command]
pub async fn refresh_all_tokens() -> Value {
    const CONCURRENCY: usize = 4;
    let started_at = config::now_ms();
    let accounts = account::load_accounts();
    if accounts.is_empty() {
        return json!({
            "ok": true,
            "total": 0,
            "refreshed": 0,
            "failed": 0,
            "results": [],
            "startedAt": started_at,
        });
    }

    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(CONCURRENCY));
    let mut handles = Vec::with_capacity(accounts.len());
    for acc in accounts {
        let permit = semaphore.clone();
        handles.push(tokio::spawn(async move {
            let _guard = permit.acquire().await;
            let label = account::account_display_name(&acc);
            // 缺 refresh token 的账号刷不动，直接给出可读原因，别空跑一次请求。
            let has_rt = !acc
                .get("refresh_token")
                .and_then(Value::as_str)
                .unwrap_or("")
                .is_empty();
            if !has_rt {
                return json!({
                    "label": label,
                    "ok": false,
                    "error": "缺少 refresh token，需重新登录",
                });
            }
            let fresh = refresh::refresh_account_token(acc).await;
            let needs_relogin = fresh.get("needs_relogin").and_then(Value::as_bool) == Some(true);
            json!({
                "label": label,
                "ok": !needs_relogin,
                "error": if needs_relogin {
                    fresh.get("needs_relogin_reason").and_then(Value::as_str).map(str::to_string)
                } else {
                    None
                },
            })
        }));
    }

    let mut results = Vec::with_capacity(handles.len());
    for handle in handles {
        match handle.await {
            Ok(value) => results.push(value),
            Err(error) => results.push(json!({
                "label": "unknown",
                "ok": false,
                "error": error.to_string(),
            })),
        }
    }
    let total = results.len();
    let refreshed = results
        .iter()
        .filter(|row| row.get("ok").and_then(Value::as_bool) == Some(true))
        .count();
    json!({
        "ok": refreshed == total,
        "total": total,
        "refreshed": refreshed,
        "failed": total - refreshed,
        "results": results,
        "startedAt": started_at,
    })
}

/// 账号卡片总览：
/// - `refresh=false`：读本地采集快照，离线秒回（余额为上次采集值）；
/// - `refresh=true`：并发（4 路）拉官方积分资源包，带积分包明细与近期到期。
/// 只读命令；`get_credit_expiry` 内部会照常落采集快照，与积分统计共用一份。
/// WorkBuddy 运行状态 + 当前登录账号（读 auth 文件，与账号库按 uid/email 匹配）。
///
/// 判定口径与 workbuddy-switch 一致：auth 里的 uid 命中账号的 id/uid，或 email 相等。
/// 两个档位（cn/ai）都查，命中的 accountId 全部返回（同一人可能双档位登录）。
/// 同一台机器上 cn / ai 是两个独立客户端，可能各登录一个号（两个 auth 文件都在），
/// 所以「当前账号」天然可能有两个。这里定一个主账号：正在运行的档位优先，
/// 其次 cn（国内版是默认档位），否则取第一个。剩下的号由前端标「已登录」。
fn pick_primary_account(
    current_by_variant: &[(String, String)],
    running_variant: Option<&str>,
) -> Option<String> {
    if let Some(running) = running_variant {
        if let Some((_, id)) = current_by_variant.iter().find(|(v, _)| v == running) {
            return Some(id.clone());
        }
    }
    if let Some((_, id)) = current_by_variant.iter().find(|(v, _)| v == "cn") {
        return Some(id.clone());
    }
    current_by_variant.first().map(|(_, id)| id.clone())
}

#[cfg(test)]
mod workbuddy_status_tests {
    use super::pick_primary_account;

    fn pairs() -> Vec<(String, String)> {
        vec![
            ("cn".to_string(), "id-cn".to_string()),
            ("ai".to_string(), "id-ai".to_string()),
        ]
    }

    #[test]
    fn running_variant_wins() {
        assert_eq!(
            pick_primary_account(&pairs(), Some("ai")).as_deref(),
            Some("id-ai")
        );
    }

    #[test]
    fn cn_is_default_when_nothing_running() {
        assert_eq!(pick_primary_account(&pairs(), None).as_deref(), Some("id-cn"));
    }

    #[test]
    fn falls_back_to_only_variant() {
        let only_ai = vec![("ai".to_string(), "id-ai".to_string())];
        assert_eq!(
            pick_primary_account(&only_ai, None).as_deref(),
            Some("id-ai")
        );
        assert_eq!(pick_primary_account(&[], None), None);
    }
}

#[tauri::command]
pub async fn workbuddy_status() -> Value {
    tauri::async_runtime::spawn_blocking(move || {
        let accounts = account::load_accounts();
        let mut current_ids: Vec<String> = Vec::new();
        // 档位 → 命中账号：cn / ai 各登录一个号时会有两条
        let mut by_variant: Vec<(String, String)> = Vec::new();
        let mut current: Option<Value> = None;
        // 只认国内版档位：国际版账号已被列表过滤，不再参与「当前账号」判定
        for variant in [WbVariant::Cn] {
            let Some(auth) = auth_file::read_auth_file(variant) else {
                continue;
            };
            let acct = auth.get("account").cloned().unwrap_or_else(|| json!({}));
            // display_value 返回 Value：uid 可能是数字，统一转成字符串再比对。
            let text = |value: &Value| match value {
                Value::String(text) => text.clone(),
                Value::Number(number) => number.to_string(),
                Value::Bool(flag) => flag.to_string(),
                _ => String::new(),
            };
            let uid = text(&account::display_value(&acct, "uid"));
            let email = text(&account::display_value(&acct, "email"));
            let nickname = account::display_value(&acct, "nickname");
            if current.is_none() && (!uid.is_empty() || !email.is_empty()) {
                current = Some(json!({
                    "uid": uid,
                    "email": email,
                    "nickname": nickname,
                    "variant": variant.as_str(),
                }));
            }
            if uid.is_empty() && email.is_empty() {
                continue;
            }
            for acc in &accounts {
                let acc_uid = acc.get("uid").and_then(Value::as_str).unwrap_or("");
                let acc_email = acc.get("email").and_then(Value::as_str).unwrap_or("");
                let acc_id = acc.get("id").and_then(Value::as_str).unwrap_or("");
                let hit = (!uid.is_empty() && (acc_uid == uid || acc_id == uid))
                    || (!email.is_empty() && !acc_email.is_empty() && acc_email == email);
                if hit {
                    if let Some(id) = acc.get("id").and_then(Value::as_str) {
                        if !current_ids.iter().any(|known| known == id) {
                            current_ids.push(id.to_string());
                        }
                        // 同一账号可能被两档位同时命中，只在首次登记该档位
                        let key = variant.as_str().to_string();
                        if !by_variant.iter().any(|(v, _)| v == &key) {
                            by_variant.push((key, id.to_string()));
                        }
                    }
                }
            }
        }
        let running_cn = process::is_workbuddy_running(WbVariant::Cn);
        let running_variant = if running_cn { Some("cn") } else { None };
        let primary = pick_primary_account(&by_variant, running_variant);
        json!({
            "running": running_cn,
            "currentAccountIds": current_ids,
            "current": current,
            // 同一台机器 cn / ai 各登录一个号时，「当前账号」不止一个：
            // primary 画强调色，其余按 variants 标「已登录」。
            "primaryAccountId": primary,
            "currentByVariant": by_variant
                .iter()
                .map(|(v, id)| json!({ "variant": v, "accountId": id }))
                .collect::<Vec<_>>(),
            "runningVariant": running_variant,
        })
    })
    .await
    .unwrap_or_else(|_| json!({ "running": false, "currentAccountIds": [], "current": null }))
}

/// 设为 WorkBuddy 当前账号（完全同步模式）。
///
/// 复用 core 的 `switch::switch_account`：备份 → 关 WorkBuddy → 以当前登录账号为准
/// 全量替换目标账号的会话 → 写认证 → 写后身份校验 → 重启。可能阻塞数十秒，走
/// blocking 线程；进度经 `switch-progress` 事件推给前端。
///
/// **不抛 Err**：失败也返回 `{ ok:false, ... }`，把「停在哪一步 / 是否已回滚」变成
/// 结构化字段。原先靠 catch 中文文案判断，前端无法区分「没写认证」和「已回滚」，
/// 只能一律报「切换失败」——这正是两次事故里用户看不懂现场的原因。
#[tauri::command(rename_all = "camelCase")]
pub async fn switch_workbuddy_account(app: tauri::AppHandle, account_id: String) -> Value {
    if account_id.trim().is_empty() {
        return json!({
            "ok": false,
            "error": "缺少 accountId",
            "result": "blocked",
            "stage": "",
            "identity": "",
            "costMs": 0,
            "rolledBack": false,
        });
    }
    let progress: switch::ProgressFn = Box::new(move |message| {
        let _ = app.emit("switch-progress", json!({ "message": message }));
    });
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        // restart=true + replace_sessions=true：即原项目的「完全同步」切换。
        switch::switch_account_detailed(Some(&progress), &account_id, true, false, &[], &[], true)
    })
    .await;

    match outcome {
        Ok((result, diagnostics)) => switch_payload(result, &diagnostics),
        Err(join_error) => json!({
            "ok": false,
            "error": format!("切换任务异常: {join_error}"),
            "result": "blocked",
            "stage": "",
            "identity": "",
            "costMs": 0,
            "rolledBack": false,
            "sessionsDirty": false,
        }),
    }
}

/// 把 core 的「结果 + 诊断」拼成前端唯一契约。
///
/// 诊断必须来自本次调用的返回值。早期实现是切换后回读历史里最新一条，历史写入失败
/// 或被并发切换插队时会读到上一条，把这次的真实现场说错（最坏是把「没回滚」报成
/// 「已回滚」）。`sessionsDirty` 专门区分「完全还原」与「认证还原了但会话已变」。
fn switch_payload(result: Result<Value, String>, diagnostics: &switch::SwitchDiagnostics) -> Value {
    let mut payload = match result {
        Ok(value) if value.is_object() => value,
        Ok(value) => json!({ "result": value }),
        Err(error) => json!({ "ok": false, "error": error }),
    };
    if let Some(object) = payload.as_object_mut() {
        object.insert("result".to_string(), json!(diagnostics.result));
        object.insert("stage".to_string(), json!(diagnostics.stage));
        object.insert("identity".to_string(), json!(diagnostics.identity_str()));
        object.insert("costMs".to_string(), json!(diagnostics.cost_ms));
        object.insert("rolledBack".to_string(), json!(diagnostics.rolled_back));
        object.insert("sessionsDirty".to_string(), json!(diagnostics.sessions_dirty));
        object.insert("sessions".to_string(), json!(diagnostics.sessions));
        if !object.contains_key("ok") {
            object.insert("ok".to_string(), json!(false));
        }
    }
    payload
}

/// 由已取到的行数据派生健康分，不额外请求。
///
/// 本地 / 官方两个分支都要算，所以抽出来。本地口径没有资源包明细，
/// 也就没有「即将到期」的输入——缺的输入当没有，不估。
fn account_health_of(
    row: &Value,
    rate_limits: &[Value],
    now: i64,
    forecasts: &HashMap<String, Value>,
) -> Value {
    let rate_limited = rate_limits.iter().any(|entry| {
        entry
            .get("resetAt")
            .and_then(Value::as_i64)
            .map(|at| at > now)
            .unwrap_or(false)
    });
    let account_id = row.get("accountId").and_then(Value::as_str).unwrap_or("");
    let days_left = forecasts
        .get(account_id)
        .and_then(|entry| entry.get("daysLeft"))
        .and_then(Value::as_f64);
    health::account_health(&health::HealthInput {
        remaining: row.get("remaining").and_then(Value::as_f64),
        capacity: row.get("capacity").and_then(Value::as_f64),
        expiring_soon_remaining: row
            .get("expiringSoonRemaining")
            .and_then(Value::as_f64),
        rate_limited,
        error: row.get("error").and_then(Value::as_str),
        days_left,
    })
}

#[tauri::command]
pub async fn account_overview(app: tauri::AppHandle, refresh: Option<bool>) -> Value {
    const CONCURRENCY: usize = 4;
    let refresh = refresh.unwrap_or(false);
    let collected_at = config::now_ms();
    // 只服务国内版：国际版账号不参与余额 / 积分 / 预测统计
    let accounts = account::cn_accounts();
    // 模型限流台账（扫本机日志 + hook 事件，内置 5 分钟缓存）：跟余额无关，
    // 两种口径都要展示，所以在这里取一次，按账号拆开。
    let rate_by_account = match tokio::task::spawn_blocking(limits::get_rate_limits).await {
        Ok(payload) => payload
            .get("accounts")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| {
                        let id = row.get("accountId").and_then(Value::as_str)?.to_string();
                        Some((id, row.get("limited").cloned().unwrap_or(json!([]))))
                    })
                    .collect::<HashMap<String, Value>>()
            })
            .unwrap_or_default(),
        Err(_) => HashMap::new(),
    };
    // 预测只算一次（14 个账号各读一遍快照太浪费），按 accountId 索引。
    let forecast_by_account: HashMap<String, Value> = forecast::account_forecasts(None)
        .get("accounts")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let id = row.get("accountId").and_then(Value::as_str)?.to_string();
                    Some((id, row.clone()))
                })
                .collect::<HashMap<String, Value>>()
        })
        .unwrap_or_default();
    if accounts.is_empty() {
        return json!({
            "ok": true,
            "mode": if refresh { "official" } else { "local" },
            "accounts": [],
            "failed": 0,
            "collectedAt": collected_at,
        });
    }

    if !refresh {
        // 本地口径：每账号取时间戳最新的一条快照，不发任何网络请求。
        let snapshots = credit_usage::load_snapshots();
        let mut latest: HashMap<String, &Value> = HashMap::new();
        for snap in &snapshots {
            let Some(id) = snap.get("accountId").and_then(Value::as_str) else {
                continue;
            };
            let ts = snap.get("ts").and_then(Value::as_i64).unwrap_or(0);
            let newer = match latest.get(id) {
                Some(old) => ts > old.get("ts").and_then(Value::as_i64).unwrap_or(0),
                None => true,
            };
            if newer {
                latest.insert(id.to_string(), snap);
            }
        }
        let rows: Vec<Value> = accounts
            .iter()
            .map(|acc| {
                let id = acc.get("id").and_then(Value::as_str).unwrap_or("");
                let snap = latest.get(id);
                let has_snapshot = snap.is_some();
                let mut row = json!({
                    "accountId": id,
                    "label": account::account_display_name(acc),
                    "variant": account::variant_of(acc).as_str(),
                    "note": acc.get("note").cloned().unwrap_or(Value::Null),
                    "ok": has_snapshot,
                    "remaining": snap.map(|s| s.get("remaining").cloned().unwrap_or(Value::Null)).unwrap_or(Value::Null),
                    "capacity": snap.map(|s| s.get("total").cloned().unwrap_or(Value::Null)).unwrap_or(Value::Null),
                    "updatedAt": snap.map(|s| s.get("ts").cloned().unwrap_or(Value::Null)).unwrap_or(Value::Null),
                    "packages": 0,
                    "resources": [],
                    "expiringSoonRemaining": Value::Null,
                    "rateLimits": rate_by_account.get(id).cloned().unwrap_or(json!([])),
                    "error": if has_snapshot { Value::Null } else { json!("暂无本地快照") },
                });
                let limits: Vec<Value> = row["rateLimits"].as_array().cloned().unwrap_or_default();
                row["health"] = account_health_of(&row, &limits, collected_at, &forecast_by_account);
                row
            })
            .collect();
        crate::tray::refresh(&app);
        return json!({
            "ok": true,
            "mode": "local",
            "accounts": rows,
            "failed": 0,
            "collectedAt": collected_at,
        });
    }

    // 官方口径：并发拉取积分资源包。
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(CONCURRENCY));
    let mut handles = Vec::with_capacity(accounts.len());
    for acc in accounts {
        let meta = json!({
            "accountId": acc.get("id").cloned().unwrap_or(Value::Null),
            "label": account::account_display_name(&acc),
            "variant": account::variant_of(&acc).as_str(),
            "note": acc.get("note").cloned().unwrap_or(Value::Null),
        });
        let permit = semaphore.clone();
        handles.push(tokio::spawn(async move {
            let _guard = permit.acquire().await;
            let res = credits::get_credit_expiry(&acc).await;
            // 成功的余额顺手写进本地快照：否则新登录的账号在「本地口径」下
            // 永远是暂无快照，只有官方口径才有数（离线秒开也就无从谈起）。
            if res.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                if let (Some(total), Some(remaining)) = (
                    res.get("totalCapacity").and_then(Value::as_f64),
                    res.get("totalRemaining").and_then(Value::as_f64),
                ) {
                    credit_usage::record_snapshot(
                        meta["accountId"].as_str().unwrap_or(""),
                        meta["label"].as_str().unwrap_or(""),
                        total,
                        remaining,
                        WbVariant::parse(Some(meta["variant"].as_str().unwrap_or("cn"))),
                    );
                }
            }
            (meta, res)
        }));
    }

    let mut rows = Vec::with_capacity(handles.len());
    let mut failed = 0usize;
    for handle in handles {
        let Ok((meta, res)) = handle.await else {
            failed += 1;
            continue;
        };
        let ok = res.get("ok").and_then(Value::as_bool).unwrap_or(false);
        if !ok {
            failed += 1;
        }
        // 近期到期：只保留仍有剩余的包，按到期时间升序（前端展示前 2 条，其余进弹框）。
        let mut packages: Vec<Value> = res
            .get("resources")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|resource| {
                resource
                    .get("remaining")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0)
                    > 0.0
            })
            .collect();
        packages.sort_by_key(|resource| {
            resource
                .get("expireAt")
                .and_then(Value::as_i64)
                .unwrap_or(i64::MAX)
        });
        let mut row = json!({
            "accountId": meta["accountId"],
            "label": meta["label"],
            "variant": meta["variant"],
            "note": meta["note"],
            "ok": ok,
            "remaining": res.get("totalRemaining").cloned().unwrap_or(Value::Null),
            "capacity": res.get("totalCapacity").cloned().unwrap_or(Value::Null),
            "updatedAt": res.get("updatedAt").cloned().unwrap_or(Value::Null),
            "packages": packages.len(),
            "resources": packages,
            "expiringSoonRemaining": res.get("expiringSoonRemaining").cloned().unwrap_or(Value::Null),
            "rateLimits": rate_by_account
                .get(meta["accountId"].as_str().unwrap_or(""))
                .cloned()
                .unwrap_or(json!([])),
            "error": res.get("error").cloned().unwrap_or(Value::Null),
        });
        let limits: Vec<Value> = row["rateLimits"].as_array().cloned().unwrap_or_default();
        row["health"] = account_health_of(&row, &limits, collected_at, &forecast_by_account);
        rows.push(row);
    }

    // 用户刚看过余额：顺手把托盘读数刷新到最新，不用等下一次启动。
    crate::tray::refresh(&app);
    json!({
        "ok": true,
        "mode": "official",
        "accounts": rows,
        "failed": failed,
        "collectedAt": collected_at,
    })
}

// ───────────────────────── 运行日志（三源合并） ─────────────────────────

/// core 的 `utc_iso()` 是 `2026-10-30T09-00-00Z`（时间用 `-` 分隔），按此解析成毫秒。
fn parse_utc_iso_ms(text: &str) -> i64 {
    chrono::NaiveDateTime::parse_from_str(text.trim_end_matches('Z'), "%Y-%m-%dT%H-%M-%S")
        .map(|dt| dt.and_utc().timestamp_millis())
        .unwrap_or(0)
}

/// 自动日常的 status 转成人话标题（前端只负责显示）。
fn auto_status_label(status: &str) -> String {
    match status {
        "ok" | "success" | "completed" => "自动日常 · 全部成功".to_string(),
        "partial" => "自动日常 · 部分成功".to_string(),
        "busy" => "自动日常 · 跳过（任务进行中）".to_string(),
        "error" | "failed" => "自动日常 · 失败".to_string(),
        other => format!("自动日常 · {other}"),
    }
}

fn auto_status_level(status: &str) -> &'static str {
    match status {
        "ok" | "success" | "completed" => "success",
        "partial" | "busy" => "warning",
        "error" | "failed" => "error",
        _ => "info",
    }
}

/// 切换历史 → 运行日志条目：一次切换一行，级别由结果决定。
///
/// 与另外三源的分工：三源只有一句提示（「切号失败」），这里带结构化前后对照
/// （谁→谁、复制几条、耗时多久、身份校验结论），出事时能直接定位停在哪一步。
fn switch_history_events() -> (Vec<Value>, usize) {
    let payload = switch_history::load(0);
    let Some(rows) = payload.get("entries").and_then(Value::as_array) else {
        return (Vec::new(), 0);
    };
    let text = |row: &Value, key: &str| -> String {
        row.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let result = text(row, "result");
        let level = match result.as_str() {
            "ok" => "success",
            "rolled_back" => "warning",
            _ => "error",
        };
        let named = |label: &str, uid: &str| -> String {
            if label.is_empty() {
                uid.to_string()
            } else {
                label.to_string()
            }
        };
        let from = named(&text(row, "fromLabel"), &text(row, "fromUid"));
        let to = named(&text(row, "toLabel"), &text(row, "toUid"));
        let target = if from.is_empty() {
            to
        } else {
            format!("{from} → {to}")
        };
        let title = match result.as_str() {
            "ok" => format!("切换成功 {target}"),
            "rolled_back" => format!("切换已回滚 {target}"),
            _ => format!("切换中止 {target}"),
        };

        let mut parts: Vec<String> = Vec::new();
        let mode = text(row, "mode");
        if !mode.is_empty() {
            parts.push(mode);
        }
        if let Some(sessions) = row.get("sessions").filter(|value| value.is_object()) {
            let copied = sessions.get("copied").and_then(Value::as_i64).unwrap_or(0);
            let total = sessions
                .get("sourceTotal")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let failed = sessions.get("failed").and_then(Value::as_i64).unwrap_or(0);
            parts.push(format!("复制 {copied}/{total}"));
            if failed > 0 {
                parts.push(format!("失败 {failed}"));
            }
            if sessions.get("replaced").and_then(Value::as_bool) == Some(true) {
                parts.push("已替换".to_string());
            }
        }
        let identity = text(row, "identity");
        if !identity.is_empty() && identity != "effective" {
            parts.push(identity);
        }
        let cost = row.get("costMs").and_then(Value::as_i64).unwrap_or(0);
        if cost > 0 {
            parts.push(format!("耗时 {:.1}s", cost as f64 / 1000.0));
        }
        let error = text(row, "error");
        if !error.is_empty() {
            parts.push(error);
        }
        items.push(json!({
            "at": row.get("ts").and_then(Value::as_i64).unwrap_or(0),
            "source": "switch",
            "level": level,
            "title": title,
            "detail": parts.join(" · "),
        }));
    }
    (items, rows.len())
}

/// 运行日志：通知存档 + 自动日常记录 + 错误日志 + 切换历史，合并成一条时间轴（新→旧）。
#[tauri::command]
pub fn events_list(limit: Option<usize>) -> Value {
    let limit = limit.unwrap_or(300).clamp(20, 1000);
    let mut items: Vec<Value> = Vec::new();
    let mut notify_count = 0usize;
    let mut error_count = 0usize;
    let mut load_errors: Vec<String> = Vec::new();

    // 1) 通知存档：签到 / 成长 / 切换 / 扫码采集等一次性提示的回看
    match notifications::list() {
        Ok(rows) => {
            notify_count = rows.len();
            for row in rows {
                items.push(json!({
                    "at": row.at,
                    "source": "notify",
                    "level": row.level,
                    "title": row.title,
                    "detail": row.description,
                }));
            }
        }
        Err(error) => load_errors.push(format!("通知存档：{error}")),
    }

    // 2) 自动日常：每条记录一次到点执行
    let auto_logs = scheduler::load_logs();
    let auto_count = auto_logs.len();
    for row in auto_logs {
        let status = row.get("status").and_then(Value::as_str).unwrap_or("unknown");
        let total = row.get("total").and_then(Value::as_i64).unwrap_or(0);
        let ok = row.get("ok").and_then(Value::as_i64).unwrap_or(0);
        let other = row.get("other").and_then(Value::as_i64).unwrap_or(0);
        let clock = row.get("time").and_then(Value::as_str).unwrap_or("-");
        let at = row
            .get("ts")
            .and_then(Value::as_str)
            .map(parse_utc_iso_ms)
            .unwrap_or(0);
        items.push(json!({
            "at": at,
            "source": "auto",
            "level": auto_status_level(status),
            "title": auto_status_label(status),
            "detail": format!("计划 {clock} · 共 {total} 个账号 · 成功 {ok} · 其它 {other}"),
        }));
    }

    // 3) 错误日志：JSON Lines，一行一条
    let error_path = error_log::error_log_path();
    let error_text = std::fs::read_to_string(&error_path).unwrap_or_default();
    for line in error_text.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(row) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        error_count += 1;
        let kind = row.get("kind").and_then(Value::as_str).unwrap_or("backend");
        let message = row.get("message").and_then(Value::as_str).unwrap_or("（无标题）");
        let detail = row.get("detail").and_then(Value::as_str).unwrap_or("");
        items.push(json!({
            "at": row.get("ts").and_then(Value::as_i64).unwrap_or(0),
            "source": "error",
            "level": "error",
            "title": message,
            "detail": if detail.is_empty() { kind.to_string() } else { format!("{kind} · {detail}") },
        }));
    }

    // 4) 切换历史：结构化前后对照（谁→谁 / 复制几条 / 耗时 / 身份校验）
    let (switch_items, switch_count) = switch_history_events();
    items.extend(switch_items);

    // 新→旧
    items.sort_by(|left, right| {
        right
            .get("at")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .cmp(&left.get("at").and_then(Value::as_i64).unwrap_or(0))
    });

    let total = items.len();
    let today_start = {
        use chrono::TimeZone;
        let now = chrono::Local::now();
        now.date_naive()
            .and_hms_opt(0, 0, 0)
            .and_then(|naive| chrono::Local.from_local_datetime(&naive).single())
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0)
    };
    let today = items
        .iter()
        .filter(|row| row.get("at").and_then(Value::as_i64).unwrap_or(0) >= today_start)
        .count();
    let warning = items
        .iter()
        .filter(|row| row.get("level").and_then(Value::as_str) == Some("warning"))
        .count();
    let error_level = items
        .iter()
        .filter(|row| row.get("level").and_then(Value::as_str) == Some("error"))
        .count();

    items.truncate(limit);

    json!({
        "ok": true,
        "items": items,
        "truncated": total > limit,
        "stats": {
            "total": total,
            "today": today,
            "warning": warning,
            "error": error_level,
            "notify": notify_count,
            "auto": auto_count,
            "errlog": error_count,
            "switch": switch_count,
        },
        "loadErrors": load_errors,
        "generatedAt": config::now_ms(),
    })
}

/// 清空某一源的日志：notify / auto / error / switch。只删本机存档，不动账号库。
#[tauri::command]
pub fn events_clear(source: String) -> Value {
    let outcome = match source.as_str() {
        "notify" => notifications::clear(),
        "auto" => scheduler::clear_logs().map_err(|error| error.to_string()),
        "error" => {
            let path = error_log::error_log_path();
            std::fs::write(&path, "").map_err(|error| error.to_string())
        }
        // 切换历史是结构化存档：清空 = 重置为空列表，保留文件与版本号。
        "switch" => switch_history::clear().map(|_| ()),
        other => return json!({ "ok": false, "error": format!("未知来源：{other}") }),
    };
    match outcome {
        Ok(()) => json!({ "ok": true }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

// ───────────────────────── 切换历史 ─────────────────────────

/// 切换历史列表：`limit` 为 0 或缺省返回全量（上限 500 条，新→旧）。
///
/// 与 `events_list` 的 `switch` 源同数据，但这里是**原始结构**：带 uid、模式、
/// 身份校验结论、会话摘要，供「切换记录」面板做筛选与详情展开。
#[tauri::command]
pub fn switch_history_list(limit: Option<usize>) -> Value {
    switch_history::load(limit.unwrap_or(0))
}

// 清空不单独开命令：统一走 `events_clear("switch")`，与其它三源同一入口。

// ───────────────────────── 限流自动切换 ─────────────────────────

/// 配置 + 最近一次决策 + 日志条数。
#[tauri::command]
pub fn auto_switch_status() -> Value {
    auto_switch::status()
}

/// 局部更新配置（只传要改的字段）。
#[tauri::command(rename_all = "camelCase")]
pub fn auto_switch_set(patch: Value) -> Value {
    match auto_switch::update_config(&patch) {
        Ok(status) => json!({ "ok": true, "status": status }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

/// 自动切换的 3 秒倒计时状态：期间用户可以取消（正在打字时不被强行关窗）。
static AUTO_SWITCH_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static AUTO_SWITCH_PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 自动切换的进程内互斥：1s 快路径 / 60s 兜底 / 2min 巡检三个入口共用。
static AUTO_SWITCH_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 取消这次待执行的自动切换（前端倒计时条上的「取消」按钮）。
#[tauri::command]
pub fn auto_switch_cancel() -> Value {
    if !AUTO_SWITCH_PENDING.load(std::sync::atomic::Ordering::SeqCst) {
        return json!({ "ok": false, "error": "当前没有待执行的切换" });
    }
    AUTO_SWITCH_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
    json!({ "ok": true })
}

/// 真正执行一次「限流 → 主动切换」。
///
/// 与 `auto_switch_check`（只算决策、不动手）分开：那些是只读的，这个会**关掉
/// WorkBuddy 并重启**。所以闸门按顺序：总开关开 → 非仅提醒 → 没有并发切换 →
/// 批量任务没在跑 → 跨进程名额抢到 → 3 秒倒计时没被取消。
///
/// **和手动切换同一条路径**（`replace_sessions=true`）：任务与空间跟着账号走，
/// 切完在新账号里能看到原来的会话。旧版这里用 `false`，只换认证不搬会话，
/// 结果切过去就是一片空白——用户会以为任务与空间被删了。
#[tauri::command]
pub async fn auto_switch_apply(app: tauri::AppHandle) -> Value {
    let cfg = auto_switch::load_config();
    if !cfg.enabled {
        return json!({ "action": "skip", "reason": "未开启自动切换" });
    }
    if cfg.notify_only {
        return json!({ "action": "skip", "reason": "仅提醒模式，不自动切换" });
    }
    // 并发闸①：同一进程内三个入口（1s 快路径 / 60s 兜底 / 2min 巡检）只能进一个。
    // 少了这道闸，16 秒内会对同一目标连切两次，会话被替换两遍。
    if AUTO_SWITCH_RUNNING.swap(true, Ordering::SeqCst) {
        return json!({ "action": "skip", "reason": "已有一次自动切换在执行" });
    }
    // 并发闸②：批量任务在跑时不切。切换要关 WorkBuddy，等于把跑到一半的批次掐断。
    if executor::is_running() {
        AUTO_SWITCH_RUNNING.store(false, Ordering::SeqCst);
        return json!({ "action": "skip", "reason": "批量任务执行中，本轮不切换" });
    }
    // 并发闸③：跨进程名额（状态文件 inFlightAt）。主机重启/崩溃留下的占位 10 分钟后自动过期。
    let now = config::now_ms();
    if !auto_switch::try_begin_switch(now) {
        AUTO_SWITCH_RUNNING.store(false, Ordering::SeqCst);
        return json!({ "action": "skip", "reason": "另一次切换尚未结束" });
    }

    let current = auto_switch::current_account_id();
    let app_for_progress = app.clone();
    let app_for_pending = app.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        auto_switch::run_once(&cfg, current.as_deref(), now, |target| {
            // 倒计时：3 秒，100ms 粒度，期间允许取消（取消不写冷却、不动认证）
            AUTO_SWITCH_CANCEL.store(false, Ordering::SeqCst);
            AUTO_SWITCH_PENDING.store(true, Ordering::SeqCst);
            let _ = app_for_pending.emit(
                "auto-switch-pending",
                json!({ "targetId": target, "seconds": 3 }),
            );
            for _ in 0..30 {
                if AUTO_SWITCH_CANCEL.load(Ordering::SeqCst) {
                    AUTO_SWITCH_PENDING.store(false, Ordering::SeqCst);
                    return Err("cancelled-by-user".to_string());
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            AUTO_SWITCH_PENDING.store(false, Ordering::SeqCst);

            // 与手动切换完全一致：完全同步（替换目标账号会话）+ 写后身份校验 + 重启
            let progress: switch::ProgressFn = Box::new(move |message| {
                let _ = app_for_progress.emit("switch-progress", json!({ "message": message }));
            });
            let (result, diagnostics) = switch::switch_account_detailed(
                Some(&progress),
                target,
                true,
                false,
                &[],
                &[],
                true,
            );
            match result {
                Ok(_) => Ok(()),
                Err(error) => Err(format!("{error}（阶段 {}）", diagnostics.stage)),
            }
        })
    })
    .await;

    let cancelled = matches!(
        &outcome,
        Ok(payload) if payload.get("error").and_then(|v| v.as_str()) == Some("cancelled-by-user")
    );
    let ok = matches!(&outcome, Ok(payload)
        if payload.get("action").and_then(|v| v.as_str()) == Some("switch")
            && payload.get("ok").and_then(|v| v.as_bool()) == Some(true));
    // 成功才吃冷却；取消/失败把名额与冷却一起还原，下一轮还能立刻重试
    auto_switch::end_switch(ok, config::now_ms());
    AUTO_SWITCH_RUNNING.store(false, Ordering::SeqCst);

    match outcome {
        Ok(payload) => {
            if cancelled {
                return json!({ "action": "cancelled", "reason": "用户取消了本次自动切换" });
            }
            payload
        }
        Err(join_error) => json!({
            "action": "switch",
            "ok": false,
            "error": format!("切换线程异常: {join_error}"),
        }),
    }
}

#[tauri::command]
pub fn auto_switch_logs(limit: Option<usize>) -> Value {
    auto_switch::logs(limit.unwrap_or(100))
}

#[tauri::command]
pub fn auto_switch_clear_logs() -> Value {
    match auto_switch::clear_logs() {
        Ok(removed) => json!({ "ok": true, "removed": removed }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

/// 手动跑一轮判定（不执行切换，只返回结论）。
///
/// 用于「装了但不知道它会不会误判」的场景：用户点一下就能看到当前会怎么判，
/// 而不用等真的被限流。
#[tauri::command]
pub fn auto_switch_check() -> Value {
    let cfg = auto_switch::load_config();
    let current = auto_switch::current_account_id();
    let now = config::now_ms();
    let candidates = auto_switch::candidates(now);
    let decision = auto_switch::decide(&cfg, &candidates, current.as_deref(), None, now);
    json!({
        "enabled": cfg.enabled,
        "notifyOnly": cfg.notify_only,
        "currentAccountId": current,
        "decision": match decision {
            auto_switch::Decision::Skip(reason) => json!({ "action": "skip", "reason": reason }),
            auto_switch::Decision::Notify { target_id, reason } => json!({
                "action": "notify", "targetId": target_id, "reason": reason
            }),
            auto_switch::Decision::Switch { target_id, reason } => json!({
                "action": "switch", "targetId": target_id, "reason": reason
            }),
        },
    })
}

/// 后台巡检：每 2 分钟判定一次（开关关闭时几乎零成本）。
///
/// 为什么是 2 分钟：限流恢复时刻的精度是分钟级，更密的轮询没有收益；
/// 判定本身只读本地台账（`limits` 有 5 分钟缓存），不打接口。
///
/// **不再自己拼切换逻辑**：统一走 `auto_switch_apply`，那里有并发互斥、
/// 批量任务闸门与「和手动一致」的完全同步。以前这里另写一份 plain 切换，
/// 三个入口各切各的，导致同一目标 16 秒内被切两次。
pub fn spawn_auto_switch(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(120)).await;
            if !auto_switch::load_config().enabled {
                continue;
            }
            let payload = auto_switch_apply(app.clone()).await;
            if payload.get("action").and_then(Value::as_str) != Some("skip") {
                let _ = app.emit("auto-switch", payload);
            }
        }
    });
}

// ───────────────────────── 外部链接 ─────────────────────────

/// 用系统默认浏览器打开链接（设置页「开源地址」用）。
///
/// 只放行 http/https：这个命令接收前端传来的任意字符串，若不校验，
/// 一个 `file://` 或自定义协议就可能被用来触发本机程序。
#[tauri::command]
pub fn open_external(url: String) -> Value {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return json!({ "ok": false, "error": "只允许打开 http/https 链接" });
    }
    match open_in_browser(trimmed) {
        Ok(()) => json!({ "ok": true }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

/// 打开 WorkBuddy 客户端深链（`workbuddy://...`），用于成长任务的官方入口。
///
/// 单独一条命令而不是放宽 `open_external`：那条接收任意字符串，
/// 一旦允许自定义协议就等于把 `file://`、安装器协议全交给页面。
/// 这里**只认 `workbuddy://` 前缀**，且拒绝空白与控制字符。
#[tauri::command]
pub fn open_workbuddy_deeplink(url: String) -> Value {
    let trimmed = url.trim();
    if !trimmed.starts_with("workbuddy://") || trimmed.len() > 512 {
        return json!({ "ok": false, "error": "只允许打开 workbuddy:// 深链" });
    }
    if trimmed.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return json!({ "ok": false, "error": "链接包含非法字符" });
    }
    match open_in_browser(trimmed) {
        Ok(()) => json!({ "ok": true }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

// ───────────────────────── 数据导出 ─────────────────────────

/// 导出一份 CSV 到用户选定的路径。`kind` ∈ accounts / tokensDaily / tokensModels / credits。
///
/// 后端直接写文件而不是把内容传回前端：CSV 可能有几 MB，过一次 IPC
/// 再落盘没有意义。前端只负责用 save 对话框拿路径。
#[tauri::command(rename_all = "camelCase")]
pub fn export_data(kind: String, path: String) -> Value {
    if path.trim().is_empty() {
        return json!({ "ok": false, "error": "未选择保存位置" });
    }
    crate::export::export_one(&kind, &path)
}

/// 导出诊断包（版本 / 平台 / 数据规模 / 日志尾部），排障用。
#[tauri::command(rename_all = "camelCase")]
pub fn export_diagnostics(path: String) -> Value {
    if path.trim().is_empty() {
        return json!({ "ok": false, "error": "未选择保存位置" });
    }
    crate::export::export_diagnostics(&path)
}

/// 把全部 CSV 写进选定目录，逐份返回结果。
#[tauri::command(rename_all = "camelCase")]
pub fn export_all_data(dir: String) -> Value {
    if dir.trim().is_empty() {
        return json!({ "ok": false, "error": "未选择导出目录" });
    }
    crate::export::export_all(&dir)
}

// ───────────────────────── 系统设置 ─────────────────────────

/// 字节数转人类可读（后端只给原始字节 + 展示串，前端不用再算）。
fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{}{}", bytes, UNITS[unit])
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}

/// 系统信息：版本 / 数据目录 / 各存档体积与条数 / 调度摘要。
#[tauri::command]
pub fn system_info() -> Value {
    let store = config::store_dir();
    let accounts = account::load_accounts();
    let notify_count = notifications::list().map(|rows| rows.len()).unwrap_or(0);
    let auto_logs = scheduler::load_logs().len();
    let error_lines = std::fs::read_to_string(error_log::error_log_path())
        .map(|text| text.lines().filter(|line| !line.trim().is_empty()).count())
        .unwrap_or(0);
    let switch_count = switch_history::load(0)
        .get("total")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;

    let file_stat = |name: &str| {
        let path = store.join(name);
        let bytes = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        json!({ "exists": path.exists(), "bytes": bytes, "human": human_bytes(bytes) })
    };

    json!({
        "ok": true,
        "version": crate::update::APP_VERSION,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "storeDir": store.display().to_string(),
        "accountCount": accounts.len(),
        "counts": {
            "notify": notify_count,
            "autoLogs": auto_logs,
            "errorLines": error_lines,
            "switchHistory": switch_count,
        },
        "files": {
            "accounts": file_stat("accounts.json"),
            "autoLogs": file_stat("auto_daily_logs.json"),
            "notifications": file_stat("notifications.json"),
            "errorLog": file_stat("error.log"),
            "switchHistory": file_stat("switch_history.json"),
        },
        "schedule": scheduler::load_config(),
    })
}

/// 打开数据目录（资源管理器 / Finder / xdg-open）。
#[tauri::command]
pub fn open_store_dir() -> Value {
    let dir = config::store_dir();
    let _ = std::fs::create_dir_all(&dir);
    let target = dir.display().to_string();
    #[cfg(target_os = "windows")]
    let spawned = std::process::Command::new("explorer").arg(&target).spawn();
    #[cfg(target_os = "macos")]
    let spawned = std::process::Command::new("open").arg(&target).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let spawned = std::process::Command::new("xdg-open").arg(&target).spawn();
    match spawned {
        Ok(_) => json!({ "ok": true, "path": target }),
        Err(error) => json!({ "ok": false, "path": target, "error": error.to_string() }),
    }
}

/// 检查更新（`force=true` 绕过 6 小时缓存）。
#[tauri::command]
pub async fn update_check(force: bool) -> Value {
    crate::update::check(force).await
}

/// 下载并安装更新：立即返回，进度与结果走 `update://progress` / `update://done` 事件。
/// Windows 上安装器被拉起后本进程由插件结束，命令不会有机会「返回安装结果」。
#[tauri::command]
pub fn update_install(app: tauri::AppHandle) -> Value {
    crate::update::install(app)
}

/// 读取更新设置。
#[tauri::command]
pub fn update_config_get() -> Value {
    let cfg = crate::update::load_config();
    json!({
        "ok": true,
        "current": crate::update::APP_VERSION,
        "autoCheck": cfg.auto_check,
        "mirror": cfg.mirror,
        "configPath": crate::update::config_path().display().to_string(),
    })
}

/// 保存更新设置：镜像变更即时生效并清空缓存（旧缓存是按旧源查的）。
#[tauri::command]
pub fn update_config_set(auto_check: bool, mirror: String) -> Value {
    let mut cfg = crate::update::load_config();
    let changed = cfg.mirror != mirror;
    cfg.auto_check = auto_check;
    cfg.mirror = mirror;
    match crate::update::save_config(&cfg) {
        Ok(()) => {
            if changed {
                crate::update::invalidate_cache();
            }
            json!({ "ok": true, "autoCheck": cfg.auto_check, "mirror": cfg.mirror })
        }
        Err(error) => json!({ "ok": false, "error": error.to_string() }),
    }
}

/// 归档任务统计（只读）：条数 + 正文体积 + WorkBuddy 运行状态。
#[tauri::command]
pub async fn archived_tasks_stats() -> Value {
    tauri::async_runtime::spawn_blocking(crate::purge::stats)
        .await
        .unwrap_or_else(|_| json!({ "ok": false, "error": "spawn failed" }))
}

/// 归档任务清单（只读）：标题 / 归档时刻 / 正文体积，供侧栏「归档任务」分区展示。
#[tauri::command]
pub async fn archived_tasks_list() -> Value {
    tauri::async_runtime::spawn_blocking(crate::purge::list)
        .await
        .unwrap_or_else(|_| json!({ "ok": false, "error": "spawn failed" }))
}

/// 执行归档任务清理：自动关 WorkBuddy → 清理 → 自动拉起（默认）。
///
/// `relaunch=false` 只清理不拉起。逐阶段进度走事件 `purge-progress`，
/// 前端弹框照「切换账号」那套显示，失败时带 `stage` 说清停在哪一步。
#[tauri::command]
pub async fn purge_archived_tasks(app: tauri::AppHandle, relaunch: Option<bool>) -> Value {
    let relaunch = relaunch.unwrap_or(true);
    let progress: Box<dyn Fn(&str) + Send + Sync> = Box::new(move |message: &str| {
        let _ = app.emit("purge-progress", json!({ "message": message }));
    });
    tauri::async_runtime::spawn_blocking(move || {
        crate::purge::purge_auto(relaunch, progress.as_ref())
    })
    .await
    .unwrap_or_else(|error| {
        json!({ "ok": false, "blocked": true, "stage": "purge", "error": format!("清理任务异常: {error}") })
    })
}

/// 主窗口前端已挂载（启动动画遮罩已绘制）：此时才显示窗口，避免先白后黑闪一下。
/// 窗口在 tauri.conf.json 里是 `visible:false`。
#[tauri::command]
pub fn app_ready(app: tauri::AppHandle) {
    // 存量账号字段补齐（外部工具导入的记录可能只有 expires_at 秒级时间）：
    // 幂等、纯本地，失败不影响开窗。
    let _ = std::panic::catch_unwind(|| wb_switch_core::modules::export_import::repair_store_fields());
    if let Some(window) = tauri::Manager::get_webview_window(&app, "main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
