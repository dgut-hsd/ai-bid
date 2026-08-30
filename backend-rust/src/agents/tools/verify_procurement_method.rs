//! `verify_procurement_method` 工具 — 采购方式适用条件校验。
//!
//! 根据《政府采购法》及其实施条例，校验项目预算金额是否满足声明的采购方式
//! 法定适用条件。本工具进行纯规则匹配——基于法定门槛表和例外条件表进行判断，
//! 不访问外部 I/O。
//!
//! ## 核心逻辑
//!
//! 1. 根据品类（货物/工程/服务）和预算金额确定法定适用方式
//! 2. 将声明的采购方式与法定要求比对
//! 3. 产出合规判定（compliant / violation / uncertain）+ 法条依据 + 建议
//!
//! ## 法定门槛表
//!
//! - 公开招标数额标准：货物 200 万、工程 400 万、服务 100 万
//! - 低于上述门槛方可采用竞争性谈判/竞争性磋商/询价
//! - 单一来源适用 5 种法定情形（详见代码内常量）

use anyhow::{Result, anyhow};
use serde::Deserialize;

use super::AgentTool;

// ─── 法定门槛常量 ──────────────────────────────────────────────

/// 公开招标数额标准（万元）。
/// 来源：《政府采购法》及各地集中采购目录及限额标准。
const OPEN_BIDDING_THRESHOLD_GOODS: f64 = 200.0;
const OPEN_BIDDING_THRESHOLD_CONSTRUCTION: f64 = 400.0;
const OPEN_BIDDING_THRESHOLD_SERVICE: f64 = 100.0;

/// 单一来源采购的 5 种法定适用情形。
const SINGLE_SOURCE_CONDITIONS: &[&str] = &[
    "只能从唯一供应商处采购的",
    "发生了不可预见的紧急情况不能从其他供应商处采购的",
    "必须保证原有采购项目一致性或者服务配套的要求，需要继续从原供应商处添购，且添购资金总额不超过原合同采购金额百分之十的",
    "招标后没有供应商投标或者没有合格标的或者重新招标未能成立的",
    "采用招标所需时间不能满足用户紧急需要的",
];

// ─── 参数 ──────────────────────────────────────────────────────

/// `verify_procurement_method` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct VerifyProcurementMethodArgs {
    /// 项目预算金额（万元）
    pub budget_amount: f64,
    /// 采购品类：货物/工程/服务
    pub procurement_category: String,
    /// 声明的采购方式：公开招标/邀请招标/竞争性谈判/竞争性磋商/询价/单一来源
    pub declared_method: String,
}

// ─── 输出 ──────────────────────────────────────────────────────

/// 采购方式校验的返回结果。
#[derive(Debug, serde::Serialize)]
struct ProcurementMethodResult {
    /// 合规判定
    status: ComplianceStatus,
    /// 采购品类
    procurement_category: String,
    /// 项目预算（万元）
    budget_amount: f64,
    /// 声明的采购方式
    declared_method: String,
    /// 该品类法定公开招标数额标准（万元）
    open_bidding_threshold: f64,
    /// 法定应适用的采购方式集合
    statutory_applicable_methods: Vec<String>,
    /// 法条依据
    legal_basis: Vec<String>,
    /// 合规建议
    suggestion: String,
    /// 详细判定说明
    detail: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ComplianceStatus {
    Compliant,
    Violation,
    Uncertain,
}

impl std::fmt::Display for ComplianceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComplianceStatus::Compliant => write!(f, "compliant"),
            ComplianceStatus::Violation => write!(f, "violation"),
            ComplianceStatus::Uncertain => write!(f, "uncertain"),
        }
    }
}

// ─── 工具实现 ──────────────────────────────────────────────────

/// `verify_procurement_method` 工具实现。
///
/// 纯查表/规则匹配工具，无外部依赖。
pub struct VerifyProcurementMethodTool;

impl VerifyProcurementMethodTool {
    /// 获取品类的公开招标数额标准。
    fn get_threshold(category: &str) -> Result<f64> {
        match category {
            "货物" => Ok(OPEN_BIDDING_THRESHOLD_GOODS),
            "工程" => Ok(OPEN_BIDDING_THRESHOLD_CONSTRUCTION),
            "服务" => Ok(OPEN_BIDDING_THRESHOLD_SERVICE),
            _ => Err(anyhow!(
                "不支持的采购品类 '{}'，有效值为: 货物/工程/服务",
                category
            )),
        }
    }

    /// 获取法定可适用的采购方式集合。
    ///
    /// 规则：
    /// - 预算 >= 门槛 → 必须采用公开招标（除非有法定例外）
    /// - 预算 < 门槛 → 可采用公开招标、邀请招标、竞争性谈判、竞争性磋商、询价
    /// - 单一来源需满足 5 种法定情形之一，不在此自动判定
    fn get_statutory_methods(budget: f64, threshold: f64) -> Vec<String> {
        if budget >= threshold {
            vec!["公开招标".to_string()]
        } else {
            vec![
                "公开招标".to_string(),
                "邀请招标".to_string(),
                "竞争性谈判".to_string(),
                "竞争性磋商".to_string(),
                "询价".to_string(),
            ]
        }
    }

    /// 判断声明的采购方式是否在法定允许范围内。
    fn is_method_allowed(declared: &str, allowed: &[String]) -> bool {
        allowed.iter().any(|m| m == declared)
    }

    /// 核心校验逻辑。
    fn verify(args: &VerifyProcurementMethodArgs) -> Result<ProcurementMethodResult> {
        // 1. 参数校验
        let valid_categories = ["货物", "工程", "服务"];
        if !valid_categories.contains(&args.procurement_category.as_str()) {
            return Err(anyhow!(
                "无效的 procurement_category '{}'，有效值为: {}",
                args.procurement_category,
                valid_categories.join("/")
            ));
        }

        let valid_methods = [
            "公开招标",
            "邀请招标",
            "竞争性谈判",
            "竞争性磋商",
            "询价",
            "单一来源",
        ];
        if !valid_methods.contains(&args.declared_method.as_str()) {
            return Err(anyhow!(
                "无效的 declared_method '{}'，有效值为: {}",
                args.declared_method,
                valid_methods.join("/")
            ));
        }

        if args.budget_amount <= 0.0 {
            return Err(anyhow!("预算金额必须大于 0"));
        }

        // 2. 获取门槛
        let threshold = Self::get_threshold(&args.procurement_category)?;

        // 3. 获取法定适用方式
        let statutory_methods = Self::get_statutory_methods(args.budget_amount, threshold);

        // 4. 构建法条依据
        let mut legal_basis = vec![
            "《政府采购法》第26条：政府采购采用以下方式：公开招标、邀请招标、竞争性谈判、单一来源采购、询价，以及国务院政府采购监督管理部门认定的其他采购方式。公开招标应作为政府采购的主要采购方式。"
                .to_string(),
        ];

        // 5. 判定
        let (status, suggestion, detail) = if args.declared_method == "单一来源" {
            // 单一来源不依赖预算阈值，依赖法定情形
            legal_basis.push(
                "《政府采购法》第31条：符合下列情形之一的货物或者服务，可以依照本法采用单一来源方式采购：(一)只能从唯一供应商处采购的；(二)发生了不可预见的紧急情况不能从其他供应商处采购的；(三)必须保证原有采购项目一致性或者服务配套的要求，需要继续从原供应商处添购，且添购资金总额不超过原合同采购金额百分之十的。"
                    .to_string(),
            );

            let conditions_display = SINGLE_SOURCE_CONDITIONS
                .iter()
                .enumerate()
                .map(|(i, c)| format!("({}) {}", i + 1, c))
                .collect::<Vec<_>>()
                .join("；");

            (
                ComplianceStatus::Uncertain,
                format!(
                    "单一来源采购必须满足《政府采购法》第31条规定的法定情形之一。请确认是否满足: {}",
                    conditions_display
                ),
                format!(
                    "单一来源采购不基于预算金额自动判定。法定适用情形共 5 种：{}。\
                    需人工确认是否存在对应的适用情形。如不满足任一情形，则不得采用单一来源。",
                    conditions_display
                ),
            )
        } else if args.budget_amount >= threshold {
            // 达到公开招标数额标准
            legal_basis.push(format!(
                "《政府采购法》第27条：采购人采购货物或者服务项目，单项或批量采购预算金额达到公开招标数额标准的，应当采用公开招标方式。本品类公开招标数额标准为 {} 万元。",
                threshold
            ));

            if Self::is_method_allowed(&args.declared_method, &statutory_methods) {
                (
                    ComplianceStatus::Compliant,
                    format!(
                        "预算 {} 万元 ≥ {} 万元（{}公开招标数额标准），采用公开招标方式合规。",
                        args.budget_amount, threshold, args.procurement_category
                    ),
                    format!(
                        "预算 {} 万元已达到 {} 品类公开招标数额标准 {} 万元，必须采用公开招标。\
                        当前声明的'{}'方式符合法定要求。",
                        args.budget_amount,
                        args.procurement_category,
                        threshold,
                        args.declared_method
                    ),
                )
            } else {
                (
                    ComplianceStatus::Violation,
                    format!(
                        "建议将采购方式变更为公开招标，或向设区的市级以上财政部门申请批准采用其他采购方式。",
                    ),
                    format!(
                        "违规！预算 {} 万元已达到 {} 品类公开招标数额标准 {} 万元，\
                        依法应当采用公开招标方式。声明的方式'{}'不符合法定要求。\
                        《政府采购法》第27条规定：达到公开招标数额标准的项目必须公开招标，\
                        确需采用其他方式的，应在采购活动开始前获得设区的市级以上财政部门批准。\
                        法定允许的方式只有：{}",
                        args.budget_amount,
                        args.procurement_category,
                        threshold,
                        args.declared_method,
                        statutory_methods.join("、")
                    ),
                )
            }
        } else {
            // 低于公开招标数额标准
            legal_basis.push(format!(
                "预算未达到{}品类公开招标数额标准 {} 万元，可采用竞争性谈判、竞争性磋商、询价等方式。",
                args.procurement_category, threshold
            ));

            if Self::is_method_allowed(&args.declared_method, &statutory_methods) {
                (
                    ComplianceStatus::Compliant,
                    format!(
                        "预算 {} 万元 < {} 万元（{}公开招标数额标准），采用'{}'方式合规。",
                        args.budget_amount, threshold, args.procurement_category, args.declared_method
                    ),
                    format!(
                        "预算 {} 万元低于 {} 品类公开招标数额标准 {} 万元，\
                        法定允许采用以下方式之一：{}。\
                        当前声明的'{}'方式在法定允许范围内。",
                        args.budget_amount,
                        args.procurement_category,
                        threshold,
                        statutory_methods.join("、"),
                        args.declared_method
                    ),
                )
            } else {
                (
                    ComplianceStatus::Violation,
                    format!(
                        "'{}'不在当前预算条件下法定可用的采购方式范围内。法定可用方式：{}",
                        args.declared_method,
                        statutory_methods.join("、")
                    ),
                    format!(
                        "违规！声明的采购方式'{}'不在法定可用方式集合中。\
                        预算 {} 万元低于公开招标数额标准 {} 万元，法定可用方式为：{}。\
                        如确需采用非常规方式，需提供特殊理由并报财政部门审批。",
                        args.declared_method,
                        args.budget_amount,
                        threshold,
                        statutory_methods.join("、")
                    ),
                )
            }
        };

        Ok(ProcurementMethodResult {
            status,
            procurement_category: args.procurement_category.clone(),
            budget_amount: args.budget_amount,
            declared_method: args.declared_method.clone(),
            open_bidding_threshold: threshold,
            statutory_applicable_methods: statutory_methods,
            legal_basis,
            suggestion,
            detail,
        })
    }
}

// ─── AgentTool 实现 ────────────────────────────────────────────

#[async_trait::async_trait]
impl AgentTool for VerifyProcurementMethodTool {
    fn name(&self) -> &str {
        "verify_procurement_method"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "verify_procurement_method",
                "description": "校验采购方式适用金额门槛(货物200万/工程400万/服务100万，达到须公开招标)。预算单位万元。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "budget_amount": {
                            "type": "number",
                            "description": "项目预算金额，单位：万元（人民币）。如预算 350 万则传入 350.0。"
                        },
                        "procurement_category": {
                            "type": "string",
                            "enum": ["货物", "工程", "服务"],
                            "description": "采购品类。'货物'为物资设备类，'工程'为施工建设类，'服务'为咨询/设计/运维等。"
                        },
                        "declared_method": {
                            "type": "string",
                            "enum": ["公开招标", "邀请招标", "竞争性谈判", "竞争性磋商", "询价", "单一来源"],
                            "description": "标书中声明的采购方式。"
                        }
                    },
                    "required": ["budget_amount", "procurement_category", "declared_method"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: VerifyProcurementMethodArgs = serde_json::from_value(args)?;
        let result = Self::verify(&parsed)?;
        Ok(serde_json::to_value(&result)?)
    }
}

// ─── 测试 ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_goods_200w_open_bidding_compliant() {
        let args = VerifyProcurementMethodArgs {
            budget_amount: 200.0,
            procurement_category: "货物".to_string(),
            declared_method: "公开招标".to_string(),
        };
        let result = VerifyProcurementMethodTool::verify(&args).unwrap();
        assert!(matches!(result.status, ComplianceStatus::Compliant));
        assert_eq!(result.open_bidding_threshold, 200.0);
    }

    #[test]
    fn test_goods_200w_competitive_negotiation_violation() {
        // 核心用例：200万货物用竞争性谈判应标违规（≥200万必须公开招标）
        let args = VerifyProcurementMethodArgs {
            budget_amount: 200.0,
            procurement_category: "货物".to_string(),
            declared_method: "竞争性谈判".to_string(),
        };
        let result = VerifyProcurementMethodTool::verify(&args).unwrap();
        assert!(
            matches!(result.status, ComplianceStatus::Violation),
            "200万货物达到公开招标数额标准，使用竞争性谈判应为违规"
        );
        assert!(
            result.detail.contains("公开招标"),
            "违规详情应提及公开招标要求"
        );
    }

    #[test]
    fn test_construction_350w_competitive_consultation_compliant() {
        // 工程400万门槛，350万未达到
        let args = VerifyProcurementMethodArgs {
            budget_amount: 350.0,
            procurement_category: "工程".to_string(),
            declared_method: "竞争性磋商".to_string(),
        };
        let result = VerifyProcurementMethodTool::verify(&args).unwrap();
        assert!(matches!(result.status, ComplianceStatus::Compliant));
        assert_eq!(result.open_bidding_threshold, 400.0);
    }

    #[test]
    fn test_single_source_always_uncertain() {
        let args = VerifyProcurementMethodArgs {
            budget_amount: 50.0,
            procurement_category: "服务".to_string(),
            declared_method: "单一来源".to_string(),
        };
        let result = VerifyProcurementMethodTool::verify(&args).unwrap();
        assert!(
            matches!(result.status, ComplianceStatus::Uncertain),
            "单一来源采购应返回 uncertain，需要人工确认法定情形"
        );
        assert!(
            result.legal_basis.iter().any(|lb| lb.contains("第31条")),
            "应引用《政府采购法》第31条"
        );
    }

    #[test]
    fn test_service_100w_open_bidding_compliant() {
        let args = VerifyProcurementMethodArgs {
            budget_amount: 100.0,
            procurement_category: "服务".to_string(),
            declared_method: "公开招标".to_string(),
        };
        let result = VerifyProcurementMethodTool::verify(&args).unwrap();
        assert!(matches!(result.status, ComplianceStatus::Compliant));
    }

    #[test]
    fn test_invalid_category_errors() {
        let args = VerifyProcurementMethodArgs {
            budget_amount: 100.0,
            procurement_category: "设计".to_string(),
            declared_method: "公开招标".to_string(),
        };
        let result = VerifyProcurementMethodTool::verify(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_zero_budget_errors() {
        let args = VerifyProcurementMethodArgs {
            budget_amount: 0.0,
            procurement_category: "货物".to_string(),
            declared_method: "公开招标".to_string(),
        };
        let result = VerifyProcurementMethodTool::verify(&args);
        assert!(result.is_err());
    }
}
