//! `verify_bid_deposit` 工具 — 保证金合规校验。
//!
//! 根据《政府采购法实施条例》第33条、第48条，校验投标保证金和履约保证金的
//! 金额比例、形式合规性及退还时限是否满足法定要求。
//! 本工具执行纯数值计算和规则匹配，不访问外部 I/O。
//!
//! ## 法定阈值
//!
//! - 投标保证金 ≤ **采购项目预算金额**的 2%（实施条例第33条）
//! - 履约保证金 ≤ 政府采购**合同金额**的 10%（实施条例第48条）
//! - 投标保证金退还时限 ≤ 5 个工作日（实施条例第33条）
//! - 合法形式：支票、汇票、本票、保函等**非现金形式**（现金为违法形式）
//! - 政府采购体系无50万/80万金额封顶（金额上限仅存在于《工程建设项目施工招标投标办法》第37条，属工程招投标体系）
//!
//! ## 法条依据
//!
//! - 《政府采购法实施条例》第33条（投标保证金）
//! - 《政府采购法实施条例》第48条（履约保证金）

use anyhow::{Result, anyhow};
use serde::Deserialize;

use super::AgentTool;

// ─── 法定阈值常量 ──────────────────────────────────────────────

/// 投标保证金比例上限：2%（实施条例第33条）
const BID_DEPOSIT_RATE_MAX: f64 = 0.02;
/// 履约保证金比例上限：10%（实施条例第48条）
const PERFORMANCE_DEPOSIT_RATE_MAX: f64 = 0.10;
/// 投标保证金退还时限：5 个工作日（实施条例第33条）
const BID_DEPOSIT_RETURN_DAYS_MAX: i64 = 5;

/// 明确合法的非现金形式（实施条例第33条/48条原文）
const LEGAL_NON_CASH_FORMS: &[&str] = &[
    "支票", "现金支票", "保兑支票",
    "汇票", "银行汇票",
    "本票",
    "保函", "银行保函", "担保保函", "电子保函",
    "保证保险", "保险保函",
];
/// 明确违法的形式
const ILLEGAL_CASH_FORM: &str = "现金";

// ─── 参数 ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct VerifyBidDepositArgs {
    /// 保证金金额（万元）。如为 None 则不校验金额比例
    #[serde(default)]
    pub deposit_amount: Option<f64>,
    /// 采购项目预算金额（万元）。bid deposit 的法定比例基数；performance 不使用此字段。
    /// 缺省时 bid 场景返回 InsufficientInput。
    #[serde(default)]
    pub budget_amount: Option<f64>,
    /// 合同金额（万元）。performance deposit 的法定比例基数；bid deposit 不使用此字段。
    /// 缺省时 performance 场景返回 InsufficientInput。
    #[serde(default)]
    pub contract_amount: Option<f64>,
    /// 保证金原始形式文本（可含"现金"等）
    #[serde(default)]
    pub deposit_form: Option<String>,
    /// 退还时限（工作日）
    #[serde(default)]
    pub return_deadline_days: Option<i64>,
    /// 保证金类型：bid（投标保证金）或 performance（履约保证金）
    pub deposit_type: String,
    /// 采购品类：货物/工程/服务。未来用于区分工程招投标体系，当前政府采购不依赖此字段。
    #[serde(default)]
    pub procurement_category: Option<String>,
}

// ─── 输出 ──────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
struct BidDepositResult {
    deposit_type: String,
    status: DepositStatus,
    checks: Vec<DepositCheck>,
    legal_basis: Vec<String>,
    suggestion: String,
}

#[derive(Debug, serde::Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum DepositStatus {
    Compliant,
    Violation,
    InsufficientInput,
    Uncertain,
}

#[derive(Debug, serde::Serialize)]
struct DepositCheck {
    check_name: String,
    status: CheckItemStatus,
    actual_value: String,
    required_value: String,
    detail: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum CheckItemStatus {
    Pass,
    Fail,
    Skip,
    Uncertain,
}

// ─── form 归一化 ──────────────────────────────────────────────

fn classify_deposit_form(raw: &str) -> (&'static str, CheckItemStatus, String) {
    let lower = raw.trim();
    // exact match cash = illegal
    if lower == ILLEGAL_CASH_FORM || lower == "现金" {
        return (ILLEGAL_CASH_FORM, CheckItemStatus::Fail,
                "现金形式提交保证金违反《条例》第33条/48条[非现金形式]要求。".to_string());
    }
    for known in LEGAL_NON_CASH_FORMS {
        if lower == *known {
            return (known, CheckItemStatus::Pass,
                    format!("{} 属于法定非现金形式", known));
        }
    }
    // unknown — 法规用"等非现金形式"，不硬判违法
    ("__unknown__", CheckItemStatus::Uncertain,
     "未识别的保证金形式，可能属于其他非现金形式（法规使用[等非现金形式]条款），建议人工确认。".to_string())
}

// ─── 工具实现 ──────────────────────────────────────────────────

pub struct VerifyBidDepositTool;

impl VerifyBidDepositTool {
    fn verify(args: &VerifyBidDepositArgs) -> Result<BidDepositResult> {
        let is_bid = match args.deposit_type.as_str() {
            "bid" => true,
            "performance" => false,
            _ => return Err(anyhow!("无效的 deposit_type '{}'，有效值为: bid/performance", args.deposit_type)),
        };

        let label = if is_bid { "投标保证金" } else { "履约保证金" };
        let mut checks: Vec<DepositCheck> = Vec::new();
        let mut legal_basis: Vec<String> = Vec::new();
        let mut has_violation = false;
        let mut has_uncertain = false;
        let mut has_data = false;
        let mut insufficient = false;

        if is_bid {
            legal_basis.push(
                "《政府采购法实施条例》第33条：投标保证金不得超过采购项目预算金额的2%。\
                 投标保证金应当以支票、汇票、本票或者金融机构、担保机构出具的保函等非现金形式提交。\
                 应当自中标通知书发出之日起5个工作日内退还未中标供应商的投标保证金，\
                 自政府采购合同签订之日起5个工作日内退还中标供应商的投标保证金。"
                    .to_string(),
            );

            // ① 金额比例检查（基数=budget_amount）
            if args.deposit_amount.is_some() && args.budget_amount.is_none() {
                checks.push(DepositCheck {
                    check_name: "投标保证金比例".to_string(),
                    status: CheckItemStatus::Skip,
                    actual_value: "缺失预算金额".to_string(),
                    required_value: format!("≤ {:.0}%（基数：采购项目预算金额）", BID_DEPOSIT_RATE_MAX * 100.0),
                    detail: "投标保证金比例需以采购项目预算金额为基数计算，当前缺少 budget_amount，无法出具确定性结论。请提供预算金额后重新校验。".to_string(),
                });
                insufficient = true;
                has_data = true;
            }

            if let (Some(deposit), Some(budget)) = (args.deposit_amount, args.budget_amount) {
                has_data = true;
                if budget <= 0.0 {
                    return Err(anyhow!("预算金额必须大于 0"));
                }
                let rate = deposit / budget;
                if rate > BID_DEPOSIT_RATE_MAX {
                    has_violation = true;
                    checks.push(DepositCheck {
                        check_name: "投标保证金比例".to_string(),
                        status: CheckItemStatus::Fail,
                        actual_value: format!("{:.2}%（{}万 / {}万预算）", rate * 100.0, deposit, budget),
                        required_value: format!("≤ {:.0}%（实施条例第33条）", BID_DEPOSIT_RATE_MAX * 100.0),
                        detail: format!("投标保证金比例 {:.2}% 超过法定上限2%。建议降至 {} 万元以下。",
                            rate * 100.0, (budget * BID_DEPOSIT_RATE_MAX * 100.0).round() / 100.0),
                    });
                } else {
                    checks.push(DepositCheck {
                        check_name: "投标保证金比例".to_string(),
                        status: CheckItemStatus::Pass,
                        actual_value: format!("{:.2}%", rate * 100.0),
                        required_value: format!("≤ {:.0}%", BID_DEPOSIT_RATE_MAX * 100.0),
                        detail: format!("投标保证金比例 {:.2}%，在法定2%上限内。", rate * 100.0),
                    });
                }
            }

            // ② 保证金形式检查
            if let Some(ref form) = args.deposit_form {
                has_data = true;
                let (normalized, item_status, detail) = classify_deposit_form(form);
                if matches!(item_status, CheckItemStatus::Fail) {
                    has_violation = true;
                }
                if matches!(item_status, CheckItemStatus::Uncertain) {
                    has_uncertain = true;
                }
                checks.push(DepositCheck {
                    check_name: "保证金形式".to_string(),
                    status: item_status,
                    actual_value: form.clone(),
                    required_value: "非现金形式（支票/汇票/本票/保函/保证保险等）".to_string(),
                    detail: format!("{}（原始输入：{}，归一化：{}）", detail, form, normalized),
                });
            }

            // ③ 退还时限检查
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
                        detail: format!("退还时限 {} 工作日超过法定上限 {} 工作日（《条例》第33条）。",
                            days, BID_DEPOSIT_RETURN_DAYS_MAX),
                    });
                }
            }
        } else {
            // 履约保证金
            legal_basis.push(
                "《政府采购法实施条例》第48条：履约保证金应当以支票、汇票、本票或者金融机构、\
                 担保机构出具的保函等非现金形式提交。履约保证金的数额不得超过政府采购合同金额的10%。"
                    .to_string(),
            );

            // ① 金额比例检查（基数=contract_amount）
            if args.deposit_amount.is_some() && args.contract_amount.is_none() {
                checks.push(DepositCheck {
                    check_name: "履约保证金比例".to_string(),
                    status: CheckItemStatus::Skip,
                    actual_value: "缺失合同金额".to_string(),
                    required_value: format!("≤ {:.0}%（基数：合同金额）", PERFORMANCE_DEPOSIT_RATE_MAX * 100.0),
                    detail: "履约保证金比例需以合同金额为基数计算，当前缺少 contract_amount。请提供合同金额后重新校验。".to_string(),
                });
                insufficient = true;
                has_data = true;
            }

            if let (Some(deposit), Some(contract)) = (args.deposit_amount, args.contract_amount) {
                has_data = true;
                if contract <= 0.0 {
                    return Err(anyhow!("合同金额必须大于 0"));
                }
                let rate = deposit / contract;
                if rate > PERFORMANCE_DEPOSIT_RATE_MAX {
                    has_violation = true;
                    checks.push(DepositCheck {
                        check_name: "履约保证金比例".to_string(),
                        status: CheckItemStatus::Fail,
                        actual_value: format!("{:.2}%（{}万 / {}万合同）", rate * 100.0, deposit, contract),
                        required_value: format!("≤ {:.0}%（实施条例第48条）", PERFORMANCE_DEPOSIT_RATE_MAX * 100.0),
                        detail: format!("履约保证金比例 {:.2}% 超过法定上限10%。建议降至 {} 万元以下。",
                            rate * 100.0, (contract * PERFORMANCE_DEPOSIT_RATE_MAX * 100.0).round() / 100.0),
                    });
                } else {
                    checks.push(DepositCheck {
                        check_name: "履约保证金比例".to_string(),
                        status: CheckItemStatus::Pass,
                        actual_value: format!("{:.2}%", rate * 100.0),
                        required_value: format!("≤ {:.0}%", PERFORMANCE_DEPOSIT_RATE_MAX * 100.0),
                        detail: format!("履约保证金比例 {:.2}%，在法定10%上限内。", rate * 100.0),
                    });
                }
            }

            // ② 保证金形式检查（履约相同 form 规则）
            if let Some(ref form) = args.deposit_form {
                has_data = true;
                let (normalized, item_status, detail) = classify_deposit_form(form);
                if matches!(item_status, CheckItemStatus::Fail) {
                    has_violation = true;
                }
                if matches!(item_status, CheckItemStatus::Uncertain) {
                    has_uncertain = true;
                }
                checks.push(DepositCheck {
                    check_name: "保证金形式".to_string(),
                    status: item_status,
                    actual_value: form.clone(),
                    required_value: "非现金形式（支票/汇票/本票/保函等）".to_string(),
                    detail: format!("{}（原始输入：{}，归一化：{}）", detail, form, normalized),
                });
            }

            // ③ 退还时限 — 履约按合同约定，仅记录
            if let Some(days) = args.return_deadline_days {
                has_data = true;
                checks.push(DepositCheck {
                    check_name: "退还时限（履约）".to_string(),
                    status: CheckItemStatus::Skip,
                    actual_value: format!("{} 工作日", days),
                    required_value: "按合同约定".to_string(),
                    detail: "履约保证金退还时限无统一法定上限，按合同约定执行。".to_string(),
                });
            }
        }

        // 判定整体状态
        let status = if insufficient {
            DepositStatus::InsufficientInput
        } else if has_violation {
            DepositStatus::Violation
        } else if has_uncertain {
            DepositStatus::Uncertain
        } else if has_data {
            DepositStatus::Compliant
        } else {
            DepositStatus::InsufficientInput
        };

        let suggestion = if insufficient {
            "缺少关键参数，无法完成校验。请补充必要信息后重试。".to_string()
        } else if has_violation {
            let fail_items: Vec<&str> = checks.iter()
                .filter(|c| matches!(c.status, CheckItemStatus::Fail))
                .map(|c| c.check_name.as_str()).collect();
            format!("存在 {} 项违规：{}。请修正后重新校验。", fail_items.len(), fail_items.join("、"))
        } else if has_uncertain {
            let uncertain_items: Vec<&str> = checks.iter()
                .filter(|c| matches!(c.status, CheckItemStatus::Uncertain))
                .map(|c| c.check_name.as_str()).collect();
            format!("{} 项结果为不确定：{}。建议人工复核。", uncertain_items.len(), uncertain_items.join("、"))
        } else {
            format!("{}合规检查通过，各项指标均满足法定要求。", label)
        };

        Ok(BidDepositResult { deposit_type: args.deposit_type.clone(), status, checks, legal_basis, suggestion })
    }
}

// ─── AgentTool 实现 ────────────────────────────────────────────

#[async_trait::async_trait]
impl AgentTool for VerifyBidDepositTool {
    fn name(&self) -> &str { "verify_bid_deposit" }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "verify_bid_deposit",
                "description": "【使用场景】校验投标保证金/履约保证金的合规性——\
                    ① 投标保证金金额是否≤采购项目预算金额的2%（基数不是合同金额）；\
                    ② 履约保证金是否≤合同金额的10%；\
                    ③ 保证金形式是否为法定非现金形式（支票/汇票/本票/保函/保证保险等）；\
                    现金为违法形式；\
                    ④ 退还时限是否符合法定要求（投标保证金≤5工作日）。\
                    【不使用场景】不负责审核保证金的具体退还流程和退还条件细节。\
                    【法条依据】《政府采购法实施条例》第33条、第48条。\
                    【注意】金额单位为万元。deposit_type='bid'时需budget_amount；\
                    'performance'时需contract_amount。政府采购体系无50/80万金额封顶（该上限属于工程招投标体系）。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "deposit_amount": {"type": "number", "description": "保证金金额（万元）"},
                        "budget_amount": {"type": "number", "description": "采购项目预算金额（万元）。投标保证金(bid)的法定比例基数，必填"},
                        "contract_amount": {"type": "number", "description": "合同金额（万元）。履约保证金(performance)的法定比例基数，必填"},
                        "deposit_form": {"type": "string", "description": "保证金形式原文（如'支票''保函''现金'等）。现金会被判违规，未识别形式返回不确定"},
                        "return_deadline_days": {"type": "integer", "description": "退还时限（工作日）"},
                        "deposit_type": {"type": "string", "enum": ["bid","performance"], "description": "保证金类型"},
                        "procurement_category": {"type": "string", "enum": ["货物","工程","服务"], "description": "采购品类。bid deposit时可选"}
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

    fn bid(deposit: f64, budget: f64) -> VerifyBidDepositArgs {
        VerifyBidDepositArgs { deposit_amount: Some(deposit), budget_amount: Some(budget),
            contract_amount: None, deposit_form: None, return_deadline_days: None,
            deposit_type: "bid".into(), procurement_category: None }
    }

    fn perf(deposit: f64, contract: f64) -> VerifyBidDepositArgs {
        VerifyBidDepositArgs { deposit_amount: Some(deposit), budget_amount: None,
            contract_amount: Some(contract), deposit_form: None, return_deadline_days: None,
            deposit_type: "performance".into(), procurement_category: None }
    }

    // ── 2% 边界 ──────────────────────────────────────────────

    #[test]
    fn bid_rate_1_99pct_compliant() {
        let r = VerifyBidDepositTool::verify(&bid(1.99, 100.0)).unwrap();
        assert!(matches!(r.status, DepositStatus::Compliant));
    }

    #[test]
    fn bid_rate_2_00pct_compliant() {
        let r = VerifyBidDepositTool::verify(&bid(2.0, 100.0)).unwrap();
        assert!(matches!(r.status, DepositStatus::Compliant));
    }

    #[test]
    fn bid_rate_2_01pct_violation() {
        let r = VerifyBidDepositTool::verify(&bid(2.01, 100.0)).unwrap();
        assert!(matches!(r.status, DepositStatus::Violation));
    }

    /// M2 regression fixture: 分母必须是 budget_amount，不是 contract_amount。
    /// deposit=2, budget=100, contract=50：
    /// 正确（分母=budget）：2/100 = 2% → compliant
    /// 错误（分母=contract）：2/50 = 4% → violation
    #[test]
    fn bid_rate_uses_budget_amount_not_contract_amount() {
        let args = VerifyBidDepositArgs {
            deposit_amount: Some(2.0),
            budget_amount: Some(100.0),
            contract_amount: Some(50.0),
            deposit_form: None,
            return_deadline_days: None,
            deposit_type: "bid".into(),
            procurement_category: None,
        };
        let r = VerifyBidDepositTool::verify(&args).unwrap();
        assert_eq!(r.status, DepositStatus::Compliant);
    }

    // ── 履约 10% 边界 ────────────────────────────────────────

    #[test]
    fn perf_rate_9_99pct_compliant() {
        let r = VerifyBidDepositTool::verify(&perf(9.99, 100.0)).unwrap();
        assert!(matches!(r.status, DepositStatus::Compliant));
    }

    #[test]
    fn perf_rate_10_00pct_compliant() {
        let r = VerifyBidDepositTool::verify(&perf(10.0, 100.0)).unwrap();
        assert!(matches!(r.status, DepositStatus::Compliant));
    }

    #[test]
    fn perf_rate_10_01pct_violation() {
        let r = VerifyBidDepositTool::verify(&perf(10.01, 100.0)).unwrap();
        assert!(matches!(r.status, DepositStatus::Violation));
    }

    // ── form normalization ────────────────────────────────────

    #[test]
    fn form_check_compliant() {
        let args = VerifyBidDepositArgs { deposit_form: Some("支票".into()), deposit_type: "bid".into(), ..Default::default() };
        let r = VerifyBidDepositTool::verify(&args).unwrap();
        assert!(matches!(r.status, DepositStatus::Compliant));
    }

    #[test]
    fn form_huipiao_compliant() {
        let args = VerifyBidDepositArgs { deposit_form: Some("汇票".into()), deposit_type: "bid".into(), ..Default::default() };
        let r = VerifyBidDepositTool::verify(&args).unwrap();
        assert!(matches!(r.status, DepositStatus::Compliant));
    }

    #[test]
    fn form_benpiao_compliant() {
        let args = VerifyBidDepositArgs { deposit_form: Some("本票".into()), deposit_type: "bid".into(), ..Default::default() };
        let r = VerifyBidDepositTool::verify(&args).unwrap();
        assert!(matches!(r.status, DepositStatus::Compliant));
    }

    #[test]
    fn form_baohan_compliant() {
        let args = VerifyBidDepositArgs { deposit_form: Some("保函".into()), deposit_type: "bid".into(), ..Default::default() };
        let r = VerifyBidDepositTool::verify(&args).unwrap();
        assert!(matches!(r.status, DepositStatus::Compliant));
    }

    #[test]
    fn form_insurance_guarantee_compliant() {
        let args = VerifyBidDepositArgs { deposit_form: Some("保证保险".into()), deposit_type: "bid".into(), ..Default::default() };
        let r = VerifyBidDepositTool::verify(&args).unwrap();
        assert!(matches!(r.status, DepositStatus::Compliant));
    }

    #[test]
    fn form_cash_violation() {
        let args = VerifyBidDepositArgs { deposit_form: Some("现金".into()), deposit_type: "bid".into(), ..Default::default() };
        let r = VerifyBidDepositTool::verify(&args).unwrap();
        assert!(matches!(r.status, DepositStatus::Violation));
    }

    #[test]
    fn form_unknown_uncertain() {
        let args = VerifyBidDepositArgs { deposit_form: Some("其他".into()), deposit_type: "bid".into(), ..Default::default() };
        let r = VerifyBidDepositTool::verify(&args).unwrap();
        assert!(matches!(r.status, DepositStatus::Uncertain));
    }

    // ── InsufficientInput ────────────────────────────────────

    #[test]
    fn bid_missing_budget_insufficient() {
        let args = VerifyBidDepositArgs { deposit_amount: Some(10.0), budget_amount: None,
            contract_amount: None, deposit_form: None, return_deadline_days: None,
            deposit_type: "bid".into(), procurement_category: None };
        let r = VerifyBidDepositTool::verify(&args).unwrap();
        assert!(matches!(r.status, DepositStatus::InsufficientInput));
    }

    #[test]
    fn perf_missing_contract_insufficient() {
        let args = VerifyBidDepositArgs { deposit_amount: Some(10.0), budget_amount: None,
            contract_amount: None, deposit_form: None, return_deadline_days: None,
            deposit_type: "performance".into(), procurement_category: None };
        let r = VerifyBidDepositTool::verify(&args).unwrap();
        assert!(matches!(r.status, DepositStatus::InsufficientInput));
    }

    // ── 删除金额封顶 ─────────────────────────────────────────

    #[test]
    fn bid_60_wan_low_rate_still_compliant() {
        // 60万保证金 / 10000万预算 = 0.6% < 2%，即使之前因"50万上限"误判
        let r = VerifyBidDepositTool::verify(&bid(60.0, 10000.0)).unwrap();
        assert!(matches!(r.status, DepositStatus::Compliant));
    }

    // ── 退还时限 ─────────────────────────────────────────────

    #[test]
    fn return_5_days_compliant() {
        let mut args = bid(1.0, 100.0);
        args.return_deadline_days = Some(5);
        let r = VerifyBidDepositTool::verify(&args).unwrap();
        assert!(matches!(r.status, DepositStatus::Compliant));
    }

    #[test]
    fn return_10_days_violation() {
        let mut args = bid(1.0, 100.0);
        args.return_deadline_days = Some(10);
        let r = VerifyBidDepositTool::verify(&args).unwrap();
        assert!(matches!(r.status, DepositStatus::Violation));
    }

    #[test]
    fn invalid_deposit_type_error() {
        let args = VerifyBidDepositArgs { deposit_type: "guarantee".into(), ..Default::default() };
        assert!(VerifyBidDepositTool::verify(&args).is_err());
    }
}
