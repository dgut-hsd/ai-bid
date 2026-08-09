//! `calculate_timeline` 工具 — 纯日期关系计算。
//!
//! LLM 做日期计算经常出错——交给代码。本工具只负责：
//! - 解析日期节点（YYYY-MM-DD，chrono typed parsing）
//! - 计算调用方**明确指定**的两个日期节点之间的 CalendarDays
//! - 计算调用方**明确指定**的两个日期节点之间的 WorkingDays
//! - 检测纯时间顺序矛盾
//! - 返回结构化日期计算结果和 diagnostic
//!
//! 不负责：
//! - 法规最低/最高期限（20 / 10 / 7 / 5 日等）
//! - 采购方式 / RuleSet / PeriodType
//! - Compliant / Violation / legal_basis
//! - 自动推断任何规则
//!
//! 日期计数统一委托 `time_domain`（CalendarDaysCounter / WorkingDaysCounter）
//! 与 `calendar`（CnCalendarProvider）。

use anyhow::{Result, anyhow};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::AgentTool;
use super::calendar::{CnCalendarProvider, WorkingDaysCounter};
use super::time_domain::{
    CalendarDaysCounter, DateCounter, DayCountType, PeriodCountingConvention,
    TimeDomainError,
};

/// `calculate_timeline` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct CalculateTimelineArgs {
    /// 日期事件列表
    pub dates: Vec<DateEvent>,
    /// 明确指定的日期关系计算请求（不自动推断）
    #[serde(default)]
    pub calculations: Vec<TimelineCalculation>,
}

/// 单个日期事件。
#[derive(Debug, Deserialize)]
pub struct DateEvent {
    /// 事件名称，如"公告发布日期"。必须非空且唯一。
    pub label: String,
    /// 日期字符串，如"2025-06-22"（YYYY-MM-DD）
    pub date_str: String,
    /// 事件类型：仅作为时间节点元数据，用于纯时间顺序矛盾检测。
    /// 禁止用于自动选择法规或自动生成 minimum。
    #[serde(default)]
    pub event_type: Option<String>,
}

/// 日期关系计算请求：from → to 的 day_count_type 日期差。
#[derive(Debug, Deserialize)]
pub struct TimelineCalculation {
    /// 起始事件 label（必须存在于 dates[]）
    pub from: String,
    /// 结束事件 label（必须存在于 dates[]）
    pub to: String,
    /// 计数类型：calendar_days（日历日）/ working_days（工作日）
    #[serde(deserialize_with = "de_day_count_type")]
    pub day_count_type: DayCountType,
}

/// day_count_type 反序列化适配层：
/// canonical 值 calendar_days / working_days，兼容 legacy "CalendarDays" / "WorkingDays"。
fn de_day_count_type<'de, D>(d: D) -> Result<DayCountType, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    match s.as_str() {
        "calendar_days" | "CalendarDays" => Ok(DayCountType::CalendarDays),
        "working_days" | "WorkingDays" => Ok(DayCountType::WorkingDays),
        other => Err(serde::de::Error::custom(format!(
            "invalid day_count_type '{}'（支持 calendar_days / working_days）",
            other
        ))),
    }
}

/// canonical 输出值。
fn day_count_type_str(t: DayCountType) -> &'static str {
    match t {
        DayCountType::CalendarDays => "calendar_days",
        DayCountType::WorkingDays => "working_days",
    }
}

/// 时间线计算的返回结果。
#[derive(Debug, Serialize)]
struct TimelineResult {
    events: Vec<ResolvedEvent>,
    calculations: Vec<TimelineCalculationResult>,
    contradictions: Vec<TimelineContradiction>,
    summary: String,
}

#[derive(Debug, Serialize)]
struct ResolvedEvent {
    label: String,
    date_str: String,
    parsed: bool,
    event_type: Option<String>,
}

/// 单个日期关系计算结果（status 仅表示计算状态，不代表法律状态）。
#[derive(Debug, Serialize)]
struct TimelineCalculationResult {
    from: String,
    to: String,
    day_count_type: String,
    actual_days: Option<i64>,
    status: CalcStatus,
    diagnostic: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum CalcStatus {
    Calculated,
    InvalidInput,
    CalendarUnavailable,
}

#[derive(Debug, Serialize)]
struct TimelineContradiction {
    description: String,
    event_a: String,
    event_b: String,
    detail: String,
}

/// `calculate_timeline` 工具实现。
///
/// 纯日期运算，无法规判断，无外部 I/O。
pub struct CalculateTimelineTool;

impl CalculateTimelineTool {
    /// 解析日期字符串（YYYY-MM-DD，chrono typed parsing）。
    fn parse_date(date_str: &str) -> Result<NaiveDate> {
        NaiveDate::parse_from_str(date_str.trim(), "%Y-%m-%d")
            .map_err(|_| anyhow!("无法解析日期 '{}'（期望 YYYY-MM-DD）", date_str))
    }

    /// 纯时间顺序矛盾检测（不涉及任何法律 threshold）。
    fn detect_contradictions(
        events: &HashMap<String, NaiveDate>,
        event_list: &[DateEvent],
    ) -> Vec<TimelineContradiction> {
        let mut contradictions = Vec::new();

        for ev in event_list {
            let Some(et) = &ev.event_type else { continue };
            let Some(&date) = events.get(&ev.label) else { continue };

            // 开标日期应在投标截止之后（87号令39条允许同一时间开标：
            // 纯时序层面仅当开标严格早于投标截止才构成顺序矛盾）
            if et == "bid_opening" {
                for other_ev in event_list {
                    if other_ev.event_type.as_deref() == Some("deadline")
                        && let Some(&deadline_date) = events.get(&other_ev.label)
                        && date < deadline_date
                    {
                        contradictions.push(TimelineContradiction {
                            description: "开标日期应在投标截止之后".to_string(),
                            event_a: ev.label.clone(),
                            event_b: other_ev.label.clone(),
                            detail: format!(
                                "开标日期({}) < 投标截止({})，时序矛盾",
                                ev.date_str, other_ev.date_str
                            ),
                        });
                    }
                }
            }
            // 公告发布日期应在投标截止之前
            if et == "announcement" {
                for other_ev in event_list {
                    if other_ev.event_type.as_deref() == Some("deadline")
                        && let Some(&deadline_date) = events.get(&other_ev.label)
                        && date >= deadline_date
                    {
                        contradictions.push(TimelineContradiction {
                            description: "公告发布日期应在投标截止之前".to_string(),
                            event_a: ev.label.clone(),
                            event_b: other_ev.label.clone(),
                            detail: format!(
                                "公告日期({}) ≥ 投标截止({})，时序矛盾",
                                ev.date_str, other_ev.date_str
                            ),
                        });
                    }
                }
            }
            // 中标日期应在开标之后
            if et == "award" {
                for other_ev in event_list {
                    if other_ev.event_type.as_deref() == Some("bid_opening")
                        && let Some(&opening_date) = events.get(&other_ev.label)
                        && date < opening_date
                    {
                        contradictions.push(TimelineContradiction {
                            description: "中标日期应在开标之后".to_string(),
                            event_a: ev.label.clone(),
                            event_b: other_ev.label.clone(),
                            detail: format!(
                                "中标日期({}) < 开标日期({})，时序矛盾",
                                ev.date_str, other_ev.date_str
                            ),
                        });
                    }
                }
            }
        }

        contradictions
    }
}

#[async_trait::async_trait]
impl AgentTool for CalculateTimelineTool {
    fn name(&self) -> &str {
        "calculate_timeline"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "calculate_timeline",
                "description": "【使用场景】计算用户明确指定的两个日期节点之间的日历日或工作日关系，并检测纯时间顺序矛盾；\
                    不执行采购法规合规判定。\
                    【不使用场景】\
                    ① 条款没有日期信息——不要强行调用；\
                    ② 法规期限验证（20日等标期、10日磋商、5工作日文件提供期等）——\
                    用 verify_bid_preparation_period / verify_announcement_period 等专门 verification 工具；\
                    ③ 需要语义判断的'时间是否合理'——LLM 做推理，计算器做算术。\
                    【关键】只计算 calculations 中明确指定的关系，绝不自动推断（不自动生成 announcement→deadline 等规则）。\
                    日期格式 YYYY-MM-DD。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "dates": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "label": {
                                        "type": "string",
                                        "description": "事件名称，必须非空且唯一（重复 label 将返回 invalid_input）"
                                    },
                                    "date_str": {
                                        "type": "string",
                                        "description": "日期，YYYY-MM-DD"
                                    },
                                    "event_type": {
                                        "type": "string",
                                        "enum": ["announcement", "deadline", "bid_opening", "clarification", "award", "issuance", "challenge", "reply"],
                                        "description": "时间节点元数据，仅用于纯时间顺序矛盾检测；不会触发任何法规规则"
                                    }
                                },
                                "required": ["label", "date_str"]
                            }
                        },
                        "calculations": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "from": {"type": "string", "description": "起始事件 label（必须存在于 dates[]）"},
                                    "to": {"type": "string", "description": "结束事件 label（必须存在于 dates[]）"},
                                    "day_count_type": {"type": "string", "enum": ["calendar_days", "working_days"], "description": "计数类型：calendar_days=日历日；working_days=工作日（排除周末与法定节假日）"}
                                },
                                "required": ["from", "to", "day_count_type"]
                            }
                        }
                    },
                    "required": ["dates"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: CalculateTimelineArgs = serde_json::from_value(args)?;

        if parsed.dates.is_empty() {
            return Err(anyhow!("dates 不能为空"));
        }

        // ── 1. label 校验：非空 + 唯一（在任何计算执行前失败）──
        let mut label_problems: Vec<String> = Vec::new();
        let mut labels_seen: HashSet<&str> = HashSet::new();
        for ev in &parsed.dates {
            if ev.label.trim().is_empty() {
                label_problems.push("存在空 label".to_string());
            } else if !labels_seen.insert(ev.label.as_str()) {
                label_problems.push(format!("重复 label '{}'", ev.label));
            }
        }

        // ── 2. 解析所有日期（chrono typed parsing）──
        let mut events_map: HashMap<String, NaiveDate> = HashMap::new();
        let mut parse_failures: Vec<String> = Vec::new();
        let mut parse_error_by_label: HashMap<String, String> = HashMap::new();
        let mut resolved_events = Vec::new();
        for ev in &parsed.dates {
            match Self::parse_date(&ev.date_str) {
                Ok(d) => {
                    events_map.insert(ev.label.clone(), d);
                    resolved_events.push(ResolvedEvent {
                        label: ev.label.clone(),
                        date_str: ev.date_str.clone(),
                        parsed: true,
                        event_type: ev.event_type.clone(),
                    });
                }
                Err(e) => {
                    parse_failures.push(format!("{}: {}", ev.label, e));
                    parse_error_by_label.insert(ev.label.clone(), format!("{}: {}", ev.label, e));
                    resolved_events.push(ResolvedEvent {
                        label: ev.label.clone(),
                        date_str: ev.date_str.clone(),
                        parsed: false,
                        event_type: ev.event_type.clone(),
                    });
                }
            }
        }

        // ── 3. 日期关系计算（只算 calculations 明确指定的对）──
        let mut calc_results = Vec::new();
        let mut done_count = 0usize;
        let mut invalid_count = 0usize;
        let mut unavailable_count = 0usize;

        if !label_problems.is_empty() {
            // label 冲突 → 所有计算请求 invalid_input，零计算执行
            let problem = label_problems.join("；");
            for c in &parsed.calculations {
                calc_results.push(TimelineCalculationResult {
                    from: c.from.clone(),
                    to: c.to.clone(),
                    day_count_type: day_count_type_str(c.day_count_type).to_string(),
                    actual_days: None,
                    status: CalcStatus::InvalidInput,
                    diagnostic: format!("label 校验失败（{}），未执行任何计算", problem),
                });
                invalid_count += 1;
            }
        } else if parsed.calculations.is_empty() {
            // insufficient input：不自动推断任何组合 / 规则，calculations 保持 0
            // 注：空 calculations 时不生成任何 TimelineCalculationResult 条目
            invalid_count = 0;
        } else {
            for c in &parsed.calculations {
                let mut result = TimelineCalculationResult {
                    from: c.from.clone(),
                    to: c.to.clone(),
                    day_count_type: day_count_type_str(c.day_count_type).to_string(),
                    actual_days: None,
                    status: CalcStatus::InvalidInput,
                    diagnostic: String::new(),
                };

                let from = events_map.get(&c.from);
                let to = events_map.get(&c.to);

                if let Some(err) = parse_error_by_label.get(&c.from) {
                    result.diagnostic = format!("from 节点日期非法：{}", err);
                } else if let Some(err) = parse_error_by_label.get(&c.to) {
                    result.diagnostic = format!("to 节点日期非法：{}", err);
                } else if from.is_none() {
                    result.diagnostic = format!("计算 from label '{}' 不存在于 dates[]", c.from);
                } else if to.is_none() {
                    result.diagnostic = format!("计算 to label '{}' 不存在于 dates[]", c.to);
                } else {
                    let from_d = *from.unwrap();
                    let to_d = *to.unwrap();
                    if to_d < from_d {
                        result.diagnostic = format!(
                            "from 日期 {} 晚于 to 日期 {}（EndBeforeStart），禁止反向计算",
                            from_d, to_d
                        );
                    } else if to_d == from_d {
                        result.actual_days = Some(0);
                        result.status = CalcStatus::Calculated;
                        result.diagnostic = "from == to，实际 0 天".to_string();
                        done_count += 1;
                    } else {
                        match c.day_count_type {
                            DayCountType::CalendarDays => {
                                match CalendarDaysCounter
                                    .count_days(from_d, to_d, PeriodCountingConvention::STANDARD)
                                {
                                    Ok(d) => {
                                        result.actual_days = Some(d as i64);
                                        result.status = CalcStatus::Calculated;
                                        result.diagnostic =
                                            format!("日历日 {} 天（STANDARD: start excluded, end included）", d);
                                        done_count += 1;
                                    }
                                    Err(e) => {
                                        result.diagnostic = format!("CalendarDays 计算失败: {}", e);
                                    }
                                }
                            }
                            DayCountType::WorkingDays => {
                                let counter = WorkingDaysCounter { provider: CnCalendarProvider::new() };
                                match counter.count_days(from_d, to_d, PeriodCountingConvention::STANDARD)
                                {
                                    Ok(d) => {
                                        result.actual_days = Some(d as i64);
                                        result.status = CalcStatus::Calculated;
                                        result.diagnostic =
                                            format!("工作日 {} 天（STANDARD: start excluded, end included）", d);
                                        done_count += 1;
                                    }
                                    Err(TimeDomainError::CalendarUnavailable { year }) => {
                                        result.status = CalcStatus::CalendarUnavailable;
                                        result.diagnostic = format!(
                                            "WorkingDays 需要日历数据，年份 {} 不在支持范围（2024-2026）内",
                                            year
                                        );
                                        unavailable_count += 1;
                                    }
                                    Err(e) => {
                                        result.diagnostic = format!("WorkingDays 计算失败: {}", e);
                                    }
                                }
                            }
                        }
                    }
                }

                if matches!(result.status, CalcStatus::InvalidInput) && !result.diagnostic.is_empty() {
                    invalid_count += 1;
                }
                calc_results.push(result);
            }
        }

        // ── 4. 纯时间顺序矛盾检测（不依赖计算）──
        let contradictions = Self::detect_contradictions(&events_map, &parsed.dates);

        // ── 5. 事实性摘要（无 ✅ 合规 / ⚠️ 违反法定期限）──
        let mut parts: Vec<String> = Vec::new();
        if label_problems.is_empty() && parsed.calculations.is_empty() {
            parts.push(format!(
                "insufficient input：未提供 calculations，未执行任何日期差计算（{} 个事件节点）",
                parsed.dates.len()
            ));
        } else {
            parts.push(format!("完成 {} 个日期关系计算", done_count));
            if invalid_count > 0 {
                parts.push(format!("{} 个因输入非法未完成", invalid_count));
            }
            if unavailable_count > 0 {
                parts.push(format!(
                    "{} 个工作日计算因 CalendarUnavailable 未完成",
                    unavailable_count
                ));
            }
        }
        if !contradictions.is_empty() {
            parts.push(format!("发现 {} 个时间顺序矛盾", contradictions.len()));
        }
        if !parse_failures.is_empty() {
            parts.push(format!("{} 个日期解析失败", parse_failures.len()));
        }
        let summary = parts.join("；");

        let result = TimelineResult {
            events: resolved_events,
            calculations: calc_results,
            contradictions,
            summary,
        };

        Ok(serde_json::to_value(&result)?)
    }
}

// ─── 测试（以 Tool.execute() 消费者集成为主）──────────────────

#[cfg(test)]
mod tests {
    use super::*;

    async fn run(dates: Vec<serde_json::Value>, calcs: Vec<serde_json::Value>) -> serde_json::Value {
        let tool = CalculateTimelineTool;
        tool.execute(serde_json::json!({
            "dates": dates,
            "calculations": calcs
        }))
        .await
        .unwrap()
    }

    fn date(label: &str, date_str: &str, event_type: Option<&str>) -> serde_json::Value {
        let mut v = serde_json::json!({ "label": label, "date_str": date_str });
        if let Some(et) = event_type {
            v["event_type"] = serde_json::json!(et);
        }
        v
    }

    fn calc(from: &str, to: &str, day_count_type: &str) -> serde_json::Value {
        serde_json::json!({ "from": from, "to": to, "day_count_type": day_count_type })
    }

    fn result_for<'a>(out: &'a serde_json::Value, from: &str, to: &str) -> &'a serde_json::Value {
        out["calculations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["from"] == from && c["to"] == to)
            .expect("缺少计算结果")
    }

    // ── label 唯一性（§29）──────────────────────────────────────

    #[tokio::test]
    async fn duplicate_label_invalid_input() {
        let out = run(
            vec![
                date("投标截止", "2025-06-01", None),
                date("投标截止", "2025-06-02", None),
            ],
            vec![calc("投标截止", "投标截止", "calendar_days")],
        )
        .await;
        let r = result_for(&out, "投标截止", "投标截止");
        assert_eq!(r["status"], "invalid_input");
        assert!(r["diagnostic"].as_str().unwrap().contains("重复 label"));
        assert!(out["calculations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["actual_days"].is_null()));
    }

    // ── missing label reference（§30）───────────────────────────

    #[tokio::test]
    async fn missing_label_reference_invalid_input() {
        let out = run(
            vec![date("A", "2025-06-01", None), date("B", "2025-06-21", None)],
            vec![calc("A", "C", "calendar_days")],
        )
        .await;
        let r = result_for(&out, "A", "C");
        assert_eq!(r["status"], "invalid_input");
        assert!(r["diagnostic"].as_str().unwrap().contains("C"));
    }

    // ── CalendarDays execute（§31）──────────────────────────────

    #[tokio::test]
    async fn calendar_days_execute_20() {
        let out = run(
            vec![date("A", "2025-06-01", None), date("B", "2025-06-21", None)],
            vec![calc("A", "B", "calendar_days")],
        )
        .await;
        let r = result_for(&out, "A", "B");
        assert_eq!(r["status"], "calculated");
        assert_eq!(r["actual_days"], 20);
        assert_eq!(r["day_count_type"], "calendar_days");
    }

    // ── WorkingDays execute（§32）───────────────────────────────

    #[tokio::test]
    async fn working_days_execute_mon_to_fri_4() {
        // 2025-06-09(Mon) → 2025-06-13(Fri)，STANDARD：count 6/10..6/13 = 4 WD
        let out = run(
            vec![date("A", "2025-06-09", None), date("B", "2025-06-13", None)],
            vec![calc("A", "B", "working_days")],
        )
        .await;
        let r = result_for(&out, "A", "B");
        assert_eq!(r["status"], "calculated");
        assert_eq!(r["actual_days"], 4);
        assert_eq!(r["day_count_type"], "working_days");
    }

    // ── weekend consumer（§33）──────────────────────────────────

    #[tokio::test]
    async fn weekend_consumer_wd_differs_from_cd() {
        // 2025-03-07(Fri) → 2025-03-13(Thu)：CalendarDays=6，WorkingDays=4
        let out = run(
            vec![date("A", "2025-03-07", None), date("B", "2025-03-13", None)],
            vec![
                calc("A", "B", "calendar_days"),
                calc("A", "B", "working_days"),
            ],
        )
        .await;
        assert_eq!(out["calculations"].as_array().unwrap().len(), 2);
        let cd = result_for(&out, "A", "B");
        // 两条同 from/to 的计算按序区分：第一条 calendar，第二条 working
        let calcs = out["calculations"].as_array().unwrap();
        assert_eq!(calcs[0]["day_count_type"], "calendar_days");
        assert_eq!(calcs[0]["actual_days"], 6);
        assert_eq!(calcs[1]["day_count_type"], "working_days");
        assert_eq!(calcs[1]["actual_days"], 4);
        assert_ne!(calcs[0]["actual_days"], calcs[1]["actual_days"]);
    }

    // ── makeup workday（§34）：2024-02-18 周日补班 ──────────────

    #[tokio::test]
    async fn makeup_workday_sunday_counts() {
        // 2024-02-16(Fri) → 2024-02-22(Thu)：2/17 假日 skip，2/18(周日调休上班) 计入 → 5 WD
        let out = run(
            vec![date("A", "2024-02-16", None), date("B", "2024-02-22", None)],
            vec![calc("A", "B", "working_days")],
        )
        .await;
        let r = result_for(&out, "A", "B");
        assert_eq!(r["status"], "calculated");
        assert_eq!(r["actual_days"], 5, "2024-02-18 调休周日必须计入工作日");
    }

    // ── scheduled day off（§35）：2025 端午 5/31-6/2 ───────────

    #[tokio::test]
    async fn scheduled_day_off_not_counted() {
        // 2025-05-30(Fri) → 2025-06-03(Tue)：5/31,6/1,6/2 端午休息，仅 6/3 计入 → 1 WD
        let out = run(
            vec![date("A", "2025-05-30", None), date("B", "2025-06-03", None)],
            vec![calc("A", "B", "working_days")],
        )
        .await;
        let r = result_for(&out, "A", "B");
        assert_eq!(r["status"], "calculated");
        assert_eq!(r["actual_days"], 1);
    }

    // ── 2027（§36）──────────────────────────────────────────────

    #[tokio::test]
    async fn calendar_days_2027_still_calculated() {
        let out = run(
            vec![date("A", "2027-01-01", None), date("B", "2027-01-11", None)],
            vec![calc("A", "B", "calendar_days")],
        )
        .await;
        let r = result_for(&out, "A", "B");
        assert_eq!(r["status"], "calculated");
        assert_eq!(r["actual_days"], 10);
    }

    #[tokio::test]
    async fn working_days_2027_calendar_unavailable() {
        let out = run(
            vec![date("A", "2027-01-01", None), date("B", "2027-01-11", None)],
            vec![calc("A", "B", "working_days")],
        )
        .await;
        let r = result_for(&out, "A", "B");
        assert_eq!(r["status"], "calendar_unavailable");
        assert!(r["diagnostic"].as_str().unwrap().contains("2027"), "必须保留 year=2027: {}", r["diagnostic"]);
        assert!(r["actual_days"].is_null());
    }

    // ── invalid date（§37）──────────────────────────────────────

    #[tokio::test]
    async fn invalid_date_diagnostic_has_label_and_date() {
        let out = run(
            vec![date("A", "2025-02-30", None), date("B", "2025-06-21", None)],
            vec![calc("A", "B", "calendar_days")],
        )
        .await;
        let r = result_for(&out, "A", "B");
        assert_eq!(r["status"], "invalid_input");
        assert!(r["diagnostic"].as_str().unwrap().contains("A"));
        assert!(r["diagnostic"].as_str().unwrap().contains("2025-02-30"));
    }

    // ── reverse date（§38）──────────────────────────────────────

    #[tokio::test]
    async fn reverse_date_invalid_input() {
        let out = run(
            vec![date("A", "2025-06-21", None), date("B", "2025-06-01", None)],
            vec![calc("A", "B", "calendar_days")],
        )
        .await;
        let r = result_for(&out, "A", "B");
        assert_eq!(r["status"], "invalid_input");
        assert!(r["diagnostic"].as_str().unwrap().contains("EndBeforeStart"));
    }

    // ── same date（§39）─────────────────────────────────────────

    #[tokio::test]
    async fn same_date_zero_days() {
        let out = run(
            vec![date("A", "2025-06-01", None), date("B", "2025-06-01", None)],
            vec![calc("A", "B", "calendar_days")],
        )
        .await;
        let r = result_for(&out, "A", "B");
        assert_eq!(r["status"], "calculated");
        assert_eq!(r["actual_days"], 0);
    }

    // ── multi relation（§40）────────────────────────────────────

    #[tokio::test]
    async fn multi_relation_no_auto_extra() {
        let out = run(
            vec![
                date("A", "2025-06-01", None),
                date("B", "2025-06-21", None),
                date("C", "2025-06-30", None),
            ],
            vec![
                calc("A", "B", "calendar_days"),
                calc("B", "C", "working_days"),
            ],
        )
        .await;
        let calcs = out["calculations"].as_array().unwrap();
        assert_eq!(calcs.len(), 2, "只允许计算明确指定的对");
        assert!(
            calcs.iter().all(|c| !(c["from"] == "A" && c["to"] == "C")),
            "不得自动出现 A→C"
        );
    }

    // ── empty calculations（§41）────────────────────────────────

    #[tokio::test]
    async fn empty_calculations_insufficient_input() {
        let out = run(
            vec![
                date("公告发布", "2025-06-01", Some("announcement")),
                date("投标截止", "2025-06-22", Some("deadline")),
            ],
            vec![],
        )
        .await;
        let json = serde_json::to_string(&out).unwrap();
        assert_eq!(out["calculations"].as_array().unwrap().len(), 0, "calculations 必须保持 0");
        assert!(out["summary"].as_str().unwrap().contains("insufficient input"));
        assert!(
            !json.contains("min_days") && !json.contains("legal_basis") && !json.contains("required"),
            "不得出现旧法规字段: {}",
            json
        );
    }

    // ── event_type 不触发法规（§42）─────────────────────────────

    #[tokio::test]
    async fn event_type_no_auto_law() {
        // 即使 event_type=announcement/deadline，calculations 为空也不得自动生成法规约束
        let out = run(
            vec![
                date("公告发布", "2025-06-01", Some("announcement")),
                date("投标截止", "2025-06-22", Some("deadline")),
            ],
            vec![],
        )
        .await;
        assert_eq!(out["calculations"].as_array().unwrap().len(), 0);
        assert!(out["summary"].as_str().unwrap().contains("insufficient input"));
    }

    // ── pure contradiction（§43）────────────────────────────────

    #[tokio::test]
    async fn pure_contradiction_no_legal_output() {
        // 开标(6/20) 早于 投标截止(6/22) → 矛盾；输出无法规字段
        let out = run(
            vec![
                date("投标截止", "2025-06-22", Some("deadline")),
                date("开标", "2025-06-20", Some("bid_opening")),
            ],
            vec![],
        )
        .await;
        assert!(!out["contradictions"].as_array().unwrap().is_empty());
        let json = serde_json::to_string(&out).unwrap();
        assert!(
            !json.contains("legal_basis") && !json.contains("required_min") && !json.contains("violation"),
            "矛盾输出不得含法规状态: {}",
            json
        );
    }

    // ── opening vs deadline equality（87号令39条，4B-5D）────────

    #[tokio::test]
    async fn opening_before_deadline_contradiction() {
        // 开标(6/19) 严格早于 投标截止(6/20) → 时序矛盾
        let out = run(
            vec![
                date("投标截止", "2025-06-20", Some("deadline")),
                date("开标", "2025-06-19", Some("bid_opening")),
            ],
            vec![],
        )
        .await;
        assert_eq!(out["contradictions"].as_array().unwrap().len(), 1, "开标早于截止必须矛盾");
    }

    #[tokio::test]
    async fn opening_equal_deadline_no_contradiction() {
        // 开标(6/20) == 投标截止(6/20)（87号令39条：同一时间开标）→ 无矛盾
        let out = run(
            vec![
                date("投标截止", "2025-06-20", Some("deadline")),
                date("开标", "2025-06-20", Some("bid_opening")),
            ],
            vec![],
        )
        .await;
        assert!(
            out["contradictions"].as_array().unwrap().is_empty(),
            "开标与投标截止同日不得产生矛盾: {:?}",
            out["contradictions"]
        );
    }

    #[tokio::test]
    async fn opening_after_deadline_no_pure_sequence_contradiction() {
        // 开标(6/21) 晚于 投标截止(6/20)：纯时序允许；完整87号令39条合规判断不属于本 Tool
        let out = run(
            vec![
                date("投标截止", "2025-06-20", Some("deadline")),
                date("开标", "2025-06-21", Some("bid_opening")),
            ],
            vec![],
        )
        .await;
        assert!(
            out["contradictions"].as_array().unwrap().is_empty(),
            "开标晚于截止不得自动产生 contradiction: {:?}",
            out["contradictions"]
        );
    }

    // ── no legal output contract（§45）──────────────────────────

    #[tokio::test]
    async fn execute_output_has_no_legal_fields() {
        let out = run(
            vec![
                date("A", "2025-06-09", None),
                date("B", "2025-06-13", None),
            ],
            vec![calc("A", "B", "working_days")],
        )
        .await;
        let json = serde_json::to_string(&out).unwrap();
        for banned in [
            "legal_basis",
            "legal_ref",
            "required_min_days",
            "required_max_days",
            "compliant",
            "violation",
            "min_days",
        ] {
            assert!(!json.contains(banned), "输出不得包含 '{}': {}", banned, json);
        }
    }

    // ── architecture contract（4B-5D：旧实现已物理删除）──────────

    #[test]
    fn architecture_contract_old_impls_physically_removed() {
        // source-level 静态断言：旧实现标识符不得再出现在本文件
        let src = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/agents/tools/calculate_timeline.rs"
        ));
        let ids = [
            format!("infer_{}", "constraints"),
            format!("cn_{}", "holidays"),
            format!("workdays_{}", "between"),
            format!("calendar_days_{}", "between"),
            format!("date_to_{}", "julian"),
            format!("julian_to_{}", "date"),
            format!("4B-5{} 待删{}", "D", "除"),
            format!("#[allow(dead_{})]", "code)"),
        ];
        for id in ids {
            assert!(!src.contains(&id), "旧实现标识符仍存在于源码: {}", id);
        }
    }
}
