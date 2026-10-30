//! 账号健康评分：把「20 个号里哪个快废了」变成一个能扫的数字。
//!
//! 为什么单独成模块：这个分数要跨三类数据（余额快照、限流台账、消耗预测）说话，
//! 塞进任何一侧都会让那侧变成大杂烩。这里只做聚合与解释，不取任何新数据。
//!
//! 评分是**一张可解释的扣分表**，不是黑盒权重：每条原因都有对应的原因码，
//! 前端翻成用户能读的句子。宁可粗，也要每条都能对用户讲清楚。

use serde_json::{json, Value};

/// 健康评分的输入。全部来自已经取到的数据，不额外请求。
pub struct HealthInput<'a> {
    pub remaining: Option<f64>,
    pub capacity: Option<f64>,
    /// 即将到期（7 天内）的额度。
    pub expiring_soon_remaining: Option<f64>,
    /// 是否有仍在生效的限流。
    pub rate_limited: bool,
    pub error: Option<&'a str>,
    /// 预测的剩余天数（`forecast.rs`）。
    pub days_left: Option<f64>,
}

/// 账号健康分：0-100，附**可解释**的原因码。
///
/// 评分不是玄学，就是一张扣分表（见 [`account_health`] 内部）。产品上要的是
/// 「20 个号里哪个快废了」，所以宁可粗一点也要每一条都能对用户讲清楚。
/// 原因码由前端翻译（`ac.hz*`），后端不返回中文，避免 i18n 分裂。
pub fn account_health(input: &HealthInput) -> Value {
    let mut score = 100.0_f64;
    let mut reasons: Vec<&'static str> = Vec::new();

    if input.rate_limited {
        score -= 35.0;
        reasons.push("rateLimited");
    }
    match (input.remaining, input.capacity) {
        (Some(remaining), Some(capacity)) if capacity > 0.0 => {
            if remaining <= 0.0 {
                score -= 50.0;
                reasons.push("empty");
            } else if remaining / capacity < 0.1 {
                // 扣 30 而不是 20：剩 5% 只掉到 80 分仍会被判成 good，等于没报警。
                score -= 30.0;
                reasons.push("lowBalance");
            }
        }
        (Some(remaining), _) if remaining <= 0.0 => {
            score -= 50.0;
            reasons.push("empty");
        }
        _ => {}
    }
    if input.error.is_some() {
        score -= 20.0;
        reasons.push("fetchError");
    }
    if let Some(days_left) = input.days_left {
        if days_left < 3.0 {
            score -= 25.0;
            reasons.push("depletingSoon");
        }
    }
    if let (Some(expiring), Some(remaining)) = (input.expiring_soon_remaining, input.remaining) {
        // 快过期的额度占了大头：不是「不能用」，是「再不用就浪费」。
        if remaining > 0.0 && expiring > remaining * 0.5 {
            score -= 10.0;
            reasons.push("expiringDominant");
        }
    }

    let score = score.clamp(0.0, 100.0);
    let grade = if score >= 80.0 {
        "good"
    } else if score >= 50.0 {
        "warn"
    } else {
        "critical"
    };
    json!({
        "score": score.round() as i64,
        "grade": grade,
        "reasons": reasons,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn health(remaining: Option<f64>, limited: bool) -> Value {
        account_health(&HealthInput {
            remaining,
            capacity: Some(1000.0),
            expiring_soon_remaining: None,
            rate_limited: limited,
            error: None,
            days_left: None,
        })
    }

    /// 健康的号就该是 100 分 good，不要动不动就报警。
    #[test]
    fn healthy_account_scores_full() {
        let result = health(Some(800.0), false);
        assert_eq!(result["score"], 100);
        assert_eq!(result["grade"], "good");
        assert!(result["reasons"].as_array().unwrap().is_empty());
    }

    /// 余额为 0 且被限流 → critical，两条原因都要能对用户讲。
    #[test]
    fn empty_and_limited_is_critical() {
        let result = health(Some(0.0), true);
        assert_eq!(result["grade"], "critical");
        let reasons = result["reasons"].as_array().unwrap();
        assert!(reasons.iter().any(|reason| reason == "empty"));
        assert!(reasons.iter().any(|reason| reason == "rateLimited"));
    }

    /// 低余额与空余额是两档：10% 以下扣 30，落到 warn（不能还是 good）。
    #[test]
    fn low_balance_is_a_warning_not_critical() {
        let result = health(Some(50.0), false);
        assert_eq!(result["score"], 70);
        assert_eq!(result["grade"], "warn");
    }

    /// 分数不会跌破 0。
    #[test]
    fn score_is_clamped() {
        let result = account_health(&HealthInput {
            remaining: Some(0.0),
            capacity: Some(1000.0),
            expiring_soon_remaining: Some(10.0),
            rate_limited: true,
            error: Some("网络错误"),
            days_left: Some(0.2),
        });
        assert_eq!(result["score"], 0);
    }
}
