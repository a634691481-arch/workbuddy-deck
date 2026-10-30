//! 「成长计划任务」真实账号只读联调（**默认 ignored**，手动运行）。
//!
//! 只调 `GET /v2/activity/growth/tasks`，不接取、不领奖、不发任何埋点，
//! 因此对账号零副作用。用于验证域名 / 鉴权头 / 响应解析。
//!
//! 运行前把家目录沙箱化（避免触碰真实 `~/.wb-switch`）：
//!
//! ```bash
//! export WB_SWITCH_HOME=$(mktemp -d)
//! mkdir -p "$WB_SWITCH_HOME/.wb-switch"
//! cp ~/.wb-switch/accounts.json "$WB_SWITCH_HOME/.wb-switch/"
//! cargo test -p wb-switch-core --test growth_status_live -- --ignored --nocapture
//! ```

use wb_switch_core::modules::{account, growth_tasks};

#[tokio::test]
#[ignore = "手动联调：需要 WB_SWITCH_HOME 指向含真实账号的沙箱目录"]
async fn live_growth_status_all_accounts() {
    let accounts = account::load_accounts();
    assert!(
        !accounts.is_empty(),
        "沙箱里没有账号：先把 accounts.json 复制到 $WB_SWITCH_HOME/.wb-switch/"
    );

    let result = growth_tasks::growth_status(None).await;
    println!("{}", serde_json::to_string_pretty(&result).unwrap());

    let rows = result["accounts"].as_array().expect("accounts 必须是数组");
    assert!(!rows.is_empty(), "没有账号参与");

    for row in rows {
        let email = row["email"].as_str().unwrap_or("");
        let total = row["total"].as_i64().unwrap_or(0);
        let done = row["done"].as_i64().unwrap_or(0);
        assert!(
            total > 0,
            "账号 {email} 任务清单为空：token 失效或接口变更，需人工确认"
        );
        assert!(done <= total, "账号 {email} 进度异常：{done}/{total}");
        let codes: Vec<&str> = row["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t.get("task_code").and_then(|c| c.as_str()))
            .collect();
        println!("  {email}: {done}/{total} · {codes:?}");
    }
}
