//! `validate_scoring_formula` 工具 — 价格分公式校验。
//!
//! 根据《政府采购货物和服务招标投标管理办法》（财政部令第87号）第55条，
//! 校验价格分权重和评分公式类型是否符合法定要求。本工具进行纯规则匹配与
//! 关键字检测，不访问外部 I/O。
//!
//! ## 核心逻辑
//!
//! 1. 价格分权重合规检查：货物 30%-60%，服务 10%-30%，工程 30%-60%
//! 2. 公式类型合理性：最低价法适用性、平均价法操纵风险
//! 3. 基准价方法风险：裁剪均值比全部报价平均更安全
//!
//! ## 法条依据
//!
//! - 《政府采购货物和服务招标投标管理办法》（财政部令第87号）第55条
//! - 价格分权重应占 30%-60%（货物/工程）、10%-30%（服务）

use anyhow::{Result, anyhow};
use serde::Deserialize;

use super::AgentTool;

// ─── 权重组距常量 ──────────────────────────────────────────────

/// 货物类价格分权重范围
const GOODS_WEIGHT_MIN: f64 = 30.0;
const GOODS_WEIGHT_MAX: f64 = 60.0;

/// 服务类价格分权重范围
const SERVICE_WEIGHT_MIN: f64 = 10.0;
const SERVICE_WEIGHT_MAX: f64 = 30.0;

/// 工程类价格分权重范围（与货物相同）
const CONSTRUCTION_WEIGHT_MIN: f64 = 30.0;
const CONSTRUCTION_WEIGHT_MAX: f64 = 60.0;

/// 基准价"去掉最高最低"风险关键词（更安全的方法）
const SAFE_BENCHMARK_KEYWORDS: &[&str] = &["去掉最高最低", "去掉最高和最低", "剔除极端值", "裁剪均值"];
/// 基准价"所有报价平均"风险关键词（容易被操纵）
const RISKY_BENCHMARK_KEYWORDS: &[&str] = &["所有报价平均", "全部报价算术平均", "所有有效报价平均"];

// ─── 参数 ──────────────────────────────────────────────────────

/// `validate_scoring_formula` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct ValidateScoringFormulaArgs {
    /// 价格分权重，如 30.0 表示 30%
    pub price_weight: f64,
    /// 采购品类："货物"/"工程"/"服务"
    pub procurement_category: String,
    /// 评分公式类型："最低价"/"平均价"/"基准价"
    pub scoring_formula_type: String,
    /// 公式文字描述（可选）
    #[serde(default)]
    pub formula_description: Option<String>,
}

// ─── 输出 ──────────────────────────────────────────────────────

/// 价格分公式校验返回结果。
#[derive(Debug, serde::Serialize)]
struct ScoringFormulaResult {
    /// 整体判定: "compliant"/"risk"/"violation"
    status: String,
    /// 权重是否合规
    weight_ok: bool,
    /// 公式类型是否合理
    formula_ok: bool,
    /// 权重检查详情
    weight_detail: String,
    /// 公式检查详情
    formula_detail: String,
    /// 风险列表
    risks: Vec<String>,
    /// 综合建议
    suggestion: String,
    /// 法条依据
    legal_basis: String,
}

// ─── 工具实现 ──────────────────────────────────────────────────

/// `validate_scoring_formula` 工具实现。
///
/// 纯计算与关键字检测工具，无外部依赖。
pub struct ValidateScoringFormulaTool;

impl ValidateScoringFormulaTool {
    /// 获取指定品类的价格分权重合法范围。
    fn get_weight_range(category: &str) -> Result<(f64, f64)> {
        match category {
            "货物" => Ok((GOODS_WEIGHT_MIN, GOODS_WEIGHT_MAX)),
            "工程" => Ok((CONSTRUCTION_WEIGHT_MIN, CONSTRUCTION_WEIGHT_MAX)),
            "服务" => Ok((SERVICE_WEIGHT_MIN, SERVICE_WEIGHT_MAX)),
            _ => Err(anyhow!(
                "不支持的采购品类 '{}'，有效值为: 货物/工程/服务",
                category
            )),
        }
    }

    /// 检测基准价描述中的风险关键词。
    fn detect_benchmark_risk(description: &str) -> Vec<String> {
        let mut risks = Vec::new();

        // 检查是否使用了"所有报价平均"等高风险方法
        for kw in RISKY_BENCHMARK_KEYWORDS {
            if description.contains(kw) {
                risks.push(format!(
                    "基准价采用'{}'方法，容易被供应商联合围标操纵。建议改为去掉最高最低报价后的算术平均值，或采用中位数。",
                    kw
                ));
                break; // 避免重复
            }
        }

        // 检查是否使用了安全方法
        let has_safe = SAFE_BENCHMARK_KEYWORDS
            .iter()
            .any(|kw| description.contains(kw));

        if !has_safe && risks.is_empty() {
            risks.push(
                "基准价计算方法未明确说明是否剔除极端值。建议明确'去掉最高和最低报价后的算术平均值'。"
                    .to_string(),
            );
        }

        risks
    }

    /// 检查公式类型合理性和风险。
    fn check_formula_type(
        formula_lower: &str,
        price_weight: f64,
        weight_min: f64,
        formula_description: &Option<String>,
        risks: &mut Vec<String>,
    ) -> (bool, String) {
        if formula_lower.contains("最低价") {
            if price_weight > 50.0 {
                (true, format!(
                    "采用最低价法：价格分权重 {}% > 50%，最低价法可有效发挥价格竞争作用，合理。",
                    price_weight
                ))
            } else if price_weight < weight_min {
                risks.push(format!(
                    "最低价法在价格分权重 {}% 时效果有限（价格分低于品类最低要求的 {}%），建议更换评分公式",
                    price_weight, weight_min
                ));
                (false, format!(
                    "采用最低价法但价格分权重仅 {}%，不足品类最低要求的 {}%。在价格分权重较低时，\
                    最低价法对总分影响有限，无法有效发挥价格竞争作用。建议考虑基准价法或平均价法。",
                    price_weight, weight_min
                ))
            } else {
                (true, format!(
                    "采用最低价法：价格分权重 {}%，可正常发挥价格竞争作用。",
                    price_weight
                ))
            }
        } else if formula_lower.contains("平均价") {
            risks.push(
                "平均价法存在围标风险：供应商可通过联合操纵报价影响平均价格基准。建议改为基准价法。"
                    .to_string(),
            );
            (false, "采用平均价法：此方法容易被供应商联合围标操纵——\
                多家供应商约定相近报价即可拉高平均价。建议改用基准价法（去掉最高最低后取平均）。"
                .to_string())
        } else if formula_lower.contains("基准价") {
            let detail = "采用基准价法：基准价法比最低价法和平均价法更合理，可兼顾价格竞争与异常值剔除。".to_string();
            if let Some(desc) = formula_description {
                let benchmark_risks = Self::detect_benchmark_risk(desc);
                if !benchmark_risks.is_empty() {
                    risks.extend(benchmark_risks);
                }
            } else {
                risks.push(
                    "基准价计算方法未提供具体描述，建议在公式描述中明确剔除极端值的规则。"
                        .to_string(),
                );
            }
            (true, detail)
        } else {
            risks.push(format!(
                "无法识别的评分公式类型 '{}'",
                formula_lower
            ));
            (false, format!(
                "无法识别的公式类型 '{}'。有效类型: 最低价/平均价/基准价",
                formula_lower
            ))
        }
    }

    /// 核心校验逻辑。
    fn validate(args: &ValidateScoringFormulaArgs) -> Result<ScoringFormulaResult> {
        let mut risks: Vec<String> = Vec::new();

        // ── 1. 权重合规检查 ──
        let (weight_min, weight_max) = Self::get_weight_range(&args.procurement_category)?;
        let weight_ok = args.price_weight >= weight_min && args.price_weight <= weight_max;

        let weight_detail = if weight_ok {
            format!(
                "价格分权重 {}%，在 {} 品类法定范围 {}-{}% 内，合规。",
                args.price_weight, args.procurement_category, weight_min, weight_max
            )
        } else if args.price_weight < weight_min {
            format!(
                "价格分权重 {}% 低于 {} 品类法定最低要求 {}%。价格分过低可能导致评审过于主观，\
                未能充分体现价格竞争。",
                args.price_weight, args.procurement_category, weight_min
            )
        } else {
            format!(
                "价格分权重 {}% 超出 {} 品类法定上限 {}%。价格分过高可能形成低价恶性竞争，\
                忽视质量和服务因素。",
                args.price_weight, args.procurement_category, weight_max
            )
        };

        if !weight_ok {
            risks.push(format!(
                "价格分权重 {}% 不在 {} 品类法定范围 {}-{}% 内",
                args.price_weight, args.procurement_category, weight_min, weight_max
            ));
        }

        // ── 2. 公式类型合理性检查 ──
        let formula_lower = args.scoring_formula_type.to_lowercase();
        let (formula_ok, formula_detail) = Self::check_formula_type(
            &formula_lower,
            args.price_weight,
            weight_min,
            &args.formula_description,
            &mut risks,
        );

        // ── 4. 综合判定 ──
        let status = if !weight_ok {
            "violation"
        } else if !formula_ok || !risks.is_empty() {
            "risk"
        } else {
            "compliant"
        };

        // ── 5. 法条依据 ──
        let legal_basis = match args.procurement_category.as_str() {
            "服务" => "《政府采购货物和服务招标投标管理办法》（财政部令第87号）第55条：\
                      服务项目的价格分值占总分值的比重（权重）不得低于10％，不得高于30％。"
                .to_string(),
            "货物" | "工程" => "《政府采购货物和服务招标投标管理办法》（财政部令第87号）第55条：\
                              货物项目的价格分值占总分值的比重（权重）不得低于30％，不得高于60％。"
                .to_string(),
            _ => "《政府采购货物和服务招标投标管理办法》（财政部令第87号）第55条".to_string(),
        };

        // ── 6. 综合建议 ──
        let suggestion = if status == "compliant" {
            format!(
                "{} 品类价格分权重 {}% 和评分公式 '{}' 均合规。",
                args.procurement_category, args.price_weight, args.scoring_formula_type
            )
        } else if status == "violation" {
            let mut parts = Vec::new();
            if !weight_ok {
                parts.push(format!(
                    "调整价格分权重至 {} 品类法定范围 {}-{}% 内",
                    args.procurement_category, weight_min, weight_max
                ));
            }
            format!(
                "存在违规项，建议：{}。",
                parts.join("；")
            )
        } else {
            format!(
                "存在 {} 项风险，建议：1) 审查公式描述是否包含防操纵措施；\
                2) 考虑采用去极端值的基准价法。",
                risks.len()
            )
        };

        Ok(ScoringFormulaResult {
            status: status.to_string(),
            weight_ok,
            formula_ok,
            weight_detail,
            formula_detail,
            risks,
            suggestion,
            legal_basis,
        })
    }
}

// ─── AgentTool 实现 ────────────────────────────────────────────

#[async_trait::async_trait]
impl AgentTool for ValidateScoringFormulaTool {
    fn name(&self) -> &str {
        "validate_scoring_formula"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "validate_scoring_formula",
                "description": "校验价格分权重与公式类型合规(货物/工程30%-60%，服务10%-30%；基准价法应去极端值)。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "price_weight": {
                            "type": "number",
                            "description": "价格分权重（百分比数值），如 30.0 表示 30%。"
                        },
                        "procurement_category": {
                            "type": "string",
                            "enum": ["货物", "工程", "服务"],
                            "description": "采购品类。"
                        },
                        "scoring_formula_type": {
                            "type": "string",
                            "enum": ["最低价", "平均价", "基准价"],
                            "description": "评分公式类型。"
                        },
                        "formula_description": {
                            "type": "string",
                            "description": "公式文字描述（可选）。如基准价法需说明是否去掉最高最低报价。"
                        }
                    },
                    "required": ["price_weight", "procurement_category", "scoring_formula_type"]
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

    // ── 权重合规测试 ──

    #[test]
    fn test_goods_60pct_weight_compliant() {
        // 货物价格分 60% = 合规（上限）
        let args = ValidateScoringFormulaArgs {
            price_weight: 60.0,
            procurement_category: "货物".to_string(),
            scoring_formula_type: "最低价".to_string(),
            formula_description: None,
        };
        let result = ValidateScoringFormulaTool::validate(&args).unwrap();
        assert_eq!(result.status, "compliant");
        assert!(result.weight_ok);
    }

    #[test]
    fn test_goods_70pct_weight_violation() {
        // 货物价格分 70% = 违规（超出上限）
        let args = ValidateScoringFormulaArgs {
            price_weight: 70.0,
            procurement_category: "货物".to_string(),
            scoring_formula_type: "最低价".to_string(),
            formula_description: None,
        };
        let result = ValidateScoringFormulaTool::validate(&args).unwrap();
        assert_eq!(result.status, "violation");
        assert!(!result.weight_ok);
    }

    #[test]
    fn test_service_15pct_weight_compliant() {
        // 服务价格分 15% = 合规
        let args = ValidateScoringFormulaArgs {
            price_weight: 15.0,
            procurement_category: "服务".to_string(),
            scoring_formula_type: "最低价".to_string(),
            formula_description: None,
        };
        let result = ValidateScoringFormulaTool::validate(&args).unwrap();
        assert_eq!(result.status, "compliant");
        assert!(result.weight_ok);
    }

    #[test]
    fn test_service_5pct_weight_violation() {
        // 服务价格分 5% = 违规（低于下限）
        let args = ValidateScoringFormulaArgs {
            price_weight: 5.0,
            procurement_category: "服务".to_string(),
            scoring_formula_type: "最低价".to_string(),
            formula_description: None,
        };
        let result = ValidateScoringFormulaTool::validate(&args).unwrap();
        assert_eq!(result.status, "violation");
        assert!(!result.weight_ok);
    }

    #[test]
    fn test_construction_30pct_weight_compliant() {
        // 工程价格分 30% = 合规（下限）
        let args = ValidateScoringFormulaArgs {
            price_weight: 30.0,
            procurement_category: "工程".to_string(),
            scoring_formula_type: "最低价".to_string(),
            formula_description: None,
        };
        let result = ValidateScoringFormulaTool::validate(&args).unwrap();
        assert!(result.weight_ok);
    }

    // ── 公式类型测试 ──

    #[test]
    fn test_avg_price_risk_flag() {
        // 平均价法应标记风险
        let args = ValidateScoringFormulaArgs {
            price_weight: 40.0,
            procurement_category: "货物".to_string(),
            scoring_formula_type: "平均价".to_string(),
            formula_description: None,
        };
        let result = ValidateScoringFormulaTool::validate(&args).unwrap();
        assert!(!result.formula_ok);
        assert!(!result.risks.is_empty());
        // 平均价法不应是 violation（仅公式有风险，权重合规）
        assert_eq!(result.status, "risk");
    }

    #[test]
    fn test_benchmark_with_all_avg_risk() {
        // 基准价法 + "所有报价平均" → 检测到操纵风险
        let args = ValidateScoringFormulaArgs {
            price_weight: 40.0,
            procurement_category: "货物".to_string(),
            scoring_formula_type: "基准价".to_string(),
            formula_description: Some("采用所有有效报价平均作为基准价".to_string()),
        };
        let result = ValidateScoringFormulaTool::validate(&args).unwrap();
        assert!(!result.risks.is_empty());
        assert!(
            result
                .risks
                .iter()
                .any(|r| r.contains("联合操纵") || r.contains("围标")),
            "应检测到所有报价平均的操纵风险"
        );
    }

    #[test]
    fn test_benchmark_with_trimmed_avg_safe() {
        // 基准价法 + "去掉最高最低" → 安全
        let args = ValidateScoringFormulaArgs {
            price_weight: 40.0,
            procurement_category: "货物".to_string(),
            scoring_formula_type: "基准价".to_string(),
            formula_description: Some("去掉最高最低报价后的算术平均值作为基准价".to_string()),
        };
        let result = ValidateScoringFormulaTool::validate(&args).unwrap();
        assert!(
            !result
                .risks
                .iter()
                .any(|r| r.contains("联合操纵") || r.contains("围标")),
            "去掉最高最低不应产生操纵风险"
        );
    }

    #[test]
    fn test_invalid_category_errors() {
        let args = ValidateScoringFormulaArgs {
            price_weight: 30.0,
            procurement_category: "设计".to_string(),
            scoring_formula_type: "最低价".to_string(),
            formula_description: None,
        };
        let result = ValidateScoringFormulaTool::validate(&args);
        assert!(result.is_err());
    }
}
