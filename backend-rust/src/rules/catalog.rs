//! 规则分类目录 — CATEGORIES 常量、display_name、owner_agent、aliases。
//!
//! 从 `agents/risk_taxonomy.rs` 迁入。15 个 canonical code 保持不变，保证
//! `risk_taxonomy.rs` facade 的 5 个 public 函数签名向后兼容。

/// 15 个稳定风险分类（code, display_name）。
/// 与 `benchmark/risk_policy.py` 的 `CATEGORY_NAMES` 保持一致。
pub const CATEGORIES: &[(&str, &str)] = &[
    ("LOCAL_REGISTRATION", "地域注册限制"),
    ("BRAND_LOCK", "指定品牌且不接受同等产品"),
    ("UNRELATED_CERT", "设置与履约无关的资格条件"),
    ("REGIONAL_PERFORMANCE", "特定区域业绩限制"),
    ("SCALE_THRESHOLD", "以经营规模设置资格门槛"),
    ("SHORT_DEADLINE", "投标准备期不足"),
    ("EXCESSIVE_DEPOSIT", "投标保证金比例过高"),
    ("OEM_AUTHORIZATION", "将厂家授权作为资格条件"),
    ("SUBJECTIVE_SCORING", "主观评分未细化量化"),
    ("LOCAL_AWARD", "本地奖项加分"),
    ("VAGUE_ACCEPTANCE", "验收标准模糊"),
    ("UNBOUNDED_IP", "知识产权责任无限扩大"),
    ("UNILATERAL_CHANGE", "采购人可单方无限变更需求"),
    ("CONFLICTING_DATES", "关键日期相互矛盾"),
    ("UNCLEAR_PENALTY", "违约责任口径不清"),
];

/// 按 canonical code 查中文展示名。
pub fn display_name(code: &str) -> Option<&'static str> {
    CATEGORIES
        .iter()
        .find_map(|(candidate, name)| (*candidate == code).then_some(*name))
}

/// 按 category code 查责任 Agent。
pub fn owner_agent(code: &str) -> &'static str {
    match code {
        "SHORT_DEADLINE" | "EXCESSIVE_DEPOSIT" | "OEM_AUTHORIZATION" => "ProcedureAgent",
        "SUBJECTIVE_SCORING" | "LOCAL_AWARD" => "ScoringAgent",
        "BRAND_LOCK" | "UNRELATED_CERT" | "SCALE_THRESHOLD" => "DemandAgent",
        "VAGUE_ACCEPTANCE" | "UNBOUNDED_IP" | "UNILATERAL_CHANGE" | "UNCLEAR_PENALTY" => {
            "ContractAgent"
        }
        "LOCAL_REGISTRATION" | "REGIONAL_PERFORMANCE" => "SemanticRiskAgent",
        "CONFLICTING_DATES" => "FactCheckAgent",
        _ => "RuleEngineAgent",
    }
}
