//! Tool Selection Eval 数据集 + Runner。
//!
//! 注意：本模块需要真实 LLM API Key（DASHSCOPE_API_KEY / OPENAI_API_KEY）才能运行。
//! 没有 Key 时，仅验证数据集和 runner 可编译、逻辑正确，不产生伪造结果。
//!
//! 运行方式（配置好 Key 后）：
//!   $env:DASHSCOPE_API_KEY="..."; cargo test --lib -- 'eval' -- --nocapture
//!
//! 指标定义：
//! - Tool Recall: 应该调用某工具的案例中，正确调用的比例
//! - Tool Precision: 实际调用的工具中，真正应该调用的比例
//! - Wrong Tool Rate: 同 Agent 下选择了错误工具的比例
//! - False Tool Call Rate: 不该调用工具却调用的比例
//! - Argument Accuracy: 工具选对后参数构造正确的比例
//! - Result Adoption: 工具确定性结果被最终 Finding 采用的比例

#[cfg(test)]
pub mod tool_selection_eval {
    use std::collections::HashMap;

    /// Eval Case 定义
    pub struct EvalCase {
        pub case_id: &'static str,
        pub clause: &'static str,
        pub expected_agent: &'static str,
        pub expected_tool: &'static str,
        /// Required: 必须调用该工具；Preferred: 推荐调用；Optional: 可选；Negative: 不应调用
        pub should_call: CallRequirement,
        /// 预期关键参数（用于 Argument Accuracy）
        pub expected_key_args: &'static [(&'static str, &'static str)],
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum CallRequirement {
        Required,
        Preferred,
        Optional,
        Negative,
    }

    // ─── Procedure Agent 正例 ────────────────────────────────────

    pub const CASE_PROC_DEPOSIT: EvalCase = EvalCase {
        case_id: "proc_001",
        clause: "投标人须在投标截止时间前提交投标保证金人民币壹拾万元整（¥100,000.00），以现金形式缴纳，中标通知书发出后10个工作日内退还。合同估算金额为人民币300万元。",
        expected_agent: "Procedure",
        expected_tool: "verify_bid_deposit",
        should_call: CallRequirement::Required,
        expected_key_args: &[("deposit_type", "bid")],
    };

    pub const CASE_PROC_NOTICE_PUBLICATION: EvalCase = EvalCase {
        case_id: "proc_002a",
        clause: "招标公告于2025年6月2日发布，公告期至2025年6月6日结束，共4个工作日。本项目采用公开招标方式，采购货物。",
        expected_agent: "Procedure",
        expected_tool: "verify_announcement_period",
        should_call: CallRequirement::Required,
        expected_key_args: &[("procurement_method", "公开招标"), ("period_type", "notice_publication")],
    };

    pub const CASE_PROC_BID_PREPARATION: EvalCase = EvalCase {
        case_id: "proc_002b",
        clause: "招标文件自2025年6月1日起发出，投标截止时间为2025年6月14日。本项目采用公开招标方式，采购货物。",
        expected_agent: "Procedure",
        expected_tool: "verify_bid_preparation_period",
        should_call: CallRequirement::Required,
        expected_key_args: &[("procurement_method", "公开招标")],
    };

    pub const CASE_PROC_PREPARATION: EvalCase = EvalCase {
        case_id: "proc_003",
        clause: "公告发布之日起至投标截止之日止，投标准备时间仅为8个日历日，本项目采用竞争性谈判方式。",
        expected_agent: "Procedure",
        expected_tool: "verify_bid_preparation_period",
        should_call: CallRequirement::Required,
        expected_key_args: &[("procurement_method", "竞争性谈判")],
    };

    pub const CASE_PROC_METHOD: EvalCase = EvalCase {
        case_id: "proc_004",
        clause: "本项目预算金额为人民币500万元，采购货物一批，拟采用竞争性磋商方式进行采购。",
        expected_agent: "Procedure",
        expected_tool: "verify_procurement_method",
        should_call: CallRequirement::Required,
        expected_key_args: &[("procurement_category", "货物"), ("budget_amount_wan", "500")],
    };

    // ─── Procedure Agent 负例/交叉工具 ──────────────────────────

    pub const CASE_PROC_CROSS_TIMELINE: EvalCase = EvalCase {
        case_id: "proc_005",
        clause: "2025年6月1日发布公告，2025年6月25日开标，2025年6月20日发售招标文件，2025年6月30日签订合同。请计算各节点之间的日期关系。",
        expected_agent: "Procedure",
        expected_tool: "calculate_timeline",
        should_call: CallRequirement::Preferred,
        expected_key_args: &[],
    };

    pub const CASE_PROC_NEGATIVE_PAYMENT: EvalCase = EvalCase {
        case_id: "proc_006",
        clause: "合同签订后30日内支付合同金额的50%作为预付款，余款在验收合格后15日内付清。",
        expected_agent: "Procedure",
        expected_tool: "NONE",
        should_call: CallRequirement::Negative,
        expected_key_args: &[],
    };

    // ─── Scoring Agent 正例 ─────────────────────────────────────

    pub const CASE_SCORE_FORMULA: EvalCase = EvalCase {
        case_id: "score_001",
        clause: "价格分权重占比70%，采用最低价法计算价格分，基准价为所有有效投标报价的算术平均值。",
        expected_agent: "Scoring",
        expected_tool: "validate_scoring_formula",
        should_call: CallRequirement::Required,
        expected_key_args: &[("procurement_object", "goods"), ("procurement_method", "open_tender"), ("price_weight", "70")],
    };

    pub const CASE_SCORE_WEIGHT: EvalCase = EvalCase {
        case_id: "score_002",
        clause: "评审因素权重分配如下：价格40分，技术50分，商务5分，服务5分，总分100分。",
        expected_agent: "Scoring",
        expected_tool: "validate_weight_distribution",
        should_call: CallRequirement::Required,
        expected_key_args: &[("procurement_category", "货物")],
    };

    pub const CASE_SCORE_SUBJECTIVE: EvalCase = EvalCase {
        case_id: "score_003",
        clause: "技术方案评分区间为0-20分，评委根据投标人的综合表现和满意程度酌情打分。",
        expected_agent: "Scoring",
        expected_tool: "detect_subjective_scoring",
        should_call: CallRequirement::Required,
        expected_key_args: &[],
    };

    pub const CASE_SCORE_COMPLETENESS: EvalCase = EvalCase {
        case_id: "score_004",
        clause: "评审因素表：价格分30分，技术分50分，商务分15分。总分应为100分但表格合计95分。",
        expected_agent: "Scoring",
        expected_tool: "check_scoring_completeness",
        should_call: CallRequirement::Required,
        expected_key_args: &[("procurement_category", "货物")],
    };

    pub const CASE_SCORE_CONSORTIUM: EvalCase = EvalCase {
        case_id: "score_005",
        clause: "本项目接受联合体投标。联合体成员资质可以叠加计算，牵头方须具备施工总承包一级资质。",
        expected_agent: "Scoring",
        expected_tool: "verify_consortium_rules",
        should_call: CallRequirement::Required,
        expected_key_args: &[],
    };

    // ─── Scoring 负例 ───────────────────────────────────────────

    pub const CASE_SCORE_NEGATIVE_BRAND: EvalCase = EvalCase {
        case_id: "score_006",
        clause: "本项目核心设备须为某指定品牌原装进口产品，不接受其他品牌，且投标人须提供厂家授权书。",
        expected_agent: "Demand",
        expected_tool: "NONE",
        should_call: CallRequirement::Negative,
        expected_key_args: &[],
    };

    // ─── Demand Agent 正例 ──────────────────────────────────────

    pub const CASE_DEMAND_IMPORTED: EvalCase = EvalCase {
        case_id: "demand_001",
        clause: "本项目拟采购进口医疗影像设备，采购前须经省级以上财政部门审核同意。",
        expected_agent: "Demand",
        expected_tool: "check_imported_products",
        should_call: CallRequirement::Required,
        expected_key_args: &[("procurement_category", "货物")],
    };

    // ─── 完整数据集 ─────────────────────────────────────────────

    pub fn all_cases() -> Vec<&'static EvalCase> {
        vec![
            &CASE_PROC_DEPOSIT,
            &CASE_PROC_NOTICE_PUBLICATION,
            &CASE_PROC_BID_PREPARATION,
            &CASE_PROC_PREPARATION,
            &CASE_PROC_METHOD,
            &CASE_PROC_CROSS_TIMELINE,
            &CASE_PROC_NEGATIVE_PAYMENT,
            &CASE_SCORE_FORMULA,
            &CASE_SCORE_WEIGHT,
            &CASE_SCORE_SUBJECTIVE,
            &CASE_SCORE_COMPLETENESS,
            &CASE_SCORE_CONSORTIUM,
            &CASE_SCORE_NEGATIVE_BRAND,
            &CASE_DEMAND_IMPORTED,
        ]
    }

    /// 数据集完整性验证（不需要 LLM）
    #[test]
    fn test_eval_dataset_complete() {
        let cases = all_cases();
        assert!(cases.len() >= 10, "数据集应至少 10 条，实际 {}", cases.len());

        let mut seen = HashMap::new();
        for c in &cases {
            assert!(
                seen.insert(c.case_id, true).is_none(),
                "case_id '{}' 重复",
                c.case_id
            );
        }

        let has_positive = cases.iter().any(|c| c.should_call != CallRequirement::Negative);
        let has_negative = cases.iter().any(|c| c.should_call == CallRequirement::Negative);
        assert!(has_positive, "缺少正例");
        assert!(has_negative, "缺少负例");

        for agent in ["Procedure", "Scoring", "Demand"] {
            assert!(
                cases.iter().any(|c| c.expected_agent == agent),
                "缺少 {} Agent 的用例",
                agent
            );
        }

        // expected_tool 必须与 expected_agent 匹配
        let tool_agent_map: HashMap<&str, &str> = [
            ("verify_bid_deposit", "Procedure"),
            ("verify_announcement_period", "Procedure"),
            ("verify_bid_preparation_period", "Procedure"),
            ("verify_procurement_method", "Procedure"),
            ("calculate_timeline", "Procedure"),
            ("validate_scoring_formula", "Scoring"),
            ("validate_weight_distribution", "Scoring"),
            ("detect_subjective_scoring", "Scoring"),
            ("check_scoring_completeness", "Scoring"),
            ("verify_consortium_rules", "Scoring"),
            ("check_imported_products", "Demand"),
        ]
        .iter()
        .map(|(t, a)| (*t, *a))
        .collect();

        for c in &cases {
            if c.should_call != CallRequirement::Negative {
                if let Some(agent) = tool_agent_map.get(c.expected_tool) {
                    assert_eq!(
                        *agent, c.expected_agent,
                        "case '{}' 工具 '{}' 与 Agent '{}' 不匹配（应为 {}）",
                        c.case_id, c.expected_tool, c.expected_agent, agent
                    );
                }
            }
        }

        println!("Eval 数据集验证通过：{} 条用例", cases.len());
    }

    /// 数据集静态覆盖检查：所有 Group A 核心工具至少有一条正例
    #[test]
    fn test_eval_dataset_covers_core_tools() {
        let cases = all_cases();
        let covered: Vec<&str> = cases
            .iter()
            .filter(|c| c.should_call != CallRequirement::Negative)
            .map(|c| c.expected_tool)
            .collect();

        for tool in [
            "verify_bid_deposit", "verify_announcement_period",
            "verify_bid_preparation_period", "verify_procurement_method",
            "calculate_timeline",
        ] {
            assert!(covered.contains(&tool), "数据集缺少 '{}' 的正例", tool);
        }
        for tool in [
            "validate_scoring_formula", "validate_weight_distribution",
            "detect_subjective_scoring", "check_scoring_completeness",
            "verify_consortium_rules",
        ] {
            assert!(covered.contains(&tool), "数据集缺少 '{}' 的正例", tool);
        }
        assert!(covered.contains(&"check_imported_products"), "数据集缺少 check_imported_products 的正例");

        println!("Eval 数据集覆盖检查通过：全部 11 个核心工具均有正例");
    }
}
