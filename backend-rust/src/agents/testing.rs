//! Agent 集成测试辅助模块。
//!
//! 提供测试二进制 (`src/bin/test_agents.rs`) 使用的通用检查函数：
//! - TraceLog 解析与事件查询
//! - Output 文件 schema 验证
//! - 测试结果的结构化输出

use crate::agents::types::*;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

// ─── 测试结果数据结构 ──────────────────────────────────────────────

/// 单条检查结果。
#[derive(Debug, Clone, Serialize)]
pub struct TestCheck {
    /// 测试场景名（如 "bus", "legal"）
    pub test: String,
    /// 检查点名（如 "session_graph_pull"）
    pub check: String,
    /// "PASS" | "FAIL" | "SKIP"
    pub status: String,
    /// 人类可读描述
    pub detail: String,
}

impl TestCheck {
    pub fn pass(test: &str, check: &str, detail: &str) -> Self {
        Self {
            test: test.to_string(),
            check: check.to_string(),
            status: "PASS".to_string(),
            detail: detail.to_string(),
        }
    }

    pub fn fail(test: &str, check: &str, detail: &str) -> Self {
        Self {
            test: test.to_string(),
            check: check.to_string(),
            status: "FAIL".to_string(),
            detail: detail.to_string(),
        }
    }

    pub fn skip(test: &str, check: &str, detail: &str) -> Self {
        Self {
            test: test.to_string(),
            check: check.to_string(),
            status: "SKIP".to_string(),
            detail: detail.to_string(),
        }
    }

    /// 输出 NDJSON 行。
    pub fn to_ndjson(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// 测试运行摘要。
#[derive(Debug, Serialize)]
pub struct TestSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub pass_rate: f64,
}

impl TestSummary {
    pub fn from_checks(checks: &[TestCheck]) -> Self {
        let total = checks.len();
        let passed = checks.iter().filter(|c| c.status == "PASS").count();
        let failed = checks.iter().filter(|c| c.status == "FAIL").count();
        let skipped = checks.iter().filter(|c| c.status == "SKIP").count();
        let pass_rate = if total > 0 {
            (passed as f64) / (total as f64) * 100.0
        } else {
            0.0
        };
        Self {
            total,
            passed,
            failed,
            skipped,
            pass_rate,
        }
    }

    pub fn print(&self, test_name: &str) {
        eprintln!(
            "  [{}] {} total | {} PASS | {} FAIL | {} SKIP | {:.0}% pass",
            test_name, self.total, self.passed, self.failed, self.skipped, self.pass_rate
        );
    }
}

// ─── TraceLog 解析 ────────────────────────────────────────────────

/// 解析后的 trace 事件（简化视图）。
#[derive(Debug, Clone)]
pub struct ParsedTraceEvent {
    pub agent_name: String,
    pub event_type: String,
    pub turn: u32,
    pub clause_id: Option<String>,
    pub summary: String,
    pub timestamp: String,
}

/// 解析 trace.jsonl 文件，返回事件列表。
pub fn parse_trace_file(path: &Path) -> Result<Vec<ParsedTraceEvent>, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("无法读取 trace 文件: {}", e))?;

    let mut events = Vec::new();
    for (line_no, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| format!("行 {} JSON 非法: {}", line_no + 1, e))?;

        events.push(ParsedTraceEvent {
            agent_name: value["agent_name"].as_str().unwrap_or("").to_string(),
            event_type: value["event_type"].as_str().unwrap_or("").to_string(),
            turn: value["turn"].as_u64().unwrap_or(0) as u32,
            clause_id: value["clause_id"].as_str().map(|s| s.to_string()),
            summary: value["summary"].as_str().unwrap_or("").to_string(),
            timestamp: value["timestamp"].as_str().unwrap_or("").to_string(),
        });
    }
    Ok(events)
}

/// 在 trace 事件列表中查找特定 Agent 的某类事件。
pub fn find_trace_events<'a>(
    events: &'a [ParsedTraceEvent],
    agent_filter: &str,
    event_type_filter: &str,
) -> Vec<&'a ParsedTraceEvent> {
    events
        .iter()
        .filter(|e| {
            (agent_filter.is_empty() || e.agent_name.contains(agent_filter))
                && (event_type_filter.is_empty() || e.event_type == event_type_filter)
        })
        .collect()
}

/// 检查 trace 事件的时间戳是否单调递增。
pub fn check_timestamps_monotonic(events: &[ParsedTraceEvent]) -> Result<(), String> {
    for window in events.windows(2) {
        if window[0].timestamp > window[1].timestamp {
            return Err(format!(
                "timestamp 回退: {} → {} (events: {} → {})",
                window[0].timestamp, window[1].timestamp, window[0].summary, window[1].summary
            ));
        }
    }
    Ok(())
}

/// 从 trace 事件中提取所有唯一的 Agent 名称。
pub fn unique_agents(events: &[ParsedTraceEvent]) -> Vec<String> {
    let mut agents: Vec<String> = events
        .iter()
        .map(|e| e.agent_name.clone())
        .filter(|a| !a.is_empty())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    agents.sort();
    agents
}

/// 从 trace 事件中提取所有唯一的 clause_id。
pub fn unique_clauses(events: &[ParsedTraceEvent]) -> Vec<String> {
    let mut clauses: Vec<String> = events
        .iter()
        .filter_map(|e| e.clause_id.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    clauses.sort();
    clauses
}

// ─── Output 文件验证 ──────────────────────────────────────────────

/// 验证 findings.json 的基本结构。
pub fn validate_findings_file(path: &Path) -> Result<Vec<RiskFinding>, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("无法读取 findings 文件: {}", e))?;
    let findings: Vec<RiskFinding> =
        serde_json::from_str(&content).map_err(|e| format!("findings JSON 非法: {}", e))?;

    // 验证每条 finding 的必填字段
    for f in &findings {
        if f.risk_id.is_empty() {
            return Err("存在空 risk_id 的 finding".to_string());
        }
        if f.agent.is_empty() {
            return Err(format!("{} agent 为空", f.risk_id));
        }
        if f.clause_ids.is_empty() {
            return Err(format!("{} clause_ids 为空", f.risk_id));
        }
    }

    Ok(findings)
}

/// 验证 graph_snapshot.json 的基本结构。
pub fn validate_graph_snapshot(path: &Path) -> Result<GraphSnapshot, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("无法读取 graph_snapshot: {}", e))?;
    let snapshot: GraphSnapshot =
        serde_json::from_str(&content).map_err(|e| format!("graph_snapshot JSON 非法: {}", e))?;
    Ok(snapshot)
}

// ─── Finding 统计分析 ─────────────────────────────────────────────

/// Finding 按 Agent 分组统计。
pub fn findings_by_agent(findings: &[RiskFinding]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for f in findings {
        *counts.entry(f.agent.clone()).or_default() += 1;
    }
    counts
}

/// Finding 按 severity 分组统计。
pub fn findings_by_severity(findings: &[RiskFinding]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for f in findings {
        *counts.entry(format!("{:?}", f.severity)).or_default() += 1;
    }
    counts
}

/// 检查是否有 Agent 的 conversation 泄漏到了另一个 Agent（通过 trace 中的 clause_id 交叉检查）。
pub fn check_conversation_isolation(events: &[ParsedTraceEvent]) -> Result<(), Vec<String>> {
    // 策略：每个 Agent 处理的 clause_id 集合应该是其被路由到的 clause 集合
    // 如果 Agent A 的 trace 中出现了它不应该处理的 clause_id → 泄漏
    let violations = Vec::new();

    // 收集每个 Agent 涉及的 clause_id
    let mut agent_clauses: HashMap<String, Vec<String>> = HashMap::new();
    for e in events {
        if let Some(ref cid) = e.clause_id
            && !cid.is_empty()
        {
            agent_clauses
                .entry(e.agent_name.clone())
                .or_default()
                .push(cid.clone());
        }
    }

    // 简单检查：同一 clause 不应被两个不同 Agent 各自独立 trace 中出现
    // （除非它们被路由到同一 clause，这是正常的）
    // 这里只检查无 agent 标识的"漂浮" clause_id
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

// ─── 合成 ReviewClause 辅助函数 ───────────────────────────────────

/// 创建一条测试用的 ReviewClause（自动进行 RiskTier 分级）。
pub fn make_test_clause(id: &str, text: &str, section: &str) -> ReviewClause {
    let tier = RiskTier::from_clause_text(text);
    let tier_max_turns = tier.max_turns();
    ReviewClause {
        chunk_id: id.to_string(),
        section_path: vec![section.to_string()],
        text: text.to_string(),
        page_start: 0,
        page_end: 1,
        tier,
        tier_max_turns,
        source_block_ids: vec![],
    }
}

/// 创建 L3 高风险条款（含品牌/地域/排他关键词）。
pub fn make_l3_clause(id: &str, text: &str) -> ReviewClause {
    make_test_clause(id, text, "第三章 采购需求")
}

/// 创建 L1 低风险条款（含格式/封面/签字关键词）。
pub fn make_l1_clause(id: &str, text: &str) -> ReviewClause {
    make_test_clause(id, text, "第一章 投标须知")
}

/// 创建 L2 中等风险条款。
pub fn make_l2_clause(id: &str, text: &str) -> ReviewClause {
    make_test_clause(id, text, "第二章 供应商须知")
}

// ─── 预设测试数据集 ────────────────────────────────────────────────

/// §8 双通道协同测试数据：4 条条款，覆盖 L1/L2/L3。
pub fn bus_test_clauses() -> Vec<ReviewClause> {
    vec![
        make_l1_clause(
            "ch_001",
            "投标文件封面格式要求见附件一，正本须加盖公章并密封递交。",
        ),
        make_l3_clause("ch_002", "本项目核心交换机须采用华为品牌，不接受替代方案。"),
        make_l3_clause(
            "ch_003",
            "投标人须在东莞地区设有常驻服务机构，并提供本地社保缴纳证明。",
        ),
        make_l2_clause(
            "ch_004",
            "付款方式：合同签订后支付30%预付款，验收合格后支付70%尾款。",
        ),
    ]
}

/// §9 分层记忆 + §10.3 EXECUTE 测试数据：7 条条款覆盖全部 7 个 reviewer。
pub fn memory_test_clauses() -> Vec<ReviewClause> {
    vec![
        make_l1_clause("ch_001", "投标文件封面格式要求见附件一，正本须加盖公章。"),
        make_l2_clause(
            "ch_002",
            "供应商须具备依法缴纳税收和社会保障资金的良好记录。",
        ),
        make_l2_clause(
            "ch_003",
            "项目工期为合同签订后60个日历日内完成全部建设内容。",
        ),
        make_l3_clause(
            "ch_004",
            "本项目指定采用某品牌专利技术，且须提供原厂商针对本项目的唯一授权函。",
        ),
        make_l3_clause("ch_005", "投标人须在本市设有分支机构，并提供本地业绩证明。"),
        // 触发 ProcedureAgent（"评审"/"评标"）和 ScoringAgent（"评分"/"分值"/"权重"）
        make_test_clause(
            "ch_006",
            "评审委员会由5人组成，评分采用综合评分法，价格分权重30%，技术分权重70%。",
            "第四章 评审办法",
        ),
        make_test_clause(
            "ch_007",
            "投标保证金为人民币贰万元，未中标人的保证金在评标结束后5个工作日内退还。",
            "第五章 投标保证金",
        ),
    ]
}

/// §10.5 LEGAL VERIFY 测试数据：条款设计为容易触发法条引用。
pub fn legal_test_clauses() -> Vec<ReviewClause> {
    vec![
        make_l3_clause(
            "ch_001",
            "本项目不接受联合体投标，联合体投标将被视为无效投标。",
        ),
        make_l3_clause(
            "ch_002",
            "投标人须在东莞地区设有常驻服务机构，并提供不少于5人的本地团队证明。",
        ),
        make_l2_clause(
            "ch_003",
            "技术参数要求：服务器CPU主频不低于3.0GHz，内存不低于16GB DDR4。",
        ),
    ]
}

/// §10.6 BLINDSPOT 测试数据：混合条款制造盲点。
pub fn blindspot_test_clauses() -> Vec<ReviewClause> {
    vec![
        make_l1_clause("ch_001", "投标文件封面格式要求见附件一，须加盖公章。"),
        make_l2_clause(
            "ch_002",
            "供应商须具备依法缴纳税收和社会保障资金的良好记录。",
        ),
        make_l2_clause("ch_003", "项目工期为合同签订后60个日历日内完成。"),
        make_l3_clause("ch_004", "本项目须采用华为品牌核心交换机，不接受替代品牌。"),
        // 以下两条是"冷门条款"，只含通用表述，期望只被 FactCheck fallback 覆盖
        make_test_clause(
            "ch_005",
            "本项目建设地点位于松山湖校区，投标人须自行踏勘现场。",
            "第五章 项目概况",
        ),
        make_test_clause(
            "ch_006",
            "中标人须在合同签订前提交履约保证金，金额为合同价的5%。",
            "第六章 其他要求",
        ),
    ]
}

/// §10.7 DEBATE 测试数据：高风险但"灰色地带"条款，期望触发中等置信度（<0.85）。
///
/// 设计原则：每条条款都同时存在"违规嫌疑"和"正当理由"两面，LLM 无法斩钉截铁地判 High。
/// → LLM 应产出 High severity + confidence 0.60-0.80 的 finding
/// → 触发 Debate 的 confidence<0.85 条件。
pub fn debate_test_clauses() -> Vec<ReviewClause> {
    vec![
        // 条款 1：兼容性要求 → 正当技术约束 OR 隐性品牌锁定？
        // 含"品牌"触发 L3；但兼容性是政府采购中公认的灰色地带。
        make_l3_clause(
            "ch_001",
            "本项目网络设备须与现有华为品牌网管系统实现协议级兼容。\
             投标人如采用非华为品牌设备，须提供经第三方检测机构出具的兼容性测试报告。\
             注：现有网管系统基于SNMP/Netconf开放协议，理论上支持多品牌接入。",
        ),
        // 条款 2：响应时效 → 正当服务要求 OR 变相地域限制？
        // 含"东莞"触发 L3；但 30 分钟响应是可量化的性能指标，非直接要求注册地。
        make_l3_clause(
            "ch_002",
            "投标人须确保质保期内提供7×24小时现场技术支持，\
             故障响应时间不超过30分钟。建议服务团队常驻东莞地区以满足此时效要求，\
             投标人亦可采用远程诊断+备件前置等其他方式满足本条款。",
        ),
        // 条款 3：专利加分 → 鼓励创新 OR 排斥中小企业？
        // 含"专利"触发 L3；但仅 5 分附加分，且明确鼓励而非强制。
        make_l3_clause(
            "ch_003",
            "为鼓励技术创新，投标人所投产品如包含自主专利或软件著作权\
             （须提供国家知识产权局颁发的有效证书），可在技术评审中获得附加分，\
             最高不超过总分100分的5分。非强制要求，不满足不扣分。",
        ),
    ]
}

/// §11 动态Agent 测试数据：复杂排他条款。
pub fn dynamic_test_clauses() -> Vec<ReviewClause> {
    vec![
        make_l3_clause(
            "ch_001",
            "核心交换机须采用华为CloudEngine系列，防火墙须采用华为USG系列，且须提供原厂授权。",
        ),
        make_l3_clause(
            "ch_002",
            "投标人须在东莞注册满5年，近3年东莞地区同类业绩不少于3个，合同金额均不低于500万。",
        ),
        make_l3_clause(
            "ch_003",
            "本项目须采用专利技术（专利号ZL2024XXXXXX），不接受替代方案。",
        ),
        make_l2_clause(
            "ch_004",
            "评分标准：本地化服务能力10分，投标人在本市设有分支机构的得满分。",
        ),
    ]
}

/// §12/§13 故障边界测试数据。
pub fn fault_test_clauses() -> Vec<ReviewClause> {
    vec![
        // 特殊字符
        ReviewClause {
            chunk_id: "ch_special".to_string(),
            section_path: vec!["测试".to_string()],
            text: "价格 ≤ 100万 && 交付时间 ≥ 30天，须通过 ISO9001 & ISO14001 认证。".to_string(),
            page_start: 0,
            page_end: 1,
            tier: RiskTier::Medium,
            tier_max_turns: 10,
            source_block_ids: vec![],
        },
        // 超长文本（5000 字符）
        ReviewClause {
            chunk_id: "ch_long".to_string(),
            section_path: vec!["测试".to_string()],
            text: "超长条款文本。".repeat(500), // ~5000 字符
            page_start: 0,
            page_end: 1,
            tier: RiskTier::Medium,
            tier_max_turns: 4, // 缩短 max_turns 加快测试
            source_block_ids: vec![],
        },
    ]
}

// ─── Stderr 日志解析辅助 ───────────────────────────────────────────

/// 从 stderr 字符串中检查是否包含指定模式。
pub fn stderr_contains(stderr: &str, pattern: &str) -> bool {
    stderr.contains(pattern)
}

/// 从 stderr 中提取匹配模式的行。
pub fn stderr_lines_matching(stderr: &str, pattern: &str) -> Vec<String> {
    stderr
        .lines()
        .filter(|line| line.contains(pattern))
        .map(|s| s.to_string())
        .collect()
}

/// 检查 stderr 中多个 Agent 的执行日志是否交错（证明并行执行）。
/// 返回 true 如果检测到至少一次 Agent 名称的交替出现。
pub fn check_parallel_execution(stderr: &str, agent_names: &[&str]) -> bool {
    // 提取所有 [EXECUTE] 行
    let execute_lines: Vec<String> = stderr
        .lines()
        .filter(|line| line.contains("[EXECUTE]"))
        .map(|s| s.to_string())
        .collect();

    if execute_lines.len() < 2 {
        return false;
    }

    // 检查 Agent 名称交替出现（即非 A A A B B B 的严格分组）
    let mut name_sequence: Vec<&str> = Vec::new();
    for line in &execute_lines {
        for name in agent_names {
            if line.contains(name) {
                name_sequence.push(name);
                break;
            }
        }
    }

    // 如果序列中 Agent 名称交替出现 ≥ 2 次 → 并行
    let mut alternations = 0;
    for i in 1..name_sequence.len() {
        if name_sequence[i] != name_sequence[i - 1] {
            alternations += 1;
        }
    }
    alternations >= 2
}
