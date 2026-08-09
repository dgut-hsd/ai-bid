//! `validate_scoring_formula` 工具 — 价格分公式校验。
//!
//! 根据《政府采购货物和服务招标投标管理办法》（财政部令第87号）第55条、
//! 财库〔2014〕214号第24条，校验价格分权重和评分公式类型是否符合法定要求。
//!
//! ## 核心规则（按适用规则体系）
//!
//! ### MofOrder87（公开招标/邀请招标，货物/服务）
//! - 货物价格分 ≥30%，无上限
//! - 服务价格分 ≥10%，无上限
//! - 价格分必须采用低价优先法（平均价→violation）
//!
//! ### CompetitiveConsultation214（竞争性磋商，货物/服务）
//! - 货物价格分 30%-60%
//! - 服务价格分 10%-30%
//! - 价格分必须采用低价优先法
//!
//! ### MofOrder74（竞争性谈判/询价/单一来源）
//! - 非综合评分法场景 → not_applicable
//!
//! ### ConstructionTendering（工程招标）
//! - 不适用87号令/214号 → not_applicable

use anyhow::{Result};
use serde::Deserialize;

use super::AgentTool;
use crate::agents::procurement_context::{self, ProcurementContext, RuleSet, ResolutionStatus};

// ─── 权重常量 ──────────────────────────────────────────────────

/// 87号令：下限，无上限
const GOODS_WEIGHT_MIN_87: f64 = 30.0;
const SERVICE_WEIGHT_MIN_87: f64 = 10.0;

/// 214号：下限+上限
const GOODS_WEIGHT_MIN_214: f64 = 30.0;
const GOODS_WEIGHT_MAX_214: f64 = 60.0;
const SERVICE_WEIGHT_MIN_214: f64 = 10.0;
const SERVICE_WEIGHT_MAX_214: f64 = 30.0;

// ─── 参数 ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ValidateScoringFormulaArgs {
    pub price_weight: f64,
    /// 旧字段保留兼容，新调用应使用 procurement_object
    #[serde(default)]
    pub procurement_category: Option<String>,
    #[serde(default)]
    pub procurement_object: Option<String>,
    #[serde(default)]
    pub procurement_method: Option<String>,
    #[serde(default)]
    pub evaluation_method: Option<String>,
    /// 定价模式：competitive / fixed / nationally_fixed / unknown
    #[serde(default)]
    pub pricing_mode: Option<String>,
    /// 磋商超范围价格权重审批：approved / not_approved / unknown
    #[serde(default)]
    pub special_weight_approval: Option<String>,
    /// 价格评审上下文（替代 pricing_mode 的语义更精确表达）。
    /// normal / uniform_price_standard / article3_item3_project / unknown
    #[serde(default)]
    pub price_evaluation_context: Option<String>,
    pub scoring_formula_type: String,
    #[serde(default)]
    pub formula_description: Option<String>,
}

// ─── 输出 ──────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
struct ScoringFormulaResult {
    status: String,
    applicable_rule_set: String,
    weight_ok: Option<bool>,
    formula_ok: Option<bool>,
    weight_detail: String,
    formula_detail: String,
    risks: Vec<String>,
    suggestion: String,
    legal_basis: String,
}

// ─── 工具实现 ──────────────────────────────────────────────────

fn uncertain_result(reason: &str, suggestion: &str) -> ScoringFormulaResult {
    ScoringFormulaResult {
        status: "uncertain".into(),
        applicable_rule_set: "Unknown".into(),
        weight_ok: None, formula_ok: None,
        weight_detail: reason.to_string(),
        formula_detail: String::new(),
        risks: vec![],
        suggestion: suggestion.to_string(),
        legal_basis: "需确定采购方式/对象/评审方式/定价模式后查询对应法规".into(),
    }
}

pub struct ValidateScoringFormulaTool;

impl ValidateScoringFormulaTool {
    fn validate(args: &ValidateScoringFormulaArgs) -> Result<ScoringFormulaResult> {
        // ── Fail Closed：关键 Context 缺失 → InsufficientContext/Uncertain，不推断默认值 ──
        // 需要 procurement_method（决定 RuleSet）
        let method = match args.procurement_method.as_deref() {
            Some(m) if !m.trim().is_empty() => m,
            _ => {
                return Ok(uncertain_result(
                    "missing procurement_method",
                    "缺少采购方式（procurement_method），无法确定适用规则体系。请提供 open_tender / competitive_consultation 等。",
                ));
            }
        };
        // 需要 procurement_object（决定权重规则）
        let object = match args.procurement_object.as_deref().or(args.procurement_category.as_deref()) {
            Some(o) if !o.trim().is_empty() => o,
            _ => {
                return Ok(uncertain_result(
                    "missing procurement_object",
                    "缺少采购对象（procurement_object），无法确定适用规则体系。请提供 goods / service / construction。",
                ));
            }
        };
        // 需要 evaluation_method（决定是否综合评分价格权重）
        let eval = match args.evaluation_method.as_deref() {
            Some(e) if !e.trim().is_empty() => e,
            _ => {
                return Ok(uncertain_result(
                    "missing evaluation_method",
                    "缺少评审方式（evaluation_method），无法确定是否适用综合评分价格权重规则。",
                ));
            }
        };

        let ctx = ProcurementContext {
            procurement_object: object.to_string(),
            procurement_method: method.to_string(),
            is_government_procurement: true,
            evaluation_method: Some(eval.to_string()),
        };
        let res = procurement_context::resolve_rule_set(&ctx);

        // 无法解析 → uncertain
        if res.status != ResolutionStatus::Resolved || res.rule_set == RuleSet::Unknown {
            return Ok(uncertain_result(
                &format!("rule_set resolution: {:?}", res.status),
                &format!("无法确定适用的规则体系：{}", res.reason),
            ));
        }

        let rs = res.rule_set;

        // ── 价格评审上下文：缺失 → uncertain ──
        let pe_ctx = match args.price_evaluation_context.as_deref() {
            Some(s) if !s.trim().is_empty() => s,
            _ => {
                return Ok(uncertain_result(
                    "missing price_evaluation_context",
                    "缺少价格评审上下文（price_evaluation_context），无法确定价格是否应列入评审因素。请提供 normal / uniform_price_standard / article3_item3_project / unknown。",
                ));
            }
        };

        match pe_ctx {
            "uniform_price_standard" => {
                // 214号24条 & 87号令55条：统一价格标准 → 价格不参评
                let rule_note = match rs {
                    RuleSet::MofOrder87 => "87号令55条：执行国家统一定价标准，价格不列入评审因素。",
                    RuleSet::CompetitiveConsultation214 => "214号24条：执行统一价格标准，价格不列为评分因素。",
                    _ => "统一价格标准下价格不参评。",
                };
                return Ok(ScoringFormulaResult {
                    status: "not_applicable".into(),
                    applicable_rule_set: rs.to_string(),
                    weight_ok: None, formula_ok: None,
                    weight_detail: rule_note.to_string(),
                    formula_detail: String::new(), risks: vec![],
                    suggestion: "执行统一价格标准，价格不作为评审/评分因素。".into(),
                    legal_basis: rule_note.to_string(),
                });
            }
            "article3_item3_project" => {
                // 仅214号适用：第3条第3项项目（艺术品/专利/专有技术/时间数量不确定）
                if rs != RuleSet::CompetitiveConsultation214 {
                    return Ok(uncertain_result(
                        "article3_item3_project not applicable",
                        "article3_item3_project（214号第3条第3项）仅适用于竞争性磋商采购方式。",
                    ));
                }
                return Ok(ScoringFormulaResult {
                    status: "not_applicable".into(),
                    applicable_rule_set: rs.to_string(),
                    weight_ok: None, formula_ok: None,
                    weight_detail: "214号第3条第3项项目（艺术品/专利/专有技术/时间数量不确定导致不能事先计算价格总额），价格不列为评分因素。".into(),
                    formula_detail: String::new(), risks: vec![],
                    suggestion: "214号第3条第3项项目不适用价格评分。".into(),
                    legal_basis: "财库〔2014〕214号第3条第3项、第24条：因艺术品采购、专利、专有技术或服务时间/数量不能确定导致不能事先计算价格总额的，价格不列为评分因素。".into(),
                });
            }
            "normal" => {
                // 正常价格评审，继续
            }
            "unknown" => {
                return Ok(uncertain_result(
                    "price_evaluation_context unknown",
                    "价格评审上下文未知，无法确定价格是否应列入评审因素。需确认。",
                ));
            }
            // 兼容旧 pricing_mode 输入
            "fixed_price" | "nationally_fixed" => {
                // 仅87号令适用
                if rs != RuleSet::MofOrder87 {
                    return Ok(uncertain_result(
                        &format!("fixed_price/nationally_fixed not applicable for {}", rs),
                        "fixed_price/nationally_fixed 例外仅适用于87号令（公开招标/邀请招标）。当前采购方式无法确认此例外是否适用。",
                    ));
                }
                return Ok(ScoringFormulaResult {
                    status: "not_applicable".into(),
                    applicable_rule_set: rs.to_string(),
                    weight_ok: None, formula_ok: None,
                    weight_detail: "87号令55条：执行国家统一定价标准或采用固定价格采购，价格不列为评审因素。".into(),
                    formula_detail: String::new(), risks: vec![],
                    suggestion: "固定价格/国家统一定价采购，价格不作为评审因素。".into(),
                    legal_basis: "87号令55条：执行国家统一定价标准或采用固定价格采购的，价格不列为评审因素。".into(),
                });
            }
            other => {
                return Ok(uncertain_result(
                    &format!("unrecognized price_evaluation_context '{}'", other),
                    "无法识别的价格评审上下文，应使用 normal / uniform_price_standard / article3_item3_project / unknown。",
                ));
            }
        }

        match rs {
            RuleSet::MofOrder87 => Self::validate_under_regime(args, rs,
                GOODS_WEIGHT_MIN_87, None,
                SERVICE_WEIGHT_MIN_87, None,
                "《政府采购货物和服务招标投标管理办法》（财政部令第87号）第55条：\
                 货物项目价格分≥30%、服务项目≥10%，价格分应采用低价优先法。"),
            RuleSet::CompetitiveConsultation214 => Self::validate_under_regime(args, rs,
                GOODS_WEIGHT_MIN_214, Some(GOODS_WEIGHT_MAX_214),
                SERVICE_WEIGHT_MIN_214, Some(SERVICE_WEIGHT_MAX_214),
                "《政府采购竞争性磋商采购方式管理暂行办法》（财库〔2014〕214号）第24条：\
                 综合评分法货物价格权重30%-60%、服务10%-30%，价格分应采用低价优先法。\
                 特殊情况经本级人民政府财政部门审核同意可超出范围设置。"),
            RuleSet::MofOrder74Negotiation | RuleSet::MofOrder74Inquiry |
            RuleSet::MofOrder74SingleSource | RuleSet::ConstructionTendering => {
                Ok(ScoringFormulaResult {
                    status: "not_applicable".into(),
                    applicable_rule_set: rs.to_string(),
                    weight_ok: None, formula_ok: None,
                    weight_detail: format!("{} 不适用综合评分价格权重规则", rs),
                    formula_detail: String::new(),
                    risks: vec![],
                    suggestion: "当前采购方式不使用综合评分法价格权重规则，无需校验。".into(),
                    legal_basis: match rs {
                        RuleSet::ConstructionTendering => "工程招标适用《招标投标法》体系，不适用87号令。".into(),
                        _ => "竞争性谈判/询价/单一来源不采用综合评分法价格权重。".into(),
                    },
                })
            }
            RuleSet::Unknown => unreachable!("handled above"),
        }
    }

    fn validate_under_regime(
        args: &ValidateScoringFormulaArgs,
        rs: RuleSet,
        goods_min: f64, goods_max: Option<f64>,
        srv_min: f64, srv_max: Option<f64>,
        legal_basis: &str,
    ) -> Result<ScoringFormulaResult> {
        let object = args.procurement_object.as_deref()
            .or(args.procurement_category.as_deref()).unwrap_or("");
        let mut risks: Vec<String> = Vec::new();

        let (weight_min, weight_max) = match object {
            "goods" | "货物" => (goods_min, goods_max),
            "service" | "服务" => (srv_min, srv_max),
            "construction" | "工程" => {
                return Ok(ScoringFormulaResult {
                    status: "not_applicable".into(),
                    applicable_rule_set: rs.to_string(),
                    weight_ok: None, formula_ok: None,
                    weight_detail: format!("{} 不适用工程项目的价格权重规则", rs),
                    formula_detail: String::new(), risks: vec![],
                    suggestion: "工程项目价格权重无全国法定强制范围。".into(),
                    legal_basis: legal_basis.into(),
                });
            }
            _ => {
                return Ok(uncertain_result(
                    &format!("无法识别的采购对象 '{}'", object),
                    "需要确认采购对象类型。",
                ));
            }
        };

        // ── 磋商超范围权重审批例外（214号24条）──
        // 超出上限（goods>60 / service>30）时，如有财政部门审批 → 例外合规；
        // 无审批 → violation；审批状态未知 → uncertain。
        let exceeds_max = weight_max.map_or(false, |mx| args.price_weight > mx);
        let is_consultation_214 = rs == RuleSet::CompetitiveConsultation214;

        let weight_ok = args.price_weight >= weight_min
            && weight_max.map_or(true, |mx| args.price_weight <= mx);

        // 仅当磋商 + 超上限 + 有审批 → 例外合规
        let approval_override = if is_consultation_214 && exceeds_max {
            match args.special_weight_approval.as_deref() {
                Some("approved") | Some("已批准") => Some(true),
                Some("not_approved") | Some("未批准") => Some(false),
                _ => None, // unknown / missing → 无法确定是否获批
            }
        } else {
            None
        };

        let weight_ok_final = match approval_override {
            Some(true) => true,
            Some(false) => false,
            None => weight_ok,
        };

        let weight_detail = if approval_override == Some(true) {
            let mx = weight_max.unwrap_or(999.0);
            format!("价格分权重 {}% 超出 {} 品类法定上限 {}%，但经本级人民政府财政部门审核同意，属合规例外（214号24条）。",
                args.price_weight, object, mx)
        } else if weight_ok {
            match weight_max {
                Some(mx) => format!("价格分权重 {}%，在 {} 品类法定范围 {}-{}% 内，合规。",
                    args.price_weight, object, weight_min, mx),
                None => format!("价格分权重 {}%，在 {} 品类法定下限 {}% 以上，合规（该规则体系无上限）。",
                    args.price_weight, object, weight_min),
            }
        } else if args.price_weight < weight_min {
            format!("价格分权重 {}% 低于 {} 品类法定最低要求 {}%。", args.price_weight, object, weight_min)
        } else {
            let mx = weight_max.unwrap_or(999.0);
            format!("价格分权重 {}% 超出 {} 品类法定上限 {}%。", args.price_weight, object, mx)
        };

        // 磋商超上限 + 审批状态未知 → 无法确定 → uncertain
        if is_consultation_214 && exceeds_max && approval_override.is_none() {
            let mx = weight_max.unwrap_or(999.0);
            return Ok(ScoringFormulaResult {
                status: "uncertain".into(),
                applicable_rule_set: rs.to_string(),
                weight_ok: None, formula_ok: None,
                weight_detail: format!(
                    "价格分权重 {}% 超出 {} 品类法定上限 {}%（214号24条）。\
                     特殊情况经财政部门审核同意可超出，但当前未提供 special_weight_approval 审批状态，无法判定。",
                    args.price_weight, object, mx),
                formula_detail: String::new(), risks: vec![],
                suggestion: "需提供本级人民政府财政部门对超范围价格权重的审核批准情况（approved / not_approved）。".into(),
                legal_basis: legal_basis.into(),
            });
        }

        if !weight_ok_final {
            risks.push(weight_detail.clone());
        }

        let formula_lower = args.scoring_formula_type.to_lowercase();
        let (formula_ok, formula_detail) = if formula_lower.contains("最低价") {
            (true, format!("采用最低价法（低价优先），符合{}要求。", rs))
        } else if formula_lower.contains("平均价") {
            risks.push("平均价法违反低价优先法要求：87号令55条/214号24条明确价格分应使用低价优先法。".into());
            (false, format!("采用平均价法，违反{}低价优先法要求。", rs))
        } else {
            // 基准价等 — 需要检查基准价是否=最低价
            let ok = if let Some(ref desc) = args.formula_description {
                !(desc.contains("所有报价平均") || desc.contains("全部报价算术平均") || desc.contains("所有有效报价平均"))
            } else { false };
            if !ok {
                risks.push("基准价方法使用了'所有报价平均'可能违反低价优先法要求。".into());
            }
            (ok, "基准价法：需确认基准价是否等价于最低有效报价。".into())
        };

        let status = if !weight_ok_final || !formula_ok { "violation" } else { "compliant" };

        let suggestion = if status == "compliant" {
            format!("{} 品类价格分权重 {}% 和评分公式均合规（适用：{}）。", object, args.price_weight, rs)
        } else {
            format!("存在违规项：详见 weight_detail / formula_detail / risks。适用规则体系：{}。", rs)
        };

        Ok(ScoringFormulaResult {
            status: status.into(), applicable_rule_set: rs.to_string(),
            weight_ok: Some(weight_ok_final), formula_ok: Some(formula_ok),
            weight_detail, formula_detail, risks, suggestion,
            legal_basis: legal_basis.into(),
        })
    }
}

// ─── AgentTool ─────────────────────────────────────────────────

#[async_trait::async_trait]
impl AgentTool for ValidateScoringFormulaTool {
    fn name(&self) -> &str { "validate_scoring_formula" }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "validate_scoring_formula",
                "description": "校验价格分权重和评分公式是否符合法定要求。\
                    公开招标/邀请招标（货物/服务）适用87号令：货物≥30%、服务≥10%（无上限），低价优先法。\
                    竞争性磋商适用214号第24条：货物30%-60%、服务10%-30%，低价优先法；\
                    特殊情况经本级人民政府财政部门审核同意可超出范围。\
                    平均价法在所有适用场景均违规。谈判/询价/单一来源/工程招标不适用本工具。\
                    固定价格/国家统一定价采购价格不参评。\
                     必须提供 procurement_object / procurement_method / evaluation_method / price_evaluation_context，缺失返回 uncertain。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "price_weight": {"type": "number", "description": "价格分权重（如30.0表示30%）"},
                        "procurement_object": {"type": "string", "enum": ["goods","service","construction"],
                            "description": "采购对象（必填，用于确定规则体系）"},
                        "procurement_method": {"type": "string", "enum": ["open_tender","invited_tender","competitive_consultation","competitive_negotiation","inquiry","single_source"],
                            "description": "采购方式（必填，用于确定规则体系）"},
                        "evaluation_method": {"type": "string", "enum": ["comprehensive_scoring","lowest_evaluated_price"],
                            "description": "评审方式（必填）"},
                        "price_evaluation_context": {"type": "string",
                            "enum": ["normal","uniform_price_standard","article3_item3_project","unknown",
                                     "fixed_price","nationally_fixed"],
                            "description": "价格评审上下文（必填）。normal=正常竞争评审；uniform_price_standard=统一价格标准（87号令/214号价格不参评）；article3_item3_project=214号第3条第3项项目；fixed_price/nationally_fixed=仅87号令适用；unknown=需确认"},
                        "special_weight_approval": {"type": "string", "enum": ["approved","not_approved","unknown"],
                            "description": "磋商超范围价格权重的财政部门审批状态（仅竞争性磋商超上限时需要）。若采购文件未明确，不传此字段，工具将返回 uncertain。不要编造审批状态。"},
                        "scoring_formula_type": {"type": "string", "enum": ["最低价","平均价","基准价"], "description": "评分公式类型"},
                        "formula_description": {"type": "string", "description": "公式详细描述（可选）"}
                    },
                    "required": ["price_weight", "procurement_object", "procurement_method", "evaluation_method", "price_evaluation_context", "scoring_formula_type"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: ValidateScoringFormulaArgs = serde_json::from_value(args)?;
        let result = Self::validate(&parsed)?;
        Ok(serde_json::to_value(&result)?)
    }
}

// ─── 测试 ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn run(pw: f64, obj: &str, method: &str, formula: &str) -> serde_json::Value {
        run_full(pw, obj, method, "comprehensive_scoring", formula, "normal", None)
    }

    fn run_full(
        pw: f64, obj: &str, method: &str, eval: &str, formula: &str,
        pe_ctx: &str, approval: Option<&str>,
    ) -> serde_json::Value {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut json = serde_json::json!({
            "price_weight": pw,
            "procurement_object": obj,
            "procurement_method": method,
            "evaluation_method": eval,
            "price_evaluation_context": pe_ctx,
            "scoring_formula_type": formula
        });
        if let Some(a) = approval {
            json["special_weight_approval"] = serde_json::json!(a);
        }
        rt.block_on(ValidateScoringFormulaTool.execute(json)).unwrap()
    }

    // ── 87号令：只有下限 ──────────────────────────────────────

    #[test]
    fn mof87_goods_30_compliant() {
        let r = run(30.0, "goods", "open_tender", "最低价");
        assert_eq!(r["status"], "compliant");
    }

    #[test]
    fn mof87_goods_70_compliant() { // 87号令无上限
        let r = run(70.0, "goods", "open_tender", "最低价");
        assert_eq!(r["status"], "compliant");
    }

    #[test]
    fn mof87_goods_29_violation() {
        let r = run(29.0, "goods", "open_tender", "最低价");
        assert_eq!(r["status"], "violation");
    }

    #[test]
    fn mof87_service_10_compliant() {
        let r = run(10.0, "service", "open_tender", "最低价");
        assert_eq!(r["status"], "compliant");
    }

    #[test]
    fn mof87_average_violation() {
        let r = run(40.0, "goods", "open_tender", "平均价");
        assert_eq!(r["status"], "violation");
    }

    // ── 214号磋商：有上限 ─────────────────────────────────────

    #[test]
    fn cs214_goods_30_compliant() {
        let r = run(30.0, "goods", "competitive_consultation", "最低价");
        assert_eq!(r["status"], "compliant");
    }

    #[test]
    fn cs214_goods_60_compliant() {
        let r = run(60.0, "goods", "competitive_consultation", "最低价");
        assert_eq!(r["status"], "compliant");
    }

    #[test]
    fn cs214_goods_61_violation_without_approval() {
        let r = run_full(61.0, "goods", "competitive_consultation", "comprehensive_scoring", "最低价", "normal", Some("not_approved"));
        assert_eq!(r["status"], "violation");
    }

    #[test]
    fn cs214_goods_61_uncertain_without_approval_status() {
        // 超上限但未提供审批状态 → uncertain（不能假设没有审批）
        let r = run(61.0, "goods", "competitive_consultation", "最低价");
        assert_eq!(r["status"], "uncertain");
    }

    #[test]
    fn cs214_goods_61_approved_exception() {
        // 超上限 + 财政部门批准 → 合规例外
        let r = run_full(61.0, "goods", "competitive_consultation", "comprehensive_scoring", "最低价", "normal", Some("approved"));
        assert_eq!(r["status"], "compliant");
    }

    #[test]
    fn cs214_service_10_compliant() {
        let r = run(10.0, "service", "competitive_consultation", "最低价");
        assert_eq!(r["status"], "compliant");
    }

    #[test]
    fn cs214_service_31_violation_without_approval() {
        let r = run_full(31.0, "service", "competitive_consultation", "comprehensive_scoring", "最低价", "normal", Some("not_approved"));
        assert_eq!(r["status"], "violation");
    }

    #[test]
    fn cs214_service_31_approved_exception() {
        let r = run_full(31.0, "service", "competitive_consultation", "comprehensive_scoring", "最低价", "normal", Some("approved"));
        assert_eq!(r["status"], "compliant");
    }

    // ── 价格评审上下文例外 ────────────────────────────────────

    #[test]
    fn mof87_fixed_price_not_applicable() { // 87号令：固定价格 → 不参评
        let r = run_full(70.0, "goods", "open_tender", "comprehensive_scoring", "最低价", "fixed_price", None);
        assert_eq!(r["status"], "not_applicable");
    }

    #[test]
    fn mof87_nationally_fixed_not_applicable() { // 87号令：国家统一定价 → 不参评
        let r = run_full(70.0, "goods", "open_tender", "comprehensive_scoring", "最低价", "nationally_fixed", None);
        assert_eq!(r["status"], "not_applicable");
    }

    #[test]
    fn cs214_uniform_price_not_applicable() { // 214号：统一价格标准 → 不参评
        let r = run_full(61.0, "goods", "competitive_consultation", "comprehensive_scoring", "最低价", "uniform_price_standard", None);
        assert_eq!(r["status"], "not_applicable");
    }

    #[test]
    fn cs214_article3_item3_not_applicable() { // 214号：第3条第3项项目 → 不参评
        let r = run_full(61.0, "goods", "competitive_consultation", "comprehensive_scoring", "最低价", "article3_item3_project", None);
        assert_eq!(r["status"], "not_applicable");
    }

    #[test]
    fn article3_item3_only_applies_to_214() { // 第3条第3项仅磋商适用
        let r = run_full(61.0, "goods", "open_tender", "comprehensive_scoring", "最低价", "article3_item3_project", None);
        assert_eq!(r["status"], "uncertain");
    }

    #[test]
    fn fixed_price_only_applies_to_87() { // fixed_price 仅87号令适用
        let r = run_full(61.0, "goods", "competitive_consultation", "comprehensive_scoring", "最低价", "fixed_price", None);
        assert_eq!(r["status"], "uncertain");
    }

    #[test]
    fn missing_price_context_uncertain() { // price_evaluation_context 缺失 → uncertain
        let rt = tokio::runtime::Runtime::new().unwrap();
        let r = rt.block_on(ValidateScoringFormulaTool.execute(serde_json::json!({
            "price_weight": 40.0,
            "procurement_object": "goods",
            "procurement_method": "open_tender",
            "evaluation_method": "comprehensive_scoring",
            "scoring_formula_type": "最低价"
        }))).unwrap();
        assert_eq!(r["status"], "uncertain");
    }

    #[test]
    fn price_context_unknown_uncertain() {
        let r = run_full(40.0, "goods", "open_tender", "comprehensive_scoring", "最低价", "unknown", None);
        assert_eq!(r["status"], "uncertain");
    }

    // ── Missing Context Fail Closed ────────────────────────────

    #[test]
    fn missing_procurement_method_uncertain() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let r = rt.block_on(ValidateScoringFormulaTool.execute(serde_json::json!({
            "price_weight": 40.0,
            "procurement_object": "goods",
            "evaluation_method": "comprehensive_scoring",
            "scoring_formula_type": "最低价"
        }))).unwrap();
        assert_eq!(r["status"], "uncertain");
    }

    #[test]
    fn missing_procurement_object_uncertain() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let r = rt.block_on(ValidateScoringFormulaTool.execute(serde_json::json!({
            "price_weight": 40.0,
            "procurement_method": "open_tender",
            "evaluation_method": "comprehensive_scoring",
            "scoring_formula_type": "最低价"
        }))).unwrap();
        assert_eq!(r["status"], "uncertain");
    }

    #[test]
    fn missing_evaluation_method_uncertain() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let r = rt.block_on(ValidateScoringFormulaTool.execute(serde_json::json!({
            "price_weight": 40.0,
            "procurement_object": "goods",
            "procurement_method": "open_tender",
            "scoring_formula_type": "最低价"
        }))).unwrap();
        assert_eq!(r["status"], "uncertain");
    }

    // ── 同一权重不同 RuleSet ──────────────────────────────────

    #[test]
    fn goods_70_pct_tender_compliant_consultation_requires_approval() {
        let tender = run(70.0, "goods", "open_tender", "最低价");
        assert_eq!(tender["status"], "compliant", "公开招标 goods 70% 应合规（87号令无上限）");
        // 磋商 goods 70% 超上限，未提供审批状态 → uncertain（不可直接判违规）
        let cs = run(70.0, "goods", "competitive_consultation", "最低价");
        assert_eq!(cs["status"], "uncertain", "磋商 goods 70% 超上限，需审批状态确认");
        // 明确未批准 → violation
        let cs_na = run_full(70.0, "goods", "competitive_consultation", "comprehensive_scoring", "最低价", "normal", Some("not_approved"));
        assert_eq!(cs_na["status"], "violation", "磋商 goods 70% 且未获批准 → 违规");
    }

    // ── NotApplicable ─────────────────────────────────────────

    #[test]
    fn negotiation_not_applicable() {
        let r = run(40.0, "goods", "competitive_negotiation", "最低价");
        assert_eq!(r["status"], "not_applicable");
    }

    #[test]
    fn inquiry_not_applicable() {
        let r = run(40.0, "goods", "inquiry", "最低价");
        assert_eq!(r["status"], "not_applicable");
    }

    #[test]
    fn construction_tender_not_applicable() {
        let r = run(40.0, "construction", "open_tender", "最低价");
        assert_eq!(r["status"], "not_applicable");
    }

    // ── 向后兼容：procurement_category（但仍需 method/eval）────

    #[test]
    fn legacy_category_field_works_with_context() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let r = rt.block_on(ValidateScoringFormulaTool.execute(serde_json::json!({
            "price_weight": 35.0,
            "procurement_category": "货物",
            "procurement_method": "open_tender",
            "evaluation_method": "comprehensive_scoring",
            "price_evaluation_context": "normal",
            "scoring_formula_type": "最低价"
        }))).unwrap();
        assert_eq!(r["status"], "compliant");
    }
}
