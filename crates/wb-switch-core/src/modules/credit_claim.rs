//! 自动领取积分：欢迎礼包与模型补偿（用户中心「套餐与积分」页的「领取」按钮）。
//!
//! 接口（2026-10-16 实测；同源相对路径，纯 Bearer 鉴权、无签名、无浏览器依赖）：
//! - POST {endpoint}/billing/meter/check-gift-claimed   欢迎礼包状态
//! - POST {endpoint}/billing/meter/claim-gift           领取欢迎礼包
//! - POST {endpoint}/billing/meter/compensation-status  补偿状态
//! - POST {endpoint}/billing/meter/claim-compensation   领取补偿
//!
//! 请求体为空；成功 `{code:0}`；重复领取 HTTP 400 + `code:10001`
//! （「每人限领一次，您已领取过无法重复领取」）→ 幂等按 already 处理。
//! 状态成功响应：`data.{active, claimed, credit_num, claimed_at?, end_time}`。
//!
//! 注意路径挂在桌面端签到前缀**之外**（`/billing/meter/*`，不带 `/v2`）；
//! `/v2/billing/meter/*` 对这两组接口返回 404 Route Not Found。
//!
//! 日志策略：只有真正发起过领取的账号才写日志（success / already / error），
//! 状态查询失败、「已领取」与「活动未开放」都不写，避免后台轮次的日志噪音。
//!
//! 账号级结果（见 `account_result`）：success / error / already / inactive /
//! unsupported / none。「已领取」与「活动未开放」必须分开上报（对齐 `checkin.rs`
//! 的 `already` / `inactive` 约定）—— 两者都映射到同一个「无可领取」时，用户无从
//! 判断到底是自己领过了还是活动没开放。

use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::modules::account::{
    account_display_name, build_auth_headers, envelope_token_error, load_accounts, variant_of,
};
use crate::modules::config::{
    add_credit_claim_log, http_request, is_unauthorized, load_checkin_config,
    load_credit_claim_config, now_ms, RunFlagGuard, CREDIT_CLAIM_API_PREFIX,
};
use crate::modules::refresh::{ensure_fresh_token, refresh_account_token};
use crate::modules::variant::WbVariant;

static CLAIM_RUNNING: AtomicBool = AtomicBool::new(false);
static CLAIM_ACCOUNTS_RUNNING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

/// 后台领取轮询周期：启动即查一轮，之后每小时查一次（配置关闭时整轮短路）。
pub const CREDIT_CLAIM_RECOVERY_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// 两类领取项：(kind, 状态路径, 领取路径)，路径相对 `CREDIT_CLAIM_API_PREFIX`。
const CLAIM_ITEMS: [(&str, &str, &str); 2] = [
    ("gift", "/check-gift-claimed", "/claim-gift"),
    (
        "compensation",
        "/compensation-status",
        "/claim-compensation",
    ),
];

struct AccountRunGuard {
    key: String,
}

impl AccountRunGuard {
    fn try_acquire(account: &Value) -> Option<Self> {
        let key = account
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(String::from)
            .unwrap_or_else(|| account_display_name(account));
        let mut running = CLAIM_ACCOUNTS_RUNNING
            .get_or_init(|| Mutex::new(HashSet::new()))
            .lock()
            .unwrap();
        if !running.insert(key.clone()) {
            return None;
        }
        Some(Self { key })
    }
}

impl Drop for AccountRunGuard {
    fn drop(&mut self) {
        if let Some(running) = CLAIM_ACCOUNTS_RUNNING.get() {
            running
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&self.key);
        }
    }
}

/// 网关 404（路径不存在）：响应形如 `{"error_msg":"404 Route Not Found"}`，无 `code`。
fn is_route_not_found(resp: &Value) -> bool {
    if crate::modules::config::is_route_missing(resp) {
        return true;
    }
    resp.get("error_msg")
        .and_then(Value::as_str)
        .is_some_and(|message| message.contains("404"))
}

/// 可读错误文案：message / msg，都缺失时给出 code。
fn response_message(resp: &Value) -> String {
    let msg = resp
        .get("message")
        .or_else(|| resp.get("msg"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if !msg.is_empty() {
        return msg.to_string();
    }
    let code = resp.get("code").and_then(Value::as_i64).unwrap_or(-1);
    format!("code={code}")
}

/// 发单次领取请求（POST 空 body）；遇到未授权且存在 refresh token 时刷新一次并重试。
async fn claim_post(path: &str, account: &Value) -> Value {
    // 加密信封凭据短路：不发空 Bearer，直接给出可读错误（issue #94）。
    if let Some(err) = envelope_token_error(account) {
        return json!({"code": -2, "message": err});
    }
    let url = format!(
        "{}{CREDIT_CLAIM_API_PREFIX}{path}",
        variant_of(account).api_endpoint()
    );
    let headers = build_auth_headers(account);
    let mut resp = http_request(&url, "POST", None, Some(&headers)).await;
    if is_unauthorized(&resp)
        && !account
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
    {
        let refreshed = refresh_account_token(account.clone()).await;
        let headers = build_auth_headers(&refreshed);
        resp = http_request(&url, "POST", None, Some(&headers)).await;
    }
    resp
}

/// 状态查询结论。
///
/// 「已领取」与「活动未开放」必须分开：前者是用户关心的结果（对齐 `checkin.rs` 的
/// `StatusDecision::Already → "already"`），后者才是真正的无事可做。
enum ItemLookup {
    /// 可领取（active 且未领取）。
    Claimable(Value),
    /// 活动已领取（`claimed=true`）。状态阶段即可判定，不发领取请求。
    AlreadyClaimed,
    /// 活动未开放 / 已结束（`active=false`）。
    Inactive,
    /// 网关 404：该档位没有此类接口（防御，不报错）。
    Unsupported,
    /// 其它失败（传输 / 鉴权 / 业务码），带可读原因。
    Failed(String),
}

/// 领取响应结论。
enum ClaimOutcome {
    Success,
    Already,
    Unsupported,
    Error(String),
}

fn lookup_item(kind: &str, resp: &Value) -> ItemLookup {
    if is_route_not_found(resp) {
        return ItemLookup::Unsupported;
    }
    let code = resp.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code != 0 && code != 200 {
        return ItemLookup::Failed(response_message(resp));
    }
    let data = resp.get("data").cloned().unwrap_or_else(|| json!({}));
    let active = data.get("active").and_then(Value::as_bool).unwrap_or(false);
    let claimed = data
        .get("claimed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // 先判 claimed：已领取的活动同样是 active=true，顺序反了会误报「未开放」。
    if claimed {
        return ItemLookup::AlreadyClaimed;
    }
    if !active {
        return ItemLookup::Inactive;
    }
    ItemLookup::Claimable(json!({
        "kind": kind,
        "creditNum": data.get("credit_num").and_then(Value::as_i64),
        "claimedAt": data.get("claimed_at").cloned().unwrap_or(Value::Null),
        "endTime": data.get("end_time").cloned().unwrap_or(Value::Null),
    }))
}

fn claim_outcome(resp: &Value) -> ClaimOutcome {
    if is_route_not_found(resp) {
        return ClaimOutcome::Unsupported;
    }
    let code = resp.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code == 0 || code == 200 {
        return ClaimOutcome::Success;
    }
    let msg = response_message(resp);
    if code == 10001
        || msg.contains("已领取")
        || msg.contains("重复")
        || msg.to_lowercase().contains("repeat")
    {
        return ClaimOutcome::Already;
    }
    ClaimOutcome::Error(msg)
}

/// 领取项展示名（错误文案用）。
fn item_label(kind: &str) -> &'static str {
    match kind {
        "gift" => "欢迎礼包",
        _ => "模型补偿",
    }
}

/// 账号级结果的归类（纯函数，便于单测）。
///
/// 优先级：success > error > already > error(状态查询) > inactive > unsupported > none。
/// 状态查询失败排在 inactive 之前，是为了让「有个接口挂了」不会被「另一个活动没开放」
/// 掩盖掉——失败更需要用户处理。
fn account_result(
    claimed: &[Value],
    errors: &[String],
    status_errors: &[String],
    already: bool,
    inactive: bool,
    unsupported: bool,
) -> &'static str {
    if !claimed.is_empty() {
        "success"
    } else if !errors.is_empty() {
        "error"
    } else if already {
        "already"
    } else if !status_errors.is_empty() {
        "error"
    } else if inactive {
        "inactive"
    } else if unsupported {
        "unsupported"
    } else {
        "none"
    }
}

/// 写一条领取日志（只在真正发起过领取时调用）。
fn write_claim_log(
    account: &Value,
    result: &str,
    kind: &str,
    credit: Option<i64>,
    error: Option<&str>,
) {
    let mut entry = json!({
        "ts": now_ms(),
        "accountId": account.get("id").cloned().unwrap_or(Value::Null),
        "email": account_display_name(account),
        "variant": variant_of(account).as_str(),
        "result": result,
        "kind": kind,
        "credit": credit,
    });
    if let Some(text) = error {
        entry["error"] = json!(text);
    }
    add_credit_claim_log(&entry);
}

/// 对单个账号执行完整领取流程：惰性刷新 → 查两类状态 → 领取未领取的活动 → 写日志。
pub async fn claim_account(account: &Value) -> Value {
    let Some(_account_guard) = AccountRunGuard::try_acquire(account) else {
        return json!({"result": "error", "error": "该账号正在领取，请稍后再试"});
    };
    if let Some(err) = envelope_token_error(account) {
        return json!({"result": "error", "error": err});
    }
    let acc = ensure_fresh_token(account.clone(), &load_checkin_config()).await;

    let mut claimed: Vec<Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut status_errors: Vec<String> = Vec::new();
    let mut already = false;
    let mut inactive = false;
    let mut unsupported = false;

    for (kind, status_path, claim_path) in CLAIM_ITEMS {
        let resp = claim_post(status_path, &acc).await;
        match lookup_item(kind, &resp) {
            ItemLookup::AlreadyClaimed => already = true,
            ItemLookup::Inactive => inactive = true,
            ItemLookup::Unsupported => unsupported = true,
            ItemLookup::Failed(message) => {
                status_errors.push(format!("{}状态查询失败：{message}", item_label(kind)));
            }
            ItemLookup::Claimable(item) => {
                let credit = item.get("creditNum").and_then(Value::as_i64);
                // 只有真的发起领取才写日志（防后台轮次噪音）。
                let claim_resp = claim_post(claim_path, &acc).await;
                match claim_outcome(&claim_resp) {
                    ClaimOutcome::Success => {
                        claimed.push(json!({"kind": kind, "credit": credit}));
                        write_claim_log(&acc, "success", kind, credit, None);
                    }
                    ClaimOutcome::Already => {
                        already = true;
                        write_claim_log(&acc, "already", kind, credit, None);
                    }
                    ClaimOutcome::Unsupported => unsupported = true,
                    ClaimOutcome::Error(message) => {
                        errors.push(format!("{}：{message}", item_label(kind)));
                        write_claim_log(&acc, "error", kind, credit, Some(&message));
                    }
                }
            }
        }
    }

    let result = account_result(
        &claimed,
        &errors,
        &status_errors,
        already,
        inactive,
        unsupported,
    );
    let mut out = json!({"result": result, "claimed": claimed});
    let mut messages: Vec<String> = errors;
    messages.extend(status_errors);
    if !messages.is_empty() {
        out["error"] = json!(messages.join("；"));
    }
    out
}

/// 只匹配稳定 id，不以昵称或邮箱排除其它账号。
fn excluded_account(account: &Value, cfg: &Value) -> bool {
    account.get("id").and_then(Value::as_str).is_some_and(|id| {
        cfg.get("excluded_account_ids")
            .and_then(Value::as_array)
            .is_some_and(|ids| ids.iter().any(|value| value.as_str() == Some(id)))
    })
}

/// 汇总行（批量入口共用）。
fn summary_row(account: &Value, result: &Value) -> Value {
    let mut row = json!({
        "accountId": account.get("id").cloned().unwrap_or(Value::Null),
        "email": account_display_name(account),
        "result": result.get("result").cloned().unwrap_or(Value::Null),
    });
    for key in ["error", "reason"] {
        if let Some(value) = result.get(key) {
            row[key] = value.clone();
        }
    }
    // 镜像 checkin 的行结构（row["inactive"] = true）：显式标记「活动未开放」，
    // 让前端不必只靠 result 字符串判断。
    if result.get("result").and_then(Value::as_str) == Some("inactive") {
        row["inactive"] = json!(true);
    }
    if result
        .get("claimed")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
    {
        row["claimed"] = result.get("claimed").cloned().unwrap_or(Value::Null);
    }
    row
}

/// 执行一轮自动领取。只遍历允许自动领取的账号；配置关闭时整轮短路。
pub async fn run_claim_cycle() -> Value {
    let Some(_guard) = RunFlagGuard::try_acquire(&CLAIM_RUNNING) else {
        return json!({"status": "skipped", "reason": "already_running"});
    };
    let cfg = load_credit_claim_config();
    if cfg.get("enabled").and_then(Value::as_bool) != Some(true) {
        return json!({"status": "disabled"});
    }
    let accounts: Vec<Value> = load_accounts()
        .into_iter()
        .filter(|acc| !excluded_account(acc, &cfg))
        .collect();
    if accounts.is_empty() {
        return json!({"status": "no_accounts"});
    }
    let mut rows: Vec<Value> = Vec::new();
    for acc in &accounts {
        let result = claim_account(acc).await;
        rows.push(summary_row(acc, &result));
    }
    json!({"status": "ok", "accounts": rows})
}

/// 对全部（或指定档位）账号立即领取（设置页「全部立即领取」）。
///
/// 与自动轮次一致地跳过关闭自动领取的账号，并逐账号返回 skipped 原因；
/// 不检查总开关——手动入口保持「立即执行」语义（对齐签到）。
pub async fn claim_all(variant: Option<WbVariant>) -> Value {
    let Some(_guard) = RunFlagGuard::try_acquire(&CLAIM_RUNNING) else {
        return json!({"accounts": [], "status": "skipped", "reason": "already_running"});
    };
    let cfg = load_credit_claim_config();
    let accounts: Vec<Value> = load_accounts()
        .into_iter()
        .filter(|acc| variant.is_none_or(|target| variant_of(acc) == target))
        .collect();
    let mut rows: Vec<Value> = Vec::new();
    for acc in &accounts {
        if excluded_account(acc, &cfg) {
            rows.push(json!({
                "accountId": acc.get("id").cloned().unwrap_or(Value::Null),
                "email": account_display_name(acc),
                "result": "skipped",
                "reason": "auto_credit_claim_disabled",
            }));
            continue;
        }
        let result = claim_account(acc).await;
        rows.push(summary_row(acc, &result));
    }
    json!({"accounts": rows})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_response_maps_claimable_item() {
        let resp = json!({
            "code": 0,
            "msg": "OK",
            "data": {
                "claimed": false,
                "active": true,
                "credit_num": 500,
                "validity_period": 1,
                "end_time": "2026-10-22 23:59:59"
            }
        });
        match lookup_item("compensation", &resp) {
            ItemLookup::Claimable(item) => {
                assert_eq!(item["kind"], "compensation");
                assert_eq!(item["creditNum"], 500);
                assert_eq!(item["endTime"], "2026-10-22 23:59:59");
            }
            _ => panic!("active + unclaimed must be claimable"),
        }
    }

    #[test]
    fn status_response_separates_claimed_from_inactive() {
        // 已领取：claimed=true（活动同时是 active=true，顺序判反会误报未开放）。
        let claimed =
            json!({"code": 0, "data": {"claimed": true, "active": true, "credit_num": 500}});
        assert!(matches!(
            lookup_item("gift", &claimed),
            ItemLookup::AlreadyClaimed
        ));
        // 未开放：active=false。
        let inactive = json!({"code": 0, "data": {"claimed": false, "active": false}});
        assert!(matches!(
            lookup_item("gift", &inactive),
            ItemLookup::Inactive
        ));
        // 两者都缺省（无 data）：按未开放处理，与改造前 `!active` 分支一致。
        let empty = json!({"code": 0});
        assert!(matches!(lookup_item("gift", &empty), ItemLookup::Inactive));
    }

    #[test]
    fn status_response_keeps_business_code_message() {
        let resp = json!({"code": 401, "msg": "token 失效"});
        match lookup_item("gift", &resp) {
            ItemLookup::Failed(message) => assert_eq!(message, "token 失效"),
            _ => panic!("non-zero code must fail"),
        }
    }

    #[test]
    fn route_not_found_is_unsupported() {
        let resp = json!({"error_msg": "404 Route Not Found"});
        assert!(matches!(
            lookup_item("gift", &resp),
            ItemLookup::Unsupported
        ));
        assert!(matches!(claim_outcome(&resp), ClaimOutcome::Unsupported));
        assert!(matches!(
            claim_outcome(&json!({"code": 404})),
            ClaimOutcome::Unsupported
        ));
    }

    #[test]
    fn claim_success_and_idempotent_already() {
        assert!(matches!(
            claim_outcome(&json!({"code": 0, "msg": "OK"})),
            ClaimOutcome::Success
        ));
        let repeat = json!({
            "code": 10001,
            "msg": "每人限领一次，您已领取过无法重复领取"
        });
        assert!(matches!(claim_outcome(&repeat), ClaimOutcome::Already));
        assert!(matches!(
            claim_outcome(&json!({"code": 1, "msg": "该活动已领取"})),
            ClaimOutcome::Already
        ));
    }

    #[test]
    fn claim_business_error_keeps_message() {
        match claim_outcome(&json!({"code": 10086, "msg": "系统繁忙"})) {
            ClaimOutcome::Error(message) => assert_eq!(message, "系统繁忙"),
            _ => panic!("business error must surface message"),
        }
    }

    #[test]
    fn account_result_priority_puts_success_first_and_error_above_inactive() {
        let claimed = vec![json!({"kind": "gift", "credit": 1500})];
        let e = || vec!["模型补偿：系统繁忙".to_string()];
        let se = || vec!["欢迎礼包状态查询失败：离线".to_string()];

        // 成功 > 一切
        assert_eq!(
            account_result(&claimed, &e(), &[], false, false, false),
            "success"
        );
        // 领取失败 > 已领取 / 未开放
        assert_eq!(account_result(&[], &e(), &[], true, true, false), "error");
        // 已领取 > 状态查询失败 / 未开放
        assert_eq!(
            account_result(&[], &[], &se(), true, true, false),
            "already"
        );
        // 状态查询失败 > 未开放（失败更需要用户处理，不能被未开放掩盖）
        assert_eq!(account_result(&[], &[], &se(), false, true, false), "error");
        // 未开放 > unsupported
        assert_eq!(account_result(&[], &[], &[], false, true, true), "inactive");
        assert_eq!(
            account_result(&[], &[], &[], false, false, true),
            "unsupported"
        );
        // 兜底
        assert_eq!(account_result(&[], &[], &[], false, false, false), "none");
    }

    #[test]
    fn summary_row_marks_inactive_explicitly() {
        let account = json!({"id": "acc-1", "email": "a@b.c"});
        // 未开放：显式补 inactive 标记（镜像 checkin 的行结构）。
        let row = summary_row(&account, &json!({"result": "inactive"}));
        assert_eq!(row["result"], "inactive");
        assert_eq!(row["inactive"], true);
        assert_eq!(row["accountId"], "acc-1");
        // 已领取：不补该标记，且不泄漏内部计数。
        let row = summary_row(&account, &json!({"result": "already"}));
        assert!(row.get("inactive").is_none());
        assert_eq!(row["result"], "already");
    }

    #[test]
    fn exclusion_matches_stable_id_only() {
        let cfg = json!({"excluded_account_ids": ["excluded-id"]});
        assert!(excluded_account(
            &json!({"id": "excluded-id", "email": "a@b.c"}),
            &cfg
        ));
        assert!(!excluded_account(
            &json!({"id": "other-id", "email": "excluded-id"}),
            &cfg
        ));
        assert!(!excluded_account(&json!({"id": "excluded-id"}), &json!({})));
    }

    #[test]
    fn same_account_cannot_acquire_two_claim_guards() {
        let account = json!({"id": "claim-guard-test-account"});
        let first = AccountRunGuard::try_acquire(&account).expect("first acquires guard");
        assert!(AccountRunGuard::try_acquire(&account).is_none());
        drop(first);
        assert!(AccountRunGuard::try_acquire(&account).is_some());
    }
}
