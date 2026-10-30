//! 切换外层包装的端到端验证：**失败也必须留下一条台账**。
//!
//! 只覆盖「账号不存在」这条最轻的失败路径：它不关进程、不写认证、不碰会话，
//! 任何机器上都能跑，但正好穿过外层包装的全部关键动作——
//! 抓取切换前身份 → 调编排 → 按结果投影 result/stage/mode → 落盘。
//!
//! 这条链路如果断了，用户切换失败后日志里什么都查不到，只能看到一句 toast。

use wb_switch_core::modules::switch;
use wb_switch_core::modules::switch_history;

struct TempHome(std::path::PathBuf);

impl TempHome {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "wb-switch-switch-flow-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 账号不存在 → Err，且台账里有一条 `blocked / start` 的记录。
///
/// `WB_SWITCH_HOME` 是进程级变量：本文件只放这一个测试，避免并行写同一变量。
#[test]
fn failed_switch_leaves_a_blocked_record() {
    let home = TempHome::new();
    // SAFETY: 本测试独占该环境变量（文件内仅此一个用例，且不与其他用例并发读写它）。
    unsafe {
        std::env::set_var("WB_SWITCH_HOME", &home.0);
    }

    let error = switch::switch_account(None, "no-such-account", false, false, &[], &[], false)
        .expect_err("账号不存在时必须失败，不能静默成功");
    assert!(error.contains("账号不存在"), "错误文案要能定位原因：{error}");

    let payload = switch_history::load(0);
    assert_eq!(payload["ok"], true, "台账读取应当成功：{payload}");
    let entries = payload["entries"]
        .as_array()
        .expect("台账 entries 必须是数组");
    assert_eq!(entries.len(), 1, "失败也必须留一条台账，否则事后无从回溯");

    let entry = &entries[0];
    assert_eq!(entry["result"], "blocked", "未写认证的失败记为 blocked");
    assert_eq!(entry["stage"], "start", "没进到任何写入阶段就失败");
    assert_eq!(entry["mode"], "plain", "无会话写入意图");
    assert_eq!(entry["accountId"], "no-such-account");
    assert_eq!(entry["identity"], "", "没走到写后校验，身份为空");
    assert!(
        entry["error"].as_str().unwrap_or("").contains("账号不存在"),
        "台账要带原始原因：{entry}"
    );

    // SAFETY: 同上去。
    unsafe {
        std::env::remove_var("WB_SWITCH_HOME");
    }
}
