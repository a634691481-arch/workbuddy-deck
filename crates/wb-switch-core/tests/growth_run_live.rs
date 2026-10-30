//! 单账号成长任务**真实执行**联调（**默认 ignored**，手动运行）。
//!
//! 会真的接取任务、发对话、上报埋点、领奖——只对沙箱里的**第一个账号**执行，
//! 用于验证新配方的进度闭环（跑完再拉一次清单对比进度）。
//!
//! ```bash
//! export WB_SWITCH_HOME=$(mktemp -d)
//! mkdir -p "$WB_SWITCH_HOME/.wb-switch"
//! cp ~/.wb-switch/accounts.json "$WB_SWITCH_HOME/.wb-switch/"
//! cargo test -p wb-switch-core --test growth_run_live -- --ignored --nocapture
//! ```

use wb_switch_core::modules::{account, config, growth_tasks};

/// 关注公众号需要微信真实关注满 24 小时，无法自动化，执行后仍会停留 0/1。
const MANUAL_TASKS: [&str; 1] = ["wb_wechat_oa_subscribe_task"];

/// 只读探测：专家搜索三件套（template_id / tag_id / keyword）怎么凑齐。
#[tokio::test]
#[ignore = "手动联调：只读，需要 WB_SWITCH_HOME 指向含真实账号的沙箱目录"]
async fn live_probe_expert_search() {
    let accounts = account::load_accounts();
    let Some(acc) = accounts.first() else {
        panic!("沙箱里没有账号");
    };
    let api = wb_switch_core::modules::growth_tasks::raw_post_pub;

    // 0b) 应用授权全链路：status → code → consent → usage report
    let apps2 = wb_switch_core::modules::growth_tasks::raw_get_pub(
        acc,
        "/api/v1/open-platform/buddy_application?limit=10",
    )
    .await;
    if let Some(app) = apps2["data"]["applications"]
        .as_array()
        .or_else(|| apps2["data"].as_array())
        .and_then(|a| a.first())
    {
        let app_id = app.get("application_id").and_then(|v| v.as_str()).unwrap_or("");
        let app_name = app.get("application_name").and_then(|v| v.as_str()).unwrap_or("");
        println!("AUTH_FLOW app={app_id} name={app_name}");
        let status = wb_switch_core::modules::growth_tasks::raw_get_pub(
            acc,
            &format!("/workbuddyopen/v1/authorize/status?application_id={app_id}"),
        )
        .await;
        let redirect_uri = status["application"]["redirect_uris"]
            .as_array()
            .and_then(|u| u.first())
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string();
        let scopes: Vec<String> = status["application"]["allowed_scopes"]
            .as_array()
            .map(|s| {
                s.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let authorized = status["authorized"]
            .as_bool()
            .or_else(|| status["data"]["authorized"].as_bool())
            .unwrap_or(false);
        println!("  STATUS authorized={authorized} redirect={redirect_uri:?} scopes={scopes:?}");
        let code = api(
            acc,
            "/workbuddyopen/v1/authorize/code",
            serde_json::json!({"application_id": app_id, "scene": "client", "redirect_uri": redirect_uri}),
        )
        .await;
        println!("  CODE: {}", serde_json::to_string(&code).unwrap_or_default().chars().take(300).collect::<String>());
        if code.get("code").and_then(|v| v.as_i64()) != Some(0)
            && code.get("error").is_some()
        {
            // 未授权 → consent 授权
            let consent = api(
                acc,
                "/workbuddyopen/v1/authorize/consent",
                serde_json::json!({
                    "application_id": app_id,
                    "redirect_uri": redirect_uri,
                    "approve": true,
                    "granted_scopes": scopes,
                }),
            )
            .await;
            println!("  CONSENT: {}", serde_json::to_string(&consent).unwrap_or_default().chars().take(300).collect::<String>());
        }
        let usage = api(
            acc,
            "/api/v2/open-platform/asset-usage/report",
            serde_json::json!({"asset_id": app_id, "asset_type": "buddy_app"}),
        )
        .await;
        println!("  USAGE: {}", serde_json::to_string(&usage).unwrap_or_default().chars().take(200).collect::<String>());
    }

    // 0) Buddy 应用列表（发现应用任务的 buddyId 来源）：GET 带 scene 参数试探
    for scene in ["discover", "recommend", ""] {
        let q = if scene.is_empty() {
            "/api/v1/open-platform/buddy_application?limit=10".to_string()
        } else {
            format!("/api/v1/open-platform/buddy_application?limit=10&scene={scene}")
        };
        let resp = wb_switch_core::modules::growth_tasks::raw_get_pub(acc, &q).await;
        let code = resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        let msg = resp
            .get("msg")
            .or_else(|| resp.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let apps = resp["data"]["applications"]
            .as_array()
            .or_else(|| resp["data"].as_array())
            .cloned()
            .unwrap_or_default();
        println!("APPS_LIST scene={scene:?} code={code} msg={msg} n={}", apps.len());
        for app in apps.iter().take(4) {
            println!(
                "  APP id={:?} name={:?} keys={:?}",
                app.get("id").or_else(|| app.get("application_id")),
                app.get("name").or_else(|| app.get("display_name")),
                app.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default(),
            );
        }
    }

    // 1) featured：完整原始响应落盘，全文搜目标专家
    let featured = api(acc, "/api/v2/open-platform/featured", serde_json::json!({})).await;
    let raw_text = serde_json::to_string_pretty(&featured).unwrap_or_default();
    std::fs::write(std::env::temp_dir().join("wb-featured-raw.json"), &raw_text).ok();
    for kw in ["无畏", "拼豆", "valorant", "Valorant"] {
        println!("FEATURED_CONTAINS {kw} → {}", raw_text.contains(kw));
    }
    if let Some(scenes) = featured["data"]["scenes"].as_array() {
        for (i, scene) in scenes.iter().enumerate() {
            let mut obj = scene.clone();
            obj.as_object_mut().map(|o| o.remove("experts"));
            let names: Vec<String> = scene["experts"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|e| {
                            e.get("profession_zh")
                                .or_else(|| e.get("display_name_zh"))
                                .and_then(|v| v.as_str())
                                .map(String::from)
                        })
                        .collect()
                })
                .unwrap_or_default();
            println!("scene[{i}] id={:?} experts={names:?}", scene.get("id"));
        }
    }

    // 2) tags 接口：不同 template_id 猜测，看哪种能拿到 tag 树
    for tid in ["", "workbuddy", "default", "0", "expert", "all"] {
        let resp = api(
            acc,
            "/api/v2/open-platform/tags",
            serde_json::json!({"template_id": tid, "type": "domain", "sub_type": "expert"}),
        )
        .await;
        let code = resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        let msg = resp.get("msg").or_else(|| resp.get("message")).and_then(|v| v.as_str()).unwrap_or("");
        let roots = resp["data"]["roots"].as_array().map(|r| r.len()).unwrap_or(0);
        println!("TAGS tid={tid:?} code={code} msg={msg} roots={roots}");
        if code == 0 && roots > 0 {
            println!("TAGS_TREE: {}", serde_json::to_string(&resp["data"]).unwrap_or_default().chars().take(2000).collect::<String>());
            break;
        }
    }

    // 3) search 接口：tag_id 用 "all" 与 featured scene id 试
    for tag_id in ["all", "expert", ""] {
        let resp = api(
            acc,
            "/api/v2/open-platform/experts/search",
            serde_json::json!({"template_id": "", "tag_id": tag_id, "keyword": "无畏契约", "page": 1, "page_size": 5}),
        )
        .await;
        let code = resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        let msg = resp.get("msg").or_else(|| resp.get("message")).and_then(|v| v.as_str()).unwrap_or("");
        println!("SEARCH tag_id={tag_id:?} code={code} msg={msg} data={}", serde_json::to_string(&resp["data"]).unwrap_or_default().chars().take(400).collect::<String>());
    }

    // 4) 应用列表 → 真实 application id 当 template_id，走通 tags → search
    let mut list = wb_switch_core::modules::growth_tasks::raw_get_pub(
        acc,
        "/api/v1/open-platform/buddy_application",
    )
    .await;
    if list.get("code").and_then(|v| v.as_i64()) != Some(0) {
        // GET 不行试 POST，再不行试 detail_full
        for (path, body) in [
            ("/api/v1/open-platform/buddy_application", serde_json::json!({})),
            ("/api/v1/open-platform/buddy_application/search", serde_json::json!({"keyword": "", "page": 1, "page_size": 20})),
            ("/api/v2/open-platform/buddy_application/detail_full", serde_json::json!({})),
        ] {
            let r = api(acc, path, body).await;
            let c = r.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
            println!("APP_TRY {path} code={c} data={}", serde_json::to_string(&r["data"]).unwrap_or_default().chars().take(500).collect::<String>());
            if c == 0 {
                list = r;
                break;
            }
        }
    }
    println!("APP_LIST code={} data={}", list.get("code").and_then(|v| v.as_i64()).unwrap_or(-1),
        serde_json::to_string(&list["data"]).unwrap_or_default().chars().take(800).collect::<String>());
    let mut app_ids: Vec<String> = Vec::new();
    fn collect_strs(v: &serde_json::Value, keys: &[&str], out: &mut Vec<String>) {
        if let Some(obj) = v.as_object() {
            for (k, val) in obj {
                if keys.contains(&k.as_str()) {
                    if let Some(s) = val.as_str() {
                        if !s.is_empty() && !out.contains(&s.to_string()) {
                            out.push(s.to_string());
                        }
                    }
                }
                collect_strs(val, keys, out);
            }
        } else if let Some(arr) = v.as_array() {
            for item in arr {
                collect_strs(item, keys, out);
            }
        }
    }
    collect_strs(&list, &["application_id", "appId", "id", "template_id"], &mut app_ids);
    println!("APP_IDS = {app_ids:?}");
    // 5) 个人账号真实搜索端点：/v2/operation-platform/market/expert/list（完整落盘）
    for kw in ["无畏契约", "抖音策略师", "直播电商教练"] {
        let resp = api(
            acc,
            "/v2/operation-platform/market/expert/list",
            serde_json::json!({"page": 1, "page_size": 10, "keyword": kw, "sort_by": "reco_rank", "sort_order": "desc"}),
        )
        .await;
        let text = serde_json::to_string_pretty(&resp).unwrap_or_default();
        let file = std::env::temp_dir().join(format!("wb-market-{}.json", kw));
        std::fs::write(&file, &text).ok();
        let ids: Vec<String> = resp["data"]["experts"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|e| {
                        e.get("id")
                            .or_else(|| e.get("expert_id"))
                            .and_then(|v| v.as_str())
                            .map(String::from)
                    })
                    .collect()
            })
            .unwrap_or_default();
        println!("MARKET kw={kw} code={} ids={ids:?} file={}", resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1), file.display());
    }
}

#[tokio::test]
#[ignore = "手动联调：会真实执行任务，需要 WB_SWITCH_HOME 指向含真实账号的沙箱目录"]
async fn live_run_growth_first_account() {
    let accounts = account::load_accounts();
    let Some(acc) = accounts.first() else {
        panic!("沙箱里没有账号");
    };

    let before = growth_tasks::growth_status(Some(account::variant_of(&acc))).await;
    let code_progress = |status: &serde_json::Value, code: &str| -> String {
        status["accounts"]
            .as_array()
            .and_then(|rows| rows.first())
            .and_then(|row| row["tasks"].as_array())
            .and_then(|tasks| {
                tasks
                    .iter()
                    .find(|t| t["task_code"].as_str() == Some(code))
                    .map(|t| {
                        format!(
                            "{}/{} {}",
                            t["progress"]["current"], t["progress"]["target"], t["accept_status"]
                        )
                    })
            })
            .unwrap_or_else(|| "无".to_string())
    };

    let watch = [
        "Expert_lighthouse",
        "WorkBuddy_Valorant",
        "expert_use_maidehao",
        "expert_use_douyinyin",
        "expert_use_shangjia_xuanchuan",
        "expert_use_bowangwang",
    ];
    println!("== 执行前 ==");
    for code in watch {
        println!("  {code}: {}", code_progress(&before, code));
    }

    let result = growth_tasks::run_growth_account(&acc, &config::load_growth_config()).await;
    println!("== 执行结果 ==");
    println!("{}", serde_json::to_string_pretty(&result).unwrap());

    let after = growth_tasks::growth_status(Some(account::variant_of(&acc))).await;
    println!("== 执行后 ==");
    for code in watch {
        println!("  {code}: {}", code_progress(&after, code));
    }
    for code in MANUAL_TASKS {
        println!("  {code}: {}（需手动）", code_progress(&after, code));
    }
}

/// **全账号**真实执行一轮成长任务（接取→补缺口→领奖），逐号汇报 6 个专家任务结果。
#[tokio::test]
#[ignore = "手动联调：会对沙箱里全部账号真实执行，需要 WB_SWITCH_HOME 指向含真实账号的沙箱目录"]
async fn live_run_growth_all_accounts() {
    let accounts = account::load_accounts();
    println!("共 {} 个账号，逐号执行（串行）", accounts.len());

    let watch = [
        "Expert_lighthouse",
        "WorkBuddy_Valorant",
        "expert_use_maidehao",
        "expert_use_douyinyin",
        "expert_use_shangjia_xuanchuan",
        "expert_use_bowangwang",
    ];

    for (idx, acc) in accounts.iter().enumerate() {
        let label = acc
            .get("nickname")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| format!("账号#{idx}"));
        println!("\n===== [{}/{}] {} =====", idx + 1, accounts.len(), label);

        let result = growth_tasks::run_growth_account(acc, &config::load_growth_config()).await;
        println!(
            "  结果: {} | 领奖 {} 个 | 本轮积分 +{}",
            result["result"].as_str().unwrap_or("?"),
            result["claimed"].as_array().map(|c| c.len()).unwrap_or(0),
            result["credit"].as_i64().unwrap_or(0),
        );
        for task in result["tasks"].as_array().cloned().unwrap_or_default() {
            let code = task["code"].as_str().unwrap_or("?");
            if !watch.contains(&code) {
                continue;
            }
            println!(
                "  {code}: {}/{} {}{}",
                task["current"].as_i64().unwrap_or(0),
                task["target"].as_i64().unwrap_or(0),
                task["status"].as_str().unwrap_or(""),
                if task["claimed"].as_bool() == Some(true) {
                    format!("（+{}积分）", task["credit"].as_i64().unwrap_or(0))
                } else if let Some(err) = task["error"].as_str() {
                    format!("（领奖失败：{err}）")
                } else {
                    String::new()
                },
            );
        }
    }
}
