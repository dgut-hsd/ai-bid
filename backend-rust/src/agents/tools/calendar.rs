//! `calendar` — 政府采购工作日 Calendar Provider。
//!
//! Phase 4B-2：提供 2024/2025/2026 官方放假/调休/法定节假日数据，
//! 实现 WorkingDaysCounter + FinalDayAdjustment。
//!
//! 语义分离：
//! - `scheduled_days_off`：国办年度放假通知中实际休息的日期（用于工作日计算）
//! - `statutory_holidays`：《全国年节及纪念日放假办法》法定节假日（用于 ExtendPastLegalHoliday）
//! - `makeup_workdays`：调休上班的周末
//!
//! 法定假日来源：
//! - 2024：《全国年节及纪念日放假办法》(2013修订) — 11天
//! - 2025/2026：国务院令第795号 (2024-11-10公布，2025-01-01施行) — 13天

use chrono::{Datelike, NaiveDate, Weekday};
use std::collections::{HashMap, HashSet};

use super::time_domain::{
    DateCounter, FinalDayAdjustment, PeriodCountingConvention, TimeDomainError,
};

// ════════════════════════════════════════════════════════════════
// CalendarYear
// ════════════════════════════════════════════════════════════════

pub struct CalendarYear {
    pub year: i32,
    /// 国办通知中因节假日安排而休息的日期（含调休相连的周末）
    pub scheduled_days_off: HashSet<NaiveDate>,
    /// 《全国年节及纪念日放假办法》规定的法定节假日
    pub statutory_holidays: HashSet<NaiveDate>,
    /// 调休上班的周末
    pub makeup_workdays: HashSet<NaiveDate>,
    pub schedule_source: &'static str,
    pub statutory_source: &'static str,
}

// ════════════════════════════════════════════════════════════════
// CalendarDayKind / CalendarError
// ════════════════════════════════════════════════════════════════

/// 某日的完整日历类别。
/// 优先级（由 provider 保证）：MakeupWorkday > StatutoryHoliday > ScheduledDayOff > Weekend > RegularWorkday。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarDayKind {
    /// 普通工作日（周一至周五）
    RegularWorkday,
    /// 普通周六/周日（非调休，非放假安排日期）
    Weekend,
    /// 国办年度通知安排的休息日（非调休周末，非法定假日）
    ScheduledDayOff,
    /// 《全国年节及纪念日放假办法》规定的法定节假日
    StatutoryHoliday,
    /// 调休上班的周末
    MakeupWorkday,
}

impl CalendarDayKind {
    /// 是否计为工作日（仅 RegularWorkday 和 MakeupWorkday）。
    pub fn is_workday(self) -> bool {
        matches!(self, CalendarDayKind::RegularWorkday | CalendarDayKind::MakeupWorkday)
    }

    /// 是否为法定节假日。
    pub fn is_statutory_holiday(self) -> bool {
        matches!(self, CalendarDayKind::StatutoryHoliday)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalendarError {
    UnsupportedYear(i32),
}

impl std::fmt::Display for CalendarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CalendarError::UnsupportedYear(y) => write!(f, "unsupported year: {}", y),
        }
    }
}

impl std::error::Error for CalendarError {}

// ════════════════════════════════════════════════════════════════
// CalendarProvider trait
// ════════════════════════════════════════════════════════════════

pub trait CalendarProvider {
    fn day_kind(&self, date: NaiveDate) -> Result<CalendarDayKind, CalendarError>;

    fn is_workday(&self, date: NaiveDate) -> Result<bool, CalendarError> {
        self.day_kind(date).map(|k| k.is_workday())
    }

    fn is_statutory_holiday(&self, date: NaiveDate) -> Result<bool, CalendarError> {
        self.day_kind(date).map(|k| k.is_statutory_holiday())
    }
}

// ════════════════════════════════════════════════════════════════
// CnCalendarProvider
// ════════════════════════════════════════════════════════════════

pub struct CnCalendarProvider {
    years: HashMap<i32, CalendarYear>,
}

impl CnCalendarProvider {
    pub fn new() -> Self {
        Self { years: build_calendar_data() }
    }
}

impl CalendarProvider for CnCalendarProvider {
    fn day_kind(&self, date: NaiveDate) -> Result<CalendarDayKind, CalendarError> {
        let y = self.years.get(&date.year())
            .ok_or(CalendarError::UnsupportedYear(date.year()))?;

        if y.makeup_workdays.contains(&date) {
            return Ok(CalendarDayKind::MakeupWorkday);
        }
        if y.statutory_holidays.contains(&date) {
            return Ok(CalendarDayKind::StatutoryHoliday);
        }
        if y.scheduled_days_off.contains(&date) {
            return Ok(CalendarDayKind::ScheduledDayOff);
        }
        if matches!(date.weekday(), Weekday::Sat | Weekday::Sun) {
            return Ok(CalendarDayKind::Weekend);
        }
        Ok(CalendarDayKind::RegularWorkday)
    }
}

// ════════════════════════════════════════════════════════════════
// WorkingDaysCounter
// ════════════════════════════════════════════════════════════════

pub struct WorkingDaysCounter<P: CalendarProvider> {
    pub provider: P,
}

impl<P: CalendarProvider> DateCounter for WorkingDaysCounter<P> {
    fn count_days(&self, start: NaiveDate, end: NaiveDate, convention: PeriodCountingConvention) -> Result<u32, TimeDomainError> {
        if end < start {
            return Err(TimeDomainError::EndBeforeStart);
        }
        if convention != PeriodCountingConvention::STANDARD {
            return Err(TimeDomainError::UnsupportedConvention);
        }
        let mut count: u32 = 0;
        let mut current = start;
        loop {
            current = current.succ_opt().ok_or(TimeDomainError::DateOverflow)?;
            if current > end { break; }
            match self.provider.day_kind(current) {
                Ok(k) if k.is_workday() => count += 1,
                Ok(_) => {}
                Err(CalendarError::UnsupportedYear(y)) => return Err(TimeDomainError::CalendarUnavailable { year: y }),
            }
        }
        Ok(count)
    }
}

// ════════════════════════════════════════════════════════════════
// FinalDayAdjustment
// ════════════════════════════════════════════════════════════════

/// 仅 StatutoryHoliday 启动顺延。只跨过连续 StatutoryHoliday。
/// 第一个非 StatutoryHoliday 即返回（即使是 Weekend/ScheduledDayOff 也不再跳过）。
pub fn adjust_final_day<P: CalendarProvider>(date: NaiveDate, adjustment: FinalDayAdjustment, provider: &P) -> Result<NaiveDate, CalendarError> {
    match adjustment {
        FinalDayAdjustment::None => Ok(date),
        FinalDayAdjustment::ExtendPastLegalHoliday => {
            if !provider.day_kind(date)?.is_statutory_holiday() {
                return Ok(date);
            }
            let mut current = date;
            loop {
                current = current.succ_opt().ok_or(CalendarError::UnsupportedYear(current.year()))?;
                if !provider.day_kind(current)?.is_statutory_holiday() {
                    return Ok(current);
                }
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════
// Calendar Data
// ════════════════════════════════════════════════════════════════

fn build_calendar_data() -> HashMap<i32, CalendarYear> {
    let mut map = HashMap::new();
    map.insert(2024, year_2024());
    map.insert(2025, year_2025());
    map.insert(2026, year_2026());
    map
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate { NaiveDate::from_ymd_opt(y, m, d).unwrap() }

fn range(from: (i32, u32, u32), to: (i32, u32, u32)) -> Vec<NaiveDate> {
    let (start, end) = (date(from.0, from.1, from.2), date(to.0, to.1, to.2));
    let mut v = Vec::new();
    let mut cur = start;
    while cur <= end { v.push(cur); cur = cur.succ_opt().unwrap(); }
    v
}

fn year_2024() -> CalendarYear {
    let mut sched = HashSet::new();
    sched.insert(date(2024, 1, 1));
    sched.extend(range((2024, 2, 10), (2024, 2, 17)));
    sched.extend(range((2024, 4, 4), (2024, 4, 6)));
    sched.extend(range((2024, 5, 1), (2024, 5, 5)));
    sched.insert(date(2024, 6, 10));
    sched.extend(range((2024, 9, 15), (2024, 9, 17)));
    sched.extend(range((2024, 10, 1), (2024, 10, 7)));

    let mut stat = HashSet::new();
    stat.insert(date(2024, 1, 1));                                // 元旦
    stat.extend([date(2024, 2, 10), date(2024, 2, 11), date(2024, 2, 12)]); // 春节 初一至初三
    stat.insert(date(2024, 4, 4));                                // 清明
    stat.insert(date(2024, 5, 1));                                // 劳动节（旧11天，仅5/1）
    stat.insert(date(2024, 6, 10));                               // 端午
    stat.insert(date(2024, 9, 17));                               // 中秋
    stat.extend([date(2024, 10, 1), date(2024, 10, 2), date(2024, 10, 3)]); // 国庆

    let mut makeup = HashSet::new();
    makeup.extend([date(2024,2,4),date(2024,2,18),date(2024,4,7),date(2024,4,28),date(2024,5,11),date(2024,9,14),date(2024,9,29),date(2024,10,12)]);

    CalendarYear { year: 2024, scheduled_days_off: sched, statutory_holidays: stat, makeup_workdays: makeup,
        schedule_source: "国办2024年节假日通知", statutory_source: "《全国年节及纪念日放假办法》(2013修订)" }
}

fn year_2025() -> CalendarYear {
    let mut sched = HashSet::new();
    sched.insert(date(2025, 1, 1));
    sched.extend(range((2025, 1, 28), (2025, 2, 4)));
    sched.extend(range((2025, 4, 4), (2025, 4, 6)));
    sched.extend(range((2025, 5, 1), (2025, 5, 5)));
    sched.extend(range((2025, 5, 31), (2025, 6, 2)));
    sched.extend(range((2025, 10, 1), (2025, 10, 8)));

    let mut stat = HashSet::new();
    stat.insert(date(2025, 1, 1));                                // 元旦
    stat.extend([date(2025, 1, 28), date(2025, 1, 29), date(2025, 1, 30), date(2025, 1, 31)]); // 春节 除夕至初三
    stat.insert(date(2025, 4, 4));                                // 清明
    stat.extend([date(2025, 5, 1), date(2025, 5, 2)]);           // 劳动节（新13天）
    stat.insert(date(2025, 5, 31));                               // 端午
    stat.insert(date(2025, 10, 6));                               // 中秋
    stat.extend([date(2025, 10, 1), date(2025, 10, 2), date(2025, 10, 3)]); // 国庆

    let mut makeup = HashSet::new();
    makeup.extend([date(2025,1,26),date(2025,2,8),date(2025,4,27),date(2025,9,28),date(2025,10,11)]);

    CalendarYear { year: 2025, scheduled_days_off: sched, statutory_holidays: stat, makeup_workdays: makeup,
        schedule_source: "国办2025年节假日通知", statutory_source: "《全国年节及纪念日放假办法》(国务院令第795号)" }
}

fn year_2026() -> CalendarYear {
    let mut sched = HashSet::new();
    sched.extend(range((2026, 1, 1), (2026, 1, 3)));
    sched.extend(range((2026, 2, 15), (2026, 2, 23)));
    sched.extend(range((2026, 4, 4), (2026, 4, 6)));
    sched.extend(range((2026, 5, 1), (2026, 5, 5)));
    sched.extend(range((2026, 6, 19), (2026, 6, 21)));
    sched.extend(range((2026, 9, 25), (2026, 9, 27)));
    sched.extend(range((2026, 10, 1), (2026, 10, 7)));

    let mut stat = HashSet::new();
    stat.insert(date(2026, 1, 1));                                // 元旦
    stat.extend([date(2026, 2, 16), date(2026, 2, 17), date(2026, 2, 18), date(2026, 2, 19)]); // 春节 除夕至初三
    stat.insert(date(2026, 4, 5));                                // 清明
    stat.extend([date(2026, 5, 1), date(2026, 5, 2)]);           // 劳动节（新13天）
    stat.insert(date(2026, 6, 19));                               // 端午
    stat.insert(date(2026, 9, 25));                               // 中秋
    stat.extend([date(2026, 10, 1), date(2026, 10, 2), date(2026, 10, 3)]); // 国庆

    let mut makeup = HashSet::new();
    makeup.extend([date(2026,1,4),date(2026,2,14),date(2026,2,28),date(2026,5,9),date(2026,9,20),date(2026,10,10)]);

    CalendarYear { year: 2026, scheduled_days_off: sched, statutory_holidays: stat, makeup_workdays: makeup,
        schedule_source: "国办2026年节假日通知", statutory_source: "《全国年节及纪念日放假办法》(国务院令第795号)" }
}

// ════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    fn d(y: i32, m: u32, day: u32) -> NaiveDate { NaiveDate::from_ymd_opt(y, m, day).unwrap() }
    fn provider() -> CnCalendarProvider { CnCalendarProvider::new() }
    fn kind(y: i32, m: u32, day: u32) -> CalendarDayKind { provider().day_kind(d(y, m, day)).unwrap() }
    fn wd(start: NaiveDate, end: NaiveDate) -> u32 {
        WorkingDaysCounter { provider: provider() }.count_days(start, end, PeriodCountingConvention::STANDARD).unwrap()
    }
    fn adj(date: NaiveDate, adj_ty: FinalDayAdjustment) -> NaiveDate { adjust_final_day(date, adj_ty, &provider()).unwrap() }

    // ── CalendarDayKind core ──────────────────────────────────

    #[test] fn kind_regular()        { assert_eq!(kind(2026, 6, 18), CalendarDayKind::RegularWorkday); }
    #[test] fn kind_weekend()        { assert_eq!(kind(2024, 6, 16), CalendarDayKind::Weekend); }
    #[test] fn kind_scheduled()      { assert_eq!(kind(2024, 2, 14), CalendarDayKind::ScheduledDayOff); } // 春节长假中，非法定
    #[test] fn kind_statutory()      { assert_eq!(kind(2024, 6, 10), CalendarDayKind::StatutoryHoliday); }
    #[test] fn kind_makeup()         { assert_eq!(kind(2024, 2, 18), CalendarDayKind::MakeupWorkday); }

    // ── is_workday ────────────────────────────────────────────

    #[test] fn wd_regular()  { assert!(provider().is_workday(d(2026, 6, 18)).unwrap()); }
    #[test] fn wd_makeup()   { assert!(provider().is_workday(d(2024, 2, 18)).unwrap()); }
    #[test] fn wd_weekend()  { assert!(!provider().is_workday(d(2024, 6, 16)).unwrap()); }
    #[test] fn wd_scheduled(){ assert!(!provider().is_workday(d(2024, 2, 14)).unwrap()); }
    #[test] fn wd_statutory(){ assert!(!provider().is_workday(d(2024, 6, 10)).unwrap()); }

    // ── 2024 Labor Day version ────────────────────────────────

    #[test] fn y2024_may01_statutory() { assert!(provider().is_statutory_holiday(d(2024, 5, 1)).unwrap()); }
    #[test] fn y2024_may02_not_statutory() { assert!(!provider().is_statutory_holiday(d(2024, 5, 2)).unwrap()); }

    // ── 2025 Labor Day version ────────────────────────────────

    #[test] fn y2025_may01_statutory() { assert!(provider().is_statutory_holiday(d(2025, 5, 1)).unwrap()); }
    #[test] fn y2025_may02_statutory() { assert!(provider().is_statutory_holiday(d(2025, 5, 2)).unwrap()); }

    // ── Spring Festival boundaries ────────────────────────────

    #[test] fn y2024_sf_statutory_only_3() {
        // 2024旧11天：仅初一/初二/初三 statutory
        assert!(provider().is_statutory_holiday(d(2024, 2, 10)).unwrap()); // 初一
        assert!(provider().is_statutory_holiday(d(2024, 2, 11)).unwrap()); // 初二
        assert!(provider().is_statutory_holiday(d(2024, 2, 12)).unwrap()); // 初三
        assert!(!provider().is_statutory_holiday(d(2024, 2, 9)).unwrap());  // 除夕（旧法不是 statutory）
        assert!(!provider().is_statutory_holiday(d(2024, 2, 8)).unwrap());  // 普通工作日
    }

    #[test] fn y2026_sf_statutory_4_with_eve() {
        // 2026新13天：除夕至初三 statutory；长假其他日期仅 scheduled
        assert!(provider().is_statutory_holiday(d(2026, 2, 16)).unwrap()); // 除夕
        assert!(provider().is_statutory_holiday(d(2026, 2, 17)).unwrap()); // 初一
        assert!(provider().is_statutory_holiday(d(2026, 2, 19)).unwrap()); // 初三
        // 2/15 is Sunday, but in scheduled range → ScheduledDayOff (not Statutory)
        assert_eq!(kind(2026, 2, 15), CalendarDayKind::ScheduledDayOff);
        assert!(!provider().is_statutory_holiday(d(2026, 2, 15)).unwrap());
        // 2/20 is weekday in scheduled range → ScheduledDayOff, not Statutory
        assert_eq!(kind(2026, 2, 20), CalendarDayKind::ScheduledDayOff);
        assert!(!provider().is_statutory_holiday(d(2026, 2, 20)).unwrap());
    }

    // ── Scheduled-vs-Statutory regression ─────────────────────

    #[test] fn scheduled_not_statutory_no_extend() {
        // 2026-02-20 is ScheduledDayOff (long vacation), NOT StatutoryHoliday
        // ExtendPastLegalHoliday must NOT trigger
        assert_eq!(adj(d(2026, 2, 20), FinalDayAdjustment::ExtendPastLegalHoliday), d(2026, 2, 20));
    }

    // ── Key exact dates ───────────────────────────────────────

    #[test] fn exact_2024_01_01() { assert_eq!(kind(2024,1,1), CalendarDayKind::StatutoryHoliday); }
    #[test] fn exact_2024_06_10() { assert_eq!(kind(2024,6,10), CalendarDayKind::StatutoryHoliday); }
    #[test] fn exact_2025_10_01() { assert_eq!(kind(2025,10,1), CalendarDayKind::StatutoryHoliday); }
    #[test] fn exact_2026_06_19() { assert_eq!(kind(2026,6,19), CalendarDayKind::StatutoryHoliday); }

    // ── Makeup priority ───────────────────────────────────────

    #[test] fn makeup_feb18() { assert_eq!(kind(2024,2,18), CalendarDayKind::MakeupWorkday); }
    #[test] fn makeup_jan26() { assert_eq!(kind(2025,1,26), CalendarDayKind::MakeupWorkday); }
    #[test] fn makeup_feb14() { assert_eq!(kind(2026,2,14), CalendarDayKind::MakeupWorkday); }
    #[test] fn makeup_priority_weekend() {
        // 2/18 is Sunday, but makeup → MakeupWorkday, NOT Weekend
        assert_eq!(kind(2024, 2, 18), CalendarDayKind::MakeupWorkday);
    }

    // ── Unsupported year ──────────────────────────────────────

    #[test] fn unsupported_year() { assert!(matches!(provider().day_kind(d(2027,1,4)), Err(CalendarError::UnsupportedYear(2027)))); }

    // ── WorkingDaysCounter ────────────────────────────────────

    #[test] fn wd_mon_fri()   { assert_eq!(wd(d(2025,6,16), d(2025,6,20)), 4); }
    #[test] fn wd_fri_mon()   { assert_eq!(wd(d(2025,6,13), d(2025,6,16)), 1); }
    #[test] fn wd_same()      { assert_eq!(wd(d(2025,6,17), d(2025,6,17)), 0); }
    #[test] fn wd_spring()    { assert_eq!(wd(d(2024,2,16), d(2024,2,19)), 2); }
    #[test] fn wd_reversed()  { assert!(matches!(
        WorkingDaysCounter{provider:provider()}.count_days(d(2025,6,2),d(2025,6,1),PeriodCountingConvention::STANDARD),
        Err(TimeDomainError::EndBeforeStart))); }
    #[test] fn wd_2027_err()  { assert!(matches!(
        WorkingDaysCounter{provider:provider()}.count_days(d(2026,12,30),d(2027,1,4),PeriodCountingConvention::STANDARD),
        Err(TimeDomainError::CalendarUnavailable{year:2027}))); }

    // ── FinalDayAdjustment ────────────────────────────────────

    #[test] fn adj_none()       { assert_eq!(adj(d(2026,6,19), FinalDayAdjustment::None), d(2026,6,19)); }
    #[test] fn adj_regular()    { assert_eq!(adj(d(2026,6,18), FinalDayAdjustment::ExtendPastLegalHoliday), d(2026,6,18)); }
    #[test] fn adj_makeup()     { assert_eq!(adj(d(2026,2,14), FinalDayAdjustment::ExtendPastLegalHoliday), d(2026,2,14)); }
    #[test] fn adj_weekend_no() {
        assert_eq!(adj(d(2026,6,13), FinalDayAdjustment::ExtendPastLegalHoliday), d(2026,6,13));
    }
    #[test] fn adj_scheduled_no_extend() {
        // ScheduledDayOff (not StatutoryHoliday) → no extension
        assert_eq!(adj(d(2026,2,20), FinalDayAdjustment::ExtendPastLegalHoliday), d(2026,2,20));
    }
    #[test] fn adj_statutory_consecutive() {
        // 2026端午：仅6/19是StatutoryHoliday，6/20-6/21是ScheduledDayOff
        // → 6/19 extends → 6/20 (first non-StatutoryHoliday) = 6/20
        assert_eq!(adj(d(2026,6,19), FinalDayAdjustment::ExtendPastLegalHoliday), d(2026,6,20));
    }

    // ── MockCalendarProvider ──────────────────────────────────

    struct MockProvider { stat: HashSet<NaiveDate>, sched: HashSet<NaiveDate> }
    impl MockProvider {
        fn new(stat: HashSet<NaiveDate>, sched: HashSet<NaiveDate>) -> Self { Self { stat, sched } }
    }
    impl CalendarProvider for MockProvider {
        fn day_kind(&self, date: NaiveDate) -> Result<CalendarDayKind, CalendarError> {
            if self.stat.contains(&date) { return Ok(CalendarDayKind::StatutoryHoliday); }
            if self.sched.contains(&date) { return Ok(CalendarDayKind::ScheduledDayOff); }
            if matches!(date.weekday(), Weekday::Sat | Weekday::Sun) { return Ok(CalendarDayKind::Weekend); }
            Ok(CalendarDayKind::RegularWorkday)
        }
    }

    #[test] fn mock_statutory_fri_to_sat() {
        let stat: HashSet<NaiveDate> = [d(2026,7,10)].into_iter().collect();
        let p = MockProvider::new(stat, HashSet::new());
        // 7/10(Fri) StatutoryHoliday → 7/11(Sat, Weekend) → return 7/11
        assert_eq!(adjust_final_day(d(2026,7,10), FinalDayAdjustment::ExtendPastLegalHoliday, &p).unwrap(), d(2026,7,11));
    }

    #[test] fn mock_scheduled_only_no_extend() {
        let sched: HashSet<NaiveDate> = [d(2026,7,10)].into_iter().collect();
        let p = MockProvider::new(HashSet::new(), sched);
        // 7/10 is ScheduledDayOff, NOT StatutoryHoliday → no extension
        assert_eq!(adjust_final_day(d(2026,7,10), FinalDayAdjustment::ExtendPastLegalHoliday, &p).unwrap(), d(2026,7,10));
    }

    // ── Schedule entry counts ─────────────────────────────────

    #[test] fn sched_2024_count() { assert_eq!(provider().years[&2024].scheduled_days_off.len(), 28); }
    #[test] fn sched_2025_count() { assert_eq!(provider().years[&2025].scheduled_days_off.len(), 28); }
    #[test] fn sched_2026_count() { assert_eq!(provider().years[&2026].scheduled_days_off.len(), 33); }
    #[test] fn stat_2024_count()  { assert_eq!(provider().years[&2024].statutory_holidays.len(), 11); }
    #[test] fn stat_2025_count()  { assert_eq!(provider().years[&2025].statutory_holidays.len(), 13); }
    #[test] fn stat_2026_count()  { assert_eq!(provider().years[&2026].statutory_holidays.len(), 13); }
}
