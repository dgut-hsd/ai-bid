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
use super::calendar::{CnCalendarProvider, WorkingDaysCounter};
use super::time_domain::{
    CalendarDaysCounter, DateCounter, DayCountType, PeriodCountingConvention, TimeDomainError,
};
use crate::agents::procurement_context::{
    self, ProcurementContext, ResolutionStatus, RuleSet,
};

// ─── 日期解析辅助函数 ──────────────────────────────────────────

/// 解析日期字符串，支持 "YYYY-MM-DD" 和 "YYYY/MM/DD" 两种格式。
/// 仅做格式解析，不做任何期间计算。
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

// ─── 法规规则表（单一来源，按 RuleSet 路由）────────────────────

/// 某 RuleSet 的准备期规则。返回 None 表示该 RuleSet 无准备期规则（NotApplicable）。
struct PrepRule {
    minimum_days: u32,
    day_count_type: DayCountType,
    day_unit: &'static str,
    citation: &'static str,
    full_text: &'static str,
}

/// 按 RuleSet 选择规则。法规路由由 ProcurementContext Resolver 决定，
/// 本表不做采购方式字符串匹配。
fn preparation_rule(rs: RuleSet) -> Option<PrepRule> {
    match rs {
        RuleSet::MofOrder87 => Some(PrepRule {
            minimum_days: 20,
            day_count_type: DayCountType::CalendarDays,
            day_unit: "日历日",
            citation: "《政府采购法》第35条",
            full_text: "货物和服务项目实行招标方式采购的，自招标文件开始发出之日起至投标人提交投标文件截止之日止，不得少于20日。",
        }),
        RuleSet::CompetitiveConsultation214 => Some(PrepRule {
            minimum_days: 10,
            day_count_type: DayCountType::CalendarDays,
            day_unit: "日历日",
            citation: "财库〔2014〕214号第10条",
            full_text: "从磋商文件发出之日起至供应商提交首次响应文件截止之日止不得少于10日。",
        }),
        RuleSet::MofOrder74Negotiation => Some(PrepRule {
            minimum_days: 3,
            day_count_type: DayCountType::WorkingDays,
            day_unit: "工作日",
            citation: "财政部令第74号第29条",
            full_text: "从谈判文件发出之日起至供应商提交首次响应文件截止之日止不得少于3个工作日。",
        }),
        RuleSet::MofOrder74Inquiry => Some(PrepRule {
            minimum_days: 3,
            day_count_type: DayCountType::WorkingDays,
            day_unit: "工作日",
            citation: "财政部令第74号第45条",
            full_text: "从询价通知书发出之日起至供应商提交响应文件截止之日止不得少于3个工作日。",
        }),
        RuleSet::MofOrder74SingleSource => None, // 单一来源无准备期规则
        RuleSet::ConstructionTendering => None,  // 工程招标属招标投标法体系，不在本工具范围
        RuleSet::Unknown => None,
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
///
/// ## 事件 Contract（4B-3A）
///
/// 本工具校验"采购文件发出 → 投标/首次响应截止"的准备期限。
/// 开始事件必须是采购文件发出日（`document_issued_date_str`），
/// 不是公告发布日期。结束事件按采购方式区分：
///
/// - 公开招标 / 邀请招标 → `bid_deadline_date_str`
/// - 竞争性磋商 / 竞争性谈判 / 询价 → `first_response_deadline_date_str`
///
/// `announcement_date_str` 为 legacy 字段，仅表示公告发布日期，
/// 不得作为文件发出日的替代值（4B-3B 起不再用于准备期起算）。
#[derive(Debug, Default, Deserialize)]
pub struct VerifyBidPreparationPeriodArgs {
    /// 采购方式
    pub procurement_method: String,
    /// 采购对象（可选）：goods / service / construction 或中文（货物/服务/工程）。
    /// 缺失时后续校验返回 uncertain；禁止推测。
    #[serde(default)]
    pub procurement_object: Option<String>,
    /// 是否政府采购（可选）。缺失时后续校验返回 uncertain；禁止默认 true。
    #[serde(default)]
    pub is_government_procurement: Option<bool>,
    /// [legacy] 公告发布日期。仅表示公告发布日，
    /// 不得作为采购文件发出日（document_issued_date_str）的替代值用于准备期起算。
    #[serde(default)]
    pub announcement_date_str: Option<String>,
    /// 采购文件发出日期（DocumentIssued）：
    /// 公开/邀请招标=招标文件发出日；竞争性磋商=磋商文件发出日；
    /// 竞争性谈判=谈判文件发出日；询价=询价通知书发出日。
    /// 原文无法确定时省略；禁止用公告发布日期替代。
    #[serde(default)]
    pub document_issued_date_str: Option<String>,
    /// 投标截止日期。仅适用于公开招标/邀请招标场景。
    /// 磋商/谈判/询价场景请使用 first_response_deadline_date_str。
    #[serde(default)]
    pub bid_deadline_date_str: Option<String>,
    /// 首次响应（响应）文件截止日期。
    /// 适用于竞争性磋商/竞争性谈判（首次响应截止）/询价（响应截止）。
    /// 原文无法确定时省略；禁止用 bid_deadline_date_str 替代。
    #[serde(default)]
    pub first_response_deadline_date_str: Option<String>,
}

// ─── 输出 ──────────────────────────────────────────────────────

/// 投标准备期校验返回结果。
#[derive(Debug, serde::Serialize)]
struct BidPreparationPeriodResult {
    /// 采购方式
    procurement_method: String,
    /// 合规判定
    status: PreparationStatus,
    /// 适用的规则体系
    rule_set: String,
    /// 期间类型
    period_type: String,
    /// 公告日期（legacy 字段回显）
    announcement_date: String,
    /// 投标截止日期（legacy 字段回显）
    bid_deadline_date: String,
    /// 实际准备天数
    actual_days: i64,
    /// 法定要求天数
    required_days: i64,
    /// 天数单位
    day_unit: String,
    /// 差额（负数 = 不足）
    shortage_days: i64,
    /// 法条依据（含完整条文）
    legal_basis: LegalBasisInfo,
    /// 风险等级（仅 violation 时非 none；不决定合规判定）
    risk_level: String,
    /// 违反后果说明
    violation_consequences: Option<String>,
    /// 日历不可用年份（WorkingDays 场景跨入未支持年份时）
    calendar_error: Option<String>,
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
    /// Context 缺失或无法解析 → 不猜
    Uncertain,
    /// 规则体系不适用本工具（单一来源/工程招标）
    NotApplicable,
    /// 输入非法（日期解析失败/日期倒置）
    InvalidInput,
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
/// 通过 ProcurementContext Resolver 确定 RuleSet，
/// 通过 CalendarDaysCounter / WorkingDaysCounter<CnCalendarProvider> 计算期间。
pub struct VerifyBidPreparationPeriodTool;

impl VerifyBidPreparationPeriodTool {
    /// 构建 Uncertain 结果（Context 缺失/Resolver 无法解析）。
    fn uncertain_result(args: &VerifyBidPreparationPeriodArgs, reason: &str, rule_set: &str) -> BidPreparationPeriodResult {
        BidPreparationPeriodResult {
            procurement_method: args.procurement_method.clone(),
            status: PreparationStatus::Uncertain,
            rule_set: rule_set.to_string(),
            period_type: String::new(),
            announcement_date: args.announcement_date_str.clone().unwrap_or_default(),
            bid_deadline_date: args.bid_deadline_date_str.clone().unwrap_or_default(),
            actual_days: 0,
            required_days: 0,
            day_unit: String::new(),
            shortage_days: 0,
            legal_basis: LegalBasisInfo { citation: String::new(), full_text: String::new() },
            risk_level: "none".to_string(),
            violation_consequences: None,
            calendar_error: None,
            suggestion: "补充缺失的 Context 后重新校验。".to_string(),
            detail: reason.to_string(),
        }
    }

    /// 构建 NotApplicable 结果。
    fn not_applicable_result(args: &VerifyBidPreparationPeriodArgs, rule_set: &str, reason: &str) -> BidPreparationPeriodResult {
        BidPreparationPeriodResult {
            procurement_method: args.procurement_method.clone(),
            status: PreparationStatus::NotApplicable,
            rule_set: rule_set.to_string(),
            period_type: String::new(),
            announcement_date: args.announcement_date_str.clone().unwrap_or_default(),
            bid_deadline_date: args.bid_deadline_date_str.clone().unwrap_or_default(),
            actual_days: 0,
            required_days: 0,
            day_unit: String::new(),
            shortage_days: 0,
            legal_basis: LegalBasisInfo { citation: String::new(), full_text: String::new() },
            risk_level: "none".to_string(),
            violation_consequences: None,
            calendar_error: None,
            suggestion: String::new(),
            detail: reason.to_string(),
        }
    }

    /// 构建 InvalidInput 结果。
    fn invalid_input_result(args: &VerifyBidPreparationPeriodArgs, rule_set: &str, reason: &str) -> BidPreparationPeriodResult {
        BidPreparationPeriodResult {
            procurement_method: args.procurement_method.clone(),
            status: PreparationStatus::InvalidInput,
            rule_set: rule_set.to_string(),
            period_type: String::new(),
            announcement_date: args.announcement_date_str.clone().unwrap_or_default(),
            bid_deadline_date: args.bid_deadline_date_str.clone().unwrap_or_default(),
            actual_days: 0,
            required_days: 0,
            day_unit: String::new(),
            shortage_days: 0,
            legal_basis: LegalBasisInfo { citation: String::new(), full_text: String::new() },
            risk_level: "none".to_string(),
            violation_consequences: None,
            calendar_error: None,
            suggestion: "请检查输入的日期是否正确。".to_string(),
            detail: reason.to_string(),
        }
    }

    /// 核心校验逻辑。
    ///
    /// 流程：Args → ProcurementContext → resolve_rule_set → 选择 RuleSet →
    /// 选择 DayCountType → CalendarDaysCounter / WorkingDaysCounter → 阈值判定。
    fn verify(args: &VerifyBidPreparationPeriodArgs) -> Result<BidPreparationPeriodResult> {
        // ── 1. Resolver 所需 Context（缺失 → Uncertain，不猜）──
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

        // ── 2. Resolver 状态检查 ──
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

        // ── 3. 规则选择（RuleSet → minimum / counter）──
        let rule = match preparation_rule(rs) {
            Some(r) => r,
            None => {
                let reason = match rs {
                    RuleSet::MofOrder74SingleSource => "单一来源采购无投标准备期规则（本工具不适用）。",
                    RuleSet::ConstructionTendering => "工程招标适用招标投标法体系，本工具不覆盖其期限规则。",
                    _ => "该规则体系无投标准备期规则。",
                };
                return Ok(Self::not_applicable_result(args, &rs.to_string(), reason));
            }
        };

        // ── 4. 开始事件：必须为采购文件发出日 ──
        let start_str = match args.document_issued_date_str.as_deref() {
            Some(s) if !s.trim().is_empty() => s,
            _ => return Ok(Self::uncertain_result(args, "missing document_issued_date_str：准备期起点必须是采购文件发出日，禁止用公告日期替代。", &rs.to_string())),
        };
        let start = match parse_date(start_str) {
            Ok(d) => d,
            Err(e) => return Ok(Self::invalid_input_result(args, &rs.to_string(), &format!("document_issued_date_str 解析失败: {}", e))),
        };

        // ── 5. 结束事件：按 RuleSet 选择 ──
        let (end_str, period_type) = match rs {
            RuleSet::MofOrder87 => match args.bid_deadline_date_str.as_deref() {
                Some(s) if !s.trim().is_empty() => (s, "BidPreparation"),
                _ => return Ok(Self::uncertain_result(args, "missing bid_deadline_date_str：招标场景必须提供投标截止日期。", &rs.to_string())),
            },
            RuleSet::CompetitiveConsultation214
            | RuleSet::MofOrder74Negotiation
            | RuleSet::MofOrder74Inquiry => match args.first_response_deadline_date_str.as_deref() {
                Some(s) if !s.trim().is_empty() => (s, "ResponsePreparation"),
                _ => return Ok(Self::uncertain_result(args, "missing first_response_deadline_date_str：磋商/谈判/询价场景必须提供首次响应（响应）截止日期，禁止用 bid_deadline 替代。", &rs.to_string())),
            },
            _ => unreachable!("preparation_rule None 已在上方处理"),
        };
        let end = match parse_date(end_str) {
            Ok(d) => d,
            Err(e) => return Ok(Self::invalid_input_result(args, &rs.to_string(), &format!("截止日期解析失败: {}", e))),
        };

        // ── 6. 日期顺序：end < start → InvalidInput ──
        if end < start {
            return Ok(Self::invalid_input_result(args, &rs.to_string(), "截止日期早于采购文件发出日期（end < start）。"));
        }

        // ── 7. 天数计算：走共享 Counter ──
        let counter_result: Result<u32, TimeDomainError> = match rule.day_count_type {
            DayCountType::CalendarDays => CalendarDaysCounter
                .count_days(start, end, PeriodCountingConvention::STANDARD),
            DayCountType::WorkingDays => WorkingDaysCounter { provider: CnCalendarProvider::new() }
                .count_days(start, end, PeriodCountingConvention::STANDARD),
        };

        let actual_days = match counter_result {
            Ok(d) => d,
            Err(TimeDomainError::CalendarUnavailable { year }) => {
                return Ok(BidPreparationPeriodResult {
                    procurement_method: args.procurement_method.clone(),
                    status: PreparationStatus::Uncertain,
                    rule_set: rs.to_string(),
                    period_type: period_type.to_string(),
                    announcement_date: args.announcement_date_str.clone().unwrap_or_default(),
                    bid_deadline_date: args.bid_deadline_date_str.clone().unwrap_or_default(),
                    actual_days: 0,
                    required_days: rule.minimum_days as i64,
                    day_unit: rule.day_unit.to_string(),
                    shortage_days: 0,
                    legal_basis: LegalBasisInfo { citation: rule.citation.to_string(), full_text: rule.full_text.to_string() },
                    risk_level: "none".to_string(),
                    violation_consequences: None,
                    calendar_error: Some(format!("calendar unavailable for year {}", year)),
                    suggestion: "补充该年份的节假日数据或改在支持的年份内校验。".to_string(),
                    detail: format!("WorkingDays 计算需要日历数据，年份 {} 不在支持范围（2024-2026）内，无法判定合规性。", year),
                });
            }
            Err(e) => return Err(anyhow!("期间计算失败: {}", e)),
        };

        // ── 8. 阈值判定（Hard Law 独立判定）──
        let required_days = rule.minimum_days;
        let (status, risk_level, violation_cons) = if actual_days >= required_days {
            (PreparationStatus::Compliant, "none".to_string(), None)
        } else {
            let (rl, _rd) = assess_risk_level(actual_days as i64, required_days as i64);
            (
                PreparationStatus::Violation,
                rl.to_string(),
                Some(violation_consequences().to_string()),
            )
        };

        let shortage = if actual_days as i64 >= required_days as i64 { 0 } else { required_days as i64 - actual_days as i64 };

        let suggestion = if matches!(status, PreparationStatus::Compliant) {
            format!("准备期 {} {} 符合法定要求（≥ {} {}），无需整改。", actual_days, rule.day_unit, required_days, rule.day_unit)
        } else {
            format!(
                "准备期不足！建议发布更正公告顺延截止日期。法定要求 ≥ {} {}，当前仅 {} {}。",
                required_days, rule.day_unit, actual_days, rule.day_unit
            )
        };

        let detail = format!(
            "采购文件发出 {} 至 {} {} 共 {} {}，满足 {} 法定要求 ≥ {} {}。风险等级：{}。",
            start_str, end_str, period_type, actual_days, rule.day_unit,
            args.procurement_method, required_days, rule.day_unit, risk_level
        );

        Ok(BidPreparationPeriodResult {
            procurement_method: args.procurement_method.clone(),
            status,
            rule_set: rs.to_string(),
            period_type: period_type.to_string(),
            announcement_date: args.announcement_date_str.clone().unwrap_or_default(),
            bid_deadline_date: args.bid_deadline_date_str.clone().unwrap_or_default(),
            actual_days: actual_days as i64,
            required_days: required_days as i64,
            day_unit: rule.day_unit.to_string(),
            shortage_days: shortage,
            legal_basis: LegalBasisInfo { citation: rule.citation.to_string(), full_text: rule.full_text.to_string() },
            risk_level,
            violation_consequences: violation_cons,
            calendar_error: None,
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
                "description": "【使用场景】校验采购文件发出日至投标/首次响应截止日的准备期限（等标期）是否满足法定最低要求——\
                    这是政府采购投诉的第一高发事由。\
                    ① 公开招标/邀请招标 ≥ 20 日历日（招标文件发出 → 投标截止）；\
                    ② 竞争性磋商 ≥ 10 日历日（磋商文件发出 → 首次响应截止）；\
                    ③ 竞争性谈判 ≥ 3 工作日（谈判文件发出 → 首次响应截止）；\
                    ④ 询价 ≥ 3 工作日（询价通知书发出 → 响应截止）。\
                    【关键字段】开始事件必须是采购文件发出日 document_issued_date_str，\
                    不是公告发布日期。结束事件：招标场景用 bid_deadline_date_str，\
                    磋商/谈判/询价场景用 first_response_deadline_date_str。\
                    原文/上下文无法确定字段时省略，禁止编造；缺失关键 Context 时工具返回 uncertain。\
                    本工具提供风险等级评估和违反后果的详细说明。\
                    【不使用场景】不校验公告期限/文件发售期（用 verify_announcement_period）；\
                    不校验保证金相关事项（用 verify_bid_deposit）。\
                    日期支持 YYYY-MM-DD 和 YYYY/MM/DD 两种格式。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "procurement_method": {
                            "type": "string",
                            "enum": ["公开招标", "邀请招标", "竞争性磋商", "竞争性谈判", "询价"],
                            "description": "采购方式。"
                        },
                        "procurement_object": {
                            "type": "string",
                            "enum": ["goods", "service", "construction"],
                            "description": "采购对象（可选）：goods/service/construction 或中文（货物/服务/工程）。缺失时后续校验返回 uncertain；禁止推测。"
                        },
                        "is_government_procurement": {
                            "type": "boolean",
                            "description": "是否政府采购（可选）。缺失时后续校验返回 uncertain；禁止默认 true。"
                        },
                        "announcement_date_str": {
                            "type": "string",
                            "description": "[legacy] 公告发布日期。仅表示公告发布日，不得作为采购文件发出日的替代值用于准备期起算。"
                        },
                        "document_issued_date_str": {
                            "type": "string",
                            "description": "采购文件发出日期：公开/邀请招标=招标文件发出日；竞争性磋商=磋商文件发出日；竞争性谈判=谈判文件发出日；询价=询价通知书发出日。原文无法确定时省略；禁止用公告发布日期替代。"
                        },
                        "bid_deadline_date_str": {
                            "type": "string",
                            "description": "投标截止日期。仅适用于公开招标/邀请招标场景。磋商/谈判/询价场景请使用 first_response_deadline_date_str。"
                        },
                        "first_response_deadline_date_str": {
                            "type": "string",
                            "description": "首次响应（响应）文件截止日期：磋商/谈判=首次响应截止日；询价=响应截止日。原文无法确定时省略；禁止用 bid_deadline_date_str 替代。"
                        }
                    },
                    "required": ["procurement_method"]
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

    // ── 构造 helper：OpenTender 20 CalendarDays ─────────────────

    fn open_tender_args(doc_issued: &str, bid_deadline: &str) -> VerifyBidPreparationPeriodArgs {
        VerifyBidPreparationPeriodArgs {
            procurement_method: "公开招标".to_string(),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            announcement_date_str: Some("2025-03-01".to_string()),
            document_issued_date_str: Some(doc_issued.to_string()),
            bid_deadline_date_str: Some(bid_deadline.to_string()),
            ..Default::default()
        }
    }

    fn verify_status(args: &VerifyBidPreparationPeriodArgs) -> PreparationStatus {
        VerifyBidPreparationPeriodTool::verify(args).unwrap().status
    }

    // ── OpenTender boundary: 19/20/21 CalendarDays ─────────────

    #[test]
    fn open_tender_19_days_violation() {
        // 2025-03-01 → 2025-03-20, start excluded/end included = 19 < 20
        let r = VerifyBidPreparationPeriodTool::verify(&open_tender_args("2025-03-01", "2025-03-20")).unwrap();
        assert!(matches!(r.status, PreparationStatus::Violation));
        assert_eq!(r.actual_days, 19);
        assert_eq!(r.required_days, 20);
        assert_eq!(r.rule_set, "MofOrder87");
        assert!(r.legal_basis.citation.contains("第35条"));
    }

    #[test]
    fn open_tender_20_days_compliant() {
        let r = VerifyBidPreparationPeriodTool::verify(&open_tender_args("2025-03-01", "2025-03-21")).unwrap();
        assert!(matches!(r.status, PreparationStatus::Compliant));
        assert_eq!(r.actual_days, 20);
    }

    #[test]
    fn open_tender_21_days_compliant() {
        let r = VerifyBidPreparationPeriodTool::verify(&open_tender_args("2025-03-01", "2025-03-22")).unwrap();
        assert!(matches!(r.status, PreparationStatus::Compliant));
        assert_eq!(r.actual_days, 21);
    }

    #[test]
    fn open_tender_25_days_compliant() {
        let r = VerifyBidPreparationPeriodTool::verify(&open_tender_args("2025-03-01", "2025-03-26")).unwrap();
        assert!(matches!(r.status, PreparationStatus::Compliant));
        assert_eq!(r.actual_days, 25);
        assert_eq!(r.risk_level, "none");
        assert!(r.violation_consequences.is_none());
    }

    #[test]
    fn open_tender_15_days_violation_medium_risk() {
        // 15/20 = 75% → medium（risk 为 metadata，不决定 Hard Law status）
        let r = VerifyBidPreparationPeriodTool::verify(&open_tender_args("2025-03-01", "2025-03-16")).unwrap();
        assert!(matches!(r.status, PreparationStatus::Violation));
        assert_eq!(r.actual_days, 15);
        assert_eq!(r.shortage_days, 5);
        assert_eq!(r.risk_level, "medium");
        assert!(r.violation_consequences.is_some());
    }

    // ── InvitedTender boundary: 19/20（必须经 Resolver 进 MofOrder87）─

    #[test]
    fn invited_tender_19_days_violation() {
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "邀请招标".to_string(),
            procurement_object: Some("service".to_string()),
            is_government_procurement: Some(true),
            announcement_date_str: Some("2025-03-01".to_string()),
            document_issued_date_str: Some("2025-03-01".to_string()),
            bid_deadline_date_str: Some("2025-03-20".to_string()),
            ..Default::default()
        };
        let r = VerifyBidPreparationPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.status, PreparationStatus::Violation));
        assert_eq!(r.actual_days, 19);
        assert_eq!(r.rule_set, "MofOrder87");
    }

    #[test]
    fn invited_tender_20_days_compliant() {
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "邀请招标".to_string(),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            announcement_date_str: Some("2025-03-01".to_string()),
            document_issued_date_str: Some("2025-03-01".to_string()),
            bid_deadline_date_str: Some("2025-03-21".to_string()),
            ..Default::default()
        };
        let r = VerifyBidPreparationPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.status, PreparationStatus::Compliant));
        assert_eq!(r.actual_days, 20);
        assert_eq!(r.rule_set, "MofOrder87");
    }

    // ── Consultation boundary: 9/10/11 CalendarDays ────────────

    fn consultation_args(doc_issued: &str, resp_deadline: &str, object: &str) -> VerifyBidPreparationPeriodArgs {
        VerifyBidPreparationPeriodArgs {
            procurement_method: "竞争性磋商".to_string(),
            procurement_object: Some(object.to_string()),
            is_government_procurement: Some(true),
            announcement_date_str: Some("2025-06-01".to_string()),
            document_issued_date_str: Some(doc_issued.to_string()),
            bid_deadline_date_str: None,
            first_response_deadline_date_str: Some(resp_deadline.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn consultation_9_days_violation() {
        let r = VerifyBidPreparationPeriodTool::verify(&consultation_args("2025-06-01", "2025-06-10", "goods")).unwrap();
        assert!(matches!(r.status, PreparationStatus::Violation));
        assert_eq!(r.actual_days, 9);
        assert_eq!(r.required_days, 10);
        assert_eq!(r.rule_set, "CompetitiveConsultation214");
        assert!(r.legal_basis.citation.contains("214"));
    }

    #[test]
    fn consultation_10_days_compliant() {
        let r = VerifyBidPreparationPeriodTool::verify(&consultation_args("2025-06-01", "2025-06-11", "goods")).unwrap();
        assert!(matches!(r.status, PreparationStatus::Compliant));
        assert_eq!(r.actual_days, 10);
    }

    #[test]
    fn consultation_11_days_compliant() {
        let r = VerifyBidPreparationPeriodTool::verify(&consultation_args("2025-06-01", "2025-06-12", "service")).unwrap();
        assert!(matches!(r.status, PreparationStatus::Compliant));
        assert_eq!(r.actual_days, 11);
    }

    #[test]
    fn consultation_construction_resolver_routes_214() {
        // Resolver 允许 construction 磋商 → 214；不得套用评分 Tool 的 goods/service 限制
        let r = VerifyBidPreparationPeriodTool::verify(&consultation_args("2025-06-01", "2025-06-11", "construction")).unwrap();
        assert_eq!(r.rule_set, "CompetitiveConsultation214");
        assert!(matches!(r.status, PreparationStatus::Compliant));
    }

    // ── Negotiation: WorkingDays 3（证明非日历日）──────────────

    fn negotiation_args(doc_issued: &str, resp_deadline: &str) -> VerifyBidPreparationPeriodArgs {
        VerifyBidPreparationPeriodArgs {
            procurement_method: "竞争性谈判".to_string(),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            announcement_date_str: Some("2025-06-13".to_string()),
            document_issued_date_str: Some(doc_issued.to_string()),
            bid_deadline_date_str: None,
            first_response_deadline_date_str: Some(resp_deadline.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn negotiation_2_working_days_violation() {
        // Fri 6/13 → Tue 6/17: Sat/Sun skip, Mon 6/16 + Tue 6/17 = 2 < 3
        // 日历差 = 4 > 3，若用日历日会误判 compliant
        let r = VerifyBidPreparationPeriodTool::verify(&negotiation_args("2025-06-13", "2025-06-17")).unwrap();
        assert!(matches!(r.status, PreparationStatus::Violation));
        assert_eq!(r.actual_days, 2);
        assert_eq!(r.required_days, 3);
        assert_eq!(r.day_unit, "工作日");
        assert_eq!(r.rule_set, "MofOrder74Negotiation");
        assert!(r.legal_basis.citation.contains("第29条"));
    }

    #[test]
    fn negotiation_3_working_days_compliant() {
        // Fri 6/13 → Wed 6/18: 6/16 + 6/17 + 6/18 = 3（日历差 5）
        let r = VerifyBidPreparationPeriodTool::verify(&negotiation_args("2025-06-13", "2025-06-18")).unwrap();
        assert!(matches!(r.status, PreparationStatus::Compliant));
        assert_eq!(r.actual_days, 3);
    }

    #[test]
    fn negotiation_4_working_days_compliant() {
        // Fri 6/13 → Thu 6/19: 4 个工作日
        let r = VerifyBidPreparationPeriodTool::verify(&negotiation_args("2025-06-13", "2025-06-19")).unwrap();
        assert!(matches!(r.status, PreparationStatus::Compliant));
        assert_eq!(r.actual_days, 4);
    }

    // ── Inquiry: WorkingDays 3 + makeup workday 2024-02-18 ─────

    fn inquiry_args(doc_issued: &str, resp_deadline: &str) -> VerifyBidPreparationPeriodArgs {
        VerifyBidPreparationPeriodArgs {
            procurement_method: "询价".to_string(),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            announcement_date_str: Some("2024-02-16".to_string()),
            document_issued_date_str: Some(doc_issued.to_string()),
            bid_deadline_date_str: None,
            first_response_deadline_date_str: Some(resp_deadline.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn inquiry_makeup_sunday_counts() {
        // 2024-02-16(Fri) → 2024-02-20(Tue): 2/17 春节假日 skip, 2/18(Sun,调休) count,
        // 2/19(Mon) count, 2/20(Tue) count = 3 → compliant。
        // 若 2/18 按 weekend-only 处理 → 只有 2 个工作日 → violation。此测试证明 makeup 计入。
        let r = VerifyBidPreparationPeriodTool::verify(&inquiry_args("2024-02-16", "2024-02-20")).unwrap();
        assert!(matches!(r.status, PreparationStatus::Compliant));
        assert_eq!(r.actual_days, 3);
        assert_eq!(r.rule_set, "MofOrder74Inquiry");
        assert!(r.legal_basis.citation.contains("第45条"));
    }

    #[test]
    fn inquiry_2_working_days_violation() {
        // 2024-02-16 → 2024-02-19: 2/17 skip, 2/18 count, 2/19 count = 2 < 3
        let r = VerifyBidPreparationPeriodTool::verify(&inquiry_args("2024-02-16", "2024-02-19")).unwrap();
        assert!(matches!(r.status, PreparationStatus::Violation));
        assert_eq!(r.actual_days, 2);
    }

    #[test]
    fn inquiry_4_working_days_compliant() {
        let r = VerifyBidPreparationPeriodTool::verify(&inquiry_args("2024-02-16", "2024-02-21")).unwrap();
        assert!(matches!(r.status, PreparationStatus::Compliant));
        assert_eq!(r.actual_days, 4);
    }

    // ── SingleSource / ConstructionTendering → NotApplicable ──

    #[test]
    fn single_source_not_applicable() {
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "单一来源".to_string(),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            announcement_date_str: Some("2025-03-01".to_string()),
            document_issued_date_str: Some("2025-03-01".to_string()),
            bid_deadline_date_str: Some("2025-03-25".to_string()),
            ..Default::default()
        };
        let r = VerifyBidPreparationPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.status, PreparationStatus::NotApplicable));
        assert_eq!(r.rule_set, "MofOrder74SingleSource");
    }

    #[test]
    fn construction_tendering_not_applicable() {
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "公开招标".to_string(),
            procurement_object: Some("construction".to_string()),
            is_government_procurement: Some(true),
            announcement_date_str: Some("2025-03-01".to_string()),
            document_issued_date_str: Some("2025-03-01".to_string()),
            bid_deadline_date_str: Some("2025-03-25".to_string()),
            ..Default::default()
        };
        let r = VerifyBidPreparationPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.status, PreparationStatus::NotApplicable));
        assert_eq!(r.rule_set, "ConstructionTendering");
    }

    // ── Missing Context → Uncertain ────────────────────────────

    #[test]
    fn missing_procurement_object_uncertain() {
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "公开招标".to_string(),
            procurement_object: None,
            is_government_procurement: Some(true),
            announcement_date_str: Some("2025-03-01".to_string()),
            document_issued_date_str: Some("2025-03-01".to_string()),
            bid_deadline_date_str: Some("2025-03-21".to_string()),
            ..Default::default()
        };
        assert!(matches!(verify_status(&args), PreparationStatus::Uncertain));
    }

    #[test]
    fn missing_government_flag_uncertain() {
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "公开招标".to_string(),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: None,
            announcement_date_str: Some("2025-03-01".to_string()),
            document_issued_date_str: Some("2025-03-01".to_string()),
            bid_deadline_date_str: Some("2025-03-21".to_string()),
            ..Default::default()
        };
        assert!(matches!(verify_status(&args), PreparationStatus::Uncertain));
    }

    #[test]
    fn government_false_uncertain() {
        // gov=false → Resolver 返回 InsufficientContext（不套 MofOrder87 规则）
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "公开招标".to_string(),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(false),
            announcement_date_str: Some("2025-03-01".to_string()),
            document_issued_date_str: Some("2025-03-01".to_string()),
            bid_deadline_date_str: Some("2025-03-21".to_string()),
            ..Default::default()
        };
        assert!(matches!(verify_status(&args), PreparationStatus::Uncertain));
    }

    // ── Legacy field regression（4B-3A 关键 Contract）──────────

    #[test]
    fn legacy_announcement_only_uncertain() {
        // 只有 announcement_date_str，无 document_issued_date_str → Uncertain
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "公开招标".to_string(),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            announcement_date_str: Some("2025-03-01".to_string()),
            document_issued_date_str: None,
            bid_deadline_date_str: Some("2025-03-21".to_string()),
            ..Default::default()
        };
        assert!(matches!(verify_status(&args), PreparationStatus::Uncertain));
    }

    #[test]
    fn consultation_bid_deadline_only_uncertain() {
        // 磋商只有 bid_deadline_date_str，无 first_response_deadline_date_str → Uncertain
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "竞争性磋商".to_string(),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            announcement_date_str: Some("2025-06-01".to_string()),
            document_issued_date_str: Some("2025-06-01".to_string()),
            bid_deadline_date_str: Some("2025-06-12".to_string()),
            first_response_deadline_date_str: None,
            ..Default::default()
        };
        assert!(matches!(verify_status(&args), PreparationStatus::Uncertain));
    }

    #[test]
    fn open_tender_missing_bid_deadline_uncertain() {
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "公开招标".to_string(),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            announcement_date_str: Some("2025-03-01".to_string()),
            document_issued_date_str: Some("2025-03-01".to_string()),
            bid_deadline_date_str: None,
            ..Default::default()
        };
        assert!(matches!(verify_status(&args), PreparationStatus::Uncertain));
    }

    // ── Invalid dates ──────────────────────────────────────────

    #[test]
    fn end_before_start_invalid_input() {
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "公开招标".to_string(),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            announcement_date_str: Some("2025-03-10".to_string()),
            document_issued_date_str: Some("2025-03-10".to_string()),
            bid_deadline_date_str: Some("2025-03-01".to_string()),
            ..Default::default()
        };
        assert!(matches!(verify_status(&args), PreparationStatus::InvalidInput));
    }

    #[test]
    fn start_equals_end_zero_days_violation() {
        // start == end → actual 0 < 20 → Violation
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "公开招标".to_string(),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            announcement_date_str: Some("2025-03-01".to_string()),
            document_issued_date_str: Some("2025-03-01".to_string()),
            bid_deadline_date_str: Some("2025-03-01".to_string()),
            ..Default::default()
        };
        let r = VerifyBidPreparationPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.status, PreparationStatus::Violation));
        assert_eq!(r.actual_days, 0);
    }

    #[test]
    fn unparseable_date_invalid_input() {
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "公开招标".to_string(),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            announcement_date_str: Some("2025-03-01".to_string()),
            document_issued_date_str: Some("not-a-date".to_string()),
            bid_deadline_date_str: Some("2025-03-21".to_string()),
            ..Default::default()
        };
        assert!(matches!(verify_status(&args), PreparationStatus::InvalidInput));
    }

    // ── Unsupported calendar year（WorkingDays 场景）────────────

    #[test]
    fn negotiation_2027_calendar_unavailable_uncertain() {
        // WorkingDays 跨入 2027 → CalendarUnavailable { year: 2027 } → Uncertain + calendar_error
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "竞争性谈判".to_string(),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            announcement_date_str: Some("2026-12-30".to_string()),
            document_issued_date_str: Some("2026-12-30".to_string()),
            bid_deadline_date_str: None,
            first_response_deadline_date_str: Some("2027-01-04".to_string()),
            ..Default::default()
        };
        let r = VerifyBidPreparationPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.status, PreparationStatus::Uncertain));
        let ce = r.calendar_error.expect("必须保留 calendar_error");
        assert!(ce.contains("2027"), "calendar_error 必须保留年份: {}", ce);
    }

    #[test]
    fn calendar_days_2027_still_computes() {
        // CalendarDays 规则不受 2027 影响：open tender 跨年仍正常计算
        let args = VerifyBidPreparationPeriodArgs {
            procurement_method: "公开招标".to_string(),
            procurement_object: Some("goods".to_string()),
            is_government_procurement: Some(true),
            announcement_date_str: Some("2026-12-30".to_string()),
            document_issued_date_str: Some("2026-12-30".to_string()),
            bid_deadline_date_str: Some("2027-01-19".to_string()),
            ..Default::default()
        };
        // 12/30 → 1/19: 12/31 + 1/1..1/19 = 20 天 → compliant
        let r = VerifyBidPreparationPeriodTool::verify(&args).unwrap();
        assert!(matches!(r.status, PreparationStatus::Compliant));
        assert_eq!(r.actual_days, 20);
    }

    // ── 4B-3A Contract 测试：Args serde ────────────────────────

    #[test]
    fn args_accepts_new_event_context_fields() {
        let args: VerifyBidPreparationPeriodArgs = serde_json::from_value(serde_json::json!({
            "procurement_method": "竞争性磋商",
            "procurement_object": "goods",
            "is_government_procurement": true,
            "announcement_date_str": "2025-06-01",
            "document_issued_date_str": "2025-06-01",
            "bid_deadline_date_str": "2025-06-12",
            "first_response_deadline_date_str": "2025-06-12"
        }))
        .unwrap();
        assert_eq!(args.procurement_object.as_deref(), Some("goods"));
        assert_eq!(args.is_government_procurement, Some(true));
        assert_eq!(args.document_issued_date_str.as_deref(), Some("2025-06-01"));
        assert_eq!(args.first_response_deadline_date_str.as_deref(), Some("2025-06-12"));
    }

    #[test]
    fn args_new_fields_optional_and_legacy_parses() {
        let args: VerifyBidPreparationPeriodArgs = serde_json::from_value(serde_json::json!({
            "procurement_method": "公开招标",
            "announcement_date_str": "2025-03-01",
            "bid_deadline_date_str": "2025-03-21"
        }))
        .unwrap();
        assert!(args.procurement_object.is_none());
        assert!(args.is_government_procurement.is_none());
        assert!(args.document_issued_date_str.is_none());
        assert!(args.first_response_deadline_date_str.is_none());
    }

    // ── Real Consumer Integration：JSON → execute() ────────────

    #[tokio::test]
    async fn integration_execute_open_tender_violation() {
        let tool = VerifyBidPreparationPeriodTool;
        let out = tool
            .execute(serde_json::json!({
                "procurement_method": "公开招标",
                "procurement_object": "goods",
                "is_government_procurement": true,
                "announcement_date_str": "2025-03-01",
                "document_issued_date_str": "2025-03-01",
                "bid_deadline_date_str": "2025-03-20"
            }))
            .await
            .unwrap();
        assert_eq!(out["status"], "violation");
        assert_eq!(out["required_days"], 20);
        assert_eq!(out["actual_days"], 19);
        assert_eq!(out["rule_set"], "MofOrder87");
    }

    #[tokio::test]
    async fn integration_execute_consultation_compliant() {
        let tool = VerifyBidPreparationPeriodTool;
        let out = tool
            .execute(serde_json::json!({
                "procurement_method": "竞争性磋商",
                "procurement_object": "goods",
                "is_government_procurement": true,
                "announcement_date_str": "2025-06-01",
                "document_issued_date_str": "2025-06-01",
                "first_response_deadline_date_str": "2025-06-11"
            }))
            .await
            .unwrap();
        assert_eq!(out["status"], "compliant");
        assert_eq!(out["actual_days"], 10);
        assert_eq!(out["rule_set"], "CompetitiveConsultation214");
    }

    #[tokio::test]
    async fn integration_execute_missing_method_is_serde_error() {
        // procurement_method 在 schema 为 required → 缺字段在 serde 层失败
        let tool = VerifyBidPreparationPeriodTool;
        let r = tool
            .execute(serde_json::json!({
                "procurement_object": "goods",
                "is_government_procurement": true,
                "announcement_date_str": "2025-03-01",
                "document_issued_date_str": "2025-03-01",
                "bid_deadline_date_str": "2025-03-21"
            }))
            .await;
        assert!(r.is_err(), "missing procurement_method 必须在 serde 层报错（schema required）");
    }
}
