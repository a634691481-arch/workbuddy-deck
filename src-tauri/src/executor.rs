//! 批量执行器：进度快照、取消标志、结果归类。
//!
//! 单独成文件的原因：命令层只该做「参数校验 + 转调」，业务编排放这里才可测、可扩展。
//! 新增一种批量动作 = 在 [`ACTIONS`] 登记 + 在 [`execute`] 加一支 + 在 [`classify`] 加一支。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde_json::{json, Value};

use wb_switch_core::modules::config;
use wb_switch_core::modules::growth_tasks;
use wb_switch_core::modules::{account, checkin, refresh, travel};

/// 已支持的批量动作。前端按钮表按这个枚举驱动，不写 if 链。
pub const ACTIONS: [&str; 4] = ["checkin", "growth", "travel", "daily"];

pub fn is_supported(action: &str) -> bool {
    ACTIONS.contains(&action)
}

/// 一轮执行的可观察状态：界面靠它显示 `3/6` 与当前账号。
#[derive(Clone, Default)]
struct Snapshot {
    action: String,
    total: usize,
    done: usize,
    ok: usize,
    fail: usize,
    skip: usize,
    current: String,
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);
static SNAP: Mutex<Option<Snapshot>> = Mutex::new(None);

fn snapshot() -> Snapshot {
    SNAP.lock()
        .ok()
        .and_then(|guard| guard.clone())
        .unwrap_or_default()
}

fn update(f: impl FnOnce(&mut Snapshot)) {
    if let Ok(mut guard) = SNAP.lock() {
        if let Some(state) = guard.as_mut() {
            f(state);
        }
    }
}

/// 结果三分：成功 / 失败 / 跳过。统计不再靠拼字符串猜。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Ok,
    Fail,
    Skip,
}

fn count(outcome: Outcome) {
    update(|state| {
        state.done += 1;
        match outcome {
            Outcome::Ok => state.ok += 1,
            Outcome::Fail => state.fail += 1,
            Outcome::Skip => state.skip += 1,
        }
    });
}

pub fn status() -> Value {
    let state = snapshot();
    json!({
        "running": RUNNING.load(Ordering::SeqCst),
        "cancelled": CANCEL.load(Ordering::SeqCst),
        "action": state.action,
        "total": state.total,
        "done": state.done,
        "ok": state.ok,
        "fail": state.fail,
        "skip": state.skip,
        "current": state.current,
    })
}

/// 请求取消：置标志位，当前账号做完这一步后停下，未开始的账号不再执行。
pub fn cancel() -> Value {
    if RUNNING.load(Ordering::SeqCst) {
        CANCEL.store(true, Ordering::SeqCst);
    }
    json!({ "cancelled": CANCEL.load(Ordering::SeqCst) })
}

async fn fresh(acc: Value) -> Value {
    refresh::ensure_fresh_token(acc, &config::load_checkin_config()).await
}

/// 选中账号：空列表 = 全部。
fn pick_accounts(ids: &[String]) -> Vec<Value> {
    let all = account::load_accounts();
    if ids.is_empty() {
        return all;
    }
    all.into_iter()
        .filter(|acc| {
            acc.get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| ids.iter().any(|want| want == id))
        })
        .collect()
}

fn text_of(row: &Value, key: &str) -> String {
    row.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn claimed_count(row: &Value) -> usize {
    row.get("claimed")
        .and_then(Value::as_array)
        .map(|items| items.len())
        .unwrap_or(0)
}

fn classify(action: &str, row: &Value, daily_checkin: &str, daily_travel: &str) -> Outcome {
    match action {
        "checkin" => match text_of(row, "result").as_str() {
            "success" => Outcome::Ok,
            "already" | "skipped" | "inactive" => Outcome::Skip,
            _ => Outcome::Fail,
        },
        "growth" => {
            if claimed_count(row) > 0 {
                Outcome::Ok
            } else if text_of(row, "result") == "partial" {
                Outcome::Fail
            } else {
                Outcome::Skip
            }
        }
        "travel" => match text_of(row, "result").as_str() {
            "success" => Outcome::Ok,
            "skipped" | "" => Outcome::Skip,
            _ => Outcome::Fail,
        },
        // daily：签到失败 + 没领到任务 + 旅行也没成功，才算失败。
        _ => {
            let growth_ok = claimed_count(row) > 0;
            let travel_ok = daily_travel == "success";
            if daily_checkin == "error" && !growth_ok && !travel_ok {
                Outcome::Fail
            } else if growth_ok || travel_ok || daily_checkin == "success" {
                Outcome::Ok
            } else {
                Outcome::Skip
            }
        }
    }
}

/// 串行执行一轮；每账号开始前检查取消标志。
pub async fn execute(action: &'static str, account_ids: Vec<String>) -> Value {
    if !is_supported(action) {
        return json!({ "status": "unsupported", "accounts": [] });
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        return json!({ "status": "busy", "accounts": [] });
    }
    CANCEL.store(false, Ordering::SeqCst);
    let accounts = pick_accounts(&account_ids);
    if let Ok(mut guard) = SNAP.lock() {
        *guard = Some(Snapshot {
            action: action.to_string(),
            total: accounts.len(),
            ..Default::default()
        });
    }

    let cfg = config::load_growth_config();
    let mut rows: Vec<Value> = Vec::new();

    for acc in accounts {
        if CANCEL.load(Ordering::SeqCst) {
            break;
        }
        let acc = fresh(acc).await;
        let label = account::account_display_name(&acc);
        let account_id = acc.get("id").cloned().unwrap_or(Value::Null);
        update(|state| state.current = label.clone());

        let mut daily_checkin = String::new();
        let mut daily_travel = String::new();

        let row = match action {
            "checkin" => {
                let mut row = checkin::checkin_account(&acc).await;
                row["accountId"] = account_id;
                row["label"] = json!(label);
                row
            }
            "growth" => {
                let mut row = growth_tasks::run_growth_account(&acc, &cfg).await;
                row["label"] = json!(label);
                row
            }
            "travel" => {
                let mut row = travel::depart_travel_for_account(&acc).await;
                row["accountId"] = account_id;
                row["label"] = json!(label);
                row
            }
            _ => {
                let checkin_row = checkin::checkin_account(&acc).await;
                daily_checkin = text_of(&checkin_row, "result");
                let growth_row = growth_tasks::run_growth_account(&acc, &cfg).await;
                let travel_row = travel::depart_travel_for_account(&acc).await;
                daily_travel = text_of(&travel_row, "result");
                let claimed = claimed_count(&growth_row);
                json!({
                    "accountId": account_id,
                    "label": label,
                    "result": if daily_checkin == "success" || claimed > 0 {
                        "success"
                    } else if daily_checkin == "error" {
                        "error"
                    } else {
                        "skipped"
                    },
                    "checkin": daily_checkin,
                    "travel": daily_travel,
                    "claimedCount": claimed,
                })
            }
        };

        count(classify(action, &row, &daily_checkin, &daily_travel));
        rows.push(row);
    }

    let cancelled = CANCEL.load(Ordering::SeqCst);
    RUNNING.store(false, Ordering::SeqCst);
    let state = snapshot();

    // 落一条通知存档：运行日志页与「回看当时提示」都靠它，一次性 toast 关掉就没了。
    {
        let label = match action {
            "checkin" => "签到",
            "growth" => "成长任务",
            "travel" => "旅行",
            _ => "全部日常",
        };
        let (level, title) = if cancelled {
            ("warning", format!("{label} · 已取消"))
        } else if state.fail > 0 {
            ("warning", format!("{label} · 部分失败"))
        } else {
            ("success", format!("{label} · 完成"))
        };
        let _ = wb_switch_core::modules::notifications::record(
            level,
            &title,
            Some(&format!(
                "成功 {} · 失败 {} · 跳过 {} · 共 {} 个账号",
                state.ok, state.fail, state.skip, state.total
            )),
        );
    }
    // 结构化观测行：动作 / 进度 / 结果三分 / 是否取消。
    eprintln!(
        "[run] action={} done={}/{} ok={} fail={} skip={} cancelled={}",
        action, state.done, state.total, state.ok, state.fail, state.skip, cancelled
    );
    json!({
        "status": if cancelled { "cancelled" } else { "done" },
        "accounts": rows,
    })
}
