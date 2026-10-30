//! 后台常驻任务的 panic 守护。
//!
//! tokio/tauri 的任务 panic 只会终止该任务本身，不会带崩进程，但**该能力会永久消失**
//! 且没有任何提示（例如签到循环在某一轮 panic 后再也不会执行）。这里统一包一层：
//! panic 记录到 stderr 并按固定间隔重启，重启后由 `factory` 重新构造循环，局部状态
//! 不复用（避免在已中毒的中间状态上继续跑）。

use std::future::Future;
use std::time::Duration;

/// 任务异常退出后的重启间隔。
pub const RESTART_DELAY: Duration = Duration::from_secs(60);

/// 守护一个永不返回的后台循环；`name` 只用于日志定位。
pub async fn run_supervised<F, Fut>(name: &'static str, factory: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    run_supervised_with_delay(name, RESTART_DELAY, factory).await;
}

/// 同 [`run_supervised`]，可指定重启间隔（测试注入短延迟）。
pub async fn run_supervised_with_delay<F, Fut>(name: &'static str, delay: Duration, mut factory: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    loop {
        match tokio::spawn(factory()).await {
            // 正常返回：当前所有循环都是死循环，走到这里说明实现被改成可退出的，
            // 不再重启（避免把「正常退出」误当成故障无限拉起）。
            Ok(()) => return,
            Err(error) => {
                let reason = if error.is_panic() {
                    let payload = error.into_panic();
                    payload
                        .downcast_ref::<&str>()
                        .map(|text| (*text).to_string())
                        .or_else(|| payload.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "未知 panic".to_string())
                } else {
                    "任务被取消".to_string()
                };
                eprintln!(
                    "[supervisor] 后台任务 {name} 异常退出（{reason}），{} 秒后重启",
                    delay.as_secs()
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// 首轮 panic 后必须被重启，且第二轮的局部状态是全新的。
    #[tokio::test]
    async fn panicking_task_is_restarted_with_fresh_state() {
        let runs = Arc::new(AtomicUsize::new(0));
        let runs_for_factory = runs.clone();
        run_supervised_with_delay("test-panic-restart", Duration::from_millis(1), move || {
            let runs = runs_for_factory.clone();
            async move {
                let run = runs.fetch_add(1, Ordering::SeqCst);
                let mut local = 0u32;
                local += 1;
                if run == 0 {
                    panic!("第一轮故意 panic");
                }
                // 本地状态每轮重建：第二轮仍是 1（若复用状态会读到 2）。
                assert_eq!(local, 1);
            }
        })
        .await;
        assert_eq!(runs.load(Ordering::SeqCst), 2, "panic 后必须重启一次");
    }

    /// 正常返回的不重启（防止把可退出循环无限拉起）。
    #[tokio::test]
    async fn clean_exit_is_not_restarted() {
        let runs = Arc::new(AtomicUsize::new(0));
        let runs_for_factory = runs.clone();
        run_supervised_with_delay("test-clean-exit", Duration::from_millis(1), move || {
            let runs = runs_for_factory.clone();
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
            }
        })
        .await;
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }
}
