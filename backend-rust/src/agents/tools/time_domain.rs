//! `time_domain` — 时间规则共享纯底层 domain。
//!
//! Phase 4B-1：定义事件类型、期间类型、日计数方式、期间计算约定、
//! 日历日计数器。本阶段仅实现 CalendarDays；WorkingDays 留待 4B-2。
//!
//! 本模块不依赖任何 Tool / Agent / Registry / CalendarProvider。
//! 所有类型均为纯数据模型。

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

// ─── DateEventType ─────────────────────────────────────────────

/// 时间线中的业务事件类型。
///
/// 不同事件对应不同法律期间的起止点。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DateEventType {
    /// 公告发布日期
    NoticePublished,
    /// 公告期限结束日
    NoticePublicationEnded,
    /// 文件开始提供日
    DocumentAvailabilityStarted,
    /// 文件停止提供日
    DocumentAvailabilityEnded,
    /// 招标/磋商/谈判/询价文件发出日
    DocumentIssued,
    /// 投标截止日
    BidDeadline,
    /// 首次响应截止日（磋商/谈判/询价）
    FirstResponseDeadline,
    /// 澄清/修改文件发出日
    ClarificationIssued,
    /// 单一来源公示开始日
    SingleSourcePublicityStarted,
    /// 单一来源公示结束日
    SingleSourcePublicityEnded,
}

// ─── PeriodType ────────────────────────────────────────────────

/// 法律期间类型。
///
/// 每个 PeriodType 对应一组法定的起止事件和最小天数要求。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PeriodType {
    /// 公告期限（NoticePublished → NoticePublicationEnded）
    NoticePublication,
    /// 文件提供期限（DocumentAvailabilityStarted → DocumentAvailabilityEnded）
    DocumentAvailability,
    /// 投标准备期——87号令公开/邀请招标（DocumentIssued → BidDeadline）
    BidPreparation,
    /// 响应准备期——214/74磋商/谈判/询价（DocumentIssued → FirstResponseDeadline）
    ResponsePreparation,
    /// 单一来源采购公示期（SingleSourcePublicityStarted → SingleSourcePublicityEnded）
    SingleSourcePreAcquisitionPublicity,
    /// 澄清/修改前置期（ClarificationIssued → BidDeadline 或 FirstResponseDeadline）
    ClarificationLeadTime,
}

// ─── DayCountType ──────────────────────────────────────────────

/// 天数计数类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DayCountType {
    /// 日历日——不排除任何日期
    CalendarDays,
    /// 工作日——排除周末和法定节假日（4B-2 实现）
    WorkingDays,
}

// ─── FinalDayAdjustment ────────────────────────────────────────

/// 最后一日调整规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinalDayAdjustment {
    /// 不调整
    None,
    /// 最后一日为法定休假日则顺延至下一个工作日（87号令第85条等）
    /// 4B-2 之前仅建模，不执行实际判断。
    ExtendPastLegalHoliday,
}

// ─── PeriodCountingConvention ──────────────────────────────────

/// 期间计算约定。
///
/// 政府采购通用默认：起算日不计入（start_included=false），
/// 截止日须达到（end_included=true）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeriodCountingConvention {
    /// 起算日是否计入第 1 天
    pub start_included: bool,
    /// 截止日是否须达到（即截止日本身是否为最后一天）
    pub end_included: bool,
    /// 最后一日调整规则
    pub final_day_adjustment: FinalDayAdjustment,
}

impl PeriodCountingConvention {
    /// 政府采购通用默认：start_excluded, end_included, no adjustment。
    pub const STANDARD: Self = Self {
        start_included: false,
        end_included: true,
        final_day_adjustment: FinalDayAdjustment::None,
    };
}

// ─── RuleApplicability ─────────────────────────────────────────

/// 规则的适用性。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleApplicability {
    /// 无条件法定要求
    HardLaw,
    /// 有条件适用（依赖 context 字段判定）
    Conditional,
}

// ─── TimeRule ──────────────────────────────────────────────────

/// 一条时间规则：指定期间类型、起止事件、最小天数、计数方式、依据来源。
///
/// 本阶段不构建完整的 RuleSet resolver。这里仅定义数据结构。
/// 实际规则查找由后续阶段实现。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRule {
    pub period_type: PeriodType,
    pub start_event: DateEventType,
    pub end_event: DateEventType,
    pub minimum_days: u32,
    pub day_count_type: DayCountType,
    pub counting: PeriodCountingConvention,
    pub rule_source: String,
    pub applicability: RuleApplicability,
}

// ─── TimeDomainError ───────────────────────────────────────────

/// 时间域错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeDomainError {
    /// 结束日期早于开始日期
    EndBeforeStart,
    /// 不支持的期间计算约定（当前计数器不支持该 convention）
    UnsupportedConvention,
    /// 日历数据不可用（年份不在支持范围内）
    CalendarUnavailable { year: i32 },
    /// 日期推进溢出（Chrono 范围边界）
    DateOverflow,
}

impl std::fmt::Display for TimeDomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TimeDomainError::EndBeforeStart => write!(f, "end date is before start date"),
            TimeDomainError::UnsupportedConvention => write!(f, "unsupported period counting convention"),
            TimeDomainError::CalendarUnavailable { year } => write!(f, "calendar data unavailable for year {}", year),
            TimeDomainError::DateOverflow => write!(f, "date overflow during iteration"),
        }
    }
}

impl std::error::Error for TimeDomainError {}

// ─── DateCounter trait ─────────────────────────────────────────

/// 日期计数器接口。
///
/// 根据给定的期间计算约定，计算两个日期之间的有效天数。
pub trait DateCounter {
    /// 计算从 `start` 到 `end` 的有效天数。
    ///
    /// # Errors
    /// - `EndBeforeStart`：`end < start`
    /// - `UnsupportedConvention`：当前计数器不支持给定的 `convention`
    fn count_days(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        convention: PeriodCountingConvention,
    ) -> Result<u32, TimeDomainError>;
}

// ─── CalendarDaysCounter ───────────────────────────────────────

/// 日历日计数器。
///
/// 仅支持日历日计算（DayCountType::CalendarDays）。
/// 当前仅接受 `PeriodCountingConvention::STANDARD`
/// （start_excluded, end_included）。
/// 其他 convention 返回 `UnsupportedConvention`。
pub struct CalendarDaysCounter;

impl DateCounter for CalendarDaysCounter {
    fn count_days(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        convention: PeriodCountingConvention,
    ) -> Result<u32, TimeDomainError> {
        if end < start {
            return Err(TimeDomainError::EndBeforeStart);
        }
        if convention != PeriodCountingConvention::STANDARD {
            return Err(TimeDomainError::UnsupportedConvention);
        }
        // 使用 chrono 自带的天数差
        // chrono day diff: end - start = 从 start 到 end 经过的天数
        let naive_diff = (end - start).num_days();
        // STANDARD: start excluded, end included.
        // 即从 start+1 到 end（含）。天数 = diff。
        // 例：6/1→6/21, diff=20, 结果=20。
        #[allow(clippy::cast_sign_loss)]
        Ok(naive_diff as u32)
    }
}

// ─── Threshold helper ──────────────────────────────────────────

/// 最小天数判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThresholdResult {
    /// 实际天数 ≥ 最小要求
    Pass,
    /// 实际天数 < 最小要求
    Fail,
}

/// 判断实际天数是否满足最小要求。
///
/// `actual >= minimum` → Pass，否则 → Fail。
pub fn evaluate_minimum(actual: u32, minimum: u32) -> ThresholdResult {
    if actual >= minimum {
        ThresholdResult::Pass
    } else {
        ThresholdResult::Fail
    }
}

// ════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn cnt(start: NaiveDate, end: NaiveDate) -> u32 {
        CalendarDaysCounter
            .count_days(start, end, PeriodCountingConvention::STANDARD)
            .unwrap()
    }

    // ── boundary: 19 / 20 / 21 ────────────────────────────────

    #[test]
    fn exact_20_days() {
        // 6/1 → 6/21, STANDARD: start excluded, end included = 20
        assert_eq!(cnt(date(2025, 6, 1), date(2025, 6, 21)), 20);
    }

    #[test]
    fn below_19_days() {
        assert_eq!(cnt(date(2025, 6, 1), date(2025, 6, 20)), 19);
    }

    #[test]
    fn above_21_days() {
        assert_eq!(cnt(date(2025, 6, 1), date(2025, 6, 22)), 21);
    }

    // ── start == end ──────────────────────────────────────────

    #[test]
    fn same_day_zero() {
        // start excluded, end included → 0
        assert_eq!(cnt(date(2025, 6, 1), date(2025, 6, 1)), 0);
    }

    // ── end < start ───────────────────────────────────────────

    #[test]
    fn reversed_dates_error() {
        let r = CalendarDaysCounter
            .count_days(date(2025, 6, 2), date(2025, 6, 1), PeriodCountingConvention::STANDARD);
        assert!(matches!(r, Err(TimeDomainError::EndBeforeStart)));
    }

    // ── cross-month ───────────────────────────────────────────

    #[test]
    fn cross_month_one_day() {
        assert_eq!(cnt(date(2025, 1, 31), date(2025, 2, 1)), 1);
    }

    // ── cross-year ────────────────────────────────────────────

    #[test]
    fn cross_year_one_day() {
        assert_eq!(cnt(date(2025, 12, 31), date(2026, 1, 1)), 1);
    }

    // ── leap year ─────────────────────────────────────────────

    #[test]
    fn leap_year_feb29() {
        assert_eq!(cnt(date(2024, 2, 28), date(2024, 2, 29)), 1);
    }

    #[test]
    fn leap_year_cross_feb() {
        assert_eq!(cnt(date(2024, 2, 28), date(2024, 3, 1)), 2);
    }

    // ── unsupported convention ────────────────────────────────

    #[test]
    fn non_standard_convention_unsupported() {
        let conv = PeriodCountingConvention {
            start_included: true,
            end_included: true,
            final_day_adjustment: FinalDayAdjustment::None,
        };
        let r = CalendarDaysCounter
            .count_days(date(2025, 6, 1), date(2025, 6, 21), conv);
        assert!(matches!(r, Err(TimeDomainError::UnsupportedConvention)));
    }

    // ── threshold ─────────────────────────────────────────────

    #[test]
    fn threshold_19_vs_20_fail() {
        assert_eq!(evaluate_minimum(19, 20), ThresholdResult::Fail);
    }

    #[test]
    fn threshold_20_vs_20_pass() {
        assert_eq!(evaluate_minimum(20, 20), ThresholdResult::Pass);
    }

    #[test]
    fn threshold_21_vs_20_pass() {
        assert_eq!(evaluate_minimum(21, 20), ThresholdResult::Pass);
    }

    #[test]
    fn threshold_9_vs_10_fail() {
        assert_eq!(evaluate_minimum(9, 10), ThresholdResult::Fail);
    }

    #[test]
    fn threshold_10_vs_10_pass() {
        assert_eq!(evaluate_minimum(10, 10), ThresholdResult::Pass);
    }

    #[test]
    fn threshold_11_vs_10_pass() {
        assert_eq!(evaluate_minimum(11, 10), ThresholdResult::Pass);
    }
}
