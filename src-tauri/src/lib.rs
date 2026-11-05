//! 应用入口：窗口、系统托盘、关闭即最小化。
//!
//! 托盘行为（用户明确要求）：
//! - 关闭按钮 / Alt+F4 不退出，只隐藏窗口，程序继续在托盘常驻。
//! - 左键点托盘图标 = 显示窗口；右键弹快捷菜单（签到 / 日常 / 显示 / 退出）。
//! - 真正退出只能通过托盘菜单「退出」，避免误关导致后台任务中断。

mod commands;
mod executor;
mod export;
mod purge;
mod scheduler;
mod token_cache;
mod tray;
mod update;
mod vibrancy;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

/// 统一入口：把窗口叫回前台。
fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // 自动更新：签名校验 + NSIS 覆盖安装由插件负责，这里只提供清单与入口
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        // 窗口位置/大小记忆：默认存在 app config 目录
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::import_local_accounts,
            commands::account_detail,
            commands::account_balance,
            commands::total_balance,
            commands::start_login,
            commands::poll_login,
            commands::checkin_now,
            commands::run_status,
            commands::cancel_run,
            commands::run_checkin,
            commands::run_growth,
            commands::run_travel,
            commands::run_daily,
            commands::launch_main_app,
            commands::export_accounts_file,
            commands::preview_import_file,
            commands::import_accounts_file,
            commands::auto_daily_get,
            commands::auto_daily_set,
            commands::credit_statistics,
            commands::token_statistics,
            commands::account_delete,
            commands::account_set_note,
            commands::account_overview,
            commands::account_forecast,
            commands::workbuddy_status,
            commands::switch_workbuddy_account,
            commands::refresh_all_tokens,
            commands::events_list,
            commands::events_clear,
            commands::switch_history_list,
            commands::system_info,
            commands::open_store_dir,
            commands::open_external,
            commands::open_workbuddy_deeplink,
            commands::export_data,
            commands::export_all_data,
            commands::export_diagnostics,
            commands::auto_switch_status,
            commands::auto_switch_set,
            commands::auto_switch_logs,
            commands::auto_switch_apply,
            commands::auto_switch_cancel,
            commands::auto_switch_clear_logs,
            commands::auto_switch_check,
            commands::update_check,
            commands::update_install,
            commands::update_config_get,
            commands::update_config_set,
            commands::archived_tasks_stats,
            commands::archived_tasks_list,
            commands::purge_archived_tasks,
            commands::app_ready,
            vibrancy::set_backdrop,
        ])
        .setup(|app| {
            start_rate_limit_watcher(app.handle().clone());

            // 托盘菜单：快捷执行在上，退出压到最下面并与其他项分隔。
            let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let checkin = MenuItem::with_id(app, "checkin", "一键签到（全部账号）", true, None::<&str>)?;
            let daily = MenuItem::with_id(app, "daily", "执行全部日常", true, None::<&str>)?;
            let refresh = MenuItem::with_id(app, "refresh", "刷新窗口数据", true, None::<&str>)?;
            let update = MenuItem::with_id(app, "update", "检查更新", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出程序", true, None::<&str>)?;
            let sep_a = PredefinedMenuItem::separator(app)?;
            let sep_b = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(
                app,
                &[&show, &sep_a, &checkin, &daily, &refresh, &update, &sep_b, &quit],
            )?;

            let mut builder = TrayIconBuilder::with_id("main-tray")
                .tooltip("WorkBuddy 任务助手")
                .menu(&menu)
                // 左键留给「显示窗口」，右键才弹菜单
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "checkin" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = executor::execute("checkin", Vec::new()).await;
                            let _ = app.emit("tray://done", "签到");
                        });
                    }
                    "daily" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = executor::execute("daily", Vec::new()).await;
                            let _ = app.emit("tray://done", "全部日常");
                        });
                    }
                    "refresh" => {
                        // 前端监听后重新拉账号与详情
                        let _ = app.emit("tray://refresh", ());
                    }
                    "update" => {
                        // 托盘手动检查：结果走事件，前端据此点亮标题栏更新标记
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let status = update::check(true).await;
                            let _ = app.emit("update://checked", status);
                            show_main(&app);
                        });
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main(tray.app_handle());
                    }
                });

            if let Some(icon) = app.default_window_icon() {
                builder = builder.icon(icon.clone());
            }
            builder.build(app)?;

            // 启动即给托盘一个真读数（而不是默认的「任务助手」）：
            // 常驻用户多数时间不开窗，tooltip 就是他对余额的唯一感知。
            tray::refresh(app.handle());

            // 限流巡检：默认关闭，开启后每 2 分钟判定一次
            commands::spawn_auto_switch(app.handle().clone());

            // 每日定时调度：常驻期间后台自转，到点自动跑「全部日常」
            scheduler::spawn(app.handle().clone());

            // 自动更新轮询：启动 8s 后首查，之后每 6 小时（可在设置里关闭）
            update::spawn_poller(app.handle().clone());
            Ok(())
        })
        // 关闭即隐藏：保留后台执行与托盘，不销毁 WebView（下次显示无需重建）
        .on_window_event(|window, event| {
            // 只拦主窗口：启动画面与其它辅助窗口必须能真正关闭，
            // 否则启动窗会一直压在主界面上（曾因这条全局拦截复现）。
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        // 调试/截图辅助：WB_START_NAV=tasks 等让窗口直达指定分区。
        // 前端监听 hashchange，这里只改 hash，不 reload（幂等比较防重复触发）。
        .on_page_load(|webview, _payload| {
            if let Ok(nav) = std::env::var("WB_START_NAV") {
                let _ = webview.eval(format!(
                    "if (window.location.hash !== '#{n}') window.location.hash = '#{n}';",
                    n = nav
                ));
            }
        })
        .run(tauri::generate_context!())
        .expect("启动 WorkBuddy 任务助手失败");
}


/// 限流实时响应：走 hook 事件文件的 1 秒 stat 快路径，**不扫日志目录**。
///
/// 为什么不用文件系统监听（notify）：日志目录按天分文件夹、活跃期每秒多次写入，
/// 递归监听会持续产生事件与上下文切换，对一台要长期挂机的工具来说不划算。
/// 现有 `rate_limit_events::spawn_watcher` 每秒只做一次 `stat`（没有新字节直接返回），
/// 命中新事件才解析——空闲开销基本为零，延迟 1 秒，足够。
///
/// 60s 兜底轮询保留：hook 未安装时日志扫描路径仍能捞出限流，只是慢一点。
fn start_rate_limit_watcher(app: tauri::AppHandle) {
    let app_for_hook = app.clone();
    wb_switch_core::modules::rate_limit_events::spawn_watcher(move || {
        let app = app_for_hook.clone();
        tauri::async_runtime::spawn(async move {
            let payload = commands::auto_switch_apply(app).await;
            if payload.get("action").and_then(|value| value.as_str()) == Some("switch") {
                eprintln!("[rate-watch] {payload}");
            }
        });
    });

    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            wb_switch_core::modules::limits::invalidate_scan_cache();
            let _ = commands::auto_switch_apply(app.clone()).await;
        }
    });
}
