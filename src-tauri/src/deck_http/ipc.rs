//! 面板 iframe 的 IPC 分发：命令名与 [`crate::commands`] 里的 `#[tauri::command]` 一一对应，
//! 参数键名按前端实际传的 camelCase 取（`rename_all = "camelCase"` 的等价展开）。
//!
//! 与真实 IPC 有两处必须不同的地方，都由「面板没有宿主窗口」决定：
//! - **`app_ready` 不能转发**：`commands::app_ready` 会 `show() + set_focus()` 主窗口，
//!   转发出去等于「用户在 WorkBuddy 里点一下图标，独立窗口被顶到前台」，内嵌面板就废了。
//!   这里只做与窗口无关的幂等活（存量账号字段补齐），开窗留给主窗口真实 IPC。
//! - 文件对话框类命令由 [`super::shim`] 直接拒绝，不会走到这里。
//!
//! 新增命令时只改这一处（`match` 分支），不要在前端另开 HTTP 通路。

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::commands as c;

pub(super) async fn dispatch(app: &AppHandle, cmd: &str, args: &Value) -> Result<Value, String> {
    let text = |key: &str| args.get(key).and_then(Value::as_str).map(str::to_string);
    let flag = |key: &str| args.get(key).and_then(Value::as_bool);
    let number = |key: &str| args.get(key).and_then(Value::as_i64);
    let list = |key: &str| {
        args.get(key).and_then(Value::as_array).map(|rows| {
            rows.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
    };

    match cmd {
        // 文件选择：对话框不依赖宿主窗口，直接在 Rust 侧弹（真原生对话框）。
        // 返回形状与 @tauri-apps/plugin-dialog 一致：路径字符串或 null。
        "plugin:dialog|open" | "plugin:dialog|save" => {
            use tauri_plugin_dialog::DialogExt;
            let save = cmd.ends_with("save");
            let app = app.clone();
            let picked = tauri::async_runtime::spawn_blocking(move || {
                if save {
                    app.dialog().file().blocking_save_file()
                } else {
                    app.dialog().file().blocking_pick_file()
                }
            })
            .await
            .map_err(|e| format!("文件对话框失败: {e}"))?;
            Ok(picked
                .and_then(|p| p.as_path().map(|x| x.to_string_lossy().to_string()))
                .map(Value::from)
                .unwrap_or(Value::Null))
        }
        "plugin:notification|is_permission_granted" => Ok(Value::Bool(true)),
        "plugin:notification|request_permission" => Ok(Value::from("granted")),
        "skin_state" => Ok(c::skin_state()),
        "skin_apply" => {
            let cfg: crate::skin::SkinConfig = args
                .get("config")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|e| format!("配置格式不对: {e}"))?
                .unwrap_or_default();
            c::skin_apply(cfg).await
        }
        "skin_remove" => c::skin_remove().await,
        "skin_wallpapers" => c::skin_wallpapers(number("page").map(|v| v as u32)).await,
        "skin_wallpaper_data" => c::skin_wallpaper_data(text("url").unwrap_or_default()).await,
        "skin_wallpaper_upload" => c::skin_wallpaper_upload(app.clone()).await,
        "list_accounts" => Ok(c::list_accounts()),
        "import_local_accounts" => Ok(c::import_local_accounts()),
        "account_detail" => c::account_detail(text("accountId").unwrap_or_default()).await,
        "checkin_now" => c::checkin_now(text("accountId").unwrap_or_default()).await,
        "start_login" => Ok(c::start_login(text("variant")).await),
        "poll_login" => Ok(c::poll_login(text("loginId").unwrap_or_default()).await),
        "total_balance" => Ok(c::total_balance().await),
        "export_accounts_file" => Ok(c::export_accounts_file(
            text("path").unwrap_or_default(),
            list("accountIds"),
        )),
        "preview_import_file" => Ok(c::preview_import_file(text("path").unwrap_or_default())),
        "import_accounts_file" => Ok(c::import_accounts_file(text("path").unwrap_or_default())),
        "account_balance" => Ok(c::account_balance(text("accountId").unwrap_or_default()).await),
        "run_status" => Ok(c::run_status()),
        "cancel_run" => Ok(c::cancel_run()),
        "run_checkin" => Ok(c::run_checkin(list("accountIds")).await),
        "run_growth" => Ok(c::run_growth(list("accountIds")).await),
        "run_travel" => Ok(c::run_travel(list("accountIds")).await),
        "run_daily" => Ok(c::run_daily(list("accountIds")).await),
        "growth_assets" => Ok(c::growth_assets(text("accountId")).await),
        "auto_daily_get" => Ok(c::auto_daily_get()),
        "auto_daily_set" => Ok(c::auto_daily_set(
            flag("enabled").unwrap_or(false),
            text("time").unwrap_or_default(),
        )),
        "cdp_status" => Ok(c::cdp_status().await),
        "cdp_restart_client" => Ok(c::cdp_restart_client().await),
        "launch_main_app" => Ok(c::launch_main_app()),
        "credit_statistics" => Ok(c::credit_statistics(flag("refresh")).await),
        "token_statistics" => Ok(c::token_statistics(number("days"), flag("refresh")).await),
        "account_delete" => Ok(c::account_delete(text("accountId").unwrap_or_default())),
        "account_set_note" => Ok(c::account_set_note(
            text("accountId").unwrap_or_default(),
            text("note").unwrap_or_default(),
        )),
        "account_forecast" => Ok(c::account_forecast(number("windowDays"))),
        "refresh_all_tokens" => Ok(c::refresh_all_tokens().await),
        "workbuddy_status" => Ok(c::workbuddy_status().await),
        "switch_workbuddy_account" => Ok(c::switch_workbuddy_account(
            app.clone(),
            text("accountId").unwrap_or_default(),
        )
        .await),
        "account_overview" => Ok(c::account_overview(app.clone(), flag("refresh")).await),
        "events_list" => Ok(c::events_list(number("limit").map(|v| v as usize))),
        "events_clear" => Ok(c::events_clear(text("source").unwrap_or_default())),
        "switch_history_list" => Ok(c::switch_history_list(number("limit").map(|v| v as usize))),
        "auto_switch_status" => Ok(c::auto_switch_status()),
        "auto_switch_set" => Ok(c::auto_switch_set(
            args.get("patch").cloned().unwrap_or_else(|| json!({})),
        )),
        "auto_switch_check" => Ok(c::auto_switch_check()),
        "auto_switch_cancel" => Ok(c::auto_switch_cancel()),
        "auto_switch_pending" => Ok(c::auto_switch_pending()),
        "auto_switch_apply" => Ok(c::auto_switch_apply(app.clone()).await),
        "auto_switch_confirm" => Ok(c::auto_switch_confirm(app.clone()).await),
        "auto_switch_logs" => Ok(c::auto_switch_logs(number("limit").map(|v| v as usize))),
        "auto_switch_clear_logs" => Ok(c::auto_switch_clear_logs()),
        "account_groups_list" => Ok(c::account_groups_list()),
        "account_group_save" => c::account_group_save(
            text("id"),
            text("name").unwrap_or_default(),
            flag("excludeAutoSwitch"),
        ),
        "account_group_remove" => c::account_group_remove(text("id").unwrap_or_default()),
        "account_member_save" => c::account_member_save(
            text("accountId").unwrap_or_default(),
            list("groups").unwrap_or_default(),
            list("tags").unwrap_or_default(),
        ),
        "alerts_list" => Ok(c::alerts_list()),
        "alerts_dismiss" => Ok(c::alerts_dismiss(text("fingerprint").unwrap_or_default())),
        "alerts_dismiss_all" => Ok(c::alerts_dismiss_all()),
        "switch_preflight" => Ok(c::switch_preflight(text("accountId").unwrap_or_default()).await),
        "open_external" => Ok(c::open_external(text("url").unwrap_or_default())),
        "open_workbuddy_deeplink" => {
            Ok(c::open_workbuddy_deeplink(text("url").unwrap_or_default()))
        }
        "export_data" => Ok(c::export_data(
            text("kind").unwrap_or_default(),
            text("path").unwrap_or_default(),
        )),
        "export_diagnostics" => Ok(c::export_diagnostics(text("path").unwrap_or_default())),
        "export_all_data" => Ok(c::export_all_data(text("dir").unwrap_or_default())),
        "system_info" => Ok(c::system_info()),
        "open_store_dir" => Ok(c::open_store_dir()),
        "update_check" => Ok(c::update_check(flag("force").unwrap_or(false)).await),
        "update_download" => Ok(c::update_download(app.clone())),
        "update_restart" => Ok(c::update_restart(app.clone())),
        "update_config_get" => Ok(c::update_config_get()),
        "update_config_set" => Ok(c::update_config_set(
            flag("autoCheck").unwrap_or(true),
            text("mirror").unwrap_or_default(),
        )),
        "archived_tasks_stats" => Ok(c::archived_tasks_stats().await),
        "archived_tasks_list" => Ok(c::archived_tasks_list().await),
        "purge_archived_tasks" => Ok(c::purge_archived_tasks(app.clone()).await),
        "app_ready" => {
            // 面板里只做与窗口无关的幂等活：存量账号字段补齐（外部工具导入的记录
            // 可能只有 expires_at 秒级时间）。**绝不 show 窗口**——见文件头注释。
            let _ = std::panic::catch_unwind(wb_switch_core::modules::export_import::repair_store_fields);
            Ok(json!({ "ok": true, "embedded": true, "windowShown": false }))
        }
        other => Err(format!("未支持的内嵌命令: {other}")),
    }
}

#[cfg(test)]
mod tests {
    /// `app_ready` 的分支形状：必须返回 embedded 标记且不含窗口动作。
    /// 真实分发需要 AppHandle，这里只锁住「决定性字符串」不被改回去。
    #[test]
    fn app_ready_branch_is_documented_as_window_free() {
        let source = include_str!("ipc.rs");
        let branch = source
            .split(r#""app_ready" => {"#)
            .nth(1)
            .expect("app_ready 分支丢了");
        let body = branch.split("other => Err").next().unwrap_or(branch);
        assert!(body.contains("repair_store_fields"), "幂等活丢了");
        assert!(!body.contains("app_ready"), "分支里不该再转调 app_ready");
        assert!(!body.contains("show()"), "桥里不能显示窗口");
    }
}
