//! 启动画面：先弹 480×300 无边框小窗播开机动画，主窗口就绪后切过去。
//!
//! 切换由**两个信号**共同决定，缺一不可：
//! - `splash_done`：启动页自己播完动画（页面时钟才准，窗口创建时刻不准——
//!   WebView2 冷启动会晚 0.6–1.5s，用固定计时会把动画切在半截）。
//! - `app_ready`：主窗口前端已挂载并画完首帧，避免先白后黑地闪一下。
//!
//! 另有 `SPLASH_MAX` 兜底：任一信号丢失（前端崩溃/页面报错）时也必须放行，
//! 否则用户永远停在启动画面上，比没有启动画面更糟。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// 兜底上限：两个信号任何一个没到，也必须切主窗口。
const SPLASH_MAX: Duration = Duration::from_millis(10_000);

static MAIN_READY: AtomicBool = AtomicBool::new(false);
static SPLASH_DONE: AtomicBool = AtomicBool::new(false);

/// 建启动窗口，并挂一个兜底计时器。
pub fn open(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, "splash", WebviewUrl::App("splash.html".into()))
        .title("WorkBuddy Deck")
        .inner_size(480.0, 300.0)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .center()
        .build()?;

    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SPLASH_MAX);
        finish(&handle);
    });
    Ok(())
}

/// 切主窗口并关掉启动窗。幂等：兜底与正常路径可能都走到这里。
pub fn finish(app: &AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
    }
    if let Some(splash) = app.get_webview_window("splash") {
        // 用 destroy 而不是 close：close 会走 CloseRequested，
        // 而主窗口的「关闭即隐藏」逻辑是按窗口事件统一处理的，容易误拦。
        let _ = splash.destroy();
    }
}

fn finish_when_ready(app: &AppHandle) {
    if MAIN_READY.load(Ordering::SeqCst) && SPLASH_DONE.load(Ordering::SeqCst) {
        finish(app);
    }
}

/// 主窗口就绪。
pub fn ready(app: &AppHandle) {
    MAIN_READY.store(true, Ordering::SeqCst);
    finish_when_ready(app);
}

/// 启动页动画播完。
pub fn done(app: &AppHandle) {
    SPLASH_DONE.store(true, Ordering::SeqCst);
    finish_when_ready(app);
}
