//! 成长中心的网络层：**时间预算 + 退避重试 + 状态分类**。
//!
//! 现有 `config::http_request` 只有固定 30s 超时、零重试，本模块给新功能单独用，
//! 不改动既有调用方（避免影响签到 / 切换等链路）。
//!
//! 重试策略（对齐社区验证方案）：
//! - 网络类失败（连接失败 / 超时）：5 / 15 / 30 / 60 / 90 秒退避；
//! - 服务端 5xx：3 / 10 秒；
//! - 4xx：不重试（业务拒绝，重试无意义）；
//! - **写操作一律不重试**（避免"服务端已处理但客户端超时"造成重复提交）；
//!   幂等写（签到那类）可以重试，由调用方用 [`RetryClass::IdempotentWrite`] 显式声明。
//!
//! 预算：每次请求前检查剩余时间，耗尽即返回 `code = -2`，调用方据此把剩余步骤
//! 标记为 `skipped` 而不是 `error`。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::modules::config::{http_request_raw, GROWTH_HOST};

const NETWORK_DELAYS: [u64; 5] = [5, 15, 30, 60, 90];
const SERVER_DELAYS: [u64; 2] = [3, 10];
/// 预算耗尽 / 网络不可达 的约定码（与既有 `http_request` 的 `-1` 保持一致）。
pub const CODE_NO_NETWORK: i64 = -1;
pub const CODE_BUDGET_OUT: i64 = -2;

/// 单次运行的时间预算。
#[derive(Debug, Clone)]
pub struct TimeBudget {
    deadline: Instant,
}

impl TimeBudget {
    pub fn new(seconds: u64) -> Self {
        Self {
            deadline: Instant::now() + Duration::from_secs(seconds.max(1)),
        }
    }

    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    /// 剩余不足 1 秒即视为耗尽（留出写日志的余量）。
    pub fn exhausted(&self) -> bool {
        self.remaining() <= Duration::from_secs(1)
    }
}

/// 请求的重试语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryClass {
    /// 只读（GET）。可重试。
    Read,
    /// 不可逆写（补签 / 兑换 / 抽奖 / 开盒）。**不重试**。
    Write,
    /// 幂等写（重复提交只会返回"已完成"）。可重试。
    IdempotentWrite,
}

/// 一次接口调用的结果。
#[derive(Debug, Clone)]
pub struct ApiReply {
    pub status: u16,
    pub body: Value,
}

impl ApiReply {
    pub fn network_error(message: impl Into<String>) -> Self {
        Self {
            status: 0,
            body: json!({ "code": CODE_NO_NETWORK, "message": message.into() }),
        }
    }

    pub fn budget_out() -> Self {
        Self {
            status: 0,
            body: json!({ "code": CODE_BUDGET_OUT, "message": "本轮时间预算已耗尽" }),
        }
    }

    pub fn code(&self) -> i64 {
        self.body
            .get("code")
            .and_then(Value::as_i64)
            .unwrap_or(if self.status == 0 {
                CODE_NO_NETWORK
            } else {
                -1
            })
    }

    /// 业务成功：HTTP 2xx 且（无 code 字段 或 code == 0）。
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
            && self
                .body
                .get("code")
                .and_then(Value::as_i64)
                .is_none_or(|code| code == 0)
    }

    pub fn message(&self) -> String {
        for key in ["message", "msg", "error_msg", "error"] {
            if let Some(text) = self.body.get(key).and_then(Value::as_str) {
                if !text.trim().is_empty() {
                    return text.trim().to_string();
                }
            }
        }
        if self.status == 0 {
            "网络不可达".to_string()
        } else {
            format!("HTTP {}", self.status)
        }
    }

    pub fn budget_exhausted(&self) -> bool {
        self.code() == CODE_BUDGET_OUT
    }

    /// 是否为「需要人工关注」的硬失败（5xx / 网络 / 预算）。
    /// 4xx 视为业务常态，不算硬失败。
    pub fn is_hard_failure(&self) -> bool {
        self.status == 0 || self.status >= 500
    }
}

/// 穿透 `data` / `result` / `resp` / `response` 取出业务对象。
pub fn payload(reply: &ApiReply) -> &Value {
    let mut node = &reply.body;
    for key in ["data", "result", "resp", "response"] {
        match node.get(key) {
            Some(inner) if inner.is_object() || inner.is_array() => node = inner,
            _ => break,
        }
    }
    node
}

fn delays_for(class: RetryClass, status: u16) -> &'static [u64] {
    // 写操作不重试：直接给空表。
    if matches!(class, RetryClass::Write) {
        return &[];
    }
    if status == 0 {
        &NETWORK_DELAYS
    } else if status >= 500 {
        &SERVER_DELAYS
    } else {
        &[]
    }
}

/// 发起一次调用（相对 `GROWTH_HOST`），按 [`RetryClass`] 决定是否退避重试。
pub async fn request(
    budget: &TimeBudget,
    path: &str,
    method: &str,
    body: Option<Value>,
    headers: &HashMap<String, String>,
    class: RetryClass,
) -> ApiReply {
    let url = format!("{GROWTH_HOST}{path}");
    let mut network_attempt = 0usize;
    let mut server_attempt = 0usize;

    loop {
        if budget.exhausted() {
            return ApiReply::budget_out();
        }
        let (status, _headers, text) =
            http_request_raw(&url, method, body.clone(), Some(headers), None, true).await;
        let parsed: Value = if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or_else(|_| {
                json!({ "code": if status == 0 { CODE_NO_NETWORK } else { status as i64 },
                        "message": text.chars().take(300).collect::<String>() })
            })
        };
        let reply = ApiReply {
            status,
            body: parsed,
        };

        let delays = delays_for(class, status);
        if delays.is_empty() {
            return reply;
        }
        let index = if status == 0 {
            network_attempt += 1;
            network_attempt - 1
        } else {
            server_attempt += 1;
            server_attempt - 1
        };
        let Some(delay) = delays.get(index).copied() else {
            return reply;
        };
        // 预算不够「退避 + 一次请求」时直接放弃，别把时间耗在等待上。
        if budget.remaining() <= Duration::from_secs(delay + 31) {
            return reply;
        }
        tokio::time::sleep(Duration::from_secs(delay)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_counts_down_and_expires() {
        let budget = TimeBudget::new(60);
        assert!(!budget.exhausted());
        assert!(budget.remaining() <= Duration::from_secs(60));
        let spent = TimeBudget {
            deadline: Instant::now(),
        };
        assert!(spent.exhausted());
    }

    #[test]
    fn write_class_never_retries() {
        assert!(delays_for(RetryClass::Write, 0).is_empty());
        assert!(delays_for(RetryClass::Write, 503).is_empty());
        assert!(delays_for(RetryClass::Write, 401).is_empty());
    }

    #[test]
    fn read_class_retries_network_and_5xx_only() {
        assert_eq!(delays_for(RetryClass::Read, 0), &NETWORK_DELAYS);
        assert_eq!(delays_for(RetryClass::Read, 502), &SERVER_DELAYS);
        assert!(delays_for(RetryClass::Read, 404).is_empty());
        assert!(delays_for(RetryClass::Read, 403).is_empty());
    }

    #[test]
    fn reply_success_requires_2xx_and_zero_code() {
        let ok = ApiReply {
            status: 200,
            body: json!({"code": 0, "data": {"a": 1}}),
        };
        assert!(ok.ok());
        // 无 code 字段的 2xx 也算成功（部分只读接口直接返回数据）。
        let plain = ApiReply {
            status: 200,
            body: json!({"data": {}}),
        };
        assert!(plain.ok());
        let business = ApiReply {
            status: 200,
            body: json!({"code": 10001}),
        };
        assert!(!business.ok());
        assert!(!business.is_hard_failure(), "业务码不算硬失败");
    }

    #[test]
    fn hard_failure_covers_network_and_5xx() {
        assert!(ApiReply::network_error("boom").is_hard_failure());
        assert!(ApiReply {
            status: 503,
            body: json!({})
        }
        .is_hard_failure());
        assert!(!ApiReply {
            status: 403,
            body: json!({})
        }
        .is_hard_failure());
    }

    #[test]
    fn message_prefers_readable_fields() {
        let reply = ApiReply {
            status: 400,
            body: json!({"code": 1, "msg": "date is not broken, no makeup needed"}),
        };
        assert_eq!(reply.message(), "date is not broken, no makeup needed");
        // network_error 保留传入原因，便于定位；没有原因时才回落到「网络不可达」。
        assert_eq!(ApiReply::network_error("x").message(), "x");
        assert_eq!(ApiReply::network_error("").message(), "网络不可达");
    }

    #[test]
    fn payload_unwraps_common_envelopes() {
        let reply = ApiReply {
            status: 200,
            body: json!({"code": 0, "data": {"streak": {"days": 7}}}),
        };
        assert_eq!(payload(&reply)["streak"]["days"], 7);
        let flat = ApiReply {
            status: 200,
            body: json!({"balance": 3}),
        };
        assert_eq!(payload(&flat)["balance"], 3);
    }
}
