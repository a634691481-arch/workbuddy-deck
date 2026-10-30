//! 「成长中心」真实账号**只读**联调（**默认 ignored**，手动运行）。
//!
//! 只发 GET：`/streak`、`/heatmap`、`/redeem/summary`、`/lottery/chances`、
//! `/buddy/quota`、`/energy`。不补签、不兑换、不抽奖、不开盒，因此零副作用。
//!
//! ```bash
//! export WB_SWITCH_HOME=$(mktemp -d)
//! mkdir -p "$WB_SWITCH_HOME/.wb-switch"
//! cp ~/.wb-switch/accounts.json "$WB_SWITCH_HOME/.wb-switch/"
//! cargo test -p wb-switch-core --test growth_center_readonly_live -- --ignored --nocapture
//! ```

use wb_switch_core::modules::{account, growth_center};

#[tokio::test]
#[ignore = "手动联调：需要 WB_SWITCH_HOME 指向含真实账号的沙箱目录"]
async fn live_growth_center_status() {
    let accounts = account::load_accounts();
    assert!(
        !accounts.is_empty(),
        "沙箱里没有账号：先把 accounts.json 复制到 $WB_SWITCH_HOME/.wb-switch/"
    );

    let result = growth_center::growth_center_status(None).await;
    let rows = result["accounts"].as_array().expect("accounts 必须是数组");
    assert!(!rows.is_empty(), "没有账号参与");

    let mut broken: Vec<String> = Vec::new();
    for row in rows {
        let email = row["email"].as_str().unwrap_or("");
        if row["ok"].as_bool() != Some(true) {
            // 加密信封账号在本机无法解密属预期，单独标注，不算接口问题。
            println!(
                "  [skip] {email}: {} {}",
                row["reason"].as_str().unwrap_or(""),
                row["error"].as_str().unwrap_or("")
            );
            continue;
        }
        println!(
            "  {email}: 连签 {} 天 · 补签卡 {} · 缺口 {:?} · 可兑换 {:?} · 抽奖 {} 次 · 能量 {} · Buddy 可开 {}",
            row["streakDays"], row["makeupCards"], row["gaps"], row["redeemable"],
            row["lotteryChances"], row["energy"], row["buddyAffordable"]
        );
        for key in ["streak", "heatmap", "redeem", "lottery", "buddy", "energy"] {
            let node = &row["raw"][key];
            if node["ok"].as_bool() != Some(true) {
                broken.push(format!(
                    "{email} {key}: HTTP {} code={} {}",
                    node["status"], node["code"], node["message"]
                ));
            }
        }
    }

    assert!(
        broken.is_empty(),
        "以下只读接口失败（多半是路径前缀不对）：\n{}",
        broken.join("\n")
    );
}
