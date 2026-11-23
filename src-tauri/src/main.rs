// Windows 下不弹控制台窗口（GUI 子系统），与 release 行为一致。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // WebView2 默认走系统代理，会把 tauri.localhost 的自定义协议请求也送进代理
    // （Fiddler/Clash 等工具一开就「localhost 拒绝连接」白屏）。本应用所有资源都在本机，
    // 直接禁代理；该变量只影响本进程的 WebView2，不动用户系统代理。
    if std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none() {
        std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--no-proxy-server");
    }
    task_assistant_lib::run()
}
