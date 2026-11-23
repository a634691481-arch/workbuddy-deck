//! 启动器前置校验：本 exe 不是原软件的替代品，角色是「启动器 + 注入器」。
//!
//! 双击 exe 后三步（对应 2026-11-21 打包分发需求）：
//! 1. 查原软件：读注册表（App Paths / Uninstall，含 WOW6432Node）+ 常见目录，
//!    确认用户安装了官方客户端 —— 没装弹窗「请先安装官方原版软件」并退出；
//! 2. 检查运行状态：官方软件正在运行 → 弹窗提示先关闭（否则无法替换或注入），
//!    用户确认后自动关闭并以调试端口重启；
//! 3. 未运行 → 直接附加 `--remote-debugging-port=9222` 拉起原软件，
//!    CDP 连接与 UI 注入交给 cdp.rs 的 widget 守护进程（20s 探活重连）。
//!
//! 仅 Windows 生效（安装向导目标用户是 Windows）；macOS/Linux 静默跳过，
//! 走原有流程。

use tauri::AppHandle;
use wb_switch_core::modules::process;
use wb_switch_core::modules::variant::WbVariant;

const DEFAULT_PORT: u16 = 9222;

#[cfg(target_os = "windows")]
pub fn spawn_preflight(app: AppHandle) {
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = run(&app) {
            eprintln!("[deck] 启动前置校验未通过: {error}");
            // 前置校验失败 = 不具备启动条件，直接退进程；
            // 走 app.exit() 会被 headless 的 prevent_exit 拦截。
            std::process::exit(0);
        }
    });
}

#[cfg(not(target_os = "windows"))]
pub fn spawn_preflight(_app: AppHandle) {}

#[cfg(target_os = "windows")]
fn run(app: &AppHandle) -> Result<(), String> {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

    let variant = WbVariant::Cn;

    // 1) 官方客户端存在性：运行中进程 Path → 缓存 → 注册表 → 常见目录全扫一遍。
    //    探测顺序复用 process 模块，与切号/签到用的路径口径一致。
    if process::windows_workbuddy_exe_path(variant).is_none() {
        app.dialog()
            .message("未检测到官方原版软件（WorkBuddy）。\n\n请先安装官方原版软件，再运行本程序。")
            .title("WorkBuddy 任务助手")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCustom("确定".into()))
            .blocking_show();
        return Err("官方客户端未安装".into());
    }

    // 2) 运行状态：在跑 → 提示先关闭；确认后代为关闭并带调试端口重启。
    if process::is_workbuddy_running(variant) {
        let proceed = app
            .dialog()
            .message("检测到官方软件正在运行。\n\n需要先关闭官方软件才能替换或注入。点击「关闭并继续」将自动关闭官方软件并以调试端口重启；点击「退出」结束本程序。")
            .title("检测到官方软件正在运行")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "关闭并继续".into(),
                "退出".into(),
            ))
            .blocking_show();
        if !proceed {
            return Err("用户选择退出（官方软件运行中）".into());
        }
        // 关旧进程 + 带参拉起，最长等 15s；失败原因写日志后退出。
        return process::restart_workbuddy_with_cdp(variant, DEFAULT_PORT, 15)
            .map_err(|error| format!("重启官方软件失败: {error}"));
    }

    // 3) 未运行：启动器职责 —— 直接带调试端口拉起，注入由 widget 守护接管。
    process::launch_workbuddy_args(
        variant,
        &[format!("--remote-debugging-port={DEFAULT_PORT}")],
        None,
    )
    .map_err(|error| format!("拉起官方软件失败: {error}"))
}
