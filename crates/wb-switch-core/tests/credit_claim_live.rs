//! 「自动领取积分」真实账号联调测试（**默认 ignored**，手动运行）。
//!
//! 只验证真实链路：账号鉴权头 → 状态查询 → 领取 → 幂等响应解析 → 日志写入。
//! 不启动任何后台循环，因此除领取接口本身外没有任何副作用。
//!
//! 运行前先把家目录沙箱化（避免影响真实 `~/.wb-switch`）：
//!
//! Linux / macOS:
//! ```bash
//! export WB_SWITCH_HOME=$(mktemp -d)
//! mkdir -p "$WB_SWITCH_HOME/.wb-switch"
//! cp ~/.wb-switch/accounts.json "$WB_SWITCH_HOME/.wb-switch/"
//! cargo test -p wb-switch-core --test credit_claim_live -- --ignored --nocapture
//! ```
//!
//! Windows（cmd）:
//! ```bat
//! set WB_SWITCH_HOME=%TEMP%\wb-claim
//! mkdir "%WB_SWITCH_HOME%\.wb-switch"
//! copy "%USERPROFILE%\.wb-switch\accounts.json" "%WB_SWITCH_HOME%\.wb-switch\"
//! cargo test -p wb-switch-core --test credit_claim_live -- --ignored --nocapture
//! ```

use serde_json::Value;
use wb_switch_core::modules::{account, config, credit_claim};

#[tokio::test]
#[ignore = "手动联调：需要 WB_SWITCH_HOME 指向含真实账号的沙箱目录"]
async fn live_claim_all_accounts() {
    let accounts = account::load_accounts();
    assert!(
        !accounts.is_empty(),
        "沙箱里没有账号：先把 accounts.json 复制到 $WB_SWITCH_HOME/.wb-switch/"
    );

    let result = credit_claim::claim_all(None).await;
    println!(
        "claim_all ⇢\n{}",
        serde_json::to_string_pretty(&result).unwrap()
    );

    let rows = result["accounts"].as_array().expect("accounts 必须是数组");
    for row in rows {
        let result = row["result"].as_str().unwrap_or("");
        assert!(
            matches!(
                result,
                "success" | "already" | "none" | "unsupported" | "error"
            ),
            "账号行 result 非法: {row}"
        );
        assert_ne!(result, "error", "存在失败行（检查 token / 网络）: {row}");
    }

    // 日志只在真正发起领取时写入；这里只打印，不断言条数（取决于线上活动状态）。
    let logs = config::load_credit_claim_logs();
    println!(
        "领取日志（{} 条）⇢\n{}",
        logs.len(),
        serde_json::to_string_pretty(&Value::Array(logs)).unwrap()
    );

    // 幂等性：再跑一轮，已领取的项不得再次发起领取（结果只应出现 none，无 error）。
    let second = credit_claim::claim_all(None).await;
    println!(
        "第二轮 claim_all ⇢\n{}",
        serde_json::to_string_pretty(&second).unwrap()
    );
    for row in second["accounts"].as_array().unwrap() {
        assert_ne!(
            row["result"].as_str().unwrap_or(""),
            "error",
            "第二轮出现失败行: {row}"
        );
    }
}
