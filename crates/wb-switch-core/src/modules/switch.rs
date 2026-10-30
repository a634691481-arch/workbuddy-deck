//! 账号切换：备份 → 关进程 → 恢复/复制会话（可选）→ 写认证 → 启动。
//!
//! 对照 server.py `switch_account`。切换过程中通过进度回调向前端推送实时进度，
//! 避免界面长时间无反馈被误认为卡死。core 不依赖 Tauri，进度回调由宿主适配
//! （桌面端转发为 `switch-progress` 事件，HTTP 端写入轮询/SSE）。
//!
//! 顺序要点（design §1）：互斥 → 关进程 → 恢复/复制/同步会话 → 写认证 → 启动。
//! 复制与同步顺序调用、共用同一把档位操作锁，且在「关闭 App + 恢复未完成写入」之后
//! 才执行；会话写入一律在 WorkBuddy 停止写入之后。普通会话操作失败不阻止认证切换，
//! 只有「无法安全恢复的中间产物」才暂停切换并明确报告恢复需求。
//!
//! 暂停启动的判定有两次，口径相同（design §5 / R5）：
//!   1. 写入之前——切换开头恢复未完成写入后仍有不可安全恢复的中间产物；
//!   2. 写入之后——本次复制/同步新留下的未完成操作（正文/数据库/组表任一阶段失败都会
//!      保留操作记录），不能带着不一致的会话内容写认证并启动 App。

use std::collections::BTreeSet;

use serde_json::{json, Value};

use crate::modules::account;
use crate::modules::auth_file::{self, IdentityState};
use crate::modules::config::now_ms;
use crate::modules::process::{close_workbuddy, launch_workbuddy};
use crate::modules::session::{self, SessionPaths, SyncSelection};
use crate::modules::session_link::{
    self, Operation, RecoveryReport, LOCK_BUSY_MESSAGE_PREFIX, LOCK_UNAVAILABLE_MESSAGE_PREFIX,
};
use crate::modules::switch_history::{self, SessionDigest, SwitchEntry};
use crate::modules::variant::WbVariant;

/// 切换进度回调（宿主注入，如 Tauri `app.emit` 或 HTTP 进度缓存）。
pub type ProgressFn = Box<dyn Fn(&str) + Send + Sync>;

/// `restart=false` 携带会话写入意图时的拒绝文案（复制/同步各自一条）。
pub const COPY_WITHOUT_RESTART_MESSAGE: &str =
    "本次切换未重启 WorkBuddy（restart=false），已拒绝会话复制请求；如需复制请勾选重启切换";
pub const SYNC_WITHOUT_RESTART_MESSAGE: &str =
    "本次切换未重启 WorkBuddy（restart=false），已拒绝会话同步请求；如需同步请勾选重启切换";
/// 替换式同步会物理删除目标账号会话，更不允许静默丢弃：直接报错。
pub const REPLACE_WITHOUT_RESTART_MESSAGE: &str =
    "本次切换未重启 WorkBuddy（restart=false），已拒绝「完全同步（替换）」请求；该操作会删除目标账号会话，请勾选重启后再试";

/// 恢复报告里是否存在阻碍启动的问题：未恢复一致的中间产物必须先处理。
///
/// 拿不到档位锁、或中间产物被改动/丢失都属此类（design §4 / §5）：此时继续写认证并
/// 启动 App 会让 Official App 在最坏状态下打开会话，因此暂停切换与启动。
/// `retryable=true` 沿用复制侧「可延后重试且不阻断启动」的约定；同步恢复失败必须
/// 返回 false，即使故障只是暂时的，也不能带着半完成的正文/数据库/基线启动。
pub fn recovery_blocks_startup(report: &RecoveryReport) -> bool {
    report.needs_recovery.iter().any(|issue| !issue.retryable)
}

/// 阻碍启动的原因汇总（用于错误文案）。
///
/// 必须带操作标识：阻断分支只返回 Err 字符串，前端 catch 不能只看到原因。
pub fn recovery_blocking_detail(report: &RecoveryReport) -> String {
    report
        .needs_recovery
        .iter()
        .filter(|issue| !issue.retryable)
        .map(|issue| format!("{}：{}", issue.operation_id, issue.reason))
        .collect::<Vec<String>>()
        .join("；")
}

/// 恢复报告的前端投影（与复制/同步报告同形，便于展示恢复信息）。
pub fn recovery_report_json(report: &RecoveryReport) -> Value {
    json!({
        "recovered": report.recovered.len(),
        "abandoned": report.abandoned.len(),
        "needsRecovery": report
            .needs_recovery
            .iter()
            .map(|issue| json!({
                "operationId": issue.operation_id,
                "reason": issue.reason,
                "retryable": issue.retryable,
            }))
            .collect::<Vec<Value>>(),
        // 待清理/待恢复的临时备份残留：与复制/同步报告同一结构。
        "temporaryFiles": report.temporary_files,
    })
}

/// 某档位当前未完成操作的 id 集合。
///
/// 用于区分「本次新产生的未完成写入」与「切换开头已尝试恢复的历史残留」：
/// 后者若是可重试类（例如映射库暂不可用），按既有口径不阻断切换。
fn pending_operation_ids(paths: &SessionPaths, variant: WbVariant) -> BTreeSet<String> {
    session_link::pending_operations(paths, variant)
        .into_iter()
        .map(|operation| operation.operation_id)
        .collect()
}

/// 本次会话写入新留下的未完成操作（按创建顺序，便于稳定输出）。
fn newly_unfinished_writes(before: &BTreeSet<String>, after: Vec<Operation>) -> Vec<Operation> {
    let mut created: Vec<Operation> = after
        .into_iter()
        .filter(|operation| !before.contains(&operation.operation_id))
        .collect();
    created.sort_by_key(|operation| operation.created_at);
    created
}

/// 未完成写入的说明文案：会话标识 + 操作标识 + 原因。
fn unfinished_writes_detail(writes: &[Operation]) -> String {
    writes
        .iter()
        .map(|operation| {
            let reason = operation
                .last_error
                .clone()
                .unwrap_or_else(|| "尚未完成".to_string());
            format!(
                "{}（操作 {}）：{reason}",
                operation.target.session_id, operation.operation_id
            )
        })
        .collect::<Vec<String>>()
        .join("；")
}

/// restart=false 时携带写入意图：显式拒绝对应的会话操作（不静默丢弃，design §4.1）。
fn reject_session_writes_without_restart(
    has_copy: bool,
    has_sync: bool,
) -> (Option<Value>, Option<Value>) {
    let copy = has_copy.then(|| json!({ "error": COPY_WITHOUT_RESTART_MESSAGE }));
    let sync = has_sync.then(|| {
        json!({
            "synced": [],
            "skipped": [],
            "errors": [{ "error": SYNC_WITHOUT_RESTART_MESSAGE }],
        })
    });
    (copy, sync)
}

/// 切换意图：只用于历史记录与结果标注，不参与控制流。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwitchMode {
    Plain,
    Copy,
    Sync,
    Replace,
}

impl SwitchMode {
    fn as_str(self) -> &'static str {
        match self {
            SwitchMode::Plain => "plain",
            SwitchMode::Copy => "copy",
            SwitchMode::Sync => "sync",
            SwitchMode::Replace => "replace",
        }
    }

    /// 替换式优先级最高：它同时包含「复制 + 删旧」，记录成 copy/sync 会误导排障。
    fn of(has_copy: bool, has_sync: bool, replace: bool) -> Self {
        if replace {
            SwitchMode::Replace
        } else if has_sync {
            SwitchMode::Sync
        } else if has_copy {
            SwitchMode::Copy
        } else {
            SwitchMode::Plain
        }
    }
}

/// 切换过程的观测轨迹：外层据此写历史，内层只管推进 stage。
///
/// 用可变引用而不是返回值传递，是为了不改动内层早退（`?`）的控制流——
/// 每一次早退都能在历史里留下「停在哪一步、为什么」。
struct SwitchTrace {
    mode: SwitchMode,
    /// `start` | `session` | `auth_write` | `verify` | `launch` | `done`
    stage: &'static str,
    identity: Option<IdentityState>,
    sessions: Option<SessionDigest>,
    /// 是否已把认证文件还原成切换前的内容。
    rolled_back: bool,
    /// 目标账号的会话是否已被本次切换**实际改写**（复制成功或替换成功）。
    ///
    /// 回滚只能还认证文件，还不了已复制的会话与已删的旧会话。这个标记决定
    /// 用户看到的措辞是「完全还原」还是「认证已还原、会话已变更」——两者对用户
    /// 的后续动作完全不同，不能混为一谈。
    sessions_written: bool,
}

impl SwitchTrace {
    fn new(mode: SwitchMode) -> Self {
        Self {
            mode,
            stage: "start",
            identity: None,
            sessions: None,
            rolled_back: false,
            sessions_written: false,
        }
    }

    /// 记一次会话搬运结果：只有真的写进去了才算「已改写」。
    fn note_sessions(&mut self, digest: SessionDigest) {
        if digest.copied > 0 || digest.replaced {
            self.sessions_written = true;
        }
        self.sessions = Some(digest);
    }
}

/// 回滚时的追加说明：会话已被改写就必须讲清楚，不能让用户以为「什么都没变」。
fn rollback_session_note(trace: &SwitchTrace) -> &'static str {
    if trace.sessions_written {
        "；注意：目标账号的会话已按所选模式更新，无法随认证一并还原"
    } else {
        ""
    }
}

/// 账号的判定身份：uid 优先，缺失时退回 id（与 `auth_file::auth_identity_at` 同口径）。
fn account_uid(acc: &Value) -> String {
    account::get_str(acc, "uid")
        .or_else(|| account::get_str(acc, "id"))
        .unwrap_or_default()
}

/// 把认证文件还原成切换前的字节。返回是否还原成功。
///
/// 失败只记日志、不 panic：还原失败本身也要让上层报出真实原因（用户至少知道
/// 「现在停在一个不该停的状态」），而不是被二次错误覆盖。
fn restore_auth_snapshot(variant: WbVariant, snapshot: Option<&[u8]>) -> bool {
    match auth_file::restore_auth_file(variant, snapshot) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("[switch] 还原认证文件失败：{error}");
            false
        }
    }
}

/// 从会话搬运报告里提取「复制了几条 / 源有几条 / 删没删」。
///
/// 替换式报告带 `copied`/`sourceTotal`/`failed`/`replaced`；复制式报告用 `copied`/`total`
/// 或 `sessions` 数组长度兜底。取不到就当 0，不猜。
fn session_digest(report: &Value) -> SessionDigest {
    let num = |key: &str| report.get(key).and_then(Value::as_i64).unwrap_or(0);
    let count_of = |key: &str| {
        report
            .get(key)
            .and_then(Value::as_array)
            .map(|items| items.len() as i64)
            .unwrap_or(0)
    };
    let copied = if report.get("copied").is_some() {
        num("copied")
    } else if report.get("sessions").is_some() {
        count_of("sessions")
    } else {
        // 同步报告用 `synced` 表示已落盘的条目：同样属于「已改写目标账号」。
        count_of("synced")
    };
    let failed = if report.get("failed").is_some() {
        num("failed")
    } else {
        count_of("errors")
    };
    let source_total = if report.get("sourceTotal").is_some() {
        num("sourceTotal")
    } else {
        num("total").max(copied + failed)
    };
    SessionDigest {
        copied,
        source_total,
        failed,
        replaced: report
            .get("replaced")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

/// 切换账号。
///
/// `copy_session_ids` 非空时按路径 B 复制勾选会话（新 id，云端可同步）；
/// `sync_selections` 非空时把来源账号的新增内容同步到目标账号的关联会话（保留目标
/// sessionId、标题与自定义标题）。两者共用同一把档位操作锁，顺序执行、不重复写入。
///
/// 外层只做两件事：切换前抓取可还原的快照与身份，切换后把结果写成一条历史。
/// 真正的编排在 [`switch_account_inner`]。
pub fn switch_account(
    progress_fn: Option<&ProgressFn>,
    account_id: &str,
    restart: bool,
    share_sessions: bool,
    copy_session_ids: &[String],
    sync_selections: &[SyncSelection],
    replace_sessions: bool,
) -> Result<Value, String> {
    switch_account_detailed(
        progress_fn,
        account_id,
        restart,
        share_sessions,
        copy_session_ids,
        sync_selections,
        replace_sessions,
    )
    .0
}

/// 一次切换的诊断结果：停在哪一步、身份校验结论、有没有回滚。
///
/// 为什么不复用历史文件：历史是**尽力而为**（写失败只打日志），而且是跨进程共享的
/// 最新一条。宿主若在切换后回读「历史里最新那条」推断本次结果，一旦历史写失败或
/// 恰好有并发切换，就会把这次的真实现场说错——最坏情况是把「没回滚」报成「已回滚」，
/// 用户据此以为停在原账号上。所以诊断必须随返回值直接带出。
#[derive(Debug, Clone)]
pub struct SwitchDiagnostics {
    /// `ok` | `rolled_back` | `blocked`
    pub result: &'static str,
    /// `start` | `session` | `auth_write` | `verify` | `launch` | `done`
    pub stage: &'static str,
    pub identity: Option<IdentityState>,
    pub rolled_back: bool,
    pub cost_ms: i64,
    pub sessions: Option<SessionDigest>,
    /// 认证已回滚，但目标账号的会话已被改写（无法还原）。
    pub sessions_dirty: bool,
}

impl SwitchDiagnostics {
    /// 身份三态字符串；未校验时为空串（前端据此区分「通过」与「没校验」）。
    pub fn identity_str(&self) -> &'static str {
        self.identity.map(|state| state.as_str()).unwrap_or("")
    }
}

/// 带诊断的切换。返回值恒非 Err（业务失败体现在诊断里），只是把两个结果分开给。
pub fn switch_account_detailed(
    progress_fn: Option<&ProgressFn>,
    account_id: &str,
    restart: bool,
    share_sessions: bool,
    copy_session_ids: &[String],
    sync_selections: &[SyncSelection],
    replace_sessions: bool,
) -> (Result<Value, String>, SwitchDiagnostics) {
    let started = now_ms();
    let target = account::find_account(account_id);
    let variant = target.as_ref().map(account::variant_of);
    // 切换前的身份与原始字节：身份用于历史对照，字节用于写后校验失败时还原。
    let before_uid = variant
        .and_then(|v| auth_file::auth_identity_at(&auth_file::auth_file_path(v)))
        .unwrap_or_default();
    let before_label = if before_uid.is_empty() {
        String::new()
    } else {
        account::load_accounts()
            .iter()
            .find(|acc| account_uid(acc) == before_uid)
            .map(account::account_display_name)
            .unwrap_or_default()
    };
    let auth_snapshot = variant.and_then(auth_file::snapshot_auth_file);
    let mut trace = SwitchTrace::new(SwitchMode::of(
        !copy_session_ids.is_empty(),
        !sync_selections.is_empty(),
        replace_sessions,
    ));

    let outcome = switch_account_inner(
        progress_fn,
        account_id,
        restart,
        share_sessions,
        copy_session_ids,
        sync_selections,
        replace_sessions,
        auth_snapshot.as_deref(),
        &mut trace,
    );

    let cost_ms = now_ms() - started;
    let result = match (&outcome, trace.rolled_back) {
        (Ok(_), _) => "ok",
        (Err(_), true) => "rolled_back",
        (Err(_), false) => "blocked",
    };
    let diagnostics = SwitchDiagnostics {
        result,
        stage: trace.stage,
        identity: trace.identity,
        rolled_back: trace.rolled_back,
        cost_ms,
        sessions: trace.sessions.clone(),
        sessions_dirty: trace.sessions_written && trace.rolled_back,
    };
    let entry = SwitchEntry {
        ts: started,
        result: result.to_string(),
        stage: trace.stage.to_string(),
        account_id: account_id.to_string(),
        to_uid: target.as_ref().map(account_uid).unwrap_or_default(),
        to_label: target
            .as_ref()
            .map(account::account_display_name)
            .unwrap_or_default(),
        from_uid: before_uid,
        from_label: before_label,
        variant: variant.map(|v| v.as_str().to_string()).unwrap_or_default(),
        mode: trace.mode.as_str().to_string(),
        identity: diagnostics.identity_str().to_string(),
        cost_ms,
        sessions: trace.sessions.clone(),
        error: outcome.as_ref().err().cloned(),
    };
    // 历史是尽力而为：记不上不能反过来让切换失败。
    if let Err(error) = switch_history::record(entry) {
        eprintln!("[switch] 切换历史写入失败（不影响本次结果）：{error}");
    }

    let value = match outcome {
        Ok(mut value) => {
            if let Some(identity) = trace.identity {
                value["identity"] = json!(identity.as_str());
            }
            value["costMs"] = json!(cost_ms);
            Ok(value)
        }
        Err(error) => Err(error),
    };
    (value, diagnostics)
}

#[allow(clippy::too_many_arguments)]
fn switch_account_inner(
    progress_fn: Option<&ProgressFn>,
    account_id: &str,
    restart: bool,
    share_sessions: bool,
    copy_session_ids: &[String],
    sync_selections: &[SyncSelection],
    replace_sessions: bool,
    auth_snapshot: Option<&[u8]>,
    trace: &mut SwitchTrace,
) -> Result<Value, String> {
    let progress = |message: &str| {
        eprintln!("[switch] progress: {message}");
        if let Some(p) = progress_fn {
            p(message);
        }
    };

    progress("开始切换账号…");
    let acc =
        account::find_account(account_id).ok_or_else(|| format!("账号不存在: {account_id}"))?;
    // 档位以账号自身为准：签名里的参数无法表达「用 A 档位操作 B 档位账号」。
    let variant = account::variant_of(&acc);
    let backup = auth_file::backup_auth_file(variant);

    let mut copy_report: Option<Value> = None;
    let mut session_report: Option<Value> = None;
    let mut sync_report: Option<Value> = None;
    let mut recovery_report: Option<Value> = None;
    if restart {
        progress("正在关闭 WorkBuddy…");
        // stage 在关进程**成功之后**才推进：关不掉时还没动过任何会话，
        // 报成 session 阶段会让排障方向整个跑偏。
        close_workbuddy(variant, 20)?;
        trace.stage = "session";
        // 关进程后先恢复未完成的会话写入：恢复成功或复制侧可安全延后重试的失败
        // 不阻断切换；同步仍未完成、拿不到锁或中间产物异常则暂停启动（design §4 / §5）。
        let recovery = match session::recover_pending_session_operations(variant) {
            Ok(report) => report,
            Err(error) => {
                return Err(format!(
                    "无法恢复未完成的会话写入（{error}），已暂停切换与启动 WorkBuddy；请稍后重试"
                ));
            }
        };
        let blocking = recovery_blocks_startup(&recovery);
        if !recovery.is_empty() {
            recovery_report = Some(recovery_report_json(&recovery));
        }
        if blocking {
            let detail = recovery_blocking_detail(&recovery);
            return Err(format!(
                "检测到无法安全恢复的会话写入（{detail}），已暂停切换与启动 WorkBuddy；请先处理该会话后再试"
            ));
        }
        // 本次是否请求了会话写入；同时记下写入前已存在的未完成记录，供写入后比对。
        let ops_paths = SessionPaths::for_variant(variant);
        let attempts_session_writes =
            !copy_session_ids.is_empty() || !sync_selections.is_empty() || replace_sessions;
        let pending_before = if attempts_session_writes {
            pending_operation_ids(&ops_paths, variant)
        } else {
            BTreeSet::new()
        };
        if replace_sessions {
            // 以**当前登录账号**为准，全量覆盖目标账号：先复制、后删旧。
            progress("正在以当前账号为准替换目标账号会话…");
            trace.stage = "session";
            let source = auth_file::import_from_auth_file(variant).ok_or_else(|| {
                "读取当前登录态失败，无法确定要同步过去的账号；已暂停切换与启动 WorkBuddy"
                    .to_string()
            })?;
            let report = match session::replace_sessions_for_switch(&source, &acc, variant) {
                Ok(report) => report,
                Err(error)
                    if error.starts_with(LOCK_BUSY_MESSAGE_PREFIX)
                        || error.starts_with(LOCK_UNAVAILABLE_MESSAGE_PREFIX) =>
                {
                    return Err(format!(
                        "无法独占会话操作（{error}），已暂停切换与启动 WorkBuddy；请稍后重试"
                    ));
                }
                // 失败也保留契约形态，好让下面的门槛统一判定（不再分两种错误样式）。
                Err(error) => json!({ "replaced": false, "error": error }),
            };
            let digest = session_digest(&report);
            trace.note_sessions(digest.clone());
            // 替换不完整 → fail-closed，不写认证、不启动。
            //
            // 这是两次真实事故的根因：复制 69/74 条却把旧 72 条删光，用户打开账号
            // 少 5 条会话且无法补齐。`replace_sessions_at` 已保证「不完整就不删」，
            // 但漏掉的是**上层仍继续写认证并报成功**。这里把它变成硬阻断。
            if report.get("replaced").and_then(Value::as_bool) != Some(true) {
                let reason = report
                    .get("reason")
                    .and_then(Value::as_str)
                    .or_else(|| report.get("error").and_then(Value::as_str))
                    .unwrap_or("会话替换未完成");
                // 已复制进去的副本会留在目标账号（旧会话按契约未删）：必须讲清楚，
                // 否则用户重试后目标账号里会出现两份同名会话。
                let residue = if digest.copied > 0 {
                    format!("；已复制的 {} 条会话副本仍留在目标账号", digest.copied)
                } else {
                    String::new()
                };
                return Err(format!(
                    "会话替换未完成（复制 {}/{}，失败 {}）：{reason}；已暂停切换与启动 WorkBuddy，目标账号原有会话已保留{residue}",
                    digest.copied, digest.source_total, digest.failed
                ));
            }
            session_report = Some(report);
        }
        if !copy_session_ids.is_empty() {
            progress("正在复制会话到目标账号…");
            // 复制失败本身只记进报告、不阻断切换；但若本次留下了未完成的写入，
            // 会在复制与同步都跑完后统一暂停切换与启动（见下方 pending 差集检查）。
            copy_report = Some(
                match session::copy_sessions_for_switch(&acc, copy_session_ids) {
                    Ok(report) => report,
                    // 档位锁被占用或无法建立互斥：此时可能另有会话写入正在进行，
                    // 不能当成普通复制失败后继续写认证并启动 App。锁失败文案前缀
                    // 由 session_link 提供，不在这里嗅探整句错误文案。
                    Err(error)
                        if error.starts_with(LOCK_BUSY_MESSAGE_PREFIX)
                            || error.starts_with(LOCK_UNAVAILABLE_MESSAGE_PREFIX) =>
                    {
                        return Err(format!(
                            "无法独占会话操作（{error}），已暂停切换与启动 WorkBuddy；请稍后重试"
                        ));
                    }
                    Err(error) => json!({"error": error}),
                },
            );
            if let Some(digest) = copy_report.as_ref().map(session_digest) {
                trace.note_sessions(digest);
            }
        }
        if !sync_selections.is_empty() {
            // 同步与复制顺序执行（同一把档位锁），不重复写入。
            progress("正在同步会话到目标账号…");
            sync_report = Some(
                match session::sync_sessions_for_switch(&acc, sync_selections) {
                    Ok(report) => report,
                    Err(error)
                        if error.starts_with(LOCK_BUSY_MESSAGE_PREFIX)
                            || error.starts_with(LOCK_UNAVAILABLE_MESSAGE_PREFIX) =>
                    {
                        return Err(format!(
                            "无法独占会话操作（{error}），已暂停切换与启动 WorkBuddy；请稍后重试"
                        ));
                    }
                    // 同步失败不阻断切换：契约与成功路径同形，错误挂在 errors 里。
                    Err(error) => json!({
                        "synced": [],
                        "skipped": [],
                        "errors": [{ "error": error }],
                    }),
                },
            );
            if let Some(digest) = sync_report.as_ref().map(session_digest) {
                trace.note_sessions(digest);
            }
        }
        if share_sessions {
            // 旧的「全体转移」兼容路径（默认关闭），Rust 版暂未实现
            session_report = Some(json!({"error": "share_sessions 兼容路径暂未在 Rust 版实现"}));
        }
        // 本次复制/同步新留下的未完成操作：不带着不一致的会话内容写认证并启动 App
        // （design §5 / R5）。未完成写入也不在这里被报告成成功——错误已在各自报告里，
        // 这里只决定「暂停」。未开始写入的失败（预览过期、源会话被删等）不产生操作记录，
        // 因此仍然只是跳过该项、继续切号。
        if attempts_session_writes {
            let created = newly_unfinished_writes(
                &pending_before,
                session_link::pending_operations(&ops_paths, variant),
            );
            if !created.is_empty() {
                return Err(format!(
                    "本次会话写入未完成（{}），已暂停切换与启动 WorkBuddy；请重试，下次切号会先完成恢复",
                    unfinished_writes_detail(&created)
                ));
            }
        }
    } else {
        // restart=false 表示本次不做任何会话写入；携带写入意图时显式拒绝该会话操作，
        // 不能静默丢弃（design §4.1）。
        if replace_sessions {
            return Err(REPLACE_WITHOUT_RESTART_MESSAGE.to_string());
        }
        let (copy, sync) = reject_session_writes_without_restart(
            !copy_session_ids.is_empty(),
            !sync_selections.is_empty(),
        );
        copy_report = copy;
        sync_report = sync;
    }
    progress("正在写入认证文件…");
    trace.stage = "auth_write";
    if let Err(error) = auth_file::write_account_to_auth_file(&acc, variant) {
        // 写入失败时文件可能已被部分写坏（写成功但校验失败会走到这里）：
        // 还原成切换前的内容，用户至少还停在原来的账号上，而不是停在半个文件上。
        trace.rolled_back = restore_auth_snapshot(variant, auth_snapshot);
        return Err(format!(
            "{error}；{}{}",
            if trace.rolled_back {
                "已还原切换前的认证文件，WorkBuddy 未启动"
            } else {
                "还原切换前的认证文件失败，请检查 WorkBuddy 认证文件状态后重试"
            },
            rollback_session_note(trace)
        ));
    }

    // 写后身份校验：读**磁盘真相**，不信内存里刚拼出来的对象。
    // token 对但 uid 不对（并发回写 / 写到别的档位）时，用户会以为切过去了，
    // 实际在别人的号上——这是本项目此前完全没有覆盖的一类静默错误。
    trace.stage = "verify";
    let expected_uid = account_uid(&acc);
    let identity = auth_file::verify_identity(variant, &expected_uid);
    trace.identity = Some(identity);
    if identity.blocks_switch() {
        let actual = auth_file::auth_identity_at(&auth_file::auth_file_path(variant))
            .unwrap_or_else(|| "<读取失败>".to_string());
        trace.rolled_back = restore_auth_snapshot(variant, auth_snapshot);
        return Err(format!(
            "写后身份校验失败：期望 {}，磁盘实际 {}；{}{}，WorkBuddy 未启动",
            if expected_uid.is_empty() {
                "<未知>"
            } else {
                expected_uid.as_str()
            },
            actual,
            if trace.rolled_back {
                "已还原切换前的认证文件"
            } else {
                "还原切换前的认证文件失败，请手动检查认证文件"
            },
            rollback_session_note(trace)
        ));
    }

    if restart {
        progress("正在启动 WorkBuddy…");
        trace.stage = "launch";
        if let Err(error) = launch_workbuddy(variant, Some(&progress)) {
            // 认证已写对、身份已校验通过：不回滚。回滚等于把一次成功的切换作废，
            // 用户反而要重来一遍。只如实报告「账号已切换但没启动起来」。
            return Err(format!("账号已切换到目标账号，但启动 WorkBuddy 失败：{error}"));
        }
    }
    trace.stage = "done";
    progress("切换完成");

    let mut result = json!({
        "ok": true,
        "account": account::account_display_name(&acc),
        "variant": variant.as_str(),
        "backup": backup.map(|p| p.to_string_lossy().to_string()),
    });
    if let Some(c) = copy_report {
        result["sessionCopy"] = c;
    }
    if let Some(s) = session_report {
        result["sessionReplace"] = s;
    }
    if let Some(s) = sync_report {
        result["sessionSync"] = s;
    }
    if let Some(r) = recovery_report {
        result["sessionRecovery"] = r;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    //! 编排层可注入纯函数的单测：切换全流程（关进程/写认证/启动）需真机验收，
    //! 这里覆盖「不可安全恢复必须暂停启动」「本次新留下的未完成写入必须暂停启动」
    //! 与「restart=false 拒绝写入意图」三条判定。

    use super::*;
    use crate::modules::session_link::{
        save_operation, OpPhase, OperationMember, RecoveryIssue, TemporaryFileIssue,
        OPERATION_VERSION,
    };

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "wb_switch_orchestrate_{}_{name}",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn paths(&self) -> SessionPaths {
            SessionPaths {
                store_root: self.0.join("store"),
                data_root: self.0.join("data"),
                link_namespace: crate::modules::session::LinkNamespace::WorkBuddy,
                auth_file: self.0.join("auth.info"),
            }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 测试用操作记录：只关心 id、阶段与失败原因。
    fn operation(id: &str, phase: OpPhase, error: Option<&str>, created_at: i64) -> Operation {
        Operation {
            version: OPERATION_VERSION,
            operation_id: id.to_string(),
            kind: "sync".to_string(),
            variant: WbVariant::Cn,
            source_variant: None,
            group_id: "g-1".to_string(),
            source: OperationMember {
                account_id: None,
                uid: "uid-a".to_string(),
                session_id: "sess-1".to_string(),
            },
            target: OperationMember {
                account_id: None,
                uid: "uid-b".to_string(),
                session_id: "sess-b".to_string(),
            },
            expected_content_digest: "digest".to_string(),
            expected_record_count: 1,
            phase,
            backup: None,
            lifecycle_version: None,
            cleanup_state: None,
            last_error: error.map(str::to_string),
            created_at,
            updated_at: created_at,
        }
    }

    fn report_with(retryable: bool) -> RecoveryReport {
        RecoveryReport {
            recovered: Vec::new(),
            abandoned: Vec::new(),
            temporary_files: Vec::new(),
            needs_recovery: vec![RecoveryIssue {
                operation_id: "op-1".to_string(),
                reason: "目标内容与操作记录不一致，已停止恢复".to_string(),
                retryable,
            }],
        }
    }

    /// 中间产物被改动/丢失（不可重试）→ 暂停启动；可重试的失败不阻断。
    #[test]
    fn recovery_blocks_startup_only_for_unrecoverable_issues() {
        assert!(recovery_blocks_startup(&report_with(false)));
        assert!(!recovery_blocks_startup(&report_with(true)));
        assert!(!recovery_blocks_startup(&RecoveryReport::default()));
        let blocking = recovery_blocking_detail(&report_with(false));
        assert!(blocking.contains("op-1"), "{blocking}");
        assert!(blocking.contains("已停止恢复"), "{blocking}");
        assert!(recovery_blocking_detail(&report_with(true)).is_empty());
    }

    /// 恢复报告投影：三类结果与不可重试标记都要能被前端看到。
    #[test]
    fn recovery_report_projection_carries_all_categories() {
        let mut report = report_with(false);
        report.recovered.push("op-2".to_string());
        report.abandoned.push("op-3".to_string());
        let value = recovery_report_json(&report);
        assert_eq!(value["recovered"], 1);
        assert_eq!(value["abandoned"], 1);
        assert_eq!(value["needsRecovery"][0]["operationId"], "op-1");
        assert_eq!(value["needsRecovery"][0]["retryable"], false);
        assert_eq!(value["temporaryFiles"], json!([]));
    }

    /// 只有临时备份残留时也不能当成「恢复什么都没做」：宿主必须把 sessionRecovery 带给前端。
    #[test]
    fn recovery_is_not_empty_when_only_temporary_files_remain() {
        let mut report = RecoveryReport::default();
        report
            .temporary_files
            .push(TemporaryFileIssue::cleanup_pending(
                "op-cleanup".to_string(),
                Some("sess-1".to_string()),
                Some("标题".to_string()),
                "临时目录删除失败：权限不足".to_string(),
            ));
        assert!(!report.is_empty());
        assert!(report.is_clean(), "待清理不得变成启动阻断");
        let value = recovery_report_json(&report);
        assert_eq!(value["temporaryFiles"][0]["operationId"], "op-cleanup");
        assert_eq!(value["temporaryFiles"][0]["state"], "cleanupPending");
        assert_eq!(value["temporaryFiles"][0]["sessionId"], "sess-1");
        assert_eq!(value["temporaryFiles"][0]["title"], "标题");
    }

    /// 本次新留下的未完成写入必须被识别出来：历史残留不算在本次头上，已完成的也不算。
    #[test]
    fn newly_unfinished_writes_only_counts_this_switch() {
        let dir = TempDir::new("pending-writes");
        let paths = dir.paths();
        // 历史残留（切换开头已尝试恢复，可重试类不阻断）。
        save_operation(
            &paths,
            &operation("op-old", OpPhase::BodyWritten, Some("映射库暂不可用"), 1),
        )
        .unwrap();
        let before = pending_operation_ids(&paths, WbVariant::Cn);
        assert_eq!(before.len(), 1);

        // 本次复制/同步新产生一条未完成操作。
        save_operation(
            &paths,
            &operation(
                "op-new",
                OpPhase::DbWritten,
                Some("目标会话记录更新失败"),
                2,
            ),
        )
        .unwrap();
        let created = newly_unfinished_writes(
            &before,
            session_link::pending_operations(&paths, WbVariant::Cn),
        );
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].operation_id, "op-new");
        let detail = unfinished_writes_detail(&created);
        assert!(detail.contains("目标会话记录更新失败"), "{detail}");

        // 已完成的写入不算未完成；没有失败原因时退化为操作 id。
        save_operation(&paths, &operation("op-done", OpPhase::Completed, None, 3)).unwrap();
        let created = newly_unfinished_writes(
            &BTreeSet::new(),
            session_link::pending_operations(&paths, WbVariant::Cn),
        );
        assert_eq!(
            created
                .iter()
                .map(|operation| operation.operation_id.as_str())
                .collect::<Vec<_>>(),
            vec!["op-old", "op-new"],
            "按创建时间排序，且不含已完成操作"
        );
        assert!(
            unfinished_writes_detail(&[operation("op-bare", OpPhase::Prepared, None, 4)])
                .contains("op-bare")
        );

        // 别的档位的未完成写入不算在本档位头上。
        let mut other = operation("op-ai", OpPhase::Prepared, Some("待恢复"), 5);
        other.variant = WbVariant::Ai;
        save_operation(&paths, &other).unwrap();
        assert!(!pending_operation_ids(&paths, WbVariant::Cn).contains("op-ai"));
    }

    /// restart=false 携带同步意图 → 与复制同样显式拒绝，且契约与成功路径同形。
    #[test]
    fn session_writes_without_restart_are_rejected_explicitly() {
        let (copy, sync) = reject_session_writes_without_restart(false, true);
        assert!(copy.is_none());
        let sync = sync.expect("携带同步意图必须给出拒绝报告");
        assert_eq!(sync["errors"][0]["error"], SYNC_WITHOUT_RESTART_MESSAGE);
        assert!(sync["errors"][0]["error"]
            .as_str()
            .unwrap()
            .contains("restart=false"));
        assert!(sync["synced"].as_array().unwrap().is_empty());
        assert!(sync["skipped"].as_array().unwrap().is_empty());

        // 两种意图同时存在：各自拒绝，互不吞掉。
        let (copy, sync) = reject_session_writes_without_restart(true, true);
        assert_eq!(copy.unwrap()["error"], COPY_WITHOUT_RESTART_MESSAGE);
        assert_eq!(
            sync.unwrap()["errors"][0]["error"],
            SYNC_WITHOUT_RESTART_MESSAGE
        );

        // 都没有写入意图：不产生任何报告（保持原有语义）。
        let (copy, sync) = reject_session_writes_without_restart(false, false);
        assert!(copy.is_none() && sync.is_none());
    }

    /// 同步报告用 `synced` 计数：必须和复制一样算「已改写目标账号」，
    /// 否则同步成功后回滚时会被说成「完全还原」（会话其实已经变了）。
    #[test]
    fn session_digest_counts_synced_entries() {
        let report = json!({
            "synced": [{"sessionId": "s1"}, {"sessionId": "s2"}],
            "skipped": [],
            "errors": [],
        });
        let digest = session_digest(&report);
        assert_eq!(digest.copied, 2);
        assert!(!digest.replaced);
    }

    /// 回滚文案只在**真的写过会话**时才补会话说明，避免无事发生时吓人。
    #[test]
    fn rollback_note_only_when_sessions_written() {
        let mut trace = SwitchTrace::new(SwitchMode::Replace);
        assert_eq!(rollback_session_note(&trace), "");

        trace.note_sessions(SessionDigest {
            copied: 0,
            source_total: 0,
            failed: 0,
            replaced: false,
        });
        assert_eq!(rollback_session_note(&trace), "", "一条都没复制不算改写");

        trace.note_sessions(SessionDigest {
            copied: 3,
            source_total: 3,
            failed: 0,
            replaced: true,
        });
        assert!(rollback_session_note(&trace).contains("无法随认证一并还原"));
    }

    /// 诊断里的身份串：未校验必须是空串，前端据此区分「通过」与「没校验」。
    #[test]
    fn diagnostics_identity_str_is_empty_when_unchecked() {
        let base = SwitchDiagnostics {
            result: "blocked",
            stage: "session",
            identity: None,
            rolled_back: false,
            cost_ms: 12,
            sessions: None,
            sessions_dirty: false,
        };
        assert_eq!(base.identity_str(), "");
        assert_eq!(
            SwitchDiagnostics {
                identity: Some(IdentityState::Mismatch),
                ..base
            }
            .identity_str(),
            "identity-mismatch"
        );
    }
}
