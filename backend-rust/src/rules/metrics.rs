//! Document-level metric extraction — data source for field_compare matcher.
//!
//! Extracts deposit ratio / amount / estimate_price / etc. from clause text.
//! Intentionally a "best-effort within one clause" implementation: the real
//! pipeline cannot carry cross-clause context (see plan for details).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DocumentMetrics {
    #[serde(default)]
    pub bid_deadline: Option<String>,
    #[serde(default)]
    pub issue_date: Option<String>,
    #[serde(default)]
    pub estimate_price: Option<f64>,
    #[serde(default)]
    pub deposit_ratio: Option<f64>,
    #[serde(default)]
    pub deposit_amount: Option<f64>,
    #[serde(default)]
    pub preparation_days: Option<i64>,
}

impl DocumentMetrics {
    /// Extract clause-level metrics from raw clause text.
    /// Regex-based; never panics — parse failures simply produce None.
    pub fn extract_from_clause_text(text: &str) -> Self {
        let mut m = Self::default();
        m.deposit_ratio = extract_deposit_ratio(text);
        m.deposit_amount = extract_deposit_amount(text);
        m.estimate_price = extract_estimate_price(text);
        m
    }
}

// ── individual extractors ─────────────────────────────────────

/// Parse a Chinese number suffix: `万元`=×10_000, `元`=×1, `亿`=×100_000_000
fn apply_suffix(value: f64, suffix: &str) -> f64 {
    match suffix {
        s if s.contains("万元") || s.contains("万块") || s.contains("万圆") => value * 10_000.0,
        s if s.contains("亿") => value * 100_000_000.0,
        _ => value, // 元 or unknown
    }
}

fn extract_percentage(text: &str, anchor: &str) -> Option<f64> {
    // match: "{anchor}...[数字]%" or "[数字]%...{anchor}"
    // Use a simple two-direction scan: find anchor, find nearest % number.
    use regex::Regex;
    static RE_NUM_PCT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE_NUM_PCT.get_or_init(|| Regex::new(r"(\d+(?:\.\d+)?)\s*%").unwrap());

    let anchor_positions: Vec<usize> = text.match_indices(anchor).map(|(i, _)| i).collect();
    let mut best: Option<(usize, f64)> = None;
    for cap in re.captures_iter(text) {
        let num: f64 = cap[1].parse().ok()?;
        let ratio = num / 100.0;
        let match_pos = cap.get(0)?.start();
        // distance to nearest anchor occurrence
        let dist = anchor_positions
            .iter()
            .map(|a| (*a as isize - match_pos as isize).unsigned_abs())
            .min()
            .unwrap_or(usize::MAX);
        if dist < 40 {
            // within reasonable token distance
            if best.map(|(d, _)| dist < d).unwrap_or(true) {
                best = Some((dist, ratio));
            }
        }
    }
    best.map(|(_, v)| v)
}

fn extract_deposit_ratio(text: &str) -> Option<f64> {
    // Anchors ordered most-specific → least-specific: "投标保证金" 优先于裸
    // "保证金"，避免与"履约保证金"等混淆；但仍需覆盖 "6%的保证金" 这类
    // 百分比前置于"保证金"的写法（blind-v2 BLIND-007-F02）。
    for anchor in ["投标保证金", "保证金比例", "保证金为", "保证金"] {
        if let Some(r) = extract_percentage(text, anchor) {
            return Some(r);
        }
    }
    None
}

fn extract_estimate_price(text: &str) -> Option<f64> {
    use regex::Regex;
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        // "预算(?:金额)?" 兼容 "预算为500万元"（blind-v2 BLIND-002-F02）
        // 与原 "预算金额" 两种写法；"项目预算" 覆盖 "本项目预算为..."。
        Regex::new(r"(估算价|控制价|预算(?:金额)?|最高限价|项目预算)[^0-9]{0,10}(\d+(?:\.\d+)?)\s*(万元|元|亿元)?").unwrap()
    });
    let cap = re.captures(text)?;
    let num: f64 = cap[2].parse().ok()?;
    let suffix = cap.get(3).map(|m| m.as_str()).unwrap_or("元");
    Some(apply_suffix(num, suffix))
}

fn extract_deposit_amount(text: &str) -> Option<f64> {
    use regex::Regex;
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"投标保证金[^0-9]{0,8}(\d+(?:\.\d+)?)\s*(万元|元|亿元)?").unwrap()
    });
    let cap = re.captures(text)?;
    let num: f64 = cap[1].parse().ok()?;
    let suffix = cap.get(2).map(|m| m.as_str()).unwrap_or("元");
    Some(apply_suffix(num, suffix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_deposit_ratio_5pct() {
        let m = DocumentMetrics::extract_from_clause_text("投标保证金不得超过估算价的5%。");
        assert_eq!(m.deposit_ratio, Some(0.05));
    }

    #[test]
    fn extracts_deposit_ratio_1pct() {
        let m = DocumentMetrics::extract_from_clause_text("投标保证金按估算价的1%计取。");
        assert_eq!(m.deposit_ratio, Some(0.01));
    }

    #[test]
    fn extracts_estimate_price_1000_wanyuan() {
        let m = DocumentMetrics::extract_from_clause_text(
            "本项目估算价为1000万元，投标保证金为50万元。",
        );
        assert_eq!(m.estimate_price, Some(10_000_000.0));
        assert_eq!(m.deposit_amount, Some(500_000.0));
    }

    #[test]
    fn no_deposit_returns_none() {
        let m = DocumentMetrics::extract_from_clause_text("本项目不设投标保证金。");
        assert_eq!(m.deposit_ratio, None);
        assert_eq!(m.deposit_amount, None);
    }

    // ── P3: blind-v2 FN 提取回归 ─────────────────────────────────

    #[test]
    fn extracts_deposit_ratio_pct_before_guarantee() {
        // BLIND-007-F02: "采购预算总额6%的保证金" —— 百分比前置于"保证金"
        let m = DocumentMetrics::extract_from_clause_text(
            "投标人应缴纳采购预算总额6%的保证金，并且只允许从基本账户以现金转账方式支付。",
        );
        assert_eq!(m.deposit_ratio, Some(0.06));
    }

    #[test]
    fn extracts_estimate_price_from_budget_only() {
        // BLIND-002-F02: "预算为500万元" —— 非"预算金额"写法
        let m = DocumentMetrics::extract_from_clause_text(
            "本项目预算为500万元，投标保证金固定收取25万元，未在开标前到账的响应无效。",
        );
        assert_eq!(m.estimate_price, Some(5_000_000.0));
        assert_eq!(m.deposit_amount, Some(250_000.0));
    }
}
