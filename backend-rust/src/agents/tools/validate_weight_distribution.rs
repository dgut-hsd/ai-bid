//! `validate_weight_distribution` 工具 — 权重分配合规检查。
//!
//! ## 核心职责
//!
//! 1. **权重总和闭合性（Product Contract）**：各权重之和应等于目标总值（不声称具体法规）
//! 2. **数值合法性**：负权重/超范围单项 → violation
//! 3. **缺失维度提示（Heuristic）**：建议货物有技术+商务、服务有服务分等
//! 4. **技术分分布（Heuristic）**：建议技术分合理范围，不产生 violation
//!
//! ## 价格权重规则
//!
//! 不在本工具内重复实现 87号令/214号的价格分范围检查。
//! 该职责由 `validate_scoring_formula` 根据 RuleSet 精确执行。
//! 本工具仅做权重总和一致性 + 数值合法性检查。

use anyhow::Result;
use serde::Deserialize;

use super::AgentTool;

// ─── 常量 ──────────────────────────────────────────────────────

/// 百分比模式目标总值
const TARGET: f64 = 100.0;
/// 单项权重下限: < 0 视为非法输入
const WEIGHT_MIN: f64 = 0.0;
/// 单项权重上限: > 100 视为非法输入（百分比模式下不超过100%）
const WEIGHT_MAX: f64 = 100.0;
/// 技术分建议范围（heuristic，不产生 violation）
const TECH_WEIGHT_SUGGESTED_MIN: f64 = 20.0;
const TECH_WEIGHT_SUGGESTED_MAX: f64 = 60.0;

/// 将百分比权重转为 basis points (×100) 做精确整数比较。
/// 同时校验最多两位小数精度。33.333 → Err, 33.33 → Ok(3333)。
fn to_basis_points(v: f64) -> Result<i64, String> {
    if !v.is_finite() { return Err(format!("non-finite value: {}", v)); }
    if v.abs() > 1_000_000.0 { return Err(format!("value out of range: {}", v)); }
    // 精度校验：最多两位小数
    let bp = (v * 100.0).round();
    if (bp / 100.0 - v).abs() > 1e-9 {
        return Err(format!("precision violation: {} exceeds 2 decimal places", v));
    }
    Ok(bp as i64)
}

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
    /// 当前权重是否覆盖全部评审因素。
    /// Some(true): 总和必须闭合；Some(false): 跳过闭合检查；
    /// None: 无法确定 → uncertain。
    pub weights_complete: Option<bool>,
}

// ─── 输出 ──────────────────────────────────────────────────────

/// 权重分配合规检查返回结果。
#[derive(Debug, serde::Serialize)]
struct WeightDistributionResult {
    /// 整体判定: "compliant"/"risk"/"uncertain"/"violation"
    status: String,
    /// 权重和 = target_total
    sum_check: bool,
    /// 实际权重总和
    weight_sum: f64,
    /// 预期目标值
    target_total: f64,
    /// 缺失的评审维度（heuristic）
    missing_dimensions: Vec<String>,
    /// 启发式风险列表（仅 heuristic，不影响 legal status）
    heuristic_risks: Vec<String>,
    /// 各检查项详情
    checks: Vec<WeightCheckItem>,
    /// 综合摘要
    summary: String,
    /// 法规/产品依据说明
    legal_basis: String,
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

fn uncertain_result(reason: &str, suggestion: &str) -> WeightDistributionResult {
    WeightDistributionResult {
        status: "uncertain".into(), sum_check: false,
        weight_sum: 0.0, target_total: 0.0,
        missing_dimensions: vec![], heuristic_risks: vec![],
        checks: vec![], summary: format!("{}：{}", reason, suggestion),
        legal_basis: "无法确定权重闭合状态。".into(),
    }
}

impl ValidateWeightDistributionTool {

    /// 核心校验逻辑。
    fn validate(args: &ValidateWeightDistributionArgs) -> Result<WeightDistributionResult> {
        // ── 0.6 weights_complete 缺失 → uncertain ──
        let complete = match args.weights_complete {
            Some(c) => c,
            None => return Ok(uncertain_result("missing weights_complete", "缺少 weights_complete 字段。无法确定当前权重是否覆盖全部评审因素，请提供。")),
        };

        // ── 0.7 invalid category → uncertain ──
        match args.procurement_category.as_str() {
            "货物" | "工程" | "服务" => {},
            other => return Ok(uncertain_result("invalid procurement_category",
                &format!("未识别的采购品类 '{}'。有效值: 货物/工程/服务", other))),
        }

        let mut checks: Vec<WeightCheckItem> = Vec::new();
        let mut missing_dimensions: Vec<String> = Vec::new();
        let mut heuristic_risks: Vec<String> = Vec::new();

        let service_w = args.service_weight.unwrap_or(0.0);
        let business_w = args.business_weight.unwrap_or(0.0);
        // ── 1. 数值合法性 + 精度校验 ──
        let mut invalid_input = false;
        for (value, label) in &[
            (args.price_weight, "价格分"), (args.technical_weight, "技术分"),
            (service_w, "服务分"), (business_w, "商务分"),
        ] {
            if *value < WEIGHT_MIN || *value > WEIGHT_MAX || !value.is_finite() {
                invalid_input = true;
                checks.push(WeightCheckItem {
                    item: format!("{}合法性", label), current_value: *value,
                    required_range: format!("{}-{}", WEIGHT_MIN, WEIGHT_MAX), pass: false,
                });
            } else if to_basis_points(*value).is_err() {
                invalid_input = true;
                checks.push(WeightCheckItem {
                    item: format!("{}精度", label), current_value: *value,
                    required_range: "最多两位小数".into(), pass: false,
                });
            }
        }
        if invalid_input {
            return Ok(WeightDistributionResult {
                status: "invalid_input".into(), sum_check: false,
                weight_sum: 0.0, target_total: TARGET,
                missing_dimensions: vec![], heuristic_risks: vec![],
                checks, summary: "输入非法：权重须在 0-100 范围内，为有限数值，且最多两位小数。".into(),
                legal_basis: "输入验证失败：权重数值不合法。".into(),
            });
        }

        let weight_sum = args.price_weight + args.technical_weight + service_w + business_w;

        // ── 2. 权重总和闭合性（Product Contract）──
        let sum_check = to_basis_points(weight_sum).ok()
            .zip(to_basis_points(TARGET).ok())
            .map_or(false, |(s, t)| s == t);

        if complete {
            checks.push(WeightCheckItem {
                item: "权重总和".to_string(),
                current_value: weight_sum,
                required_range: format!("应等于 {}", TARGET),
                pass: sum_check,
            });
        } else {
            checks.push(WeightCheckItem {
                item: "权重总和（仅部分评审因素）".to_string(),
                current_value: weight_sum,
                required_range: format!("目标值 {}", TARGET),
                pass: true,
            });
            if !sum_check {
                heuristic_risks.push(format!("部分权重总和 {} 不等于目标值 {}，需确认是否有遗漏的评审因素。", weight_sum, TARGET));
            }
        }

        // ── 3. 缺失维度（Heuristic）──
        match args.procurement_category.as_str() {
            "货物" => {
                if args.technical_weight <= 0.0 { missing_dimensions.push("技术分".to_string()); }
                if business_w <= 0.0 { missing_dimensions.push("商务分".to_string()); }
            }
            "服务" => {
                if service_w <= 0.0 { missing_dimensions.push("服务分".to_string()); }
                if args.technical_weight <= 0.0 { missing_dimensions.push("技术分".to_string()); }
            }
            "工程" => {
                if args.technical_weight <= 0.0 { missing_dimensions.push("技术分".to_string()); }
            }
            _ => unreachable!("handled above"),
        }
        if !missing_dimensions.is_empty() {
            heuristic_risks.push(format!("建议补充评审维度：{}", missing_dimensions.join("、")));
        }

        // ── 4. 技术分分布（Heuristic）──
        if args.technical_weight > 0.0
            && (args.technical_weight < TECH_WEIGHT_SUGGESTED_MIN
                || args.technical_weight > TECH_WEIGHT_SUGGESTED_MAX)
        {
            checks.push(WeightCheckItem {
                item: "技术分权重".to_string(),
                current_value: args.technical_weight,
                required_range: format!("建议 {}-{}%", TECH_WEIGHT_SUGGESTED_MIN, TECH_WEIGHT_SUGGESTED_MAX),
                pass: false,
            });
            heuristic_risks.push(format!(
                "技术分 {}% 不在建议范围 {}-{}%。（无法规强制依据）",
                args.technical_weight, TECH_WEIGHT_SUGGESTED_MIN, TECH_WEIGHT_SUGGESTED_MAX
            ));
        }

        // ── 5. 综合判定 ──
        let legal_violation = complete && !sum_check;
        let has_risk = !heuristic_risks.is_empty();

        let status = if legal_violation { "violation".to_string() }
            else if has_risk { "risk".to_string() }
            else { "compliant".to_string() };

        let legal_basis: String = if legal_violation {
            "权重一致性校验：所有权重之和应等于目标总值。该检查基于数值闭合性原则，不声称某一具体法规条款。".into()
        } else {
            String::new()
        };

        // ── 6. 综合摘要 ──
        let mut summary_parts: Vec<String> = Vec::new();
        if sum_check {
            summary_parts.push(format!("权重总和 {} 等于目标值 {}", weight_sum, TARGET));
        } else if complete {
            summary_parts.push(format!(
                "权重总和不闭合：各项和 {} 不等于目标值 {}（差额 {:.2}）",
                weight_sum, TARGET, (weight_sum - TARGET).abs()
            ));
        } else {
            summary_parts.push(format!("部分权重总和 {}（目标值 {}）", weight_sum, TARGET));
        }
        if !missing_dimensions.is_empty() { summary_parts.push(format!("建议补充：{}", missing_dimensions.join("、"))); }
        summary_parts.push(format!("状态：{}", if status == "compliant" { "合规" } else if status == "violation" { "违规" } else { "需关注" }));

        Ok(WeightDistributionResult {
            status, sum_check,
            weight_sum, target_total: TARGET,
            missing_dimensions, heuristic_risks, checks,
            summary: summary_parts.join("；") + "。",
            legal_basis,
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
                "description": "【使用场景】校验评审因素权重分配是否合规——\
                    ① 所有权重之和等于目标值（数值闭合性）；\
                    ② 各项权重非负数；\
                    ③ 建议性检查：核心评审维度是否遗漏、技术分分布是否合理。\
                    【不使用场景】不校验价格分具体范围（用 validate_scoring_formula）；\
                    不校验评分细则主观性（用 detect_subjective_scoring）。\
                    【注意】\n\
                    - 价格分法定范围由 validate_scoring_formula 根据采购方式/规则体系精确判定。\n\
                    - 本工具基于数值闭合性原则，不声称某一具体法规条款。\n\
                    - weights_complete 必填：true=严格闭合 / false=仅提示 / 缺失→uncertain。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "price_weight": {"type": "number", "description": "价格分权重"},
                        "technical_weight": {"type": "number", "description": "技术分权重"},
                        "service_weight": {"type": "number", "description": "服务分权重（可选）"},
                        "business_weight": {"type": "number", "description": "商务分权重（可选）"},
                        "procurement_category": {"type": "string", "enum": ["货物", "工程", "服务"], "description": "采购品类"},
                        "weights_complete": {"type": "boolean", "description": "必填。当前权重是否覆盖全部评审因素。true=严格闭合 / false=仅提示"}
                    },
                    "required": ["price_weight", "technical_weight", "procurement_category", "weights_complete"]
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

    fn _make(pw: f64, tw: f64, sw: Option<f64>, bw: Option<f64>, cat: &str, complete: Option<bool>) -> ValidateWeightDistributionArgs {
        ValidateWeightDistributionArgs {
            price_weight: pw, technical_weight: tw,
            service_weight: sw, business_weight: bw,
            procurement_category: cat.to_string(),
            weights_complete: complete,
        }
    }
    fn _v(a: &ValidateWeightDistributionArgs) -> WeightDistributionResult { ValidateWeightDistributionTool::validate(a).unwrap() }

    #[test] fn goods_30_50_20_compliant() { let a=_make(30.,50.,None,Some(20.),"货物",Some(true)); assert_eq!(_v(&a).status,"compliant"); }
    #[test] fn sum_90_violation() { let a=_make(30.,40.,None,Some(20.),"货物",Some(true)); assert_eq!(_v(&a).status,"violation"); }
    #[test] fn sum_99_99_violation() { let a=_make(30.,40.,None,Some(29.99),"货物",Some(true)); assert_eq!(_v(&a).status,"violation"); }
    #[test] fn sum_3333_3333_3334_compliant() { let a=_make(33.33,33.33,None,Some(33.34),"货物",Some(true)); assert_eq!(_v(&a).status,"compliant"); }
    #[test] fn sum_3333_3333_3333_violation() { let a=_make(33.33,33.33,None,Some(33.33),"货物",Some(true)); assert_eq!(_v(&a).status,"violation"); }
    #[test] fn sum_301_202_497_compliant() { let a=_make(30.1,20.2,None,Some(49.7),"货物",Some(true)); assert_eq!(_v(&a).status,"compliant"); }
    #[test] fn incomplete_sum_90_risk() { let a=_make(30.,40.,None,Some(20.),"货物",Some(false)); assert_eq!(_v(&a).status,"risk"); }
    #[test] fn missing_complete_uncertain() { let a=_make(30.,50.,None,Some(20.),"货物",None); assert_eq!(_v(&a).status,"uncertain"); }
    #[test] fn invalid_category_uncertain() { let a=_make(30.,50.,None,Some(20.),"banana",Some(true)); assert_eq!(_v(&a).status,"uncertain"); }
    #[test] fn negative_invalid_input() { let a=_make(-5.,80.,None,Some(25.),"货物",Some(true)); assert_eq!(_v(&a).status,"invalid_input"); }
    #[test] fn weight_0_valid() { let a=_make(0.,60.,None,Some(40.),"货物",Some(true)); assert_eq!(_v(&a).status,"compliant"); }
    #[test] fn weight_100_valid() { let a=_make(100.,0.,None,Some(0.),"货物",Some(true)); assert!(_v(&a).status != "invalid_input", "100.0 must be valid range"); }
    #[test] fn weight_100_01_invalid() { let a=_make(100.01,0.,None,Some(0.),"货物",Some(true)); assert_eq!(_v(&a).status,"invalid_input"); }
    #[test] fn weight_150_invalid() { let a=_make(150.,0.,None,Some(0.),"货物",Some(true)); assert_eq!(_v(&a).status,"invalid_input"); }
    #[test] fn huge_finite_invalid() { let a=_make(1e6,0.,None,Some(0.),"货物",Some(true)); assert_eq!(_v(&a).status,"invalid_input"); }
    #[test] fn incomplete_but_over_100_invalid() { let a=_make(150.,20.,None,Some(10.),"货物",Some(false)); assert_eq!(_v(&a).status,"invalid_input"); }
    #[test] fn nan_invalid_input() { let a=_make(f64::NAN,50.,None,Some(50.),"货物",Some(true)); assert_eq!(_v(&a).status,"invalid_input"); }
    #[test] fn inf_invalid_input() { let a=_make(f64::INFINITY,0.,None,Some(0.),"货物",Some(true)); assert_eq!(_v(&a).status,"invalid_input"); }
    #[test] fn precision_33_333_invalid() { let a=_make(33.333,33.333,None,Some(33.334),"货物",Some(true)); assert_eq!(_v(&a).status,"invalid_input"); }
    #[test] fn precision_30_00_valid() { let a=_make(30.00,50.00,None,Some(20.00),"货物",Some(true)); assert_eq!(_v(&a).status,"compliant"); }
    #[test] fn missing_service_risk() { let a=_make(20.,80.,None,None,"服务",Some(true)); let r=_v(&a); assert_eq!(r.status,"risk"); assert!(r.missing_dimensions.iter().any(|d| d=="服务分")); }
    #[test] fn tech_85_risk() { let a=_make(15.,85.,None,None,"货物",Some(true)); let r=_v(&a); assert_eq!(r.status,"risk"); assert!(r.heuristic_risks.iter().any(|h|h.contains("技术分"))); }
}
