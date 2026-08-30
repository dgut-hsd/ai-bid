//! `validate_weight_distribution` 工具 — 权重分配合规检查。
//!
//! 根据《政府采购货物和服务招标投标管理办法》（财政部令第87号）第55条，
//! 校验评审因素权重分配是否合规。本工具进行纯数值计算与规则匹配，
//! 不访问外部 I/O。
//!
//! ## 核心逻辑
//!
//! 1. 权重求和验证（各项权重之和应等于 total_score）
//! 2. 价格分范围检查：货物 30%-60%，服务 10%-30%，工程 30%-60%
//! 3. 缺失维度检测：货物必须有技术分+商务分，服务必须有服务分，工程必须有技术分
//!
//! ## 法条依据
//!
//! - 《政府采购货物和服务招标投标管理办法》（财政部令第87号）第55条

use anyhow::{Result, anyhow};
use serde::Deserialize;

use super::AgentTool;

// ─── 权重组距常量 ──────────────────────────────────────────────

/// 货物类价格分权重范围
const GOODS_PRICE_MIN: f64 = 30.0;
const GOODS_PRICE_MAX: f64 = 60.0;

/// 服务类价格分权重范围
const SERVICE_PRICE_MIN: f64 = 10.0;
const SERVICE_PRICE_MAX: f64 = 30.0;

/// 工程类价格分权重范围
const CONSTRUCTION_PRICE_MIN: f64 = 30.0;
const CONSTRUCTION_PRICE_MAX: f64 = 60.0;

/// 浮点数比较容差
const EPSILON: f64 = 1e-6;

// ─── 参数 ──────────────────────────────────────────────────────

/// `validate_weight_distribution` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct ValidateWeightDistributionArgs {
    /// 价格分权重
    pub price_weight: f64,
    /// 技术分权重
    pub technical_weight: f64,
    /// 服务分权重（可选）
    #[serde(default)]
    pub service_weight: Option<f64>,
    /// 商务分权重（可选）
    #[serde(default)]
    pub business_weight: Option<f64>,
    /// 采购品类："货物"/"工程"/"服务"
    pub procurement_category: String,
    /// 总分（通常为 100）
    pub total_score: f64,
}

// ─── 输出 ──────────────────────────────────────────────────────

/// 权重分配合规检查返回结果。
#[derive(Debug, serde::Serialize)]
struct WeightDistributionResult {
    /// 整体判定: "compliant"/"risk"/"violation"
    status: String,
    /// 权重和 = total_score
    sum_check: bool,
    /// 价格分在法定范围内
    price_range_ok: bool,
    /// 缺失的评审维度
    missing_dimensions: Vec<String>,
    /// 各检查项详情
    checks: Vec<WeightCheckItem>,
    /// 综合摘要
    summary: String,
}

/// 单项权重检查结果。
#[derive(Debug, serde::Serialize)]
struct WeightCheckItem {
    /// 检查项名称
    item: String,
    /// 当前值
    current_value: f64,
    /// 要求范围描述
    required_range: String,
    /// 是否通过
    pass: bool,
}

// ─── 工具实现 ──────────────────────────────────────────────────

/// `validate_weight_distribution` 工具实现。
///
/// 纯数值计算与规则匹配工具，无外部依赖。
pub struct ValidateWeightDistributionTool;

impl ValidateWeightDistributionTool {
    /// 获取品类的价格分法定范围。
    fn get_price_range(category: &str) -> Result<(f64, f64)> {
        match category {
            "货物" => Ok((GOODS_PRICE_MIN, GOODS_PRICE_MAX)),
            "工程" => Ok((CONSTRUCTION_PRICE_MIN, CONSTRUCTION_PRICE_MAX)),
            "服务" => Ok((SERVICE_PRICE_MIN, SERVICE_PRICE_MAX)),
            _ => Err(anyhow!(
                "不支持的采购品类 '{}'，有效值为: 货物/工程/服务",
                category
            )),
        }
    }

    /// 判断两个 f64 是否相等（在容差范围内）。
    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < EPSILON
    }

    /// 核心校验逻辑。
    fn validate(args: &ValidateWeightDistributionArgs) -> Result<WeightDistributionResult> {
        let mut checks: Vec<WeightCheckItem> = Vec::new();
        let mut missing_dimensions: Vec<String> = Vec::new();

        // ── 1. 权重求和验证 ──
        let service_w = args.service_weight.unwrap_or(0.0);
        let business_w = args.business_weight.unwrap_or(0.0);
        let weight_sum = args.price_weight + args.technical_weight + service_w + business_w;

        let sum_check = Self::approx_eq(weight_sum, args.total_score);

        checks.push(WeightCheckItem {
            item: "权重总和".to_string(),
            current_value: weight_sum,
            required_range: format!("应等于 {}", args.total_score),
            pass: sum_check,
        });

        // ── 2. 价格分范围检查 ──
        let (price_min, price_max) = Self::get_price_range(&args.procurement_category)?;
        let price_range_ok =
            args.price_weight >= price_min - EPSILON && args.price_weight <= price_max + EPSILON;

        checks.push(WeightCheckItem {
            item: format!("{}品类价格分范围", args.procurement_category),
            current_value: args.price_weight,
            required_range: format!("{}-{}%", price_min, price_max),
            pass: price_range_ok,
        });

        // ── 3. 缺失维度检测 ──
        match args.procurement_category.as_str() {
            "货物" => {
                // 货物必须有技术分 + 商务分
                if args.technical_weight <= 0.0 {
                    missing_dimensions.push("技术分".to_string());
                }
                if business_w <= 0.0 {
                    missing_dimensions.push("商务分".to_string());
                }
            }
            "服务" => {
                // 服务必须有服务分
                if service_w <= 0.0 {
                    missing_dimensions.push("服务分".to_string());
                }
                // 服务也应有技术分
                if args.technical_weight <= 0.0 {
                    missing_dimensions.push("技术分".to_string());
                }
            }
            "工程" => {
                // 工程必须有技术分
                if args.technical_weight <= 0.0 {
                    missing_dimensions.push("技术分".to_string());
                }
                // 工程商务分可选的，但建议有
                if business_w <= 0.0 {
                    // 工程商务分不是必须，但标记
                }
            }
            _ => {}
        }

        // ── 4. 技术分也做范围检查（建议性） ──
        // 技术分应占合理比例
        if args.technical_weight > 0.0 {
            checks.push(WeightCheckItem {
                item: "技术分权重".to_string(),
                current_value: args.technical_weight,
                required_range: "建议 20%-60%".to_string(),
                pass: args.technical_weight >= 20.0 && args.technical_weight <= 60.0,
            });
        }

        // ── 5. 综合判定 ──
        let has_violation = !sum_check
            || !price_range_ok
            || !missing_dimensions.is_empty();

        let has_risk = !has_violation
            && (!price_range_ok || args.technical_weight > 0.0
                && (args.technical_weight < 20.0 || args.technical_weight > 60.0));

        let status = if has_violation {
            "violation"
        } else if has_risk {
            "risk"
        } else {
            "compliant"
        };

        // ── 6. 综合摘要 ──
        let mut summary_parts: Vec<String> = Vec::new();

        if sum_check {
            summary_parts.push(format!(
                "权重总和 {} 等于总分 {}，通过",
                weight_sum, args.total_score
            ));
        } else {
            summary_parts.push(format!(
                "权重总和不闭合：各分项之和 {} 不等于总分 {}（差额 {}）",
                weight_sum,
                args.total_score,
                (weight_sum - args.total_score).abs()
            ));
        }

        if price_range_ok {
            summary_parts.push(format!(
                "价格分 {}% 在 {} 品类法定范围 {}-{}% 内",
                args.price_weight, args.procurement_category, price_min, price_max
            ));
        } else {
            summary_parts.push(format!(
                "价格分 {}% 超出 {} 品类法定范围 {}-{}%",
                args.price_weight, args.procurement_category, price_min, price_max
            ));
        }

        if !missing_dimensions.is_empty() {
            summary_parts.push(format!(
                "缺失评审维度：{}",
                missing_dimensions.join("、")
            ));
        } else {
            summary_parts.push("评审维度完整".to_string());
        }

        let summary = summary_parts.join("；") + "。";

        Ok(WeightDistributionResult {
            status: status.to_string(),
            sum_check,
            price_range_ok,
            missing_dimensions,
            checks,
            summary,
        })
    }
}

// ─── AgentTool 实现 ────────────────────────────────────────────

#[async_trait::async_trait]
impl AgentTool for ValidateWeightDistributionTool {
    fn name(&self) -> &str {
        "validate_weight_distribution"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "validate_weight_distribution",
                "description": "校验权重分配合规：各项权重和=总分、价格分在法定范围、必要评审维度齐全。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "price_weight": {
                            "type": "number",
                            "description": "价格分权重（百分比数值）。"
                        },
                        "technical_weight": {
                            "type": "number",
                            "description": "技术分权重（百分比数值）。"
                        },
                        "service_weight": {
                            "type": "number",
                            "description": "服务分权重（可选，百分比数值）。"
                        },
                        "business_weight": {
                            "type": "number",
                            "description": "商务分权重（可选，百分比数值）。"
                        },
                        "procurement_category": {
                            "type": "string",
                            "enum": ["货物", "工程", "服务"],
                            "description": "采购品类。"
                        },
                        "total_score": {
                            "type": "number",
                            "description": "总分值，通常为 100。"
                        }
                    },
                    "required": ["price_weight", "technical_weight", "procurement_category", "total_score"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: ValidateWeightDistributionArgs = serde_json::from_value(args)?;
        let result = Self::validate(&parsed)?;
        Ok(serde_json::to_value(&result)?)
    }
}

// ─── 测试 ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_goods_30_50_20_compliant() {
        // 货物：30%价格 + 50%技术 + 20%商务 = 100 → 合规
        let args = ValidateWeightDistributionArgs {
            price_weight: 30.0,
            technical_weight: 50.0,
            service_weight: None,
            business_weight: Some(20.0),
            procurement_category: "货物".to_string(),
            total_score: 100.0,
        };
        let result = ValidateWeightDistributionTool::validate(&args).unwrap();
        assert_eq!(result.status, "compliant");
        assert!(result.sum_check);
        assert!(result.price_range_ok);
        assert!(result.missing_dimensions.is_empty());
    }

    #[test]
    fn test_goods_15_60_25_violation_price_too_low() {
        // 货物：15%价格（低于30%下限）→ 违规
        let args = ValidateWeightDistributionArgs {
            price_weight: 15.0,
            technical_weight: 60.0,
            service_weight: None,
            business_weight: Some(25.0),
            procurement_category: "货物".to_string(),
            total_score: 100.0,
        };
        let result = ValidateWeightDistributionTool::validate(&args).unwrap();
        assert_eq!(result.status, "violation");
        assert!(!result.price_range_ok);
    }

    #[test]
    fn test_sum_not_100_violation() {
        // 权重只和 90 ≠ 100 → 违规
        let args = ValidateWeightDistributionArgs {
            price_weight: 30.0,
            technical_weight: 40.0,
            service_weight: None,
            business_weight: Some(20.0),
            procurement_category: "货物".to_string(),
            total_score: 100.0,
        };
        let result = ValidateWeightDistributionTool::validate(&args).unwrap();
        assert_eq!(result.status, "violation");
        assert!(!result.sum_check);
    }

    #[test]
    fn test_service_missing_service_dimension() {
        // 服务品类缺少服务分 → 违规
        let args = ValidateWeightDistributionArgs {
            price_weight: 20.0,
            technical_weight: 80.0,
            service_weight: None,
            business_weight: None,
            procurement_category: "服务".to_string(),
            total_score: 100.0,
        };
        let result = ValidateWeightDistributionTool::validate(&args).unwrap();
        assert_eq!(result.status, "violation");
        assert!(
            result
                .missing_dimensions
                .iter()
                .any(|d| d == "服务分"),
            "服务品类缺少服务分维度"
        );
    }

    #[test]
    fn test_service_20_40_40_compliant() {
        // 服务：20%价格(在10-30%内) + 40%技术 + 40%服务 = 100 → 合规
        let args = ValidateWeightDistributionArgs {
            price_weight: 20.0,
            technical_weight: 40.0,
            service_weight: Some(40.0),
            business_weight: None,
            procurement_category: "服务".to_string(),
            total_score: 100.0,
        };
        let result = ValidateWeightDistributionTool::validate(&args).unwrap();
        assert_eq!(result.status, "compliant");
    }

    #[test]
    fn test_construction_missing_technical() {
        // 工程缺少技术分 → 违规
        let args = ValidateWeightDistributionArgs {
            price_weight: 50.0,
            technical_weight: 0.0,
            service_weight: None,
            business_weight: Some(50.0),
            procurement_category: "工程".to_string(),
            total_score: 100.0,
        };
        let result = ValidateWeightDistributionTool::validate(&args).unwrap();
        assert_eq!(result.status, "violation");
        assert!(
            result
                .missing_dimensions
                .iter()
                .any(|d| d == "技术分"),
            "工程品类缺少技术分维度"
        );
    }

    #[test]
    fn test_invalid_category_errors() {
        let args = ValidateWeightDistributionArgs {
            price_weight: 30.0,
            technical_weight: 70.0,
            service_weight: None,
            business_weight: None,
            procurement_category: "咨询".to_string(),
            total_score: 100.0,
        };
        let result = ValidateWeightDistributionTool::validate(&args);
        assert!(result.is_err());
    }
}
