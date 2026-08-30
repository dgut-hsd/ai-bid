//! `verify_consortium_rules` 工具 — 联合体投标规则检查。
//!
//! 根据《政府采购法》第24条，检查采购文件中关于联合体投标的条款是否合规。
//! 本工具执行文本关键词匹配与规则判定，不访问外部 I/O。
//!
//! ## 核心逻辑
//!
//! - 联合体允许性：检查条款明确允许/禁止联合体投标
//! - 资质叠加规则："就低不就高"为法定规则，"叠加"方式违规
//! - 牵头方要求：过高的牵头方要求标记风险
//! - 联合体协议：联合体必须提交联合体协议
//! - 矛盾检测：禁止联合体后又出现联合体相关条款
//!
//! ## 法条依据
//!
//! - 《政府采购法》第24条：以联合体形式进行政府采购的，参加联合体的供应商
//!   均应当具备本法第二十二条规定的条件，并应当向采购人提交联合体协议，
//!   载明联合体各方承担的工作和义务。联合体各方应当共同与采购人签订采购合同，
//!   就采购合同约定的事项向采购人承担连带责任。
//! - 《招标投标法》第31条：联合体各方均应当具备承担招标项目的相应能力；
//!   国家有关规定或者招标文件对投标人资格条件有规定的，联合体各方均应当
//!   具备规定的相应资格条件。由同一专业的单位组成的联合体，按照资质等级
//!   较低的单位确定资质等级。

use anyhow::{Result, anyhow};
use serde::Deserialize;

use super::AgentTool;

// ─── 关键词常量 ──────────────────────────────────────────────

/// 禁止联合体的关键词。
const FORBID_CONSORTIUM_KEYWORDS: &[&str] = &[
    "不接受联合体",
    "禁止联合体",
    "不允许联合体",
    "不得以联合体",
    "不接受以联合体",
    "不接受联合体投标",
    "不适用联合体",
];

/// 允许联合体的关键词。
const ALLOW_CONSORTIUM_KEYWORDS: &[&str] = &[
    "允许联合体",
    "接受联合体",
    "联合体投标",
    "以联合体形式",
];

/// 联合体协议相关关键词。
const AGREEMENT_KEYWORDS: &[&str] = &[
    "联合体协议",
    "联合体协议书",
    "联合投标协议",
];

/// 资质就低不就高（法定正确规则）关键词。
const QUALIFICATION_LOWEST_KEYWORDS: &[&str] = &[
    "就低不就高",
    "按照资质等级较低",
    "资质等级较低的单位",
];

/// 资质叠加（违规）关键词。
const QUALIFICATION_STACK_KEYWORDS: &[&str] = &[
    "叠加",
    "资质可叠加",
    "资质叠加",
    "合并计算资质",
];

/// 牵头方要求过高关键词。
const LEAD_PARTY_STRICT_KEYWORDS: &[&str] = &[
    "牵头方必须承担全部",
    "牵头方须独立承担",
    "牵头方承担全部主要工作",
    "牵头单位负全部责任",
];

// ─── 参数 ──────────────────────────────────────────────────────

/// `verify_consortium_rules` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct VerifyConsortiumRulesArgs {
    /// 联合体相关条款原文
    pub consortium_clause_text: String,
    /// 是否明确允许联合体
    #[serde(default)]
    pub is_allowed_explicitly: Option<bool>,
    /// 资质叠加规则描述："就低不就高"/"叠加"/"以牵头方为准"
    #[serde(default)]
    pub qualification_rule: Option<String>,
    /// 牵头方要求描述
    #[serde(default)]
    pub lead_party_requirements: Option<String>,
    /// 是否要求联合体协议
    #[serde(default)]
    pub requires_agreement: Option<bool>,
}

// ─── 输出 ──────────────────────────────────────────────────────

/// 联合体规则检查的返回结果。
#[derive(Debug, serde::Serialize)]
pub struct ConsortiumRulesResult {
    /// 整体合规判定：compliant / violation / risk / clean
    pub status: String,
    /// 是否允许联合体（从条款中推断）
    pub consortium_allowed: Option<bool>,
    /// 资质叠加规则是否合规
    pub qualification_rule_ok: Option<bool>,
    /// 资质规则详细说明
    pub qualification_rule_detail: Option<String>,
    /// 风险信号列表
    pub risks: Vec<String>,
    /// 矛盾点列表
    pub contradictions: Vec<String>,
    /// 综合建议
    pub suggestion: String,
    /// 法条依据
    pub legal_basis: String,
}

// ─── 工具实现 ──────────────────────────────────────────────────

/// `verify_consortium_rules` 工具实现。
///
/// 纯文本关键词匹配与规则判定工具，无外部依赖。
pub struct VerifyConsortiumRulesTool;

impl VerifyConsortiumRulesTool {
    /// 核心检查逻辑。
    fn verify(args: &VerifyConsortiumRulesArgs) -> Result<ConsortiumRulesResult> {
        let text = &args.consortium_clause_text;
        if text.trim().is_empty() {
            return Err(anyhow!("consortium_clause_text 不能为空"));
        }

        let mut risks: Vec<String> = Vec::new();
        let mut contradictions: Vec<String> = Vec::new();
        let mut has_violation = false;

        // ─── 1. 联合体允许性检测 ────────────────────────────
        let mut is_forbidden = false;
        let mut is_allowed_in_text = false;

        for kw in FORBID_CONSORTIUM_KEYWORDS {
            if text.contains(kw) {
                is_forbidden = true;
                break;
            }
        }

        for kw in ALLOW_CONSORTIUM_KEYWORDS {
            if text.contains(kw) {
                is_allowed_in_text = true;
                break;
            }
        }

        // 如果调用方显式指定了 is_allowed_explicitly，以显式指定为准
        let consortium_allowed = if let Some(explicit) = args.is_allowed_explicitly {
            Some(explicit)
        } else if is_forbidden && !is_allowed_in_text {
            Some(false)
        } else if is_allowed_in_text && !is_forbidden {
            Some(true)
        } else if is_forbidden && is_allowed_in_text {
            // 同时出现禁止和允许关键词 → 以禁止为准并标记矛盾
            Some(false)
        } else {
            None
        };

        // ─── 2. 联合体协议检测 ────────────────────────────
        let mut has_agreement_in_text = false;
        for kw in AGREEMENT_KEYWORDS {
            if text.contains(kw) {
                has_agreement_in_text = true;
                break;
            }
        }

        let requires_agreement = args.requires_agreement.unwrap_or(has_agreement_in_text);

        // ─── 3. 矛盾检测：禁止联合体但提及联合体协议 ─────────
        if is_forbidden && has_agreement_in_text {
            has_violation = true;
            contradictions.push(
                "条款同时出现'不接受联合体'（或等同表述）和'联合体协议'相关内容，\
                存在逻辑矛盾：既然不接受联合体投标，为何还要求联合体协议？\
                建议删除联合体协议相关条款或修改联合体接受规则。"
                    .to_string(),
            );
        }

        // 同时出现禁止和允许关键词
        if is_forbidden && is_allowed_in_text {
            contradictions.push(
                "条款中同时出现了禁止联合体（'不接受联合体'等）和允许联合体（'联合体投标'等）\
                的矛盾表述，请核实并统一。建议明确单一立场。"
                    .to_string(),
            );
        }

        // ─── 4. 资质叠加规则检查 ─────────────────────────────
        let (qualification_rule_ok, qualification_rule_detail) = if let Some(ref rule) = args.qualification_rule {
            let is_stack = QUALIFICATION_STACK_KEYWORDS.iter().any(|kw| rule.contains(kw));
            let is_lowest = QUALIFICATION_LOWEST_KEYWORDS.iter().any(|kw| rule.contains(kw));

            if is_stack {
                has_violation = true;
                (
                    Some(false),
                    Some(format!(
                        "资质规则'{}'使用'叠加'方式，违反法定规则。\
                         根据《招标投标法》第31条和《政府采购法》第24条，\
                         同一专业单位组成的联合体，应按资质等级较低的单位确定资质等级（即'就低不就高'原则），\
                         不允许将各方资质叠加计算。建议修改为'就低不就高'方式。",
                        rule
                    )),
                )
            } else if is_lowest {
                (
                    Some(true),
                    Some(format!(
                        "资质规则'{}'符合'就低不就高'法定原则（《招标投标法》第31条），合规。",
                        rule
                    )),
                )
            } else {
                (
                    None,
                    Some(format!(
                        "资质规则'{}'表述不够明确，建议使用'就低不就高'的规范表述，\
                         以符合《招标投标法》第31条的规定。",
                        rule
                    )),
                )
            }
        } else {
            // 从原文中检测
            let found_stack = QUALIFICATION_STACK_KEYWORDS.iter().any(|kw| text.contains(kw));
            let found_lowest = QUALIFICATION_LOWEST_KEYWORDS.iter().any(|kw| text.contains(kw));

            if found_stack {
                has_violation = true;
                (
                    Some(false),
                    Some(
                        "条款中出现'叠加'等资质叠加表述，违反法定'就低不就高'原则。\
                         根据《招标投标法》第31条，同一专业单位组成的联合体应按资质等级\
                         较低的单位确定资质等级，不允许叠加计算。\
                         建议修改为'就低不就高'方式。"
                            .to_string(),
                    ),
                )
            } else if found_lowest {
                (
                    Some(true),
                    Some(
                        "条款符合'就低不就高'法定原则，合规。"
                            .to_string(),
                    ),
                )
            } else {
                (None, None)
            }
        };

        // ─── 5. 牵头方要求检查 ─────────────────────────────
        if let Some(ref lead_req) = args.lead_party_requirements {
            let is_strict = LEAD_PARTY_STRICT_KEYWORDS
                .iter()
                .any(|kw| lead_req.contains(kw));
            if is_strict {
                risks.push(format!(
                    "牵头方要求'{}'过于严格：牵头方承担全部/主要工作可能构成对\
                     其他联合体成员的歧视性待遇，也可能被质疑为变相排斥联合体投标。\
                     建议调整为仅要求牵头方具备统筹协调能力，各成员按协议分工承担相应工作。",
                    lead_req
                ));
            }
        }
        // 也从原文检测
        for kw in LEAD_PARTY_STRICT_KEYWORDS {
            if text.contains(kw) && args.lead_party_requirements.is_none() {
                risks.push(format!(
                    "条款中'{}'的表述对牵头方要求过高，可能构成对联合体成员\
                     的歧视性待遇。建议调整为合理的牵头方资质要求。",
                    kw
                ));
            }
        }

        // ─── 6. 联合体允许但无协议要求 → 风险 ─────────────
        let is_effectively_allowed = consortium_allowed.unwrap_or(is_allowed_in_text);
        if is_effectively_allowed && !requires_agreement && !has_agreement_in_text {
            risks.push(
                "条款允许联合体投标但未要求提交联合体协议。\
                 根据《政府采购法》第24条，以联合体形式参加政府采购的，\
                 供应商应当向采购人提交联合体协议，载明各方承担的工作和义务。\
                 建议在采购文件中明确要求联合体提交联合体协议。"
                    .to_string(),
            );
        }

        // ─── 7. 综合判定 ──────────────────────────────────

        let status = if has_violation {
            "violation".to_string()
        } else if !risks.is_empty() {
            "risk".to_string()
        } else if qualification_rule_ok == Some(true) || consortium_allowed == Some(true) {
            "compliant".to_string()
        } else if consortium_allowed.is_none() && qualification_rule_ok.is_none() {
            "clean".to_string()
        } else {
            "compliant".to_string()
        };

        let mut suggestion_parts: Vec<String> = Vec::new();

        if !contradictions.is_empty() {
            suggestion_parts.push(format!(
                "发现 {} 处矛盾，需立即修正。",
                contradictions.len()
            ));
        }

        if has_violation && qualification_rule_ok == Some(false) {
            suggestion_parts.push(
                "资质规则违规：应将'叠加'方式改为'就低不就高'。".to_string()
            );
        }

        if !risks.is_empty() {
            suggestion_parts.push(format!(
                "存在 {} 项需关注的风险点，建议在发布前修正。",
                risks.len()
            ));
        }

        if suggestion_parts.is_empty() {
            suggestion_parts.push("联合体投标条款整体合规，未发现明显问题。".to_string());
        }

        let suggestion = suggestion_parts.join(" ");

        let legal_basis = "《政府采购法》第24条：以联合体形式进行政府采购的，\
            参加联合体的供应商均应当具备本法第二十二条规定的条件，\
            并应当向采购人提交联合体协议，载明联合体各方承担的工作和义务。\
            联合体各方应当共同与采购人签订采购合同，就采购合同约定的事项向采购人承担连带责任。\
            《招标投标法》第31条：……由同一专业的单位组成的联合体，\
            按照资质等级较低的单位确定资质等级（即'就低不就高'原则）。"
            .to_string();

        Ok(ConsortiumRulesResult {
            status,
            consortium_allowed,
            qualification_rule_ok,
            qualification_rule_detail,
            risks,
            contradictions,
            suggestion,
            legal_basis,
        })
    }
}

// ─── AgentTool 实现 ────────────────────────────────────────────

#[async_trait::async_trait]
impl AgentTool for VerifyConsortiumRulesTool {
    fn name(&self) -> &str {
        "verify_consortium_rules"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "verify_consortium_rules",
                "description": "检查联合体条款：允许/禁止、资质就低不就高、协议要求、矛盾条款。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "consortium_clause_text": {
                            "type": "string",
                            "description": "联合体相关条款原文，通常来自招标文件'投标人资格要求'部分。"
                        },
                        "is_allowed_explicitly": {
                            "type": "boolean",
                            "description": "是否明确允许联合体投标。如已知可传入，否则从原文推断。可选。"
                        },
                        "qualification_rule": {
                            "type": "string",
                            "description": "资质叠加规则描述，如'就低不就高'、'叠加'或'以牵头方为准'等。可选。"
                        },
                        "lead_party_requirements": {
                            "type": "string",
                            "description": "牵头方要求描述，如'牵头方须具备XXX资质'。可选。"
                        },
                        "requires_agreement": {
                            "type": "boolean",
                            "description": "是否要求联合体协议。如已知可传入，否则从原文推断。可选。"
                        }
                    },
                    "required": ["consortium_clause_text"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: VerifyConsortiumRulesArgs = serde_json::from_value(args)?;
        let result = Self::verify(&parsed)?;
        Ok(serde_json::to_value(&result)?)
    }
}

// ─── 测试 ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lowest_qualification_compliant() {
        // "就低不就高" → compliant
        let args = VerifyConsortiumRulesArgs {
            consortium_clause_text: "本项目接受联合体投标，联合体各方资格要求按就低不就高原则确定。"
                .to_string(),
            is_allowed_explicitly: Some(true),
            qualification_rule: Some("就低不就高".to_string()),
            lead_party_requirements: None,
            requires_agreement: Some(true),
        };
        let result = VerifyConsortiumRulesTool::verify(&args).unwrap();
        assert_eq!(result.status, "compliant");
        assert_eq!(result.qualification_rule_ok, Some(true));
        assert!(result.contradictions.is_empty());
    }

    #[test]
    fn test_stack_qualification_violation() {
        // "资质可叠加" → violation
        let args = VerifyConsortiumRulesArgs {
            consortium_clause_text: "本项目允许联合体投标，联合体各方资质可叠加计算。"
                .to_string(),
            is_allowed_explicitly: Some(true),
            qualification_rule: Some("叠加".to_string()),
            lead_party_requirements: None,
            requires_agreement: Some(true),
        };
        let result = VerifyConsortiumRulesTool::verify(&args).unwrap();
        assert_eq!(result.status, "violation");
        assert_eq!(result.qualification_rule_ok, Some(false));
        assert!(result
            .qualification_rule_detail
            .as_ref()
            .unwrap()
            .contains("就低不就高"));
    }

    #[test]
    fn test_forbid_with_agreement_contradiction() {
        // "不接受联合体"后有联合体协议条款 → 矛盾
        let args = VerifyConsortiumRulesArgs {
            consortium_clause_text:
                "本项目不接受联合体投标。联合体各方须签署联合体协议，明确各方权利义务。"
                    .to_string(),
            is_allowed_explicitly: None,
            qualification_rule: None,
            lead_party_requirements: None,
            requires_agreement: None,
        };
        let result = VerifyConsortiumRulesTool::verify(&args).unwrap();
        assert_eq!(result.status, "violation");
        assert!(!result.contradictions.is_empty());
        assert!(result
            .contradictions
            .iter()
            .any(|c| c.contains("不接受联合体") && c.contains("联合体协议")));
    }

    #[test]
    fn test_allow_without_agreement_risk() {
        // 允许联合体但无协议要求 → risk
        let args = VerifyConsortiumRulesArgs {
            consortium_clause_text: "本项目接受联合体投标。"
                .to_string(),
            is_allowed_explicitly: Some(true),
            qualification_rule: Some("就低不就高".to_string()),
            lead_party_requirements: None,
            requires_agreement: Some(false),
        };
        let result = VerifyConsortiumRulesTool::verify(&args).unwrap();
        assert_eq!(result.status, "risk");
        assert!(!result.risks.is_empty());
        assert!(result
            .risks
            .iter()
            .any(|r| r.contains("联合体协议")));
    }

    #[test]
    fn test_strict_lead_party_risk() {
        // 牵头方要求过高 → risk
        let args = VerifyConsortiumRulesArgs {
            consortium_clause_text: "本项目接受联合体投标，联合体各方须提交联合体协议。"
                .to_string(),
            is_allowed_explicitly: Some(true),
            qualification_rule: Some("就低不就高".to_string()),
            lead_party_requirements: Some(
                "牵头方必须承担全部主要工作内容，并对项目整体负全部责任。"
                    .to_string(),
            ),
            requires_agreement: Some(true),
        };
        let result = VerifyConsortiumRulesTool::verify(&args).unwrap();
        assert_eq!(result.status, "risk");
        assert!(!result.risks.is_empty());
        assert!(result.risks.iter().any(|r| r.contains("牵头方")));
    }

    #[test]
    fn test_valid_consortium_compliant() {
        // 完整合规的联合体条款 → compliant
        let args = VerifyConsortiumRulesArgs {
            consortium_clause_text:
                "本项目接受联合体投标。联合体成员不得超过3家。\
                 联合体各方均应满足投标人资格条件，同一专业资质的联合体成员\
                 按资质等级较低的单位确定资质等级。联合体各方须签署联合体协议，\
                 明确各方承担的工作和义务，并随投标文件一并提交。"
                    .to_string(),
            is_allowed_explicitly: Some(true),
            qualification_rule: Some("就低不就高".to_string()),
            lead_party_requirements: Some(
                "牵头方应具备协调统筹能力。".to_string(),
            ),
            requires_agreement: Some(true),
        };
        let result = VerifyConsortiumRulesTool::verify(&args).unwrap();
        assert_eq!(result.status, "compliant");
        assert!(result.risks.is_empty());
        assert!(result.contradictions.is_empty());
    }

    #[test]
    fn test_empty_text_error() {
        let args = VerifyConsortiumRulesArgs {
            consortium_clause_text: "".to_string(),
            is_allowed_explicitly: None,
            qualification_rule: None,
            lead_party_requirements: None,
            requires_agreement: None,
        };
        let result = VerifyConsortiumRulesTool::verify(&args);
        assert!(result.is_err());
    }
}
