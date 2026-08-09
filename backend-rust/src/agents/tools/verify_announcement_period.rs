//! `verify_announcement_period` 工具 — 公告期限（等标期）校验。
//!
//! 根据《政府采购法》第35条、《政府采购竞争性磋商采购方式管理暂行办法》
//! 以及其他相关规定，校验采购公告期限和文件发售期限是否符合法定要求。
//! 本工具进行日历日差计算与法定时限比对，不访问外部 I/O。
//!
//! ## 法定时限要求
//!
//! - 公开招标公告期（等标期）≥ 20 日历日
//! - 竞争性磋商公告期 ≥ 10 日历日
//! - 竞争性谈判公告期 ≥ 3 工作日
//! - 询价公告期 ≥ 3 工作日
//! - 文件发售期 ≥ 5 工作日（所有采购方式通用）
//!
//! ## 日期格式
//!
//! 支持 "YYYY-MM-DD" 和 "YYYY/MM/DD" 两种格式。
//! 日期差使用简单日历日差计算。

use anyhow::{Result, anyhow};
use serde::Deserialize;

use super::AgentTool;

// ─── 法定时限常量 ──────────────────────────────────────────────

/// 公开招标公告期 ≥ 20 日历日
const ANNOUNCEMENT_OPEN_BIDDING_DAYS: i64 = 20;
/// 竞争性磋商公告期 ≥ 10 日历日
const ANNOUNCEMENT_CONSULTATION_DAYS: i64 = 10;
/// 竞争性谈判公告期 ≥ 3 工作日（按日历日近似）
const ANNOUNCEMENT_NEGOTIATION_DAYS: i64 = 3;
/// 询价公告期 ≥ 3 工作日（按日历日近似）
const ANNOUNCEMENT_INQUIRY_DAYS: i64 = 3;
/// 文件发售期 ≥ 5 工作日（按日历日近似，应为至少 5 个工作日）
const DOCUMENT_SALE_DAYS_MIN: i64 = 5;

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

/// 计算两个日期之间的日历日差。
/// 返回 (date1, date2) 之间的天数差 = (date2 - date1).num_days()。
fn days_between(start: chrono::NaiveDate, end: chrono::NaiveDate) -> i64 {
    (end - start).num_days()
}

/// 获取采购方式对应的法定公告期限（日历日）。
fn get_required_announcement_days(method: &str) -> Result<(i64, &'static str, &'static str)> {
    match method {
        "公开招标" => Ok((
            ANNOUNCEMENT_OPEN_BIDDING_DAYS,
            "日历日",
            "《政府采购法》第35条：货物和服务项目实行招标方式采购的，自招标文件开始发出之日起至投标人提交投标文件截止之日止，不得少于20日。",
        )),
        "竞争性磋商" => Ok((
            ANNOUNCEMENT_CONSULTATION_DAYS,
            "日历日",
            "《政府采购竞争性磋商采购方式管理暂行办法》第10条：从磋商文件发出之日起至供应商提交首次响应文件截止之日止不得少于10日。",
        )),
        "竞争性谈判" => Ok((
            ANNOUNCEMENT_NEGOTIATION_DAYS,
            "工作日(按日历日近似)",
            "《政府采购非招标采购方式管理办法》（财政部令第74号）第29条：从谈判文件发出之日起至供应商提交首次响应文件截止之日止不得少于3个工作日。",
        )),
        "询价" => Ok((
            ANNOUNCEMENT_INQUIRY_DAYS,
            "工作日(按日历日近似)",
            "《政府采购非招标采购方式管理办法》（财政部令第74号）第45条：从询价通知书发出之日起至供应商提交响应文件截止之日止不得少于3个工作日。",
        )),
        _ => Err(anyhow!(
            "不支持的采购方式 '{}'，有效值为: 公开招标/竞争性磋商/竞争性谈判/询价",
            method
        )),
    }
}

// ─── 参数 ──────────────────────────────────────────────────────

/// `verify_announcement_period` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct VerifyAnnouncementPeriodArgs {
    /// 采购方式
    pub procurement_method: String,
    /// 公告发布日期（YYYY-MM-DD 或 YYYY/MM/DD）
    pub announcement_date_str: String,
    /// 投标/响应文件截止日期（YYYY-MM-DD 或 YYYY/MM/DD）
    pub bid_deadline_date_str: String,
    /// 文件发售开始日期（可选）
    #[serde(default)]
    pub document_sale_start_str: Option<String>,
    /// 文件发售结束日期（可选）
    #[serde(default)]
    pub document_sale_end_str: Option<String>,
}

// ─── 输出 ──────────────────────────────────────────────────────

/// 公告期限校验返回结果。
#[derive(Debug, serde::Serialize)]
struct AnnouncementPeriodResult {
    /// 采购方式
    procurement_method: String,
    /// 整体合规判定
    overall_status: PeriodStatus,
    /// 公告期/等标期检查
    announcement_period: PeriodCheck,
    /// 文件发售期检查（如有提供）
    document_sale_period: Option<PeriodCheck>,
    /// 法条依据
    legal_basis: Vec<String>,
    /// 综合建议
    suggestion: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum PeriodStatus {
    Compliant,
    Violation,
}

#[derive(Debug, serde::Serialize)]
struct PeriodCheck {
    /// 检查项名称
    check_name: String,
    /// 该项判定
    status: CheckStatus,
    /// 公告日期
    start_date: String,
    /// 截止日期
    end_date: String,
    /// 实际天数
    actual_days: i64,
    /// 法定要求天数
    required_days: i64,
    /// 天数单位
    day_unit: String,
    /// 法条依据
    legal_basis: String,
    /// 详细说明
    detail: String,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum CheckStatus {
    Pass,
    Fail,
    Skip,
}

// ─── 工具实现 ──────────────────────────────────────────────────

/// `verify_announcement_period` 工具实现。
///
/// 纯日期计算与规则匹配工具，无外部依赖。
pub struct VerifyAnnouncementPeriodTool;

impl VerifyAnnouncementPeriodTool {
    /// 核心校验逻辑。
    fn verify(args: &VerifyAnnouncementPeriodArgs) -> Result<AnnouncementPeriodResult> {
        // 1. 解析日期
        let announcement_date = parse_date(&args.announcement_date_str)?;
        let bid_deadline_date = parse_date(&args.bid_deadline_date_str)?;

        // 2. 获取采购方式对应的法定时限
        let (required_days, day_unit, legal_basis_text) =
            get_required_announcement_days(&args.procurement_method)?;

        // 3. 计算公告期天数
        let actual_days = days_between(announcement_date, bid_deadline_date);

        if actual_days < 0 {
            return Err(anyhow!(
                "投标截止日期 ({}) 早于公告日期 ({})，请检查日期是否正确。",
                args.bid_deadline_date_str,
                args.announcement_date_str
            ));
        }

        // 4. 判定公告期合规性
        let announcement_check_status = if actual_days >= required_days {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        };

        let announcement_detail = if actual_days >= required_days {
            format!(
                "公告期 {} 天，满足法定 ≥ {} {} 的要求，合规。",
                actual_days, required_days, day_unit
            )
        } else {
            format!(
                "公告期仅 {} 天，不满足法定 ≥ {} {} 的要求，违规。差额 {} 天。\
                建议延长公告截止日期或重新发布公告。违反此规定可能导致采购程序无效，\
                并面临供应商质疑投诉风险。",
                actual_days,
                required_days,
                day_unit,
                required_days - actual_days
            )
        };

        let announcement_period = PeriodCheck {
            check_name: format!("{}公告期", args.procurement_method),
            status: announcement_check_status,
            start_date: args.announcement_date_str.clone(),
            end_date: args.bid_deadline_date_str.clone(),
            actual_days,
            required_days,
            day_unit: day_unit.to_string(),
            legal_basis: legal_basis_text.to_string(),
            detail: announcement_detail,
        };

        // 5. 文件发售期检查
        let mut document_sale_period: Option<PeriodCheck> = None;
        let mut doc_sale_status = CheckStatus::Skip;

        if let (Some(start_str), Some(end_str)) = (
            &args.document_sale_start_str,
            &args.document_sale_end_str,
        ) {
            let sale_start = parse_date(start_str)?;
            let sale_end = parse_date(end_str)?;
            let sale_days = days_between(sale_start, sale_end);

            if sale_days < 0 {
                return Err(anyhow!(
                    "文件发售结束日期 ({}) 早于开始日期 ({})，请检查日期是否正确。",
                    end_str,
                    start_str
                ));
            }

            let sale_legal = "《政府采购法实施条例》第31条：招标文件的提供期限自招标文件开始发出之日起不得少于5个工作日。";

            doc_sale_status = if sale_days >= DOCUMENT_SALE_DAYS_MIN {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            };

            let sale_detail = if sale_days >= DOCUMENT_SALE_DAYS_MIN {
                format!(
                    "文件发售期 {} 天，满足法定 ≥ {} 工作日的要求。",
                    sale_days, DOCUMENT_SALE_DAYS_MIN
                )
            } else {
                format!(
                    "文件发售期仅 {} 天，不满足法定 ≥ {} 工作日的要求，违规。差额 {} 天。",
                    sale_days,
                    DOCUMENT_SALE_DAYS_MIN,
                    DOCUMENT_SALE_DAYS_MIN - sale_days
                )
            };

            document_sale_period = Some(PeriodCheck {
                check_name: "文件发售期".to_string(),
                status: doc_sale_status,
                start_date: start_str.clone(),
                end_date: end_str.clone(),
                actual_days: sale_days,
                required_days: DOCUMENT_SALE_DAYS_MIN,
                day_unit: "工作日(按日历日近似)".to_string(),
                legal_basis: sale_legal.to_string(),
                detail: sale_detail,
            });
        }

        // 6. 综合判定
        let has_violation = matches!(announcement_period.status, CheckStatus::Fail)
            || matches!(doc_sale_status, CheckStatus::Fail);

        let overall_status = if has_violation {
            PeriodStatus::Violation
        } else {
            PeriodStatus::Compliant
        };

        // 7. 构建法条依据
        let mut legal_basis = vec![legal_basis_text.to_string()];
        if document_sale_period.is_some() {
            legal_basis.push(
                "《政府采购法实施条例》第31条：招标文件的提供期限自招标文件开始发出之日起不得少于5个工作日。"
                    .to_string(),
            );
        }

        // 8. 建议
        let suggestion = if has_violation {
            let mut issues = Vec::new();
            if matches!(announcement_period.status, CheckStatus::Fail) {
                issues.push(format!(
                    "公告期不足（实际 {} 天，需要至少 {} {}）",
                    actual_days, required_days, day_unit
                ));
            }
            if let Some(ref dp) = document_sale_period {
                if matches!(dp.status, CheckStatus::Fail) {
                    issues.push(format!(
                        "文件发售期不足（实际 {} 天，需要至少 {} 工作日）",
                        dp.actual_days, DOCUMENT_SALE_DAYS_MIN
                    ));
                }
            }
            format!(
                "存在以下违规：{}。建议重新调整公告时间安排后重新发布公告。",
                issues.join("；")
            )
        } else {
            format!(
                "{}公告期和文件发售期限均满足法定要求。",
                args.procurement_method
            )
        };

        Ok(AnnouncementPeriodResult {
            procurement_method: args.procurement_method.clone(),
            overall_status,
            announcement_period,
            document_sale_period,
            legal_basis,
            suggestion,
        })
    }
}

// ─── AgentTool 实现 ────────────────────────────────────────────

#[async_trait::async_trait]
impl AgentTool for VerifyAnnouncementPeriodTool {
    fn name(&self) -> &str {
        "verify_announcement_period"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "verify_announcement_period",
                "description": "【使用场景】校验采购公告期限和文件发售期限是否符合法定要求——\
                    ① 公开招标公告期（等标期）≥ 20 日历日；\
                    ② 竞争性磋商公告期 ≥ 10 日历日；\
                    ③ 竞争性谈判公告期 ≥ 3 工作日；\
                    ④ 询价公告期 ≥ 3 工作日；\
                    ⑤ 文件发售期 ≥ 5 工作日。\
                    【不使用场景】不校验公告内容是否完整、是否包含法定必要信息；\
                    不校验公告发布媒介是否合规；不校验采购方式的适用条件（用 verify_procurement_method）。\
                    【法条依据】《政府采购法》第35条、《政府采购法实施条例》第31条、\
                    《政府采购竞争性磋商采购方式管理暂行办法》第10条、财政部令第74号。\
                    【注意】日期支持 YYYY-MM-DD 和 YYYY/MM/DD 两种格式。",
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
                        },
                        "document_sale_start_str": {
                            "type": "string",
                            "description": "文件发售开始日期，格式同上。可选，提供后同时校验文件发售期。"
                        },
                        "document_sale_end_str": {
                            "type": "string",
                            "description": "文件发售结束日期，格式同上。可选。"
                        }
                    },
                    "required": ["procurement_method", "announcement_date_str", "bid_deadline_date_str"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: VerifyAnnouncementPeriodArgs = serde_json::from_value(args)?;
        let result = Self::verify(&parsed)?;
        Ok(serde_json::to_value(&result)?)
    }
}

// ─── 测试 ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;

    #[test]
    fn test_parse_date_ymd() {
        let d = parse_date("2025-03-01").unwrap();
        assert_eq!(d.year(), 2025);
        assert_eq!(d.month(), 3);
        assert_eq!(d.day(), 1);
    }

    #[test]
    fn test_parse_date_slash() {
        let d = parse_date("2025/03/15").unwrap();
        assert_eq!(d.year(), 2025);
        assert_eq!(d.month(), 3);
        assert_eq!(d.day(), 15);
    }

    #[test]
    fn test_parse_date_invalid() {
        assert!(parse_date("03-01-2025").is_err());
        assert!(parse_date("not a date").is_err());
    }

    #[test]
    fn test_open_bidding_25_days_compliant() {
        let args = VerifyAnnouncementPeriodArgs {
            procurement_method: "公开招标".to_string(),
            announcement_date_str: "2025-03-01".to_string(),
            bid_deadline_date_str: "2025-03-26".to_string(),
            document_sale_start_str: None,
            document_sale_end_str: None,
        };
        let result = VerifyAnnouncementPeriodTool::verify(&args).unwrap();
        assert!(matches!(result.overall_status, PeriodStatus::Compliant));
        assert_eq!(result.announcement_period.actual_days, 25);
        assert_eq!(result.announcement_period.required_days, 20);
        assert!(matches!(result.announcement_period.status, CheckStatus::Pass));
    }

    #[test]
    fn test_open_bidding_15_days_violation() {
        // 公开招标仅 15 天 → 违规
        let args = VerifyAnnouncementPeriodArgs {
            procurement_method: "公开招标".to_string(),
            announcement_date_str: "2025-03-01".to_string(),
            bid_deadline_date_str: "2025-03-16".to_string(),
            document_sale_start_str: None,
            document_sale_end_str: None,
        };
        let result = VerifyAnnouncementPeriodTool::verify(&args).unwrap();
        assert!(matches!(result.overall_status, PeriodStatus::Violation));
        assert!(matches!(result.announcement_period.status, CheckStatus::Fail));
        assert_eq!(result.announcement_period.actual_days, 15);
    }

    #[test]
    fn test_document_sale_period_violation() {
        // 文件发售期仅 3 天 → 违规
        let args = VerifyAnnouncementPeriodArgs {
            procurement_method: "公开招标".to_string(),
            announcement_date_str: "2025-03-01".to_string(),
            bid_deadline_date_str: "2025-03-25".to_string(),
            document_sale_start_str: Some("2025-03-01".to_string()),
            document_sale_end_str: Some("2025-03-04".to_string()),
        };
        let result = VerifyAnnouncementPeriodTool::verify(&args).unwrap();
        assert!(matches!(result.overall_status, PeriodStatus::Violation));
        let doc_sale = result.document_sale_period.unwrap();
        assert!(matches!(doc_sale.status, CheckStatus::Fail));
        assert_eq!(doc_sale.actual_days, 3);
        assert_eq!(doc_sale.required_days, 5);
    }

    #[test]
    fn test_consultation_12_days_compliant() {
        let args = VerifyAnnouncementPeriodArgs {
            procurement_method: "竞争性磋商".to_string(),
            announcement_date_str: "2025-06-01".to_string(),
            bid_deadline_date_str: "2025-06-13".to_string(),
            document_sale_start_str: None,
            document_sale_end_str: None,
        };
        let result = VerifyAnnouncementPeriodTool::verify(&args).unwrap();
        assert!(matches!(result.overall_status, PeriodStatus::Compliant));
        assert_eq!(result.announcement_period.required_days, 10);
    }

    #[test]
    fn test_invalid_method_errors() {
        let args = VerifyAnnouncementPeriodArgs {
            procurement_method: "邀请招标".to_string(),
            announcement_date_str: "2025-03-01".to_string(),
            bid_deadline_date_str: "2025-03-25".to_string(),
            document_sale_start_str: None,
            document_sale_end_str: None,
        };
        let result = VerifyAnnouncementPeriodTool::verify(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_deadline_before_announcement_errors() {
        let args = VerifyAnnouncementPeriodArgs {
            procurement_method: "公开招标".to_string(),
            announcement_date_str: "2025-03-10".to_string(),
            bid_deadline_date_str: "2025-03-01".to_string(),
            document_sale_start_str: None,
            document_sale_end_str: None,
        };
        let result = VerifyAnnouncementPeriodTool::verify(&args);
        assert!(result.is_err());
    }
}
