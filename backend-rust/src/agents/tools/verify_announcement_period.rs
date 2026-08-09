//! `verify_announcement_period` 工具 — 公告期 / 文件提供期 / 单一来源公示期校验。
//!
//! 按 `period_type` 区分三类独立法律期间：
//!
//! - `notice_publication`：公告期（NoticePublished → NoticePublicationEnded）
//! - `document_availability`：采购文件提供/发售期（DocumentAvailabilityStarted → DocumentAvailabilityEnded）
//! - `single_source_pre_acquisition_publicity`：单一来源采购前公示（条件化）
//!
//! 不负责投标/响应准备期（20 日等标期、10 日磋商、3 工作日谈判/询价）——
//! 那些属于 `verify_bid_preparation_period`。
//! 缺失 `period_type` → Uncertain，禁止按采购方式猜测期间类型。
//! 所有期间计算使用 `WorkingDaysCounter<CnCalendarProvider>`，无日历日近似。
//!
//! ## 日期格式
//!
//! 支持 "YYYY-MM-DD" 和 "YYYY/MM/DD" 两种格式。

use anyhow::{Result, anyhow};
use serde::Deserialize;

use super::AgentTool;
use super::calendar::{CnCalendarProvider, WorkingDaysCounter};
use super::time_domain::{DateCounter, PeriodCountingConvention, TimeDomainError};
use crate::agents::procurement_context::{
    self, ProcurementContext, ResolutionStatus, RuleSet,
};

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

// ─── 参数 ──────────────────────────────────────────────────────

/// `verify_announcement_period` 工具的参数。
///
/// ## PeriodType Contract（4B-4B）
///
/// 本工具未来承载两类独立法律期间，由 `period_type` 区分：
///
/// - `notice_publication`：公告期（NoticePublished → NoticePublicationEnded）
/// - `document_availability`：采购文件提供/发售期（DocumentAvailabilityStarted → DocumentAvailabilityEnded）
/// - `single_source_pre_acquisition_publicity`：单一来源采购前公示（条件化，4B-4E 已实现）
///
/// 禁止用 `procurement_method` 猜测用户要检查哪一类期间。
/// `announcement_date_str` / `bid_deadline_date_str` 为 legacy 字段，
/// 仅兼容解析与回显，未来不得用于上述任何期限计算。
#[derive(Debug, Default, Deserialize)]
pub struct VerifyAnnouncementPeriodArgs {
    /// 采购方式
    pub procurement_method: String,
    /// 期间类型（可选）：notice_publication / document_availability /
    /// single_source_pre_acquisition_publicity。
    /// 缺失时后续校验返回 uncertain；禁止按采购方式猜测。
    #[serde(default)]
    pub period_type: Option<String>,
    /// [legacy] 公告发布日期。仅表示公告发布日，
    /// 不得作为采购文件发出/提供起点，不得与 bid_deadline 组合计算准备期。
    #[serde(default)]
    pub announcement_date_str: Option<String>,
    /// [legacy] 投标/响应文件截止日期。
    /// 不得用于 NoticePublication 或 DocumentAvailability 期限计算。
    #[serde(default)]
    pub bid_deadline_date_str: Option<String>,
    /// 公告开始日期（NoticePublished）。用于 notice_publication。
    #[serde(default)]
    pub notice_start_date_str: Option<String>,
    /// 公告结束日期（NoticePublicationEnded）。用于 notice_publication。
    #[serde(default)]
    pub notice_end_date_str: Option<String>,
    /// 采购文件开始提供/发售日期（DocumentAvailabilityStarted）。
    #[serde(default)]
    pub document_availability_start_date_str: Option<String>,
    /// 采购文件结束提供/发售日期（DocumentAvailabilityEnded）。
    #[serde(default)]
    pub document_availability_end_date_str: Option<String>,
    /// 采购对象（可选）：goods / service / construction 或中文（货物/服务/工程）。
    /// 缺失时后续校验返回 uncertain；禁止推测。
    #[serde(default)]
    pub procurement_object: Option<String>,
    /// 是否政府采购（可选）。缺失时后续校验返回 uncertain；禁止默认 true。
    #[serde(default)]
    pub is_government_procurement: Option<bool>,
    /// 邀请招标的供应商选择方式：prequalification_notice / supplier_pool /
    /// written_recommendation（87号令第14条）。
    /// 缺失时后续校验返回 uncertain；禁止默认 prequalification_notice。
    #[serde(default)]
    pub supplier_selection_method: Option<String>,
    /// 非招标方式（磋商/谈判/询价）的供应商邀请方式：public_notice /
    /// supplier_pool / written_recommendation（214号第6条、74号令第12条）。
    #[serde(default)]
    pub invitation_method: Option<String>,
    /// 单一来源采购理由（可选）：如 only_supplier。仅 Contract 承载。
    #[serde(default)]
    pub single_source_reason: Option<String>,
    /// 是否达到公开招标数额标准（可选）。仅 Contract 承载。
    #[serde(default)]
    pub above_public_tender_threshold: Option<bool>,
    /// 单一来源采购公示开始日期（SingleSourcePublicityStarted）。
    /// 用于 single_source_pre_acquisition_publicity。原文无法确定时省略，禁止编造。
    #[serde(default)]
    pub single_source_publicity_start_date_str: Option<String>,
    /// 单一来源采购公示结束日期（SingleSourcePublicityEnded）。
    /// 用于 single_source_pre_acquisition_publicity。原文无法确定时省略，禁止编造。
    #[serde(default)]
    pub single_source_publicity_end_date_str: Option<String>,
    /// [legacy] 文件发售开始日期（DocumentAvailability 的旧字段别名）。
    #[serde(default)]
    pub document_sale_start_str: Option<String>,
    /// [legacy] 文件发售结束日期（DocumentAvailability 的旧字段别名）。
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

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum PeriodStatus {
    Compliant,
    Violation,
    /// Context 缺失 / 期间类型尚未迁移 / 无法解析
    Uncertain,
    /// 规则体系或邀请方式下公告期不适用
    NotApplicable,
    /// 输入非法（日期解析失败 / 日期倒置）
    InvalidInput,
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
    /// 核心校验逻辑（按 period_type 分发）。
    ///
    /// - `notice_publication` → 4B-4C 公告期路径
    /// - `document_availability` → 4B-4D 文件提供期路径
    /// - `single_source_pre_acquisition_publicity` → 4B-4E 单一来源公示路径
    /// - `None` → Uncertain（fail-closed，禁止按采购方式猜测期间类型）
    fn verify(args: &VerifyAnnouncementPeriodArgs) -> Result<AnnouncementPeriodResult> {
        match args.period_type.as_deref() {
            Some("notice_publication") => Self::verify_notice_publication(args),
            Some("document_availability") => Self::verify_document_availability(args),
            Some("single_source_pre_acquisition_publicity") => Self::verify_single_source_publicity(args),
            Some(other) => Ok(Self::deferred_result(
                args,
                &format!("unknown period_type '{}'", other),
            )),
            None => Ok(Self::uncertain_result(
                args,
                "missing period_type：无法确定要检查哪一类期间，禁止按采购方式猜测。",
                "Unknown",
            )),
        }
    }

    /// 未知期间类型的安全结果（Uncertain，不套任何规则）。
    fn deferred_result(args: &VerifyAnnouncementPeriodArgs, reason: &str) -> AnnouncementPeriodResult {
        AnnouncementPeriodResult {
            procurement_method: args.procurement_method.clone(),
            overall_status: PeriodStatus::Uncertain,
            announcement_period: PeriodCheck {
                check_name: format!("{}公告期", args.procurement_method),
                status: CheckStatus::Skip,
                start_date: args.notice_start_date_str.clone().unwrap_or_default(),
                end_date: args.notice_end_date_str.clone().unwrap_or_default(),
                actual_days: 0,
                required_days: 0,
                day_unit: String::new(),
                legal_basis: String::new(),
                detail: reason.to_string(),
            },
            document_sale_period: None,
            legal_basis: Vec::new(),
            suggestion: "无法识别的期间类型，请检查 period_type 取值。".to_string(),
        }
    }

    /// 构建不确定结果（legal_basis 保持为空——技术/上下文原因不进入法规来源）。
    fn uncertain_result(args: &VerifyAnnouncementPeriodArgs, reason: &str, rule_set: &str) -> AnnouncementPeriodResult {
        AnnouncementPeriodResult {
            procurement_method: args.procurement_method.clone(),
            overall_status: PeriodStatus::Uncertain,
            announcement_period: PeriodCheck {
                check_name: format!("{}公告期", args.procurement_method),
                status: CheckStatus::Skip,
                start_date: args.notice_start_date_str.clone().unwrap_or_default(),
                end_date: args.notice_end_date_str.clone().unwrap_or_default(),
                actual_days: 0,
                required_days: 0,
                day_unit: String::new(),
                legal_basis: String::new(),
                detail: reason.to_string(),
            },
            document_sale_period: None,
            legal_basis: Vec::new(),
            suggestion: "补充缺失的 Context 后重新校验。".to_string(),
        }
    }

    /// 构建不适用结果（NotApplicable）。
    fn not_applicable_result(args: &VerifyAnnouncementPeriodArgs, rule_set: &str, reason: &str) -> AnnouncementPeriodResult {
        AnnouncementPeriodResult {
            procurement_method: args.procurement_method.clone(),
            overall_status: PeriodStatus::NotApplicable,
            announcement_period: PeriodCheck {
                check_name: format!("{}公告期", args.procurement_method),
                status: CheckStatus::Skip,
                start_date: args.notice_start_date_str.clone().unwrap_or_default(),
                end_date: args.notice_end_date_str.clone().unwrap_or_default(),
                actual_days: 0,
                required_days: 0,
                day_unit: String::new(),
                legal_basis: String::new(),
                detail: reason.to_string(),
            },
            document_sale_period: None,
            legal_basis: Vec::new(),
            suggestion: String::new(),
        }
    }

    /// 构建非法输入结果。
    fn invalid_input_result(args: &VerifyAnnouncementPeriodArgs, rule_set: &str, reason: &str) -> AnnouncementPeriodResult {
        AnnouncementPeriodResult {
            procurement_method: args.procurement_method.clone(),
            overall_status: PeriodStatus::InvalidInput,
            announcement_period: PeriodCheck {
                check_name: format!("{}公告期", args.procurement_method),
                status: CheckStatus::Skip,
                start_date: args.notice_start_date_str.clone().unwrap_or_default(),
                end_date: args.notice_end_date_str.clone().unwrap_or_default(),
                actual_days: 0,
                required_days: 0,
                day_unit: String::new(),
                legal_basis: String::new(),
                detail: reason.to_string(),
            },
            document_sale_period: None,
            legal_basis: Vec::new(),
            suggestion: "请检查输入的日期是否正确。".to_string(),
        }
    }

    /// 4B-4C：NoticePublication 新路径。
    ///
    /// 流程：Args → ProcurementContext → resolve_rule_set →
    /// 条件上下文（invited selection / invitation method）→
    /// WorkingDaysCounter<CnCalendarProvider> → 阈值判定。
    /// 唯一使用的日期字段：notice_start_date_str / notice_end_date_str。
    fn verify_notice_publication(args: &VerifyAnnouncementPeriodArgs) -> Result<AnnouncementPeriodResult> {
        // ── 1. Resolver 所需 Context ──
        let object = match args.procurement_object.as_deref() {
            Some(o) if !o.trim().is_empty() => o.to_string(),
            _ => return Ok(Self::uncertain_result(args, "missing procurement_object：无法确定适用规则体系。", "Unknown")),
        };
        let gov = match args.is_government_procurement {
            Some(g) => g,
            None => return Ok(Self::uncertain_result(args, "missing is_government_procurement：无法确定是否适用政府采购规则。", "Unknown")),
        };

        let ctx = ProcurementContext {
            procurement_object: object,
            procurement_method: args.procurement_method.clone(),
            is_government_procurement: gov,
            evaluation_method: None,
        };
        let res = procurement_context::resolve_rule_set(&ctx);

        if res.status != ResolutionStatus::Resolved {
            return Ok(Self::uncertain_result(
                args,
                &format!("rule_set resolution: {:?} — {}", res.status, res.reason),
                &res.rule_set.to_string(),
            ));
        }
        if res.rule_set == RuleSet::Unknown {
            return Ok(Self::uncertain_result(args, &format!("rule_set resolution: Unknown — {}", res.reason), "Unknown"));
        }
        let rs = res.rule_set;

        // ── 2. 规则选择（RuleSet + 条件上下文）──
        // OpenTender / InvitedTender 同属 MofOrder87，需用 procurement_method 区分 open vs invited
        let is_invited = matches!(
            args.procurement_method.to_lowercase().as_str(),
            "邀请招标" | "invited_tender"
        );

        enum NoticeRule {
            WorkingDays { min: u32, citation: &'static str, full_text: &'static str },
            NotApplicable(&'static str),
        }

        let rule = match rs {
            RuleSet::MofOrder87 => {
                if is_invited {
                    match args.supplier_selection_method.as_deref() {
                        Some("prequalification_notice") => NoticeRule::WorkingDays {
                            min: 5,
                            citation: "财政部令87号第14条、第16条",
                            full_text: "资格预审公告的公告期限自公告发布之日起不得少于5个工作日。",
                        },
                        Some("supplier_pool") | Some("written_recommendation") => {
                            NoticeRule::NotApplicable("该邀请方式不发布公告，NoticePublication 不适用。")
                        }
                        Some(other) => return Ok(Self::uncertain_result(args, &format!("invalid supplier_selection_method '{}'", other), &rs.to_string())),
                        None => return Ok(Self::uncertain_result(args, "missing supplier_selection_method：邀请招标无法确定公告适用性。", &rs.to_string())),
                    }
                } else {
                    NoticeRule::WorkingDays {
                        min: 5,
                        citation: "财政部令87号第16条",
                        full_text: "招标公告的公告期限自公告发布之日起不得少于5个工作日。",
                    }
                }
            }
            RuleSet::CompetitiveConsultation214 | RuleSet::MofOrder74Negotiation | RuleSet::MofOrder74Inquiry => {
                match args.invitation_method.as_deref() {
                    Some("public_notice") => NoticeRule::WorkingDays {
                        min: 3,
                        citation: "财库〔2015〕135号",
                        full_text: "采用公告方式邀请供应商的，公告期限不得少于3个工作日。",
                    },
                    Some("supplier_pool") | Some("written_recommendation") => {
                        NoticeRule::NotApplicable("该邀请方式不发布公告，NoticePublication 不适用。")
                    }
                    Some(other) => return Ok(Self::uncertain_result(args, &format!("invalid invitation_method '{}'", other), &rs.to_string())),
                    None => return Ok(Self::uncertain_result(args, "missing invitation_method：非招标方式无法确定公告适用性。", &rs.to_string())),
                }
            }
            RuleSet::MofOrder74SingleSource => {
                return Ok(Self::not_applicable_result(args, &rs.to_string(), "单一来源采购无公告期规则（本阶段不实现）。"));
            }
            RuleSet::ConstructionTendering => {
                return Ok(Self::not_applicable_result(args, &rs.to_string(), "工程招标适用招标投标法体系，本工具不覆盖其公告期限。"));
            }
            RuleSet::Unknown => return Ok(Self::uncertain_result(args, "rule_set Unknown。", "Unknown")),
        };

        let (min_days, citation, full_text) = match rule {
            NoticeRule::WorkingDays { min, citation, full_text } => (min, citation, full_text),
            NoticeRule::NotApplicable(reason) => {
                return Ok(Self::not_applicable_result(args, &rs.to_string(), reason));
            }
        };

        // ── 3. 日期事件：仅 notice_start / notice_end ──
        let start_str = match args.notice_start_date_str.as_deref() {
            Some(s) if !s.trim().is_empty() => s,
            _ => return Ok(Self::uncertain_result(args, "missing notice_start_date_str：公告期必须提供公告开始日期，禁止用公告发布日期替代。", &rs.to_string())),
        };
        let end_str = match args.notice_end_date_str.as_deref() {
            Some(s) if !s.trim().is_empty() => s,
            _ => return Ok(Self::uncertain_result(args, "missing notice_end_date_str：公告期必须提供公告结束日期，禁止用投标截止日期替代。", &rs.to_string())),
        };
        let start = match parse_date(start_str) {
            Ok(d) => d,
            Err(e) => return Ok(Self::invalid_input_result(args, &rs.to_string(), &format!("notice_start_date_str 解析失败: {}", e))),
        };
        let end = match parse_date(end_str) {
            Ok(d) => d,
            Err(e) => return Ok(Self::invalid_input_result(args, &rs.to_string(), &format!("notice_end_date_str 解析失败: {}", e))),
        };
        if end < start {
            return Ok(Self::invalid_input_result(args, &rs.to_string(), "公告结束日期早于公告开始日期（end < start）。"));
        }

        // ── 4. WorkingDays 计算（STANDARD: start excluded, end included）──
        let counter_result = WorkingDaysCounter { provider: CnCalendarProvider::new() }
            .count_days(start, end, PeriodCountingConvention::STANDARD);

        let actual_days = match counter_result {
            Ok(d) => d,
            Err(TimeDomainError::CalendarUnavailable { year }) => {
                // 技术错误放 detail，不污染 legal_basis（保持 legal_basis 为法律来源）
                let mut r = Self::uncertain_result(
                    args,
                    &format!("WorkingDays 需要日历数据，年份 {} 不在支持范围（2024-2026）内。", year),
                    &rs.to_string(),
                );
                r.suggestion = format!("补充 {} 年节假日数据或改在支持年份内校验。", year);
                return Ok(r);
            }
            Err(e) => return Err(anyhow!("公告期计算失败: {}", e)),
        };

        // ── 5. 阈值判定 ──
        let is_compliant = actual_days >= min_days;
        let status = if is_compliant {
            PeriodStatus::Compliant
        } else {
            PeriodStatus::Violation
        };
        let check_status = if is_compliant { CheckStatus::Pass } else { CheckStatus::Fail };

        let detail = if actual_days >= min_days {
            format!("公告期 {} 个工作日，满足法定 ≥ {} 个工作日的要求，合规。", actual_days, min_days)
        } else {
            format!(
                "公告期仅 {} 个工作日，不满足法定 ≥ {} 个工作日的要求，违规。差额 {} 个工作日。",
                actual_days, min_days, min_days as i64 - actual_days as i64
            )
        };

        Ok(AnnouncementPeriodResult {
            procurement_method: args.procurement_method.clone(),
            overall_status: status,
            announcement_period: PeriodCheck {
                check_name: format!("{}公告期", args.procurement_method),
                status: check_status,
                start_date: start_str.to_string(),
                end_date: end_str.to_string(),
                actual_days: actual_days as i64,
                required_days: min_days as i64,
                day_unit: "工作日".to_string(),
                legal_basis: citation.to_string(),
                detail,
            },
            document_sale_period: None,
            legal_basis: vec![format!("{}：{}", citation, full_text)],
            suggestion: if matches!(status, PeriodStatus::Compliant) {
                format!("{}公告期符合法定要求。", args.procurement_method)
            } else {
                "公告期不足，建议延长公告期限或重新发布公告。".to_string()
            },
        })
    }

    /// 4B-4D：DocumentAvailability 新路径。
    ///
    /// 流程：Args → ProcurementContext → resolve_rule_set → 规则选择 →
    /// WorkingDaysCounter<CnCalendarProvider> → 阈值判定。
    /// 唯一使用的日期字段：document_availability_start_date_str / document_availability_end_date_str。
    /// 不依赖 supplier_selection_method / invitation_method（与 NoticePublication 不同维度）。
    fn verify_document_availability(args: &VerifyAnnouncementPeriodArgs) -> Result<AnnouncementPeriodResult> {
        // ── 1. Resolver 所需 Context ──
        let object = match args.procurement_object.as_deref() {
            Some(o) if !o.trim().is_empty() => o.to_string(),
            _ => return Ok(Self::uncertain_result(args, "missing procurement_object：无法确定适用规则体系。", "Unknown")),
        };
        let gov = match args.is_government_procurement {
            Some(g) => g,
            None => return Ok(Self::uncertain_result(args, "missing is_government_procurement：无法确定是否适用政府采购规则。", "Unknown")),
        };

        let ctx = ProcurementContext {
            procurement_object: object,
            procurement_method: args.procurement_method.clone(),
            is_government_procurement: gov,
            evaluation_method: None,
        };
        let res = procurement_context::resolve_rule_set(&ctx);

        if res.status != ResolutionStatus::Resolved {
            return Ok(Self::uncertain_result(
                args,
                &format!("rule_set resolution: {:?} — {}", res.status, res.reason),
                &res.rule_set.to_string(),
            ));
        }
        if res.rule_set == RuleSet::Unknown {
            return Ok(Self::uncertain_result(args, &format!("rule_set resolution: Unknown — {}", res.reason), "Unknown"));
        }
        let rs = res.rule_set;

        // ── 2. 规则选择（RuleSet 决定；不依赖邀请/公告方式）──
        enum DocAvailRule {
            WorkingDays { min: u32, citation: &'static str, full_text: &'static str },
            NotApplicable(&'static str),
        }

        let rule = match rs {
            // Open/Invited Tender：招标文件提供期限 ≥5 工作日。
            // 本 Tool 直接验证的 Hard-Law basis 为《政府采购法实施条例》第31条
            // （招标文件开始发出之日起计算）。
            // 财政部令87号第18条（招标/资格预审文件提供期限自公告发布之日起计算）为
            // Additional / Context-Specific Rule：需要 NoticePublished / PrequalificationNoticePublished
            // 事件与 document kind 才能单独验证，当前无真实资格预审文件调用方 → Future Scope，
            // 不得作为"已验证依据"混入当前 result。
            RuleSet::MofOrder87 => DocAvailRule::WorkingDays {
                min: 5,
                citation: "《政府采购法实施条例》第31条",
                full_text: "招标文件的提供期限自招标文件开始发出之日起不得少于5个工作日。",
            },
            // 磋商文件发售期限 ≥5 工作日（214号第10条）
            RuleSet::CompetitiveConsultation214 => DocAvailRule::WorkingDays {
                min: 5,
                citation: "财库〔2014〕214号第10条",
                full_text: "磋商文件的发售期限自开始之日起不得少于5个工作日。",
            },
            // 谈判/询价：当前无全国性通用 5 工作日文件发售期限规则
            RuleSet::MofOrder74Negotiation => DocAvailRule::NotApplicable(
                "当前全国通用法规没有谈判文件发售期限 ≥5 工作日的规则，DocumentAvailability 不适用。",
            ),
            RuleSet::MofOrder74Inquiry => DocAvailRule::NotApplicable(
                "当前全国通用法规没有询价通知书发售期限 ≥5 工作日的规则，DocumentAvailability 不适用（3 个工作日属响应准备期，由 verify_bid_preparation_period 处理）。",
            ),
            RuleSet::MofOrder74SingleSource => DocAvailRule::NotApplicable("单一来源采购无本工具文件提供期规则。"),
            RuleSet::ConstructionTendering => DocAvailRule::NotApplicable("工程招标适用招标投标法体系，本工具不覆盖其文件出售期限。"),
            RuleSet::Unknown => return Ok(Self::uncertain_result(args, "rule_set Unknown。", "Unknown")),
        };

        let (min_days, citation, full_text) = match rule {
            DocAvailRule::WorkingDays { min, citation, full_text } => (min, citation, full_text),
            DocAvailRule::NotApplicable(reason) => {
                return Ok(Self::not_applicable_result(args, &rs.to_string(), reason));
            }
        };

        // ── 3. 日期事件：仅 document_availability_start / end ──
        let start_str = match args.document_availability_start_date_str.as_deref() {
            Some(s) if !s.trim().is_empty() => s,
            _ => return Ok(Self::uncertain_result(args, "missing document_availability_start_date_str：文件提供期必须提供开始日期，禁止用 document_sale_start 等 legacy 字段替代。", &rs.to_string())),
        };
        let end_str = match args.document_availability_end_date_str.as_deref() {
            Some(s) if !s.trim().is_empty() => s,
            _ => return Ok(Self::uncertain_result(args, "missing document_availability_end_date_str：文件提供期必须提供结束日期，禁止用 document_sale_end 等 legacy 字段替代。", &rs.to_string())),
        };
        let start = match parse_date(start_str) {
            Ok(d) => d,
            Err(e) => return Ok(Self::invalid_input_result(args, &rs.to_string(), &format!("document_availability_start_date_str 解析失败: {}", e))),
        };
        let end = match parse_date(end_str) {
            Ok(d) => d,
            Err(e) => return Ok(Self::invalid_input_result(args, &rs.to_string(), &format!("document_availability_end_date_str 解析失败: {}", e))),
        };
        if end < start {
            return Ok(Self::invalid_input_result(args, &rs.to_string(), "文件提供结束日期早于开始日期（end < start）。"));
        }

        // ── 4. WorkingDays 计算（STANDARD: start excluded, end included；不做 FinalDayAdjustment）──
        let counter_result = WorkingDaysCounter { provider: CnCalendarProvider::new() }
            .count_days(start, end, PeriodCountingConvention::STANDARD);

        let actual_days = match counter_result {
            Ok(d) => d,
            Err(TimeDomainError::CalendarUnavailable { year }) => {
                // 技术错误放 detail，不污染 legal_basis（保持 legal_basis 为法律来源）
                let mut r = Self::uncertain_result(
                    args,
                    &format!("WorkingDays 需要日历数据，年份 {} 不在支持范围（2024-2026）内，无法判定文件提供期合规性。", year),
                    &rs.to_string(),
                );
                r.legal_basis = Vec::new(); // 技术错误不写入 legal_basis
                r.suggestion = format!("补充 {} 年节假日数据或改在支持年份内校验。", year);
                return Ok(r);
            }
            Err(e) => return Err(anyhow!("文件提供期计算失败: {}", e)),
        };

        // ── 5. 阈值判定 ──
        let is_compliant = actual_days >= min_days;
        let status = if is_compliant {
            PeriodStatus::Compliant
        } else {
            PeriodStatus::Violation
        };
        let check_status = if is_compliant { CheckStatus::Pass } else { CheckStatus::Fail };

        let detail = if is_compliant {
            format!("文件提供期 {} 个工作日，满足法定 ≥ {} 个工作日的要求，合规。", actual_days, min_days)
        } else {
            format!(
                "文件提供期仅 {} 个工作日，不满足法定 ≥ {} 个工作日的要求，违规。差额 {} 个工作日。",
                actual_days, min_days, min_days as i64 - actual_days as i64
            )
        };

        Ok(AnnouncementPeriodResult {
            procurement_method: args.procurement_method.clone(),
            overall_status: status,
            announcement_period: PeriodCheck {
                check_name: format!("{}文件提供期", args.procurement_method),
                status: check_status,
                start_date: start_str.to_string(),
                end_date: end_str.to_string(),
                actual_days: actual_days as i64,
                required_days: min_days as i64,
                day_unit: "工作日".to_string(),
                legal_basis: citation.to_string(),
                detail,
            },
            document_sale_period: None,
            legal_basis: vec![format!("{}：{}", citation, full_text)],
            suggestion: if is_compliant {
                format!("{}文件提供期符合法定要求。", args.procurement_method)
            } else {
                "文件提供期不足，建议延长文件提供/发售期限。".to_string()
            },
        })
    }

    /// 4B-4E：SingleSourcePreAcquisitionPublicity 新路径。
    ///
    /// 适用条件（全部满足才执行 5 WorkingDays 公示期，缺一不可）：
    /// - 采购方式 = single_source（Resolver → MofOrder74SingleSource）
    /// - procurement_object ∈ {goods, service}（construction → NotApplicable）
    /// - is_government_procurement = true
    /// - single_source_reason = only_supplier
    /// - above_public_tender_threshold = true
    ///
    /// 唯一日期字段：single_source_publicity_start_date_str / single_source_publicity_end_date_str。
    /// 禁止 fallback notice / announcement / document_sale 等任何其他字段。
    fn verify_single_source_publicity(args: &VerifyAnnouncementPeriodArgs) -> Result<AnnouncementPeriodResult> {
        const MIN_DAYS: u32 = 5;
        const CITATION: &str = "《政府采购法实施条例》第38条、财政部令第74号第38条";
        const FULL_TEXT: &str = "采取单一来源方式采购的，采购人应当公示，公示期不得少于5个工作日。";
        const RULE_SET: &str = "MofOrder74SingleSource";

        // ── 1. 上下文条件（缺失 → Uncertain，禁止默认）──
        let object = match args.procurement_object.as_deref() {
            Some(o) if !o.trim().is_empty() => o.to_string(),
            _ => return Ok(Self::uncertain_result(args, "missing procurement_object：单一来源公示需要采购对象。", RULE_SET)),
        };
        let gov = match args.is_government_procurement {
            Some(g) => g,
            None => return Ok(Self::uncertain_result(args, "missing is_government_procurement：无法确定是否适用政府采购规则。", RULE_SET)),
        };
        if !gov {
            return Ok(Self::not_applicable_result(args, RULE_SET, "非政府采购，单一来源公示规则不适用。"));
        }

        // Resolver 确认 RuleSet（method 必须是单一来源）
        let ctx = ProcurementContext {
            procurement_object: object.clone(),
            procurement_method: args.procurement_method.clone(),
            is_government_procurement: true,
            evaluation_method: None,
        };
        let res = procurement_context::resolve_rule_set(&ctx);
        if res.status != ResolutionStatus::Resolved || res.rule_set != RuleSet::MofOrder74SingleSource {
            return Ok(Self::not_applicable_result(args, RULE_SET, "采购方式非单一来源，SingleSourcePreAcquisitionPublicity 不适用。"));
        }

        // ── 2. 条件化适用性 ──
        if matches!(object.as_str(), "construction" | "工程") {
            return Ok(Self::not_applicable_result(args, RULE_SET, "单一来源公示仅适用于货物/服务采购（construction → NotApplicable）。"));
        }
        let reason = match args.single_source_reason.as_deref() {
            Some(r) if !r.trim().is_empty() => r,
            _ => return Ok(Self::uncertain_result(args, "missing single_source_reason：单一来源公示需要采购理由。", RULE_SET)),
        };
        // 未知任意字符串 → InvalidInput（"无法识别"不能等同于"已确认不适用"）
        match reason {
            "only_supplier" | "emergency" | "continuity_additional_purchase" => {}
            other => {
                return Ok(Self::invalid_input_result(
                    args,
                    RULE_SET,
                    &format!("invalid single_source_reason '{}'：仅支持 only_supplier / emergency / continuity_additional_purchase。", other),
                ));
            }
        }
        let threshold = match args.above_public_tender_threshold {
            Some(t) => t,
            None => return Ok(Self::uncertain_result(args, "missing above_public_tender_threshold：无法确定是否达到公开招标数额标准。", RULE_SET)),
        };
        match reason {
            "emergency" | "continuity_additional_purchase" => {
                return Ok(Self::not_applicable_result(args, RULE_SET, "该类单一来源情形不适用 5 工作日公示（仅 only_supplier 适用）。"));
            }
            _ => {}
        }
        if !threshold {
            return Ok(Self::not_applicable_result(
                args,
                RULE_SET,
                "only_supplier + 达到公开招标数额标准 才适用 5 工作日公示；当前未达数额标准。",
            ));
        }

        // ── 3. 日期事件：仅 single_source_publicity_start/end ──
        let start_str = match args.single_source_publicity_start_date_str.as_deref() {
            Some(s) if !s.trim().is_empty() => s,
            _ => return Ok(Self::uncertain_result(args, "missing single_source_publicity_start_date_str：公示期必须提供公示开始日期，禁止用 notice/announcement 等字段替代。", RULE_SET)),
        };
        let end_str = match args.single_source_publicity_end_date_str.as_deref() {
            Some(s) if !s.trim().is_empty() => s,
            _ => return Ok(Self::uncertain_result(args, "missing single_source_publicity_end_date_str：公示期必须提供公示结束日期，禁止用 notice/announcement 等字段替代。", RULE_SET)),
        };
        let start = match parse_date(start_str) {
            Ok(d) => d,
            Err(e) => return Ok(Self::invalid_input_result(args, RULE_SET, &format!("single_source_publicity_start_date_str 解析失败: {}", e))),
        };
        let end = match parse_date(end_str) {
            Ok(d) => d,
            Err(e) => return Ok(Self::invalid_input_result(args, RULE_SET, &format!("single_source_publicity_end_date_str 解析失败: {}", e))),
        };
        if end < start {
            return Ok(Self::invalid_input_result(args, RULE_SET, "公示结束日期早于开始日期（end < start）。"));
        }

        // ── 4. WorkingDays 计算（STANDARD: start excluded, end included）──
        let counter_result = WorkingDaysCounter { provider: CnCalendarProvider::new() }
            .count_days(start, end, PeriodCountingConvention::STANDARD);

        let actual_days = match counter_result {
            Ok(d) => d,
            Err(TimeDomainError::CalendarUnavailable { year }) => {
                let mut r = Self::uncertain_result(
                    args,
                    &format!("WorkingDays 需要日历数据，年份 {} 不在支持范围（2024-2026）内，无法判定公示期合规性。", year),
                    RULE_SET,
                );
                r.suggestion = format!("补充 {} 年节假日数据或改在支持年份内校验。", year);
                return Ok(r);
            }
            Err(e) => return Err(anyhow!("公示期计算失败: {}", e)),
        };

        // ── 5. 阈值判定 ──
        let is_compliant = actual_days >= MIN_DAYS;
        let status = if is_compliant {
            PeriodStatus::Compliant
        } else {
            PeriodStatus::Violation
        };
        let check_status = if is_compliant { CheckStatus::Pass } else { CheckStatus::Fail };

        let detail = if is_compliant {
            format!("单一来源公示期 {} 个工作日，满足法定 ≥ {} 个工作日的要求，合规。", actual_days, MIN_DAYS)
        } else {
            format!(
                "单一来源公示期仅 {} 个工作日，不满足法定 ≥ {} 个工作日的要求，违规。差额 {} 个工作日。",
                actual_days, MIN_DAYS, MIN_DAYS as i64 - actual_days as i64
            )
        };

        Ok(AnnouncementPeriodResult {
            procurement_method: args.procurement_method.clone(),
            overall_status: status,
            announcement_period: PeriodCheck {
                check_name: format!("{}单一来源公示期", args.procurement_method),
                status: check_status,
                start_date: start_str.to_string(),
                end_date: end_str.to_string(),
                actual_days: actual_days as i64,
                required_days: MIN_DAYS as i64,
                day_unit: "工作日".to_string(),
                legal_basis: CITATION.to_string(),
                detail,
            },
            document_sale_period: None,
            legal_basis: vec![format!("{}：{}", CITATION, FULL_TEXT)],
            suggestion: if is_compliant {
                format!("{}单一来源公示期符合法定要求。", args.procurement_method)
            } else {
                "单一来源公示期不足，建议延长公示期限。".to_string()
            },
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
                "description": "【使用场景】按 period_type 校验三类独立法律期间：\
                    ① notice_publication：公告期（公告开始 → 公告结束）；\
                    ② document_availability：采购文件提供/发售期（提供开始 → 提供结束）；\
                    ③ single_source_pre_acquisition_publicity：单一来源采购前公示\
                    （需 single_source + goods/service + only_supplier + 达到公开招标数额标准）。\
                    禁止用采购方式猜测期间类型；缺失 period_type 时返回 uncertain。\
                    【不使用场景】不校验投标/响应准备期（20日等标期、10日磋商、3工作日谈判/询价——\
                    用 verify_bid_preparation_period）；不校验公告内容完整性；不校验公告媒介。\
                    【关键】announcement_date_str / bid_deadline_date_str / document_sale_start_str / \
                    document_sale_end_str 为 legacy 字段，仅兼容反序列化，任何期间计算均不读取。\
                    日期支持 YYYY-MM-DD 和 YYYY/MM/DD 两种格式。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "procurement_method": {
                            "type": "string",
                            "enum": ["公开招标", "邀请招标", "竞争性磋商", "竞争性谈判", "询价", "单一来源"],
                            "description": "采购方式。"
                        },
                        "period_type": {
                            "type": "string",
                            "enum": ["notice_publication", "document_availability", "single_source_pre_acquisition_publicity"],
                            "description": "期间类型。缺失时返回 uncertain；禁止按采购方式猜测。"
                        },
                        "announcement_date_str": {
                            "type": "string",
                            "description": "[legacy] 公告发布日期。仅表示公告发布日，不得作为采购文件发出/提供起点，不得与 bid_deadline 组合计算准备期。"
                        },
                        "bid_deadline_date_str": {
                            "type": "string",
                            "description": "[legacy] 投标/响应文件截止日期。不得用于公告期或文件提供期计算。"
                        },
                        "notice_start_date_str": {
                            "type": "string",
                            "description": "公告开始日期（NoticePublished）。用于 notice_publication。原文无法确定时省略，禁止用公告发布日期替代。"
                        },
                        "notice_end_date_str": {
                            "type": "string",
                            "description": "公告结束日期（NoticePublicationEnded）。用于 notice_publication。原文无法确定时省略，禁止用投标截止日期替代。"
                        },
                        "document_availability_start_date_str": {
                            "type": "string",
                            "description": "采购文件开始提供/发售日期（DocumentAvailabilityStarted）。用于 document_availability。"
                        },
                        "document_availability_end_date_str": {
                            "type": "string",
                            "description": "采购文件结束提供/发售日期（DocumentAvailabilityEnded）。用于 document_availability。"
                        },
                        "procurement_object": {
                            "type": "string",
                            "enum": ["goods", "service", "construction"],
                            "description": "采购对象（可选）：goods/service/construction 或中文。缺失时后续校验返回 uncertain；禁止推测。"
                        },
                        "is_government_procurement": {
                            "type": "boolean",
                            "description": "是否政府采购（可选）。缺失时后续校验返回 uncertain；禁止默认 true。"
                        },
                        "supplier_selection_method": {
                            "type": "string",
                            "enum": ["prequalification_notice", "supplier_pool", "written_recommendation"],
                            "description": "邀请招标供应商选择方式（87号令第14条）。缺失时后续校验返回 uncertain；禁止默认。"
                        },
                        "invitation_method": {
                            "type": "string",
                            "enum": ["public_notice", "supplier_pool", "written_recommendation"],
                            "description": "非招标方式供应商邀请方式（214号第6条、74号令第12条）。缺失时后续校验返回 uncertain；禁止默认。"
                        },
                        "single_source_reason": {
                            "type": "string",
                            "enum": ["only_supplier", "emergency", "continuity_additional_purchase"],
                            "description": "单一来源采购理由（政府采购法第31条三类情形）：\
                                only_supplier=只能从唯一供应商处采购；\
                                emergency=发生不可预见的紧急情况；\
                                continuity_additional_purchase=原项目一致性或配套追加采购。\
                                仅这三类受控值；原文无法确定是哪一类时省略字段（返回 uncertain），禁止编造或猜测。"
                        },
                        "above_public_tender_threshold": {
                            "type": "boolean",
                            "description": "是否达到公开招标数额标准。缺失时后续校验返回 uncertain；禁止默认。"
                        },
                        "single_source_publicity_start_date_str": {
                            "type": "string",
                            "description": "单一来源采购公示开始日期（SingleSourcePublicityStarted）。用于 single_source_pre_acquisition_publicity。原文无法确定时省略，禁止编造。"
                        },
                        "single_source_publicity_end_date_str": {
                            "type": "string",
                            "description": "单一来源采购公示结束日期（SingleSourcePublicityEnded）。用于 single_source_pre_acquisition_publicity。原文无法确定时省略，禁止编造。"
                        },
                        "document_sale_start_str": {
                            "type": "string",
                            "description": "[legacy alias] 文件发售开始日期（document_availability_start_date_str 的旧别名）。"
                        },
                        "document_sale_end_str": {
                            "type": "string",
                            "description": "[legacy alias] 文件发售结束日期（document_availability_end_date_str 的旧别名）。"
                        }
                    },
                    "required": ["procurement_method"]
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

    // ── Legacy Contract：解析兼容，业务死亡 ─────────────────────

    #[test]
    fn legacy_payload_still_deserializes_but_execute_fail_closed() {
        // 旧 payload 仍可 serde deserialize（字段保留），但缺 period_type → Uncertain
        let args: VerifyAnnouncementPeriodArgs = serde_json::from_value(serde_json::json!({
            "procurement_method": "公开招标",
            "announcement_date_str": "2025-03-01",
            "bid_deadline_date_str": "2025-03-26",
            "document_sale_start_str": "2025-03-01",
            "document_sale_end_str": "2025-03-07"
        }))
        .unwrap();
        assert_eq!(args.announcement_date_str.as_deref(), Some("2025-03-01"));
        // 缺 period_type → Uncertain，旧 20 日等标期业务已死亡
        let r = VerifyAnnouncementPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Uncertain));
    }

    #[test]
    fn missing_period_type_fail_closed_uncertain() {
        // period_type=None + 任意合法日期 → 不猜期间类型 → Uncertain
        let args = VerifyAnnouncementPeriodArgs {
            procurement_method: "公开招标".to_string(),
            period_type: None,
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            notice_start_date_str: Some("2025-03-03".to_string()),
            notice_end_date_str: Some("2025-03-10".to_string()),
            ..Default::default()
        };
        let r = VerifyAnnouncementPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Uncertain));
    }

    #[test]
    fn legacy_fields_never_read_any_path() {
        // legacy 字段仅兼容反序列化；新路径结果只由新字段决定
        let args = VerifyAnnouncementPeriodArgs {
            procurement_method: "公开招标".to_string(),
            period_type: Some("notice_publication".to_string()),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            notice_start_date_str: Some("2025-03-03".to_string()),
            notice_end_date_str: Some("2025-03-10".to_string()),
            announcement_date_str: Some("2024-01-01".to_string()),
            bid_deadline_date_str: Some("2024-12-31".to_string()),
            document_sale_start_str: Some("2024-01-01".to_string()),
            document_sale_end_str: Some("2024-01-05".to_string()),
            ..Default::default()
        };
        let r = VerifyAnnouncementPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 5, "结果必须只由 notice_start/end 决定");
    }

    // ── 4B-4B Contract serde 测试 ──────────────────────────────

    #[test]
    fn serde_legacy_payload_still_parses() {
        // 旧 payload（legacy 字段）仍可 deserialize
        let args: VerifyAnnouncementPeriodArgs = serde_json::from_value(serde_json::json!({
            "procurement_method": "公开招标",
            "announcement_date_str": "2025-03-01",
            "bid_deadline_date_str": "2025-03-21",
            "document_sale_start_str": "2025-03-01",
            "document_sale_end_str": "2025-03-07"
        }))
        .unwrap();
        assert_eq!(args.announcement_date_str.as_deref(), Some("2025-03-01"));
        assert_eq!(args.bid_deadline_date_str.as_deref(), Some("2025-03-21"));
        assert!(args.period_type.is_none());
    }

    #[test]
    fn serde_notice_publication_payload_parses() {
        let args: VerifyAnnouncementPeriodArgs = serde_json::from_value(serde_json::json!({
            "procurement_method": "邀请招标",
            "period_type": "notice_publication",
            "notice_start_date_str": "2025-03-01",
            "notice_end_date_str": "2025-03-07",
            "procurement_object": "goods",
            "is_government_procurement": true,
            "supplier_selection_method": "prequalification_notice"
        }))
        .unwrap();
        assert_eq!(args.period_type.as_deref(), Some("notice_publication"));
        assert_eq!(args.notice_start_date_str.as_deref(), Some("2025-03-01"));
        assert_eq!(args.notice_end_date_str.as_deref(), Some("2025-03-07"));
        assert_eq!(args.supplier_selection_method.as_deref(), Some("prequalification_notice"));
        assert!(args.announcement_date_str.is_none());
        assert!(args.bid_deadline_date_str.is_none());
    }

    #[test]
    fn serde_document_availability_payload_parses() {
        let args: VerifyAnnouncementPeriodArgs = serde_json::from_value(serde_json::json!({
            "procurement_method": "竞争性磋商",
            "period_type": "document_availability",
            "document_availability_start_date_str": "2025-06-01",
            "document_availability_end_date_str": "2025-06-08",
            "procurement_object": "service",
            "is_government_procurement": true,
            "invitation_method": "public_notice"
        }))
        .unwrap();
        assert_eq!(args.period_type.as_deref(), Some("document_availability"));
        assert_eq!(args.document_availability_start_date_str.as_deref(), Some("2025-06-01"));
        assert_eq!(args.document_availability_end_date_str.as_deref(), Some("2025-06-08"));
        assert_eq!(args.invitation_method.as_deref(), Some("public_notice"));
    }

    #[test]
    fn serde_invitation_methods_enums_parse() {
        for (field, value) in [
            ("supplier_selection_method", "supplier_pool"),
            ("supplier_selection_method", "written_recommendation"),
            ("invitation_method", "supplier_pool"),
            ("invitation_method", "written_recommendation"),
        ] {
            let mut json = serde_json::Map::new();
            json.insert("procurement_method".into(), serde_json::Value::String("邀请招标".into()));
            json.insert(field.into(), serde_json::Value::String(value.into()));
            let args: VerifyAnnouncementPeriodArgs = serde_json::from_value(serde_json::Value::Object(json)).unwrap();
            if field == "supplier_selection_method" {
                assert_eq!(args.supplier_selection_method.as_deref(), Some(value));
            } else {
                assert_eq!(args.invitation_method.as_deref(), Some(value));
            }
        }
    }

    #[test]
    fn serde_new_optional_context_defaults_none() {
        let args: VerifyAnnouncementPeriodArgs = serde_json::from_value(serde_json::json!({
            "procurement_method": "公开招标"
        }))
        .unwrap();
        assert!(args.period_type.is_none());
        assert!(args.notice_start_date_str.is_none());
        assert!(args.notice_end_date_str.is_none());
        assert!(args.document_availability_start_date_str.is_none());
        assert!(args.document_availability_end_date_str.is_none());
        assert!(args.procurement_object.is_none());
        assert!(args.is_government_procurement.is_none());
        assert!(args.supplier_selection_method.is_none());
        assert!(args.invitation_method.is_none());
        assert!(args.single_source_reason.is_none());
        assert!(args.above_public_tender_threshold.is_none());
    }

    #[test]
    fn serde_single_source_contract_payload_parses() {
        let args: VerifyAnnouncementPeriodArgs = serde_json::from_value(serde_json::json!({
            "procurement_method": "单一来源",
            "period_type": "single_source_pre_acquisition_publicity",
            "single_source_reason": "only_supplier",
            "above_public_tender_threshold": true,
            "procurement_object": "goods"
        }))
        .unwrap();
        assert_eq!(args.period_type.as_deref(), Some("single_source_pre_acquisition_publicity"));
        assert_eq!(args.single_source_reason.as_deref(), Some("only_supplier"));
        assert_eq!(args.above_public_tender_threshold, Some(true));
        assert_eq!(args.procurement_object.as_deref(), Some("goods"));
    }

    // ════════════════════════════════════════════════════════════
    // 4B-4C NoticePublication Behavior Tests
    // ════════════════════════════════════════════════════════════

    fn notice_args(method: &str, object: &str, start: &str, end: &str) -> VerifyAnnouncementPeriodArgs {
        VerifyAnnouncementPeriodArgs {
            procurement_method: method.to_string(),
            period_type: Some("notice_publication".to_string()),
            procurement_object: Some(object.to_string()),
            is_government_procurement: Some(true),
            notice_start_date_str: Some(start.to_string()),
            notice_end_date_str: Some(end.to_string()),
            ..Default::default()
        }
    }

    // ── OpenTender：5 WorkingDays 边界 ─────────────────────────

    #[test]
    fn notice_open_4_wd_violation() {
        // 2025-03-03(Mon) → 2025-03-07(Fri)：3/4,3/5,3/6,3/7 = 4 WD < 5
        let r = VerifyAnnouncementPeriodTool::verify(&notice_args("公开招标", "goods", "2025-03-03", "2025-03-07")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Violation));
        assert_eq!(r.announcement_period.actual_days, 4);
        assert_eq!(r.announcement_period.required_days, 5);
        assert_eq!(r.announcement_period.day_unit, "工作日");
        assert!(r.announcement_period.legal_basis.contains("第16条"));
    }

    #[test]
    fn notice_open_5_wd_compliant() {
        let r = VerifyAnnouncementPeriodTool::verify(&notice_args("公开招标", "goods", "2025-03-03", "2025-03-10")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 5);
        assert_eq!(r.announcement_period.required_days, 5);
    }

    #[test]
    fn notice_open_6_wd_compliant() {
        let r = VerifyAnnouncementPeriodTool::verify(&notice_args("公开招标", "service", "2025-03-03", "2025-03-11")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 6);
    }

    // ── Weekend crossing consumer proof ────────────────────────

    #[test]
    fn notice_weekend_skipped_not_calendar_days() {
        // 2025-03-07(Fri) → 2025-03-13(Thu)：WD = 3/10,3/11,3/12,3/13 = 4 < 5 → violation
        // 日历差 = 6 ≥ 5 → 若用 CalendarDaysCounter 会误判 compliant
        let r = VerifyAnnouncementPeriodTool::verify(&notice_args("公开招标", "goods", "2025-03-07", "2025-03-13")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Violation));
        assert_eq!(r.announcement_period.actual_days, 4);
    }

    // ── Makeup workday 2024-02-18 consumer proof ───────────────

    #[test]
    fn notice_makeup_sunday_counts() {
        // 2024-02-16(Fri) → 2024-02-22(Thu)：2/17 春节假日 skip，2/18(Sun,调休) count，
        // 2/19..2/22 count = 5 WD → compliant。若 2/18 按 weekend-only → 4 WD → violation。
        let r = VerifyAnnouncementPeriodTool::verify(&notice_args("公开招标", "goods", "2024-02-16", "2024-02-22")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 5);
    }

    // ── InvitedTender：prequalification_notice / NotApplicable ─

    fn invited_notice_args(selection: Option<&str>, start: &str, end: &str) -> VerifyAnnouncementPeriodArgs {
        let mut a = notice_args("邀请招标", "goods", start, end);
        a.supplier_selection_method = selection.map(|s| s.to_string());
        a
    }

    #[test]
    fn notice_invited_prequal_4_wd_violation() {
        let a = invited_notice_args(Some("prequalification_notice"), "2025-03-03", "2025-03-07");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Violation));
        assert_eq!(r.announcement_period.actual_days, 4);
    }

    #[test]
    fn notice_invited_prequal_5_wd_compliant() {
        let a = invited_notice_args(Some("prequalification_notice"), "2025-03-03", "2025-03-10");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 5);
        assert!(r.announcement_period.legal_basis.contains("第14条"), "资格预审应引用87号令第14条: {}", r.announcement_period.legal_basis);
    }

    #[test]
    fn notice_invited_supplier_pool_not_applicable() {
        let a = invited_notice_args(Some("supplier_pool"), "2025-03-03", "2025-03-10");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::NotApplicable));
    }

    #[test]
    fn notice_invited_written_recommendation_not_applicable() {
        let a = invited_notice_args(Some("written_recommendation"), "2025-03-03", "2025-03-10");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::NotApplicable));
    }

    #[test]
    fn notice_invited_missing_selection_uncertain() {
        let a = invited_notice_args(None, "2025-03-03", "2025-03-10");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Uncertain));
    }

    // ── Consultation：public_notice 3 WD / NotApplicable ───────

    fn consultation_notice_args(invitation: Option<&str>, start: &str, end: &str) -> VerifyAnnouncementPeriodArgs {
        let mut a = notice_args("竞争性磋商", "goods", start, end);
        a.invitation_method = invitation.map(|s| s.to_string());
        a
    }

    #[test]
    fn notice_consultation_public_2_wd_violation() {
        let a = consultation_notice_args(Some("public_notice"), "2025-06-02", "2025-06-04");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Violation));
        assert_eq!(r.announcement_period.actual_days, 2);
        assert_eq!(r.announcement_period.required_days, 3);
    }

    #[test]
    fn notice_consultation_public_3_wd_compliant() {
        let a = consultation_notice_args(Some("public_notice"), "2025-06-02", "2025-06-05");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 3);
        assert!(r.announcement_period.legal_basis.contains("135"), "磋商公告应引用财库〔2015〕135号: {}", r.announcement_period.legal_basis);
    }

    #[test]
    fn notice_consultation_public_4_wd_compliant() {
        let a = consultation_notice_args(Some("public_notice"), "2025-06-02", "2025-06-06");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 4);
    }

    #[test]
    fn notice_consultation_supplier_pool_not_applicable() {
        let a = consultation_notice_args(Some("supplier_pool"), "2025-06-02", "2025-06-05");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::NotApplicable));
    }

    #[test]
    fn notice_consultation_missing_invitation_uncertain() {
        let a = consultation_notice_args(None, "2025-06-02", "2025-06-05");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Uncertain));
    }

    // ── Negotiation / Inquiry ──────────────────────────────────

    #[test]
    fn notice_negotiation_public_3_wd_compliant() {
        let mut a = notice_args("竞争性谈判", "goods", "2025-06-02", "2025-06-05");
        a.invitation_method = Some("public_notice".to_string());
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 3);
    }

    #[test]
    fn notice_inquiry_public_3_wd_compliant() {
        let mut a = notice_args("询价", "goods", "2025-06-02", "2025-06-05");
        a.invitation_method = Some("public_notice".to_string());
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 3);
    }

    #[test]
    fn notice_inquiry_supplier_pool_not_applicable() {
        let mut a = notice_args("询价", "goods", "2025-06-02", "2025-06-05");
        a.invitation_method = Some("supplier_pool".to_string());
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::NotApplicable));
    }

    // ── ConstructionTendering / SingleSource 隔离 ──────────────

    #[test]
    fn notice_construction_tendering_not_applicable() {
        let a = notice_args("公开招标", "construction", "2025-03-03", "2025-03-10");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::NotApplicable));
    }

    // ── Deferred PeriodTypes 隔离 ──────────────────────────────

    #[test]
    fn document_availability_no_legacy_fallback_start() {
        // start 缺失 + end 存在 + legacy sale_start 存在 → 不得 fallback → Uncertain
        let args = VerifyAnnouncementPeriodArgs {
            procurement_method: "公开招标".to_string(),
            period_type: Some("document_availability".to_string()),
            document_availability_end_date_str: Some("2025-03-07".to_string()),
            document_sale_start_str: Some("2025-03-01".to_string()),
            document_sale_end_str: Some("2025-03-07".to_string()),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            ..Default::default()
        };
        let r = VerifyAnnouncementPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Uncertain));
    }

    #[test]
    fn document_availability_no_legacy_fallback_end() {
        // end 缺失 + start 存在 + legacy sale_end 存在 → 不得 fallback → Uncertain
        let args = VerifyAnnouncementPeriodArgs {
            procurement_method: "公开招标".to_string(),
            period_type: Some("document_availability".to_string()),
            document_availability_start_date_str: Some("2025-03-01".to_string()),
            document_sale_start_str: Some("2025-03-01".to_string()),
            document_sale_end_str: Some("2025-03-07".to_string()),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            ..Default::default()
        };
        let r = VerifyAnnouncementPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Uncertain));
    }

    // ── SingleSourcePreAcquisitionPublicity（4B-4E）──────────────

    fn single_source_args(reason: Option<&str>, threshold: Option<bool>, object: &str, start: &str, end: &str) -> VerifyAnnouncementPeriodArgs {
        VerifyAnnouncementPeriodArgs {
            procurement_method: "单一来源".to_string(),
            period_type: Some("single_source_pre_acquisition_publicity".to_string()),
            procurement_object: Some(object.to_string()),
            is_government_procurement: Some(true),
            single_source_reason: reason.map(|s| s.to_string()),
            above_public_tender_threshold: threshold,
            single_source_publicity_start_date_str: Some(start.to_string()),
            single_source_publicity_end_date_str: Some(end.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn ss_4_wd_violation() {
        let a = single_source_args(Some("only_supplier"), Some(true), "goods", "2025-03-03", "2025-03-07");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Violation));
        assert_eq!(r.announcement_period.actual_days, 4);
        assert_eq!(r.announcement_period.required_days, 5);
        assert_eq!(r.announcement_period.day_unit, "工作日");
        assert!(r.announcement_period.legal_basis.contains("第38条"), "单一来源公示应引用38条: {}", r.announcement_period.legal_basis);
    }

    #[test]
    fn ss_5_wd_compliant() {
        let a = single_source_args(Some("only_supplier"), Some(true), "goods", "2025-03-03", "2025-03-10");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 5);
    }

    #[test]
    fn ss_6_wd_compliant() {
        let a = single_source_args(Some("only_supplier"), Some(true), "service", "2025-03-03", "2025-03-11");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 6);
    }

    #[test]
    fn ss_weekend_skipped_not_calendar_days() {
        // 2025-03-07(Fri) → 2025-03-13(Thu)：WD=4 < 5 → violation；日历差=6 ≥ 5
        let a = single_source_args(Some("only_supplier"), Some(true), "goods", "2025-03-07", "2025-03-13");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Violation));
        assert_eq!(r.announcement_period.actual_days, 4);
    }

    #[test]
    fn ss_makeup_sunday_counts() {
        // 2024-02-16 → 2024-02-22：2/17 假日 skip，2/18(Sun,调休) count → 5 WD → compliant
        let a = single_source_args(Some("only_supplier"), Some(true), "goods", "2024-02-16", "2024-02-22");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 5);
    }

    // ── 上下文条件 ──────────────────────────────────────────────

    #[test]
    fn ss_above_threshold_false_not_applicable() {
        let a = single_source_args(Some("only_supplier"), Some(false), "goods", "2025-03-03", "2025-03-10");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::NotApplicable));
    }

    #[test]
    fn ss_emergency_not_applicable() {
        let a = single_source_args(Some("emergency"), Some(true), "goods", "2025-03-03", "2025-03-10");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::NotApplicable));
    }

    #[test]
    fn ss_continuity_purchase_not_applicable() {
        let a = single_source_args(Some("continuity_additional_purchase"), Some(true), "goods", "2025-03-03", "2025-03-10");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::NotApplicable));
    }

    #[test]
    fn ss_reason_missing_uncertain() {
        let a = single_source_args(None, Some(true), "goods", "2025-03-03", "2025-03-10");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Uncertain));
    }

    #[test]
    fn ss_threshold_missing_uncertain() {
        let a = single_source_args(Some("only_supplier"), None, "goods", "2025-03-03", "2025-03-10");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Uncertain));
    }

    #[test]
    fn ss_unknown_reason_not_not_applicable() {
        // 任意自由文本 reason 不得被当作"已确认不适用" → 必须 fail-closed（InvalidInput）
        let a = single_source_args(Some("random_reason"), Some(true), "goods", "2025-03-03", "2025-03-10");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(!matches!(r.overall_status, PeriodStatus::NotApplicable), "unknown reason 不能返回 NotApplicable: {:?}", r.overall_status);
        assert!(matches!(r.overall_status, PeriodStatus::InvalidInput), "unknown reason 应 fail-closed 为 InvalidInput: {:?}", r.overall_status);
    }

    #[test]
    fn ss_only_supplier_executes_rule() {
        // only_supplier + above=true + goods + gov=true → 5 WD 执行
        let a = single_source_args(Some("only_supplier"), Some(true), "goods", "2025-03-03", "2025-03-10");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 5);
        assert!(r.announcement_period.legal_basis.contains("第38条"));
    }

    #[test]
    fn schema_single_source_reason_enum() {
        let def = VerifyAnnouncementPeriodTool.definition();
        let props = &def["function"]["parameters"]["properties"];
        let e = props["single_source_reason"]["enum"].as_array().expect("single_source_reason 必须为 enum");
        let values: Vec<&str> = e.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(values, vec!["only_supplier", "emergency", "continuity_additional_purchase"], "enum 必须精确为三类受控值");
    }

    #[test]
    fn ss_construction_not_applicable() {
        let a = single_source_args(Some("only_supplier"), Some(true), "construction", "2025-03-03", "2025-03-10");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::NotApplicable));
    }

    // ── Legacy isolation（4B-4E §11）───────────────────────────

    #[test]
    fn ss_notice_dates_only_no_fallback() {
        // Test A：single_source start 缺失 + end 存在 + notice_start 存在 → 不得 fallback → Uncertain
        let args = VerifyAnnouncementPeriodArgs {
            procurement_method: "单一来源".to_string(),
            period_type: Some("single_source_pre_acquisition_publicity".to_string()),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            single_source_reason: Some("only_supplier".to_string()),
            above_public_tender_threshold: Some(true),
            single_source_publicity_end_date_str: Some("2025-03-10".to_string()),
            notice_start_date_str: Some("2025-03-03".to_string()),
            notice_end_date_str: Some("2025-03-10".to_string()),
            ..Default::default()
        };
        let r = VerifyAnnouncementPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Uncertain));
    }

    #[test]
    fn ss_publicity_dates_not_polluted_by_notice() {
        // Test B：single-source 专用日期 4 WD（violation），notice 日期为很长区间 → 仍 Violation
        let args = VerifyAnnouncementPeriodArgs {
            procurement_method: "单一来源".to_string(),
            period_type: Some("single_source_pre_acquisition_publicity".to_string()),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            single_source_reason: Some("only_supplier".to_string()),
            above_public_tender_threshold: Some(true),
            single_source_publicity_start_date_str: Some("2025-03-03".to_string()),
            single_source_publicity_end_date_str: Some("2025-03-07".to_string()),
            notice_start_date_str: Some("2024-01-01".to_string()),
            notice_end_date_str: Some("2024-12-31".to_string()),
            ..Default::default()
        };
        let r = VerifyAnnouncementPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Violation));
        assert_eq!(r.announcement_period.actual_days, 4, "两个事件不能串");
    }

    // ── Unsupported year / Invalid dates ───────────────────────

    #[test]
    fn ss_2027_calendar_unavailable_uncertain() {
        let a = single_source_args(Some("only_supplier"), Some(true), "goods", "2026-12-30", "2027-01-06");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Uncertain));
        assert!(r.announcement_period.detail.contains("2027"), "detail 必须保留 year=2027: {}", r.announcement_period.detail);
        assert!(r.legal_basis.iter().all(|l| !l.contains("2027")), "技术错误不应写入 legal_basis: {:?}", r.legal_basis);
    }

    #[test]
    fn ss_end_before_start_invalid_input() {
        let a = single_source_args(Some("only_supplier"), Some(true), "goods", "2025-03-10", "2025-03-01");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::InvalidInput));
    }

    #[tokio::test]
    async fn integration_execute_single_source_4_wd_violation() {
        let tool = VerifyAnnouncementPeriodTool;
        let out = tool
            .execute(serde_json::json!({
                "procurement_method": "单一来源",
                "period_type": "single_source_pre_acquisition_publicity",
                "procurement_object": "goods",
                "is_government_procurement": true,
                "single_source_reason": "only_supplier",
                "above_public_tender_threshold": true,
                "single_source_publicity_start_date_str": "2025-03-03",
                "single_source_publicity_end_date_str": "2025-03-07"
            }))
            .await
            .unwrap();
        assert_eq!(out["overall_status"], "violation");
        assert_eq!(out["announcement_period"]["actual_days"], 4);
        assert_eq!(out["announcement_period"]["required_days"], 5);
        assert!(out["announcement_period"]["legal_basis"].as_str().unwrap().contains("第38条"));
    }

    // ── Responsibility Separation（§31）────────────────────────

    #[test]
    fn responsibility_separation_open_required_is_5_not_20() {
        // 公开招标公告期在 verify_announcement_period 中必须是 5 WD（87号令16条），
        // 不得回归 20 日等标期（那属于 verify_bid_preparation_period）
        let r = VerifyAnnouncementPeriodTool::verify(&notice_args("公开招标", "goods", "2025-03-03", "2025-03-10")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.required_days, 5, "公开招标公告期必须为 5 WD，不是 20 日");
    }

    // ── No-legacy-fallback regression（4B-4B 关键 Contract）────

    #[test]
    fn notice_no_fallback_to_announcement_date() {
        // notice_start 缺失，但 announcement_date_str 存在 → 不得 fallback → Uncertain
        let args = VerifyAnnouncementPeriodArgs {
            procurement_method: "公开招标".to_string(),
            period_type: Some("notice_publication".to_string()),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            announcement_date_str: Some("2025-03-01".to_string()),
            notice_end_date_str: Some("2025-03-10".to_string()),
            ..Default::default()
        };
        let r = VerifyAnnouncementPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Uncertain));
    }

    #[test]
    fn notice_no_fallback_to_bid_deadline() {
        // notice_end 缺失，但 bid_deadline_date_str 存在 → 不得 fallback → Uncertain
        let args = VerifyAnnouncementPeriodArgs {
            procurement_method: "公开招标".to_string(),
            period_type: Some("notice_publication".to_string()),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            notice_start_date_str: Some("2025-03-03".to_string()),
            bid_deadline_date_str: Some("2025-03-10".to_string()),
            ..Default::default()
        };
        let r = VerifyAnnouncementPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Uncertain));
    }

    #[test]
    fn notice_legacy_dates_do_not_pollute() {
        // notice 日期与 legacy 日期完全不同 → 结果必须只由 notice_start/end 决定
        let args = VerifyAnnouncementPeriodArgs {
            procurement_method: "竞争性磋商".to_string(),
            period_type: Some("notice_publication".to_string()),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            invitation_method: Some("public_notice".to_string()),
            notice_start_date_str: Some("2025-06-02".to_string()),
            notice_end_date_str: Some("2025-06-05".to_string()),
            announcement_date_str: Some("2025-01-01".to_string()),
            bid_deadline_date_str: Some("2025-12-31".to_string()),
            ..Default::default()
        };
        let r = VerifyAnnouncementPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 3, "结果必须只由 notice_start/end 决定");
    }

    // ── Unsupported year / Invalid dates ───────────────────────

    #[test]
    fn notice_2027_calendar_unavailable_uncertain() {
        let a = notice_args("公开招标", "goods", "2026-12-30", "2027-01-05");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Uncertain));
        assert!(r.announcement_period.detail.contains("2027"), "detail 必须保留 year=2027: {}", r.announcement_period.detail);
        assert!(r.legal_basis.iter().all(|l| !l.contains("2027")), "技术错误不应写入 legal_basis: {:?}", r.legal_basis);
    }

    #[test]
    fn notice_end_before_start_invalid_input() {
        let a = notice_args("公开招标", "goods", "2025-03-10", "2025-03-01");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::InvalidInput));
    }

    #[test]
    fn notice_unparseable_invalid_input() {
        let a = notice_args("公开招标", "goods", "not-a-date", "2025-03-10");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::InvalidInput));
    }

    #[test]
    fn notice_start_equals_end_zero_violation() {
        // start == end → actual 0 < 5 → violation
        let a = notice_args("公开招标", "goods", "2025-03-10", "2025-03-10");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Violation));
        assert_eq!(r.announcement_period.actual_days, 0);
    }

    // ── Real Consumer execute integration ──────────────────────

    #[tokio::test]
    async fn integration_execute_notice_open_tender_4_wd_violation() {
        let tool = VerifyAnnouncementPeriodTool;
        let out = tool
            .execute(serde_json::json!({
                "procurement_method": "公开招标",
                "period_type": "notice_publication",
                "procurement_object": "goods",
                "is_government_procurement": true,
                "notice_start_date_str": "2025-03-03",
                "notice_end_date_str": "2025-03-07"
            }))
            .await
            .unwrap();
        assert_eq!(out["overall_status"], "violation");
        assert_eq!(out["announcement_period"]["actual_days"], 4);
        assert_eq!(out["announcement_period"]["required_days"], 5);
        assert_eq!(out["announcement_period"]["day_unit"], "工作日");
    }

    #[tokio::test]
    async fn integration_execute_notice_consultation_public_notice_compliant() {
        let tool = VerifyAnnouncementPeriodTool;
        let out = tool
            .execute(serde_json::json!({
                "procurement_method": "竞争性磋商",
                "period_type": "notice_publication",
                "procurement_object": "goods",
                "is_government_procurement": true,
                "invitation_method": "public_notice",
                "notice_start_date_str": "2025-06-02",
                "notice_end_date_str": "2025-06-05"
            }))
            .await
            .unwrap();
        assert_eq!(out["overall_status"], "compliant");
        assert_eq!(out["announcement_period"]["actual_days"], 3);
    }

    // ════════════════════════════════════════════════════════════
    // 4B-4D DocumentAvailability Behavior Tests
    // ════════════════════════════════════════════════════════════

    fn doc_avail_args(method: &str, object: &str, start: &str, end: &str) -> VerifyAnnouncementPeriodArgs {
        VerifyAnnouncementPeriodArgs {
            procurement_method: method.to_string(),
            period_type: Some("document_availability".to_string()),
            procurement_object: Some(object.to_string()),
            is_government_procurement: Some(true),
            document_availability_start_date_str: Some(start.to_string()),
            document_availability_end_date_str: Some(end.to_string()),
            ..Default::default()
        }
    }

    // ── OpenTender：5 WorkingDays 边界 ─────────────────────────

    #[test]
    fn docavail_open_4_wd_violation() {
        let r = VerifyAnnouncementPeriodTool::verify(&doc_avail_args("公开招标", "goods", "2025-03-03", "2025-03-07")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Violation));
        assert_eq!(r.announcement_period.actual_days, 4);
        assert_eq!(r.announcement_period.required_days, 5);
        assert_eq!(r.announcement_period.day_unit, "工作日");
        assert!(r.announcement_period.legal_basis.contains("实施条例"), "招标文件提供期主依据为实施条例: {}", r.announcement_period.legal_basis);
        assert!(r.announcement_period.legal_basis.contains("第31条"), "招标文件提供期必须含第31条: {}", r.announcement_period.legal_basis);
    }

    #[test]
    fn docavail_open_legal_basis_no_87_claim() {
        // 不得虚报"已验证87号令第18条"：legal_basis 只能含实施条例31条
        let r = VerifyAnnouncementPeriodTool::verify(&doc_avail_args("公开招标", "goods", "2025-03-03", "2025-03-10")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert!(r.announcement_period.legal_basis.contains("实施条例"), "legal_basis 必须含实施条例: {}", r.announcement_period.legal_basis);
        assert!(r.announcement_period.legal_basis.contains("第31条"), "legal_basis 必须含第31条: {}", r.announcement_period.legal_basis);
        assert!(!r.announcement_period.legal_basis.contains("87号令"), "不得声称已验证87号令第18条: {}", r.announcement_period.legal_basis);
        assert!(!r.announcement_period.legal_basis.contains("第18条"), "不得声称已验证87号令第18条: {}", r.announcement_period.legal_basis);
        assert!(!r.announcement_period.legal_basis.contains("已验证"), "不得混入已验证表述: {}", r.announcement_period.legal_basis);
        // legal_basis 向量同理
        assert!(r.legal_basis.iter().all(|l| !l.contains("87号令") && !l.contains("第18条")), "legal_basis 向量不得含87号令18条: {:?}", r.legal_basis);
    }

    #[test]
    fn docavail_open_event_semantics_independent_of_notice() {
        // document_availability_start/end 独立完成实施条例31条判断，不要求 notice_start/end 存在
        let args = VerifyAnnouncementPeriodArgs {
            procurement_method: "公开招标".to_string(),
            period_type: Some("document_availability".to_string()),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            document_availability_start_date_str: Some("2025-03-03".to_string()),
            document_availability_end_date_str: Some("2025-03-10".to_string()),
            notice_start_date_str: None,
            notice_end_date_str: None,
            ..Default::default()
        };
        let r = VerifyAnnouncementPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant), "无需 notice 事件即可判定，不能 Uncertain");
        assert_eq!(r.announcement_period.actual_days, 5);
        assert!(r.announcement_period.legal_basis.contains("第31条"), "判定依据必须是实施条例31条: {}", r.announcement_period.legal_basis);
    }

    #[test]
    fn docavail_open_5_wd_compliant() {
        let r = VerifyAnnouncementPeriodTool::verify(&doc_avail_args("公开招标", "goods", "2025-03-03", "2025-03-10")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 5);
    }

    #[test]
    fn docavail_open_6_wd_compliant() {
        let r = VerifyAnnouncementPeriodTool::verify(&doc_avail_args("公开招标", "service", "2025-03-03", "2025-03-11")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 6);
    }

    // ── InvitedTender：不依赖 supplier_selection_method ────────

    #[test]
    fn docavail_invited_4_wd_violation() {
        let r = VerifyAnnouncementPeriodTool::verify(&doc_avail_args("邀请招标", "goods", "2025-03-03", "2025-03-07")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Violation));
        assert_eq!(r.announcement_period.actual_days, 4);
    }

    #[test]
    fn docavail_invited_5_wd_compliant() {
        let r = VerifyAnnouncementPeriodTool::verify(&doc_avail_args("邀请招标", "goods", "2025-03-03", "2025-03-10")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 5);
    }

    #[test]
    fn docavail_invited_supplier_pool_still_compliant() {
        // supplier_pool 不影响招标文件提供期是否存在（与 NoticePublication 不同维度）
        let mut a = doc_avail_args("邀请招标", "goods", "2025-03-03", "2025-03-10");
        a.supplier_selection_method = Some("supplier_pool".to_string());
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant), "邀请招标经供应商库仍须提供招标文件");
        assert_eq!(r.announcement_period.actual_days, 5);
    }

    // ── Consultation：5 WD 边界 + construction + 邀请方式独立 ──

    #[test]
    fn docavail_consultation_4_wd_violation() {
        let r = VerifyAnnouncementPeriodTool::verify(&doc_avail_args("竞争性磋商", "goods", "2025-06-02", "2025-06-06")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Violation));
        assert_eq!(r.announcement_period.actual_days, 4);
        assert_eq!(r.announcement_period.required_days, 5);
        assert!(r.announcement_period.legal_basis.contains("214"), "磋商文件发售期应引用214号: {}", r.announcement_period.legal_basis);
    }

    #[test]
    fn docavail_consultation_5_wd_compliant() {
        let r = VerifyAnnouncementPeriodTool::verify(&doc_avail_args("竞争性磋商", "goods", "2025-06-02", "2025-06-09")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 5);
    }

    #[test]
    fn docavail_consultation_6_wd_compliant() {
        let r = VerifyAnnouncementPeriodTool::verify(&doc_avail_args("竞争性磋商", "service", "2025-06-02", "2025-06-10")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 6);
    }

    #[test]
    fn docavail_consultation_construction_compliant() {
        // Resolver 允许 construction 磋商 → 214；不得套用评分 Tool 的 goods/service 限制
        let r = VerifyAnnouncementPeriodTool::verify(&doc_avail_args("竞争性磋商", "construction", "2025-06-02", "2025-06-09")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 5);
    }

    #[test]
    fn docavail_consultation_supplier_pool_independent() {
        // invitation_method=supplier_pool 不影响磋商文件发售期 5 WD
        let mut a = doc_avail_args("竞争性磋商", "goods", "2025-06-02", "2025-06-09");
        a.invitation_method = Some("supplier_pool".to_string());
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant), "磋商文件发售期与邀请方式独立");
        assert_eq!(r.announcement_period.actual_days, 5);
    }

    #[test]
    fn docavail_consultation_written_recommendation_independent() {
        let mut a = doc_avail_args("竞争性磋商", "goods", "2025-06-02", "2025-06-09");
        a.invitation_method = Some("written_recommendation".to_string());
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
    }

    // ── Negotiation / Inquiry → NotApplicable ──────────────────

    #[test]
    fn docavail_negotiation_not_applicable() {
        let r = VerifyAnnouncementPeriodTool::verify(&doc_avail_args("竞争性谈判", "goods", "2025-06-02", "2025-06-09")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::NotApplicable), "谈判文件发售期无全国性 5 工作日规则");
    }

    #[test]
    fn docavail_inquiry_not_applicable() {
        let r = VerifyAnnouncementPeriodTool::verify(&doc_avail_args("询价", "goods", "2025-06-02", "2025-06-09")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::NotApplicable), "询价 3 工作日属响应准备期，不适用本路径");
    }

    #[test]
    fn docavail_single_source_not_applicable() {
        let r = VerifyAnnouncementPeriodTool::verify(&doc_avail_args("单一来源", "goods", "2025-06-02", "2025-06-09")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::NotApplicable));
    }

    #[test]
    fn docavail_construction_not_applicable() {
        let r = VerifyAnnouncementPeriodTool::verify(&doc_avail_args("公开招标", "construction", "2025-03-03", "2025-03-10")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::NotApplicable));
    }

    // ── Working-day consumer proofs ────────────────────────────

    #[test]
    fn docavail_weekend_skipped_not_calendar_days() {
        // 2025-03-07(Fri) → 2025-03-13(Thu)：WD=4 < 5 → violation；日历差=6 ≥ 5
        let r = VerifyAnnouncementPeriodTool::verify(&doc_avail_args("公开招标", "goods", "2025-03-07", "2025-03-13")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Violation));
        assert_eq!(r.announcement_period.actual_days, 4);
    }

    #[test]
    fn docavail_makeup_sunday_counts() {
        // 2024-02-16 → 2024-02-22：2/17 假日 skip，2/18(Sun,调休) count → 5 WD → compliant
        let r = VerifyAnnouncementPeriodTool::verify(&doc_avail_args("公开招标", "goods", "2024-02-16", "2024-02-22")).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 5);
    }

    // ── Legacy isolation ───────────────────────────────────────

    #[test]
    fn docavail_legacy_sale_not_used_when_new_present() {
        // Test B：新字段 4 WD（violation），legacy document_sale 为 20 日 → 仍 Violation（只由新字段决定）
        let args = VerifyAnnouncementPeriodArgs {
            procurement_method: "公开招标".to_string(),
            period_type: Some("document_availability".to_string()),
            document_availability_start_date_str: Some("2025-03-03".to_string()),
            document_availability_end_date_str: Some("2025-03-07".to_string()),
            document_sale_start_str: Some("2025-03-01".to_string()),
            document_sale_end_str: Some("2025-03-21".to_string()),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            ..Default::default()
        };
        let r = VerifyAnnouncementPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Violation));
        assert_eq!(r.announcement_period.actual_days, 4, "结果必须只由 document_availability 新字段决定");
    }

    #[test]
    fn docavail_conflicting_notice_dates_ignored() {
        // Test C：新字段 5 WD（compliant），notice/bid_deadline 完全冲突 → 仍按新字段 Compliant
        let args = VerifyAnnouncementPeriodArgs {
            procurement_method: "竞争性磋商".to_string(),
            period_type: Some("document_availability".to_string()),
            document_availability_start_date_str: Some("2025-06-02".to_string()),
            document_availability_end_date_str: Some("2025-06-09".to_string()),
            notice_start_date_str: Some("2024-01-01".to_string()),
            notice_end_date_str: Some("2024-01-02".to_string()),
            announcement_date_str: Some("2024-01-01".to_string()),
            bid_deadline_date_str: Some("2024-12-31".to_string()),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            ..Default::default()
        };
        let r = VerifyAnnouncementPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Compliant));
        assert_eq!(r.announcement_period.actual_days, 5);
    }

    // ── Unsupported year / Invalid dates ───────────────────────

    #[test]
    fn docavail_2027_calendar_unavailable_uncertain() {
        let a = doc_avail_args("公开招标", "goods", "2026-12-30", "2027-01-06");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Uncertain));
        assert!(r.announcement_period.detail.contains("2027"), "detail 必须保留 year=2027: {}", r.announcement_period.detail);
        // legal_basis 保持法律来源，不被技术错误污染
        assert!(r.legal_basis.iter().all(|l| !l.contains("2027")), "技术错误不应写入 legal_basis: {:?}", r.legal_basis);
    }

    #[test]
    fn docavail_end_before_start_invalid_input() {
        let a = doc_avail_args("公开招标", "goods", "2025-03-10", "2025-03-01");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::InvalidInput));
    }

    #[test]
    fn docavail_unparseable_invalid_input() {
        let a = doc_avail_args("公开招标", "goods", "not-a-date", "2025-03-10");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::InvalidInput));
    }

    #[test]
    fn docavail_start_equals_end_zero_violation() {
        let a = doc_avail_args("公开招标", "goods", "2025-03-10", "2025-03-10");
        let r = VerifyAnnouncementPeriodTool::verify(&a).unwrap();
        assert!(matches!(r.overall_status, PeriodStatus::Violation));
        assert_eq!(r.announcement_period.actual_days, 0);
    }

    // ── Real Consumer execute integration ──────────────────────

    #[tokio::test]
    async fn integration_execute_docavail_open_tender_4_wd_violation() {
        let tool = VerifyAnnouncementPeriodTool;
        let out = tool
            .execute(serde_json::json!({
                "procurement_method": "公开招标",
                "period_type": "document_availability",
                "procurement_object": "goods",
                "is_government_procurement": true,
                "document_availability_start_date_str": "2025-03-03",
                "document_availability_end_date_str": "2025-03-07"
            }))
            .await
            .unwrap();
        assert_eq!(out["overall_status"], "violation");
        assert_eq!(out["announcement_period"]["actual_days"], 4);
        assert_eq!(out["announcement_period"]["required_days"], 5);
    }

    #[tokio::test]
    async fn integration_execute_docavail_consultation_compliant() {
        let tool = VerifyAnnouncementPeriodTool;
        let out = tool
            .execute(serde_json::json!({
                "procurement_method": "竞争性磋商",
                "period_type": "document_availability",
                "procurement_object": "construction",
                "is_government_procurement": true,
                "document_availability_start_date_str": "2025-06-02",
                "document_availability_end_date_str": "2025-06-09"
            }))
            .await
            .unwrap();
        assert_eq!(out["overall_status"], "compliant");
        assert_eq!(out["announcement_period"]["actual_days"], 5);
    }
}
