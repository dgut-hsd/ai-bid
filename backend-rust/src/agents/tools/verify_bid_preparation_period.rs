//! `verify_bid_preparation_period` 工具 — 投标准备期（等标期）校验。
//!
//! 根据《政府采购法》第35条及非招标采购方式管理办法，校验投标人从公告发布
//! 至投标截止之间的准备时间是否满足法定最低要求。这是政府采购投诉的第一高发事由，
//! 违反后果严重——可能导致中标结果无效、采购程序重启乃至行政处罚。
//!
//! ## 法定时限要求
//!
//! - 公开招标 ≥ 20 日历日
//! - 竞争性磋商 ≥ 10 日历日
//! - 竞争性谈判 ≥ 3 工作日（按日历日近似）
//! - 询价 ≥ 3 工作日（按日历日近似）
//!
//! ## 投诉高发事由说明
//!
//! 等标期不足是供应商投诉中占比最高的单一事由。典型投诉理由：
//! ① 公告至截标不足20日，影响投标准备；
//! ② 周末/节假日被计入但实际工作天数不足；
//! ③ 澄清/修改后未相应延长等标期。
//!
//! 违反后果根据《政府采购法》第71条、《政府采购法实施条例》第68条：
//! - 责令限期改正，给予警告；
//! - 可以并处罚款，对直接负责的主管人员和其他直接责任人员由其行政主管部门给予处分；
//! - 情节严重的，中标/成交结果无效。

use anyhow::{Result, anyhow};
use serde::Deserialize;

use super::AgentTool;

// ─── 法定时限常量 ──────────────────────────────────────────────

/// 公开招标投标准备期 ≥ 20 日历日
const PREPARATION_OPEN_BIDDING_DAYS: i64 = 20;
/// 竞争性磋商标准备期 ≥ 10 日历日
const PREPARATION_CONSULTATION_DAYS: i64 = 10;
/// 竞争性谈判投标准备期 ≥ 3 工作日
const PREPARATION_NEGOTIATION_DAYS: i64 = 3;
/// 询价投标准备期 ≥ 3 工作日
const PREPARATION_INQUIRY_DAYS: i64 = 3;

// ─── 日期解析辅助函数 ──────────────────────────────────────────

/// 解析日期字符串，支持 "YYYY-MM-DD" 和 "YYYY/MM/DD" 两种格式。
fn parse_date(date_str: &str) -> Result<chrono::NaiveDate> {
    // 尝试 "YYYY-MM-DD"
    if let Ok(d) = chrono::NaiveDate::parse_from_str(date_str.trim(), "%Y-%m-%d") {
        return Ok(d);
    }
    // 尝试 "YYYY/MM/DD"
    if let Ok(d) = chrono::NaiveDate::parse_from_str(date_str.trim(), "%Y/%m/%d") {
        return Ok(d);
    }
    Err(anyhow!(
        "无法解析日期 '{}'，支持的格式为 YYYY-MM-DD 或 YYYY/MM/DD",
        date_str
    ))
}

/// 计算两个日期之间的日历日差（end - start）。
fn days_between(start: chrono::NaiveDate, end: chrono::NaiveDate) -> i64 {
    (end - start).num_days()
}

/// 获取采购方式对应的法定投标准备期要求。
fn get_required_days(method: &str) -> Result<(i64, &'static str, &'static str, &'static str)> {
    match method {
        "公开招标" => Ok((
            PREPARATION_OPEN_BIDDING_DAYS,
            "日历日",
            "《政府采购法》第35条",
            "货物和服务项目实行招标方式采购的，自招标文件开始发出之日起至投标人提交投标文件截止之日止，不得少于20日。",
        )),
        "竞争性磋商" => Ok((
            PREPARATION_CONSULTATION_DAYS,
            "日历日",
            "《政府采购竞争性磋商采购方式管理暂行办法》第10条",
            "从磋商文件发出之日起至供应商提交首次响应文件截止之日止不得少于10日。",
        )),
        "竞争性谈判" => Ok((
            PREPARATION_NEGOTIATION_DAYS,
            "工作日(按日历日近似)",
            "《政府采购非招标采购方式管理办法》（财政部令第74号）第29条",
            "从谈判文件发出之日起至供应商提交首次响应文件截止之日止不得少于3个工作日。",
        )),
        "询价" => Ok((
            PREPARATION_INQUIRY_DAYS,
            "工作日(按日历日近似)",
            "《政府采购非招标采购方式管理办法》（财政部令第74号）第45条",
            "从询价通知书发出之日起至供应商提交响应文件截止之日止不得少于3个工作日。",
        )),
        _ => Err(anyhow!(
            "不支持的采购方式 '{}'，有效值为: 公开招标/竞争性磋商/竞争性谈判/询价",
            method
        )),
    }
}

/// 根据投标准备期差额评估风险等级。
fn assess_risk_level(actual: i64, required: i64) -> (&'static str, &'static str) {
    let ratio = actual as f64 / required as f64;
    if ratio >= 1.0 {
        ("none", "投标准备期满足法定要求，无风险。")
    } else if ratio >= 0.75 {
        (
            "medium",
            "投标准备期不足，但差额较小（≥75%法定要求），存在被供应商质疑的风险。",
        )
    } else if ratio >= 0.5 {
        (
            "high",
            "投标准备期严重不足（仅为法定要求的50%-75%），供应商质疑/投诉成功率较高。",
        )
    } else {
        (
            "critical",
            "投标准备期极度不足（不足法定要求的50%），几乎必然引发有效质疑/投诉，且中标结果可能被宣告无效。",
        )
    }
}

/// 违反投标准备期规定的法律后果说明。
fn violation_consequences() -> &'static str {
    "【违反后果】根据《政府采购法》第71条、《政府采购法实施条例》第68条：\
    ① 责令限期改正，给予警告；\
    ② 可以并处罚款，对直接负责的主管人员和其他直接责任人员由其行政主管部门给予处分，并予通报；\
    ③ 采购活动完成后发现等标期不足的，已确定中标/成交结果的可能被宣告无效；\
    ④ 供应商可依法提起质疑（7个工作日内）和投诉（质疑答复后15个工作日内），\
    ⑤ 财政部门可对采购人/代理机构进行行政处罚。\
    投标准备期不足是政府采购投诉的第一高发事由，一旦被质疑/投诉成立，将严重影响采购进度和采购人信誉。"
}

// ─── 参数 ──────────────────────────────────────────────────────

/// `verify_bid_preparation_period` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct VerifyBidPreparationPeriodArgs {
    /// 采购方式
    pub procurement_method: String,
    /// 公告发布日期（YYYY-MM-DD 或 YYYY/MM/DD）
    pub announcement_date_str: String,
    /// 投标/响应文件截止日期（YYYY-MM-DD 或 YYYY/MM/DD）
    pub bid_deadline_date_str: String,
}

// ─── 输出 ──────────────────────────────────────────────────────

/// 投标准备期校验返回结果。
#[derive(Debug, serde::Serialize)]
struct BidPreparationPeriodResult {
    /// 采购方式
    procurement_method: String,
    /// 合规判定
    status: PreparationStatus,
    /// 公告日期
    announcement_date: String,
    /// 投标截止日期
    bid_deadline_date: String,
    /// 实际准备天数（日历日）
    actual_days: i64,
    /// 法定要求天数
    required_days: i64,
    /// 天数单位
    day_unit: String,
    /// 差额（负数 = 不足）
    shortage_days: i64,
    /// 法条依据（含完整条文）
    legal_basis: LegalBasisInfo,
    /// 风险等级
    risk_level: String,
    /// 违反后果说明
    violation_consequences: Option<String>,
    /// 整改建议
    suggestion: String,
    /// 详细分析
    detail: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum PreparationStatus {
    Compliant,
    Violation,
}

#[derive(Debug, serde::Serialize)]
struct LegalBasisInfo {
    /// 法条引用
    citation: String,
    /// 完整条文
    full_text: String,
}

// ─── 工具实现 ──────────────────────────────────────────────────

/// `verify_bid_preparation_period` 工具实现。
///
/// 纯日期计算与规则匹配工具，无外部依赖。
/// 重点关注投标准备期——政府采购投诉第一高发事由。
pub struct VerifyBidPreparationPeriodTool;

impl VerifyBidPreparationPeriodTool {
    /// 核心校验逻辑。
    fn verify(args: &VerifyBidPreparationPeriodArgs) -> Result<BidPreparationPeriodResult> {
        // 1. 解析日期
        let announcement_date = parse_date(&args.announcement_date_str)?;
        let bid_deadline_date = parse_date(&args.bid_deadline_date_str)?;

        // 2. 获取法定要求
        let (required_days, day_unit, citation, full_text) =
            get_required_days(&args.procurement_method)?;

        // 3. 计算天数
        let actual_days = days_between(announcement_date, bid_deadline_date);

        if actual_days < 0 {
            return Err(anyhow!(
                "投标截止日期 ({}) 早于公告日期 ({})，请检查日期是否正确。",
                args.bid_deadline_date_str,
                args.announcement_date_str
            ));
        }

        let shortage_days = required_days - actual_days;

        // 4. 判定合规性
        let (status, risk_level, _risk_detail, violation_cons, suggestion, detail) =
            if actual_days >= required_days {
                (
                    PreparationStatus::Compliant,
                    "none".to_string(),
                    "投标准备期满足法定要求，无风险。".to_string(),
                    None,
                    format!(
                        "投标准备期 {} {} 符合法定要求（≥ {} {}），无需整改。",
                        actual_days, day_unit, required_days, day_unit
                    ),
                    format!(
                        "公告日期 {} 至投标截止日期 {} 共 {} {}，满足 {} 法定要求 ≥ {} {}。各项合规。",
                        args.announcement_date_str,
                        args.bid_deadline_date_str,
                        actual_days,
                        day_unit,
                        args.procurement_method,
                        required_days,
                        day_unit
                    ),
                )
            } else {
                let (rl, rd) = assess_risk_level(actual_days, required_days);

                let s = format!(
                    "投标准备期不足！建议立即采取以下措施之一：\
                    ① 发布更正公告，顺延投标截止日期至 {} {} 之后（建议延至 {}）；\
                    ② 如已发布公告但未到截止日期，紧急发布更正公告延长等标期；\
                    ③ 如已截标但尚未开标/评审，立即终止程序并重新发布公告；\
                    ④ 如已确定中标结果，应评估是否主动废标重招以规避后续投诉风险。",
                    day_unit,
                    required_days,
                    format!(
                        "{}",
                        announcement_date
                            .checked_add_signed(chrono::TimeDelta::days(required_days))
                            .map(|d| d.format("%Y-%m-%d").to_string())
                            .unwrap_or_else(|| "计算错误".to_string())
                    )
                );

                let d = format!(
                    "投标准备期仅 {} {}，法定要求 ≥ {} {}，短缺 {} {}。{} \
                    风险等级：{}。{} \
                    此为政府采购投诉第一高发事由，必须高度重视。",
                    actual_days,
                    day_unit,
                    required_days,
                    day_unit,
                    shortage_days,
                    day_unit,
                    rd,
                    rl,
                    violation_consequences()
                );

                (
                    PreparationStatus::Violation,
                    rl.to_string(),
                    rd.to_string(),
                    Some(violation_consequences().to_string()),
                    s,
                    d,
                )
            };

        Ok(BidPreparationPeriodResult {
            procurement_method: args.procurement_method.clone(),
            status,
            announcement_date: args.announcement_date_str.clone(),
            bid_deadline_date: args.bid_deadline_date_str.clone(),
            actual_days,
            required_days,
            day_unit: day_unit.to_string(),
            shortage_days: if shortage_days < 0 { 0 } else { shortage_days },
            legal_basis: LegalBasisInfo {
                citation: citation.to_string(),
                full_text: full_text.to_string(),
            },
            risk_level,
            violation_consequences: violation_cons,
            suggestion,
            detail,
        })
    }
}

// ─── AgentTool 实现 ────────────────────────────────────────────

#[async_trait::async_trait]
impl AgentTool for VerifyBidPreparationPeriodTool {
    fn name(&self) -> &str {
        "verify_bid_preparation_period"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "verify_bid_preparation_period",
                "description": "【使用场景】校验投标人从公告发布到投标截止的准备时间（等标期）是否满足法定最低要求——\
                    这是政府采购投诉的第一高发事由。\
                    ① 公开招标 ≥ 20 日历日；\
                    ② 竞争性磋商 ≥ 10 日历日；\
                    ③ 竞争性谈判 ≥ 3 工作日；\
                    ④ 询价 ≥ 3 工作日。\
                    本工具提供风险等级评估和违反后果的详细说明，包括可能面临的质疑/投诉风险、\
                    行政处罚依据和采购结果无效风险。\
                    【不使用场景】不校验公告内容的完整性和规范性；\
                    不校验文件发售期是否充足（用 verify_announcement_period）；\
                    不校验保证金相关事项（用 verify_bid_deposit）。\
                    【注意】这是投诉数量最高的单一审查事项，等标期不足一旦被投诉几乎必然成立。\
                    日期支持 YYYY-MM-DD 和 YYYY/MM/DD 两种格式。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "procurement_method": {
                            "type": "string",
                            "enum": ["公开招标", "竞争性磋商", "竞争性谈判", "询价"],
                            "description": "采购方式。"
                        },
                        "announcement_date_str": {
                            "type": "string",
                            "description": "公告发布日期，格式 YYYY-MM-DD 或 YYYY/MM/DD，如 '2025-03-01'。"
                        },
                        "bid_deadline_date_str": {
                            "type": "string",
                            "description": "投标/响应文件截止日期，格式 YYYY-MM-DD 或 YYYY/MM/DD。"
                        }
                    },
                    "required": ["procurement_method", "announcement_date_str", "bid_deadline_date_str"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: VerifyBidPreparationPeriodArgs = serde_json::from_value(args)?;
        let result = Self::verify(&parsed)?;
        Ok(serde_json::to_value(&result)?)
    }
}

// ─── 测试 ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_bidding_25_days_compliant() {
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "公开招标".to_string(),
            announcement_date_str: "2025-03-01".to_string(),
            bid_deadline_date_str: "2025-03-26".to_string(),
        };
        let result = VerifyBidPreparationPeriodTool::verify(&args).unwrap();
        assert!(matches!(result.status, PreparationStatus::Compliant));
        assert_eq!(result.actual_days, 25);
        assert_eq!(result.required_days, 20);
        assert_eq!(result.risk_level, "none");
        assert!(result.violation_consequences.is_none());
    }

    #[test]
    fn test_open_bidding_exactly_20_days_compliant() {
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "公开招标".to_string(),
            announcement_date_str: "2025-03-01".to_string(),
            bid_deadline_date_str: "2025-03-21".to_string(),
        };
        let result = VerifyBidPreparationPeriodTool::verify(&args).unwrap();
        assert!(matches!(result.status, PreparationStatus::Compliant));
        assert_eq!(result.actual_days, 20);
    }

    #[test]
    fn test_open_bidding_15_days_violation_high_risk() {
        // 15/20 = 75%，应判定为 medium 风险
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "公开招标".to_string(),
            announcement_date_str: "2025-03-01".to_string(),
            bid_deadline_date_str: "2025-03-16".to_string(),
        };
        let result = VerifyBidPreparationPeriodTool::verify(&args).unwrap();
        assert!(matches!(result.status, PreparationStatus::Violation));
        assert_eq!(result.actual_days, 15);
        assert_eq!(result.shortage_days, 5);
        assert_eq!(result.risk_level, "medium");
        assert!(result.violation_consequences.is_some());
        assert!(result.detail.contains("投诉"));
    }

    #[test]
    fn test_open_bidding_8_days_violation_critical_risk() {
        // 8/20 = 40%，应判定为 critical 风险
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "公开招标".to_string(),
            announcement_date_str: "2025-03-01".to_string(),
            bid_deadline_date_str: "2025-03-09".to_string(),
        };
        let result = VerifyBidPreparationPeriodTool::verify(&args).unwrap();
        assert!(matches!(result.status, PreparationStatus::Violation));
        assert_eq!(result.risk_level, "critical");
        assert!(result.violation_consequences.is_some());
        assert!(result.suggestion.contains("顺延"));
    }

    #[test]
    fn test_negotiation_5_days_compliant() {
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "竞争性谈判".to_string(),
            announcement_date_str: "2025-03-01".to_string(),
            bid_deadline_date_str: "2025-03-06".to_string(),
        };
        let result = VerifyBidPreparationPeriodTool::verify(&args).unwrap();
        assert!(matches!(result.status, PreparationStatus::Compliant));
        assert_eq!(result.required_days, 3);
        assert_eq!(result.actual_days, 5);
    }

    #[test]
    fn test_invalid_method_errors() {
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "单一来源".to_string(),
            announcement_date_str: "2025-03-01".to_string(),
            bid_deadline_date_str: "2025-03-25".to_string(),
        };
        let result = VerifyBidPreparationPeriodTool::verify(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_deadline_before_announcement_errors() {
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "公开招标".to_string(),
            announcement_date_str: "2025-03-10".to_string(),
            bid_deadline_date_str: "2025-03-01".to_string(),
        };
        let result = VerifyBidPreparationPeriodTool::verify(&args);
        assert!(result.is_err());
    }
}
