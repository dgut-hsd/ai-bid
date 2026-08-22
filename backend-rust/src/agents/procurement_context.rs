//! Procurement Context → Applicable Rule Set Resolver
//!
//! 根据采购对象（goods/service/construction）和采购方式（open_tender / competitive_consultation / …）
//! 路由到正确的规则体系（MofOrder87 / CompetitiveConsultation214 / MofOrder74 / …）。
//!
//! ## 职责
//!
//! - 输入 ProcurementContext → 输出 RuleResolution
//! - 纯函数、无 I/O、无全局状态、无 panic
//! - **不包含任何量化阈值**（30%、60%、2%、20日 等属于各 RuleSet/Tool）
//!
//! ## 调用方
//!
//! 未来 verify_bid_deposit、validate_scoring_formula 等 Tool 调用此 Resolver 获取 applicable_rule_set。

use std::fmt;

// ─── ProcurementContext ────────────────────────────────────────

/// 采购上下文 — Resolver 的最小输入。
#[derive(Debug, Clone, PartialEq)]
pub struct ProcurementContext {
    /// 采购对象：goods / service / construction
    pub procurement_object: String,
    /// 采购方式
    pub procurement_method: String,
    /// 是否政府采购（区分国企/事业单位采购）。
    /// false 时不应套用财政部规章（MofOrder87 / CompetitiveConsultation214 / MofOrder74 等）。
    pub is_government_procurement: bool,
    /// 评审方式（可选）。缺失时 selection_method 可能为 Unspecified。
    pub evaluation_method: Option<String>,
}

// ─── RuleSet ───────────────────────────────────────────────────

/// 适用的规则体系。
///
/// 每个枚举对应一部/一组具体规章，不是粗粒度"政府采购"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuleSet {
    /// 87 号令：政府采购货物/服务招标（公开/邀请）
    MofOrder87,
    /// 财库〔2014〕214 号：竞争性磋商
    CompetitiveConsultation214,
    /// 74 号令 — 竞争性谈判
    MofOrder74Negotiation,
    /// 74 号令 — 询价
    MofOrder74Inquiry,
    /// 74 号令 — 单一来源
    MofOrder74SingleSource,
    /// 工程招标：招标投标法体系（无论 is_government_procurement）
    ConstructionTendering,
    /// 无法确定
    Unknown,
}

impl fmt::Display for RuleSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleSet::MofOrder87 => write!(f, "MofOrder87"),
            RuleSet::CompetitiveConsultation214 => write!(f, "CompetitiveConsultation214"),
            RuleSet::MofOrder74Negotiation => write!(f, "MofOrder74Negotiation"),
            RuleSet::MofOrder74Inquiry => write!(f, "MofOrder74Inquiry"),
            RuleSet::MofOrder74SingleSource => write!(f, "MofOrder74SingleSource"),
            RuleSet::ConstructionTendering => write!(f, "ConstructionTendering"),
            RuleSet::Unknown => write!(f, "Unknown"),
        }
    }
}

// ─── SelectionMethod ───────────────────────────────────────────

/// 评审/定标方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SelectionMethod {
    /// 综合评分法（价格权重规则适用）
    ComprehensiveScoring,
    /// 最低评标价法（货物/服务招标）
    LowestEvaluatedPrice,
    /// 最低最终报价（竞争性谈判、询价）
    LowestFinalPrice,
    /// 单一来源协商定价
    NegotiatedPrice,
    /// 其他
    Other,
    /// Context 信息不足以确定
    Unspecified,
}

impl fmt::Display for SelectionMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SelectionMethod::ComprehensiveScoring => write!(f, "ComprehensiveScoring"),
            SelectionMethod::LowestEvaluatedPrice => write!(f, "LowestEvaluatedPrice"),
            SelectionMethod::LowestFinalPrice => write!(f, "LowestFinalPrice"),
            SelectionMethod::NegotiatedPrice => write!(f, "NegotiatedPrice"),
            SelectionMethod::Other => write!(f, "Other"),
            SelectionMethod::Unspecified => write!(f, "Unspecified"),
        }
    }
}

// ─── NormalizedProcurementMethod ────────────────────────────────

/// 内部枚举 — 保证 route() exhaustive match，无需 unreachable!()。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NormMethod {
    OpenTender,
    InvitedTender,
    CompetitiveConsultation,
    CompetitiveNegotiation,
    Inquiry,
    SingleSource,
}

// ─── Resolution ────────────────────────────────────────────────

/// Resolver 输出。
#[derive(Debug, Clone, PartialEq)]
pub struct RuleResolution {
    pub rule_set: RuleSet,
    pub selection_method: SelectionMethod,
    pub status: ResolutionStatus,
    /// 人类可读的判定理由
    pub reason: String,
}

/// Resolver 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolutionStatus {
    /// 成功解析
    Resolved,
    /// 输入组合在法律上不存在（如 service + inquiry）
    InvalidCombination,
    /// 缺少必要 Context（如非政府采购+开放招标+货物）
    InsufficientContext,
    /// 未识别的 procurement_method
    InvalidInput,
}

// ─── Resolver ──────────────────────────────────────────────────

/// 主路由函数：ProcurementContext → RuleResolution
///
/// 不 panic、无 I/O。
pub fn resolve_rule_set(ctx: &ProcurementContext) -> RuleResolution {
    let method = match normalize_method(&ctx.procurement_method) {
        Some((m, _raw)) => m,
        None => {
            return RuleResolution {
                rule_set: RuleSet::Unknown,
                selection_method: SelectionMethod::Unspecified,
                status: ResolutionStatus::InvalidInput,
                reason: format!(
                    "Unrecognized procurement_method: '{}'. \
                     Valid: open_tender, invited_tender, competitive_consultation, \
                     competitive_negotiation, inquiry, single_source",
                    ctx.procurement_method
                ),
            };
        }
    };

    let object = normalize_object(&ctx.procurement_object);
    let (rule_set, selection_method, status, reason) = route(method, object, ctx);
    RuleResolution { rule_set, selection_method, status, reason }
}

// ─── 内部归一化 ────────────────────────────────────────────────

/// 返回 (NormMethod, 归一化后名称)。
fn normalize_method(raw: &str) -> Option<(NormMethod, &'static str)> {
    match raw.to_lowercase().as_str() {
        "open_tender" | "公开招标" => Some((NormMethod::OpenTender, "open_tender")),
        "invited_tender" | "邀请招标" => Some((NormMethod::InvitedTender, "invited_tender")),
        "competitive_consultation" | "竞争性磋商" => Some((NormMethod::CompetitiveConsultation, "competitive_consultation")),
        "competitive_negotiation" | "竞争性谈判" => Some((NormMethod::CompetitiveNegotiation, "competitive_negotiation")),
        "inquiry" | "询价" => Some((NormMethod::Inquiry, "inquiry")),
        "single_source" | "单一来源" | "单一来源采购" => Some((NormMethod::SingleSource, "single_source")),
        _ => None,
    }
}

fn normalize_object(raw: &str) -> &str {
    match raw.to_lowercase().as_str() {
        "goods" | "货物" => "goods",
        "service" | "服务" => "service",
        "construction" | "工程" => "construction",
        _ => raw, // 不识别也保留原文
    }
}

// ─── 非政府采购守卫 ────────────────────────────────────────────

/// 返回 Some(insufficient_reason) 如果非政府采购不应套用财政部规章。
fn check_gov_only(ctx: &ProcurementContext, rule_label: &str) -> Option<String> {
    if !ctx.is_government_procurement {
        Some(format!(
            "Not government procurement — cannot apply {}. \
             Set is_government_procurement=true if this is a government procurement project.",
            rule_label
        ))
    } else {
        None
    }
}

// ─── 核心路由矩阵 ──────────────────────────────────────────────

fn route(
    method: NormMethod,
    object: &str,
    ctx: &ProcurementContext,
) -> (RuleSet, SelectionMethod, ResolutionStatus, String) {
    match method {
        // ── 公开招标 / 邀请招标 ──
        NormMethod::OpenTender | NormMethod::InvitedTender => match object {
            "goods" | "service" => {
                // 非政府采购 → 不返回 MofOrder87
                if let Some(reason) = check_gov_only(ctx, "MofOrder87") {
                    let sm = resolve_selection_method_for_tender(ctx);
                    return (RuleSet::Unknown, sm, ResolutionStatus::InsufficientContext, reason);
                }
                let sm = resolve_selection_method_for_tender(ctx);
                (RuleSet::MofOrder87, sm, ResolutionStatus::Resolved,
                 format!("MofOrder87: {} + {} (gov procurement goods/service tender)",
                         method_name(method), object))
            }
            "construction" => {
                // 工程招标总是走投标法体系，不论 is_government_procurement
                let sm = resolve_selection_method_for_tender(ctx);
                let gov_note = if ctx.is_government_procurement { " (government procurement)" } else { "" };
                (RuleSet::ConstructionTendering, sm, ResolutionStatus::Resolved,
                 format!("ConstructionTendering: {} + construction{} — Tendering Law regime",
                         method_name(method), gov_note))
            }
            other => (
                RuleSet::Unknown, SelectionMethod::Unspecified,
                ResolutionStatus::InvalidCombination,
                format!("Unrecognized procurement_object '{}' for tender", other),
            ),
        },

        // ── 竞争性磋商 ──
        NormMethod::CompetitiveConsultation => match object {
            "goods" | "service" => {
                if let Some(reason) = check_gov_only(ctx, "CompetitiveConsultation214") {
                    return (RuleSet::Unknown, SelectionMethod::Unspecified,
                            ResolutionStatus::InsufficientContext, reason);
                }
                (RuleSet::CompetitiveConsultation214, SelectionMethod::ComprehensiveScoring,
                 ResolutionStatus::Resolved,
                 format!("CompetitiveConsultation214: consultation for {}", object))
            }
            "construction" => {
                if let Some(reason) = check_gov_only(ctx, "CompetitiveConsultation214") {
                    return (RuleSet::Unknown, SelectionMethod::Unspecified,
                            ResolutionStatus::InsufficientContext, reason);
                }
                (RuleSet::CompetitiveConsultation214, SelectionMethod::ComprehensiveScoring,
                 ResolutionStatus::Resolved,
                 "CompetitiveConsultation214: consultation for construction (price weight rules may not apply)".into())
            }
            other => (
                RuleSet::Unknown, SelectionMethod::Unspecified,
                ResolutionStatus::InvalidCombination,
                format!("Consultation not applicable for '{}'", other),
            ),
        },

        // ── 竞争性谈判（74号令）──
        NormMethod::CompetitiveNegotiation => match object {
            "goods" | "service" => {
                if let Some(reason) = check_gov_only(ctx, "MofOrder74 (Negotiation)") {
                    return (RuleSet::Unknown, SelectionMethod::Unspecified,
                            ResolutionStatus::InsufficientContext, reason);
                }
                (RuleSet::MofOrder74Negotiation, SelectionMethod::LowestFinalPrice,
                 ResolutionStatus::Resolved,
                 format!("MofOrder74Negotiation: negotiation for {}", object))
            }
            "construction" => {
                if let Some(reason) = check_gov_only(ctx, "MofOrder74 (Negotiation)") {
                    return (RuleSet::Unknown, SelectionMethod::Unspecified,
                            ResolutionStatus::InsufficientContext, reason);
                }
                (RuleSet::MofOrder74Negotiation, SelectionMethod::LowestFinalPrice,
                 ResolutionStatus::Resolved,
                 "MofOrder74Negotiation: negotiation for construction (government procurement)".into())
            }
            other => (
                RuleSet::Unknown, SelectionMethod::Unspecified,
                ResolutionStatus::InvalidCombination,
                format!("Negotiation not applicable for '{}'", other),
            ),
        },

        // ── 询价（74号令，仅货物）──
        NormMethod::Inquiry => match object {
            "goods" => {
                if let Some(reason) = check_gov_only(ctx, "MofOrder74 (Inquiry)") {
                    return (RuleSet::Unknown, SelectionMethod::Unspecified,
                            ResolutionStatus::InsufficientContext, reason);
                }
                (RuleSet::MofOrder74Inquiry, SelectionMethod::LowestFinalPrice,
                 ResolutionStatus::Resolved,
                 "MofOrder74Inquiry: inquiry for goods".into())
            }
            "service" => (
                RuleSet::Unknown, SelectionMethod::Unspecified,
                ResolutionStatus::InvalidCombination,
                "Inquiry is only applicable to goods (service not allowed)".into(),
            ),
            "construction" => (
                RuleSet::Unknown, SelectionMethod::Unspecified,
                ResolutionStatus::InvalidCombination,
                "Inquiry is only applicable to goods (construction not allowed)".into(),
            ),
            other => (
                RuleSet::Unknown, SelectionMethod::Unspecified,
                ResolutionStatus::InvalidCombination,
                format!("Inquiry not applicable for '{}'", other),
            ),
        },

        // ── 单一来源（74号令第四章）──
        NormMethod::SingleSource => match object {
            "goods" | "service" => {
                if let Some(reason) = check_gov_only(ctx, "MofOrder74 (Single Source)") {
                    return (RuleSet::Unknown, SelectionMethod::Unspecified,
                            ResolutionStatus::InsufficientContext, reason);
                }
                (RuleSet::MofOrder74SingleSource, SelectionMethod::NegotiatedPrice,
                 ResolutionStatus::Resolved,
                 format!("MofOrder74SingleSource: single source for {}", object))
            }
            "construction" => {
                if let Some(reason) = check_gov_only(ctx, "MofOrder74 (Single Source)") {
                    return (RuleSet::Unknown, SelectionMethod::Unspecified,
                            ResolutionStatus::InsufficientContext, reason);
                }
                (RuleSet::MofOrder74SingleSource, SelectionMethod::NegotiatedPrice,
                 ResolutionStatus::Resolved,
                 "MofOrder74SingleSource: single source for construction (government procurement)".into())
            }
            other => (
                RuleSet::Unknown, SelectionMethod::Unspecified,
                ResolutionStatus::InvalidCombination,
                format!("Single source not applicable for '{}'", other),
            ),
        },
    }
}

fn method_name(m: NormMethod) -> &'static str {
    match m {
        NormMethod::OpenTender => "open_tender",
        NormMethod::InvitedTender => "invited_tender",
        NormMethod::CompetitiveConsultation => "competitive_consultation",
        NormMethod::CompetitiveNegotiation => "competitive_negotiation",
        NormMethod::Inquiry => "inquiry",
        NormMethod::SingleSource => "single_source",
    }
}

/// 招标场景的 SelectionMethod：Context 有 eval_method 就用，否则 Unspecified。
fn resolve_selection_method_for_tender(ctx: &ProcurementContext) -> SelectionMethod {
    if let Some(ref em) = ctx.evaluation_method {
        match em.to_lowercase().as_str() {
            "comprehensive_scoring" | "综合评分法" => SelectionMethod::ComprehensiveScoring,
            "lowest_evaluated_price" | "最低评标价法" => SelectionMethod::LowestEvaluatedPrice,
            _ => SelectionMethod::Other,
        }
    } else {
        SelectionMethod::Unspecified
    }
}

// ─── 测试 ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(method: &str, object: &str) -> ProcurementContext {
        ProcurementContext {
            procurement_method: method.to_string(),
            procurement_object: object.to_string(),
            is_government_procurement: true,
            evaluation_method: None,
        }
    }

    fn ctx_non_gov(method: &str, object: &str) -> ProcurementContext {
        ProcurementContext {
            procurement_method: method.to_string(),
            procurement_object: object.to_string(),
            is_government_procurement: false,
            evaluation_method: None,
        }
    }

    fn ctx_with_eval(method: &str, object: &str, eval: &str) -> ProcurementContext {
        ProcurementContext {
            procurement_method: method.to_string(),
            procurement_object: object.to_string(),
            is_government_procurement: true,
            evaluation_method: Some(eval.to_string()),
        }
    }

    // ── Gov Tender × 对象 ────────────────────────────────────

    #[test]
    fn open_tender_goods() {
        let r = resolve_rule_set(&ctx("open_tender", "goods"));
        assert_eq!(r.rule_set, RuleSet::MofOrder87);
        assert_eq!(r.selection_method, SelectionMethod::Unspecified);
        assert_eq!(r.status, ResolutionStatus::Resolved);
    }

    #[test]
    fn open_tender_service() {
        let r = resolve_rule_set(&ctx("open_tender", "service"));
        assert_eq!(r.rule_set, RuleSet::MofOrder87);
        assert_eq!(r.status, ResolutionStatus::Resolved);
    }

    #[test]
    fn open_tender_construction() {
        let r = resolve_rule_set(&ctx("open_tender", "construction"));
        assert_eq!(r.rule_set, RuleSet::ConstructionTendering);
        assert_eq!(r.status, ResolutionStatus::Resolved);
    }

    #[test]
    fn invited_tender_goods() {
        let r = resolve_rule_set(&ctx("invited_tender", "goods"));
        assert_eq!(r.rule_set, RuleSet::MofOrder87);
    }

    #[test]
    fn invited_tender_service() {
        let r = resolve_rule_set(&ctx("invited_tender", "service"));
        assert_eq!(r.rule_set, RuleSet::MofOrder87);
    }

    #[test]
    fn invited_tender_construction() {
        let r = resolve_rule_set(&ctx("invited_tender", "construction"));
        assert_eq!(r.rule_set, RuleSet::ConstructionTendering);
    }

    // ── Tender × Selection Method ────────────────────────────

    #[test]
    fn tender_with_comprehensive_scoring() {
        let r = resolve_rule_set(&ctx_with_eval("open_tender", "goods", "comprehensive_scoring"));
        assert_eq!(r.selection_method, SelectionMethod::ComprehensiveScoring);
    }

    #[test]
    fn tender_with_lowest_evaluated_price() {
        let r = resolve_rule_set(&ctx_with_eval("open_tender", "goods", "lowest_evaluated_price"));
        assert_eq!(r.selection_method, SelectionMethod::LowestEvaluatedPrice);
    }

    #[test]
    fn tender_with_other_eval_method() {
        let r = resolve_rule_set(&ctx_with_eval("open_tender", "goods", "quality_based"));
        assert_eq!(r.selection_method, SelectionMethod::Other);
    }

    // ── Consultation ─────────────────────────────────────────

    #[test]
    fn consultation_goods() {
        let r = resolve_rule_set(&ctx("competitive_consultation", "goods"));
        assert_eq!(r.rule_set, RuleSet::CompetitiveConsultation214);
        assert_eq!(r.selection_method, SelectionMethod::ComprehensiveScoring);
    }

    #[test]
    fn consultation_service() {
        let r = resolve_rule_set(&ctx("competitive_consultation", "service"));
        assert_eq!(r.rule_set, RuleSet::CompetitiveConsultation214);
    }

    #[test]
    fn consultation_construction() {
        let r = resolve_rule_set(&ctx("competitive_consultation", "construction"));
        assert_eq!(r.rule_set, RuleSet::CompetitiveConsultation214);
    }

    // ── Negotiation ──────────────────────────────────────────

    #[test]
    fn negotiation_goods() {
        let r = resolve_rule_set(&ctx("competitive_negotiation", "goods"));
        assert_eq!(r.rule_set, RuleSet::MofOrder74Negotiation);
        assert_eq!(r.selection_method, SelectionMethod::LowestFinalPrice);
    }

    #[test]
    fn negotiation_service() {
        let r = resolve_rule_set(&ctx("competitive_negotiation", "service"));
        assert_eq!(r.rule_set, RuleSet::MofOrder74Negotiation);
    }

    #[test]
    fn negotiation_construction_gov_procurement() {
        let r = resolve_rule_set(&ctx("competitive_negotiation", "construction"));
        assert_eq!(r.rule_set, RuleSet::MofOrder74Negotiation);
    }

    // ── Inquiry ──────────────────────────────────────────────

    #[test]
    fn inquiry_goods() {
        let r = resolve_rule_set(&ctx("inquiry", "goods"));
        assert_eq!(r.rule_set, RuleSet::MofOrder74Inquiry);
        assert_eq!(r.selection_method, SelectionMethod::LowestFinalPrice);
    }

    #[test]
    fn inquiry_service_invalid() {
        let r = resolve_rule_set(&ctx("inquiry", "service"));
        assert_eq!(r.status, ResolutionStatus::InvalidCombination);
    }

    #[test]
    fn inquiry_construction_invalid() {
        let r = resolve_rule_set(&ctx("inquiry", "construction"));
        assert_eq!(r.status, ResolutionStatus::InvalidCombination);
    }

    // ── Single Source ────────────────────────────────────────

    #[test]
    fn single_source_goods() {
        let r = resolve_rule_set(&ctx("single_source", "goods"));
        assert_eq!(r.rule_set, RuleSet::MofOrder74SingleSource);
        assert_eq!(r.selection_method, SelectionMethod::NegotiatedPrice);
    }

    #[test]
    fn single_source_service() {
        let r = resolve_rule_set(&ctx("single_source", "service"));
        assert_eq!(r.rule_set, RuleSet::MofOrder74SingleSource);
    }

    #[test]
    fn single_source_construction_gov() {
        let r = resolve_rule_set(&ctx("single_source", "construction"));
        assert_eq!(r.rule_set, RuleSet::MofOrder74SingleSource);
    }

    // ── Invalid / Unknown ────────────────────────────────────

    #[test]
    fn unknown_method() {
        let r = resolve_rule_set(&ctx("sealed_bid", "goods"));
        assert_eq!(r.status, ResolutionStatus::InvalidInput);
        assert_eq!(r.rule_set, RuleSet::Unknown);
    }

    #[test]
    fn unknown_object_for_tender() {
        let r = resolve_rule_set(&ctx("open_tender", "intangible_asset"));
        assert_eq!(r.status, ResolutionStatus::InvalidCombination);
    }

    // ── 中文输入 ─────────────────────────────────────────────

    #[test]
    fn cn_open_tender_goods() {
        let r = resolve_rule_set(&ctx("公开招标", "货物"));
        assert_eq!(r.rule_set, RuleSet::MofOrder87);
    }

    #[test]
    fn cn_consultation_service() {
        let r = resolve_rule_set(&ctx("竞争性磋商", "服务"));
        assert_eq!(r.rule_set, RuleSet::CompetitiveConsultation214);
    }

    #[test]
    fn cn_negotiation_goods() {
        let r = resolve_rule_set(&ctx("竞争性谈判", "货物"));
        assert_eq!(r.rule_set, RuleSet::MofOrder74Negotiation);
    }

    #[test]
    fn cn_inquiry_goods() {
        let r = resolve_rule_set(&ctx("询价", "货物"));
        assert_eq!(r.rule_set, RuleSet::MofOrder74Inquiry);
    }

    #[test]
    fn cn_single_source_goods() {
        let r = resolve_rule_set(&ctx("单一来源", "货物"));
        assert_eq!(r.rule_set, RuleSet::MofOrder74SingleSource);
    }

    // ── 中文 eval method ─────────────────────────────────────

    #[test]
    fn tender_with_eval_comprehensive_correct_selection() {
        let r = resolve_rule_set(&ctx_with_eval("open_tender", "goods", "综合评分法"));
        assert_eq!(r.selection_method, SelectionMethod::ComprehensiveScoring);
    }

    #[test]
    fn tender_with_eval_lowest_price_correct_selection() {
        let r = resolve_rule_set(&ctx_with_eval("open_tender", "service", "最低评标价法"));
        assert_eq!(r.selection_method, SelectionMethod::LowestEvaluatedPrice);
    }

    #[test]
    fn test_invalid_combination_reason_contains_details() {
        let r = resolve_rule_set(&ctx("inquiry", "service"));
        assert!(r.reason.contains("inquiry") || r.reason.contains("Inquiry"));
        assert!(!r.reason.is_empty());
    }

    #[test]
    fn test_invalid_input_reason_contains_field() {
        let r = resolve_rule_set(&ctx("nonexistent", "goods"));
        assert!(r.reason.contains("procurement_method"));
    }

    // ── Non-Government Matrix ─────────────────────────────────

    #[test]
    fn non_gov_open_tender_goods() {
        let r = resolve_rule_set(&ctx_non_gov("open_tender", "goods"));
        assert_eq!(r.status, ResolutionStatus::InsufficientContext);
        assert_eq!(r.rule_set, RuleSet::Unknown);
    }

    #[test]
    fn non_gov_open_tender_service() {
        let r = resolve_rule_set(&ctx_non_gov("open_tender", "service"));
        assert_eq!(r.status, ResolutionStatus::InsufficientContext);
        assert_eq!(r.rule_set, RuleSet::Unknown);
    }

    #[test]
    fn non_gov_invited_tender_goods() {
        let r = resolve_rule_set(&ctx_non_gov("invited_tender", "goods"));
        assert_eq!(r.status, ResolutionStatus::InsufficientContext);
    }

    #[test]
    fn non_gov_consultation_goods() {
        let r = resolve_rule_set(&ctx_non_gov("competitive_consultation", "goods"));
        assert_eq!(r.status, ResolutionStatus::InsufficientContext);
    }

    #[test]
    fn non_gov_consultation_service() {
        let r = resolve_rule_set(&ctx_non_gov("competitive_consultation", "service"));
        assert_eq!(r.status, ResolutionStatus::InsufficientContext);
    }

    #[test]
    fn non_gov_negotiation_goods() {
        let r = resolve_rule_set(&ctx_non_gov("competitive_negotiation", "goods"));
        assert_eq!(r.status, ResolutionStatus::InsufficientContext);
    }

    #[test]
    fn non_gov_negotiation_service() {
        let r = resolve_rule_set(&ctx_non_gov("competitive_negotiation", "service"));
        assert_eq!(r.status, ResolutionStatus::InsufficientContext);
    }

    #[test]
    fn non_gov_inquiry_goods() {
        let r = resolve_rule_set(&ctx_non_gov("inquiry", "goods"));
        assert_eq!(r.status, ResolutionStatus::InsufficientContext);
    }

    #[test]
    fn non_gov_single_source_goods() {
        let r = resolve_rule_set(&ctx_non_gov("single_source", "goods"));
        assert_eq!(r.status, ResolutionStatus::InsufficientContext);
    }

    #[test]
    fn non_gov_single_source_service() {
        let r = resolve_rule_set(&ctx_non_gov("single_source", "service"));
        assert_eq!(r.status, ResolutionStatus::InsufficientContext);
    }

    // ── non-gov construction 仍走 ConstructionTendering ──────

    #[test]
    fn non_gov_open_tender_construction_still_tendering_law() {
        let r = resolve_rule_set(&ctx_non_gov("open_tender", "construction"));
        assert_eq!(r.rule_set, RuleSet::ConstructionTendering);
        assert_eq!(r.status, ResolutionStatus::Resolved);
    }

    #[test]
    fn non_gov_invited_tender_construction_still_tendering_law() {
        let r = resolve_rule_set(&ctx_non_gov("invited_tender", "construction"));
        assert_eq!(r.rule_set, RuleSet::ConstructionTendering);
        assert_eq!(r.status, ResolutionStatus::Resolved);
    }

    // ── 大小写 ───────────────────────────────────────────────

    #[test]
    fn uppercase_object_normalized() {
        let r = resolve_rule_set(&ctx("open_tender", "GOODS"));
        assert_eq!(r.rule_set, RuleSet::MofOrder87);
        assert_eq!(r.status, ResolutionStatus::Resolved);
    }

    #[test]
    fn mixed_case_method_normalized() {
        let r = resolve_rule_set(&ctx("Open_Tender", "goods"));
        assert_eq!(r.rule_set, RuleSet::MofOrder87);
    }
}
