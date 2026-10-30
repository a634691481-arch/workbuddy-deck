// Windows 下不弹控制台窗口（GUI 子系统），与 release 行为一致。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    task_assistant_lib::run()
}
