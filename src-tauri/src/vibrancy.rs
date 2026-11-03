//! 窗口背景特效：Windows acrylic / macOS vibrancy，其它平台直接返回不支持。
//! 只做「开 / 关」两态，失败不回滚也不 panic——效果属于装饰，不能影响主流程。
use serde_json::{json, Value};
use tauri::{Manager, WebviewWindow};

fn window(app: &tauri::AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

#[tauri::command]
pub fn set_backdrop(app: tauri::AppHandle, kind: String) -> Value {
    let Some(win) = window(&app) else {
        return json!({ "ok": false, "error": "main window not found" });
    };

    #[cfg(target_os = "windows")]
    {
        if kind == "off" {
            return match window_vibrancy::clear_acrylic(&win) {
                Ok(()) => json!({ "ok": true }),
                Err(err) => json!({ "ok": false, "error": err.to_string() }),
            };
        }
        // 深色半透明底：与界面底色一致，透出桌面但正文仍可读
        match window_vibrancy::apply_acrylic(&win, Some((10, 10, 10, 150))) {
            Ok(()) => json!({ "ok": true }),
            Err(err) => json!({ "ok": false, "error": err.to_string() }),
        }
    }

    #[cfg(target_os = "macos")]
    {
        use window_vibrancy::{NSVisualEffectMaterial, NSVisualEffectState};
        if kind == "off" {
            return match window_vibrancy::clear_vibrancy(&win) {
                Ok(()) => json!({ "ok": true }),
                Err(err) => json!({ "ok": false, "error": err.to_string() }),
            };
        }
        return match window_vibrancy::apply_vibrancy(
            &win,
            NSVisualEffectMaterial::HudWindow,
            Some(NSVisualEffectState::Active),
            None,
        ) {
            Ok(()) => json!({ "ok": true }),
            Err(err) => json!({ "ok": false, "error": err.to_string() }),
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = (&kind, &win);
        json!({ "ok": false, "error": "unsupported platform" })
    }
}
