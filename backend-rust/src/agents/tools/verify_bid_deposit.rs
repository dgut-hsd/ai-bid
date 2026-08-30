//! `verify_bid_deposit` 工具 — 保证金合规校验。
//!
//! 根据《政府采购法实施条例》第33条、第48条，校验投标保证金和履约保证金的
//! 金额比例、金额上限、形式合规性及退还时限是否满足法定要求。
//! 本工具执行纯数值计算和规则匹配，不访问外部 I/O。
//!
//! ## 法定阈值
//!
//! - 投标保证金 ≤ 合同金额的 2%；上限：货物/服务 50 万、工程 80 万
//! - 履约保证金 ≤ 合同金额的 10%
//! - 投标保证金退还时限 ≤ 5 个工作日
//! - 接受形式：现金/保函/保证保险
//!
//! ## 法条依据
//!
//! - 《政府采购法实施条例》第33条（投标保证金）
//! - 《政府采购法实施条例》第48条（履约保证金）

use anyhow::{Result, anyhow};
use serde::Deserialize;

use super::AgentTool;

// ─── 法定阈值常量 ──────────────────────────────────────────────

/// 投标保证金比例上限：2%
const BID_DEPOSIT_RATE_MAX: f64 = 0.02;
/// 投标保证金金额上限：货物/服务 50 万
const BID_DEPOSIT_CAP_GOODS_SERVICE: f64 = 50.0;
/// 投标保证金金额上限：工程 80 万
const BID_DEPOSIT_CAP_CONSTRUCTION: f64 = 80.0;
/// 履约保证金比例上限：10%
const PERFORMANCE_DEPOSIT_RATE_MAX: f64 = 0.10;
/// 投标保证金退还时限：5 个工作日
const BID_DEPOSIT_RETURN_DAYS_MAX: i64 = 5;
/// 接受的保证金形式
/// 《政府采购法实施条例》第33条：投标保证金应当以非现金形式提交。
const VALID_DEPOSIT_FORMS: &[&str] = &["保函", "保证保险"];

// ─── 参数 ──────────────────────────────────────────────────────

/// `verify_bid_deposit` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct VerifyBidDepositArgs {
    /// 保证金金额（万元）。如为 None 则不校验金额比例
    #[serde(default)]
    pub deposit_amount: Option<f64>,
    /// 合同金额（万元）。校验保证金比例时必填
    #[serde(default)]
    pub contract_amount: Option<f64>,
    /// 保证金形式：保函/保证保险（非现金形式）
    #[serde(default)]
    pub deposit_form: Option<String>,
    /// 退还时限（工作日），如为负数额外告警
    #[serde(default)]
    pub return_deadline_days: Option<i64>,
    /// 保证金类型：bid（投标保证金）或 performance（履约保证金）
    pub deposit_type: String,
    /// 采购品类：货物/工程/服务，用于区分数额上限（工程 80 万，货物+服务 50 万）
    #[serde(default)]
    pub procurement_category: Option<String>,
}

// ─── 输出 ──────────────────────────────────────────────────────

/// 保证金校验的返回结果。
#[derive(Debug, serde::Serialize)]
struct BidDepositResult {
    /// 保证金类型
    deposit_type: String,
    /// 整体合规判定
    status: DepositStatus,
    /// 各项检查的详细结果
    checks: Vec<DepositCheck>,
    /// 法条依据
    legal_basis: Vec<String>,
    /// 综合建议
    suggestion: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum DepositStatus {
    Compliant,
    Violation,
    InsufficientData,
}

#[derive(Debug, serde::Serialize)]
struct DepositCheck {
    /// 检查项名称
    check_name: String,
    /// 该项判定
    status: CheckItemStatus,
    /// 实际值描述
    actual_value: String,
    /// 法定要求
    required_value: String,
    /// 判定说明
    detail: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum CheckItemStatus {
    Pass,
    Fail,
    Skip,
}

// ─── 工具实现 ──────────────────────────────────────────────────

/// `verify_bid_deposit` 工具实现。
///
/// 纯数值计算与规则匹配工具，无外部依赖。
pub struct VerifyBidDepositTool;

impl VerifyBidDepositTool {
    /// 核心校验逻辑。
    fn verify(args: &VerifyBidDepositArgs) -> Result<BidDepositResult> {
        // 1. 校验 deposit_type
        let is_bid_deposit = match args.deposit_type.as_str() {
            "bid" => true,
            "performance" => false,
            _ => {
                return Err(anyhow!(
                    "无效的 deposit_type '{}'，有效值为: bid/performance",
                    args.deposit_type
                ))
            }
        };

        let deposit_type_label = if is_bid_deposit { "投标保证金" } else { "履约保证金" };
        let mut checks: Vec<DepositCheck> = Vec::new();
        let mut legal_basis: Vec<String> = Vec::new();
        let mut has_violation = false;
        let mut has_data = false;

        if is_bid_deposit {
            legal_basis.push(
                "《政府采购法实施条例》第33条：招标文件要求投标人提交投标保证金的，\
                投标保证金不得超过采购项目预算金额的2%。\
                投标保证金应当以支票、汇票、本票或者金融机构、担保机构出具的保函等非现金形式提交。\
                采购人或者采购代理机构应当自中标通知书发出之日起5个工作日内退还未中标供应商的投标保证金，\
                自政府采购合同签订之日起5个工作日内退还中标供应商的投标保证金。"
                    .to_string(),
            );

            // ① 金额比例检查
            if let (Some(deposit), Some(contract)) = (args.deposit_amount, args.contract_amount) {
                has_data = true;
                if contract <= 0.0 {
                    return Err(anyhow!("合同金额必须大于 0"));
                }
                let actual_rate = deposit / contract;
                if actual_rate > BID_DEPOSIT_RATE_MAX {
                    has_violation = true;
                    checks.push(DepositCheck {
                        check_name: "投标保证金比例".to_string(),
                        status: CheckItemStatus::Fail,
                        actual_value: format!("{:.2}% ({}万 / {}万)", actual_rate * 100.0, deposit, contract),
                        required_value: format!("≤ {:.0}%", BID_DEPOSIT_RATE_MAX * 100.0),
                        detail: format!(
                            "投标保证金比例 {:.2}% 超过法定上限 2%（《政府采购法实施条例》第33条）。\
                            建议将保证金金额降至 {} 万元以下。",
                            actual_rate * 100.0,
                            (contract * BID_DEPOSIT_RATE_MAX * 100.0).round() / 100.0
                        ),
                    });
                } else {
                    checks.push(DepositCheck {
                        check_name: "投标保证金比例".to_string(),
                        status: CheckItemStatus::Pass,
                        actual_value: format!("{:.2}%", actual_rate * 100.0),
                        required_value: format!("≤ {:.0}%", BID_DEPOSIT_RATE_MAX * 100.0),
                        detail: format!("投标保证金比例 {:.2}%，在法定 2% 上限内。", actual_rate * 100.0),
                    });
                }
            }

            // ② 金额上限检查（货物/服务 50万，工程 80万）
            if let Some(deposit) = args.deposit_amount {
                has_data = true;
                let cat = args.procurement_category.as_deref().unwrap_or("货物");
                let cap = if cat == "工程" {
                    BID_DEPOSIT_CAP_CONSTRUCTION
                } else {
                    BID_DEPOSIT_CAP_GOODS_SERVICE
                };
                if deposit > cap {
                    has_violation = true;
                    checks.push(DepositCheck {
                        check_name: "投标保证金金额上限".to_string(),
                        status: CheckItemStatus::Fail,
                        actual_value: format!("{} 万元", deposit),
                        required_value: format!("≤ {} 万元（货物/服务）或 ≤ {} 万元（工程）", BID_DEPOSIT_CAP_GOODS_SERVICE, BID_DEPOSIT_CAP_CONSTRUCTION),
                        detail: format!(
                            "投标保证金 {} 万元超过法定上限（货物/服务 ≤ 50万，工程 ≤ 80万）。",
                            deposit
                        ),
                    });
                } else {
                    checks.push(DepositCheck {
                        check_name: "投标保证金金额上限".to_string(),
                        status: CheckItemStatus::Pass,
                        actual_value: format!("{} 万元", deposit),
                        required_value: format!("≤ {} 万元（货物/服务）或 ≤ {} 万元（工程）", BID_DEPOSIT_CAP_GOODS_SERVICE, BID_DEPOSIT_CAP_CONSTRUCTION),
                        detail: format!("投标保证金 {} 万元在法定上限内。", deposit),
                    });
                }
            }

            // ③ 保证金形式检查
            if let Some(ref form) = args.deposit_form {
                has_data = true;
                if VALID_DEPOSIT_FORMS.contains(&form.as_str()) {
                    checks.push(DepositCheck {
                        check_name: "保证金形式".to_string(),
                        status: CheckItemStatus::Pass,
                        actual_value: form.clone(),
                        required_value: "现金/保函/保证保险".to_string(),
                        detail: format!("保证金形式'{}'符合法定要求。", form),
                    });
                } else {
                    has_violation = true;
                    checks.push(DepositCheck {
                        check_name: "保证金形式".to_string(),
                        status: CheckItemStatus::Fail,
                        actual_value: form.clone(),
                        required_value: "现金/保函/保证保险".to_string(),
                        detail: format!(
                            "保证金形式'{}'不在法定接受范围内。应使用以下形式之一：{}。\
                            《条例》第33条要求以非现金形式提交。",
                            form,
                            VALID_DEPOSIT_FORMS.join("、")
                        ),
                    });
                }
            }

            // ④ 退还时限检查
            if let Some(days) = args.return_deadline_days {
                has_data = true;
                if days <= BID_DEPOSIT_RETURN_DAYS_MAX {
                    checks.push(DepositCheck {
                        check_name: "退还时限".to_string(),
                        status: CheckItemStatus::Pass,
                        actual_value: format!("{} 工作日", days),
                        required_value: format!("≤ {} 工作日", BID_DEPOSIT_RETURN_DAYS_MAX),
                        detail: format!("退还时限 {} 工作日符合法定要求。", days),
                    });
                } else {
                    has_violation = true;
                    checks.push(DepositCheck {
                        check_name: "退还时限".to_string(),
                        status: CheckItemStatus::Fail,
                        actual_value: format!("{} 工作日", days),
                        required_value: format!("≤ {} 工作日", BID_DEPOSIT_RETURN_DAYS_MAX),
                        detail: format!(
                            "退还时限 {} 工作日超过法定上限 {} 工作日（《条例》第33条）。\
                            建议将退还时限压缩至 5 个工作日内。",
                            days, BID_DEPOSIT_RETURN_DAYS_MAX
                        ),
                    });
                }
            }
        } else {
            // 履约保证金
            legal_basis.push(
                "《政府采购法实施条例》第48条：采购文件要求中标或者成交供应商提交履约保证金的，\
                供应商应当以支票、汇票、本票或者金融机构、担保机构出具的保函等非现金形式提交。\
                履约保证金的数额不得超过政府采购合同金额的10%。"
                    .to_string(),
            );

            // ① 金额比例检查
            if let (Some(deposit), Some(contract)) = (args.deposit_amount, args.contract_amount) {
                has_data = true;
                if contract <= 0.0 {
                    return Err(anyhow!("合同金额必须大于 0"));
                }
                let actual_rate = deposit / contract;
                if actual_rate > PERFORMANCE_DEPOSIT_RATE_MAX {
                    has_violation = true;
                    checks.push(DepositCheck {
                        check_name: "履约保证金比例".to_string(),
                        status: CheckItemStatus::Fail,
                        actual_value: format!("{:.2}% ({}万 / {}万)", actual_rate * 100.0, deposit, contract),
                        required_value: format!("≤ {:.0}%", PERFORMANCE_DEPOSIT_RATE_MAX * 100.0),
                        detail: format!(
                            "履约保证金比例 {:.2}% 超过法定上限 10%（《条例》第48条）。\
                            建议将履约保证金金额降至 {} 万元以下。",
                            actual_rate * 100.0,
                            (contract * PERFORMANCE_DEPOSIT_RATE_MAX * 100.0).round() / 100.0
                        ),
                    });
                } else {
                    checks.push(DepositCheck {
                        check_name: "履约保证金比例".to_string(),
                        status: CheckItemStatus::Pass,
                        actual_value: format!("{:.2}%", actual_rate * 100.0),
                        required_value: format!("≤ {:.0}%", PERFORMANCE_DEPOSIT_RATE_MAX * 100.0),
                        detail: format!("履约保证金比例 {:.2}%，在法定 10% 上限内。", actual_rate * 100.0),
                    });
                }
            }

            // ② 保证金形式检查
            if let Some(ref form) = args.deposit_form {
                has_data = true;
                if VALID_DEPOSIT_FORMS.contains(&form.as_str()) {
                    checks.push(DepositCheck {
                        check_name: "保证金形式".to_string(),
                        status: CheckItemStatus::Pass,
                        actual_value: form.clone(),
                        required_value: "现金/保函/保证保险".to_string(),
                        detail: format!("保证金形式'{}'符合法定要求。", form),
                    });
                } else {
                    has_violation = true;
                    checks.push(DepositCheck {
                        check_name: "保证金形式".to_string(),
                        status: CheckItemStatus::Fail,
                        actual_value: form.clone(),
                        required_value: "现金/保函/保证保险".to_string(),
                        detail: format!(
                            "保证金形式'{}'不在法定接受范围内。应使用以下形式之一：{}。",
                            form,
                            VALID_DEPOSIT_FORMS.join("、")
                        ),
                    });
                }
            }

            // ③ 退还时限 — 履约保证金按合同约定，仅记录
            if let Some(days) = args.return_deadline_days {
                has_data = true;
                checks.push(DepositCheck {
                    check_name: "退还时限（履约）".to_string(),
                    status: CheckItemStatus::Skip,
                    actual_value: format!("{} 工作日", days),
                    required_value: "按合同约定".to_string(),
                    detail: "履约保证金退还时限无统一法定上限，按合同约定执行。建议合同明确约定退还条件与时限。".to_string(),
                });
            }
        }

        if checks.is_empty() && !has_data {
            return Err(anyhow!(
                "缺少必要的校验参数：需至少提供 deposit_amount+contract_amount、deposit_form 或 return_deadline_days 之一"
            ));
        }

        // 判定整体状态
        let status = if has_violation {
            DepositStatus::Violation
        } else if has_data {
            DepositStatus::Compliant
        } else {
            DepositStatus::InsufficientData
        };

        let suggestion = if has_violation {
            let fail_items: Vec<&str> = checks
                .iter()
                .filter(|c| matches!(c.status, CheckItemStatus::Fail))
                .map(|c| c.check_name.as_str())
                .collect();
            format!(
                "存在 {} 项违规：{}。请修正后重新校验。",
                fail_items.len(),
                fail_items.join("、")
            )
        } else {
            format!("{}合规检查通过，各项指标均满足法定要求。", deposit_type_label)
        };

        Ok(BidDepositResult {
            deposit_type: args.deposit_type.clone(),
            status,
            checks,
            legal_basis,
            suggestion,
        })
    }
}

// ─── AgentTool 实现 ────────────────────────────────────────────

#[async_trait::async_trait]
impl AgentTool for VerifyBidDepositTool {
    fn name(&self) -> &str {
        "verify_bid_deposit"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "verify_bid_deposit",
                "description": "校验投标/履约保证金(投标≤2%且≤50万/80万、履约≤10%、退还≤5工作日)。金额单位万元。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "deposit_amount": {
                            "type": "number",
                            "description": "保证金金额，单位：万元。如 5 万保证金则传入 5.0。可选。"
                        },
                        "contract_amount": {
                            "type": "number",
                            "description": "合同金额/采购预算金额，单位：万元。校验保证金比例时必填。可选。"
                        },
                        "deposit_form": {
                            "type": "string",
                            "enum": ["现金", "保函", "保证保险"],
                            "description": "保证金形式。可选。"
                        },
                        "return_deadline_days": {
                            "type": "integer",
                            "description": "保证金退还时限，单位：工作日。如中标后 5 个工作日内退则传入 5。可选。"
                        },
                        "deposit_type": {
                            "type": "string",
                            "enum": ["bid", "performance"],
                            "description": "保证金类型：'bid' 为投标保证金，'performance' 为履约保证金。"
                        }
                    },
                    "required": ["deposit_type"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: VerifyBidDepositArgs = serde_json::from_value(args)?;
        let result = Self::verify(&parsed)?;
        Ok(serde_json::to_value(&result)?)
    }
}

// ─── 测试 ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bid_deposit_rate_compliant() {
        // 1000 万合同，15 万投标保证金 → 1.5% < 2%，合规
        let args = VerifyBidDepositArgs {
            deposit_amount: Some(15.0),
            contract_amount: Some(1000.0),
            deposit_form: None,
            return_deadline_days: None,
            deposit_type: "bid".to_string(),
            procurement_category: None,
        };
        let result = VerifyBidDepositTool::verify(&args).unwrap();
        assert!(matches!(result.status, DepositStatus::Compliant));
    }

    #[test]
    fn test_bid_deposit_rate_violation() {
        // 1000 万合同，30 万投标保证金 → 3.0% > 2%，违规
        let args = VerifyBidDepositArgs {
            deposit_amount: Some(30.0),
            contract_amount: Some(1000.0),
            deposit_form: None,
            return_deadline_days: None,
            deposit_type: "bid".to_string(),
            procurement_category: None,
        };
        let result = VerifyBidDepositTool::verify(&args).unwrap();
        assert!(matches!(result.status, DepositStatus::Violation));
        assert!(
            result.checks.iter().any(|c| c.check_name == "投标保证金比例"
                && matches!(c.status, CheckItemStatus::Fail))
        );
    }

    #[test]
    fn test_bid_deposit_cap_violation() {
        // 10000 万合同，60 万投标保证金 → 0.6% < 2%，但 60万 > 50万上限（货物/服务），违规
        let args = VerifyBidDepositArgs {
            deposit_amount: Some(60.0),
            contract_amount: Some(10000.0),
            deposit_form: None,
            return_deadline_days: None,
            deposit_type: "bid".to_string(),
            procurement_category: None,
        };
        let result = VerifyBidDepositTool::verify(&args).unwrap();
        assert!(matches!(result.status, DepositStatus::Violation));
        assert!(
            result.checks.iter().any(|c| c.check_name == "投标保证金金额上限"
                && matches!(c.status, CheckItemStatus::Fail))
        );
    }

    #[test]
    fn test_performance_deposit_compliant() {
        let args = VerifyBidDepositArgs {
            deposit_amount: Some(80.0),
            contract_amount: Some(1000.0),
            deposit_form: Some("保函".to_string()),
            return_deadline_days: None,
            deposit_type: "performance".to_string(),
            procurement_category: None,
        };
        let result = VerifyBidDepositTool::verify(&args).unwrap();
        assert!(matches!(result.status, DepositStatus::Compliant));
    }

    #[test]
    fn test_performance_deposit_violation() {
        // 1000 万合同，150 万履约保证金 → 15% > 10%，违规
        let args = VerifyBidDepositArgs {
            deposit_amount: Some(150.0),
            contract_amount: Some(1000.0),
            deposit_form: None,
            return_deadline_days: None,
            deposit_type: "performance".to_string(),
            procurement_category: None,
        };
        let result = VerifyBidDepositTool::verify(&args).unwrap();
        assert!(matches!(result.status, DepositStatus::Violation));
    }

    #[test]
    fn test_return_deadline_violation() {
        let args = VerifyBidDepositArgs {
            deposit_amount: None,
            contract_amount: None,
            deposit_form: None,
            return_deadline_days: Some(10),
            deposit_type: "bid".to_string(),
            procurement_category: None,
        };
        let result = VerifyBidDepositTool::verify(&args).unwrap();
        assert!(matches!(result.status, DepositStatus::Violation));
        assert!(
            result.checks.iter().any(|c| c.check_name == "退还时限"
                && matches!(c.status, CheckItemStatus::Fail))
        );
    }

    #[test]
    fn test_invalid_deposit_form() {
        let args = VerifyBidDepositArgs {
            deposit_amount: None,
            contract_amount: None,
            deposit_form: Some("支票".to_string()),
            return_deadline_days: None,
            deposit_type: "bid".to_string(),
            procurement_category: None,
        };
        let result = VerifyBidDepositTool::verify(&args).unwrap();
        assert!(matches!(result.status, DepositStatus::Violation));
    }

    #[test]
    fn test_invalid_deposit_type() {
        let args = VerifyBidDepositArgs {
            deposit_amount: Some(10.0),
            contract_amount: Some(1000.0),
            deposit_form: None,
            return_deadline_days: None,
            deposit_type: "guarantee".to_string(),
            procurement_category: None,
        };
        let result = VerifyBidDepositTool::verify(&args);
        assert!(result.is_err());
    }
}
