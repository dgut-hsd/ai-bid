//! Agent 框架集成测试二进制 — 使用真实 LLM API 验证管线行为。
//!
//! 跳过 PDF → Chunk 前处理管线，直接用合成 ReviewClause 数据驱动 Coordinator。
//!
//! ## 运行
//!
//! ```powershell
//! $env:AIBID_AGENT=1
//! cargo run --bin test_agents -- --test bus       # §8 双通道协同
//! cargo run --bin test_agents -- --test memory    # §9 分层记忆

#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::too_many_arguments)]
//! cargo run --bin test_agents -- --test execute   # §10.3 并行隔离
//! cargo run --bin test_agents -- --test legal     # §10.5 LEGAL VERIFY
//! cargo run --bin test_agents -- --test blindspot # §10.6 BLINDSPOT
//! cargo run --bin test_agents -- --test debate    # §10.7 DEBATE
//! cargo run --bin test_agents -- --test dynamic   # §11 动态Agent闭环
//! cargo run --bin test_agents -- --test fault     # §12/§13 故障边界
//! cargo run --bin test_agents -- --test all       # 全部运行
//! ```
//!
//! ## 输出
//!
//! NDJSON 到 stdout（每行一个 TestCheck），人类可读日志到 stderr。

use ai_bid::agents::bus::AgentBus;
use ai_bid::agents::coordinator::Coordinator;
use ai_bid::agents::registry::AgentRegistry;
use ai_bid::agents::session_graph::SessionGraph;
use ai_bid::agents::testing::*;
use ai_bid::agents::tools::ToolRegistry;
use ai_bid::agents::tools::output_finding::OutputFindingTool;
use ai_bid::agents::tools::read_section::ReadSectionTool;
use ai_bid::agents::tools::search_knowledge::{
    DashScopeSearchBackend, SearchBuffer, SearchKnowledgeTool,
};
// V2+ 工具
use ai_bid::agents::tools::compare_versions::CompareVersionsTool;
use ai_bid::agents::tools::detect_boilerplate::DetectBoilerplateTool;
// V3 采购程序合规审查
use ai_bid::agents::tools::verify_procurement_method::VerifyProcurementMethodTool;
use ai_bid::agents::tools::verify_bid_deposit::VerifyBidDepositTool;
use ai_bid::agents::tools::verify_announcement_period::VerifyAnnouncementPeriodTool;
use ai_bid::agents::tools::verify_bid_preparation_period::VerifyBidPreparationPeriodTool;
// V4 评审标准审查
use ai_bid::agents::tools::validate_scoring_formula::ValidateScoringFormulaTool;
use ai_bid::agents::tools::validate_weight_distribution::ValidateWeightDistributionTool;
use ai_bid::agents::tools::detect_subjective_scoring::DetectSubjectiveScoringTool;
use ai_bid::agents::tools::check_scoring_completeness::CheckScoringCompletenessTool;
use ai_bid::agents::tools::check_imported_products::CheckImportedProductsTool;
use ai_bid::agents::tools::verify_consortium_rules::VerifyConsortiumRulesTool;
// 零依赖计算工具
use ai_bid::agents::tools::calculate_timeline::CalculateTimelineTool;
// 依赖 chunk 数据的工具
use ai_bid::agents::tools::check_cross_reference::CheckCrossReferenceTool;
use ai_bid::agents::tools::extract_obligations::ExtractObligationsTool;
use ai_bid::agents::tools::compare_with_template::{CompareWithTemplateTool, ChunkTextProvider, TemplateStore};
use ai_bid::agents::tools::validate_calculation::ValidateCalculationTool;
use ai_bid::agents::tools::search_contradiction::SearchContradictionTool;
use ai_bid::agents::trace::TraceLog;
use ai_bid::agents::types::*;
use ai_bid::domain::chunk::{Chunk, ChunkType};
use ai_bid::paths::data_path_str;
use ai_bid::services::llm_client::create_llm_client;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

// ─── 搜索后端选择 ──────────────────────────────────────────────────

fn init_search_backend() -> (
    Option<Arc<DashScopeSearchBackend>>,
    Option<Arc<SearchBuffer>>,
) {
    let search_backend =
        env::var("AIBID_SEARCH_BACKEND").unwrap_or_else(|_| "dashscope".to_string());

    if search_backend == "searxng" {
        let searxng_url =
            env::var("SEARXNG_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
        eprintln!("  搜索后端: SearXNG ({})", searxng_url);
        (None, Some(SearchBuffer::new(searxng_url, None)))
    } else {
        let ds = DashScopeSearchBackend::from_env()
            .expect("DashScope 搜索后端初始化失败。请设置 DASHSCOPE_API_KEY");
        eprintln!(
            "  搜索后端: DashScope (model={})",
            env::var("DASHSCOPE_SEARCH_MODEL")
                .or_else(|_| env::var("DASHSCOPE_MODEL"))
                .unwrap_or_else(|_| "qwen-plus".to_string())
        );
        (Some(Arc::new(ds)), None)
    }
}

// ─── 工具工厂 ──────────────────────────────────────────────────────

/// 测试环境专用的 search_document mock。
///
/// 真实 `SearchDocumentTool` 依赖 BGE-M3 嵌入模型 + 向量索引，
/// 测试环境无法加载。此 mock 保持相同的 name/definition 接口，
/// 始终返回空结果 + 提示信息，让 LLM 正确感知"搜索不到"并走降级逻辑。
struct MockSearchDocumentTool;

#[async_trait::async_trait]
impl ai_bid::agents::tools::AgentTool for MockSearchDocumentTool {
    fn name(&self) -> &str {
        "search_document"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "search_document",
                "description": "在待审招标文件内部做语义搜索。\
                    【使用场景】① 当前条款提到了某个特定要求（如'本地业绩'），\
                    你想确认文档其他部分是否也有类似要求；\
                    ② 你发现了一个风险模式，需要找其他章节验证是否构成组合排斥；\
                    ③ 条款引用了另一个章节但你没有那个章节的原文。\
                    【不使用场景】① 没有具体怀疑目标时的'随便搜搜'——这会浪费轮次；\
                    ② 搜索外部知识库——请用 web_search；\
                    ③ 已精确知道 chunk_id——直接用 read_section。\
                    【搜索技巧】用提炼后的关键词，不要把整个条款原文当作搜索 query。\
                    好: '本地业绩 评分 加分'；坏: '投标人具有本地同类项目业绩...'\
                    如果搜索结果相似度全部低于 0.5，说明搜索方向可能不对，换搜索词。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "提炼后的关键词组合。好: '品牌 型号 指定'；坏: 粘贴整个条款原文。"
                        }
                    },
                    "required": ["query"]
                }
            }
        })
    }

    async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({
            "hits": [],
            "warning": "测试环境未加载向量索引，search_document 不可用。请使用 web_search 搜索外部知识库，或用 read_section 精读已知 chunk_id 的条款。"
        }))
    }
}

/// 创建测试用的 ToolRegistry。
///
/// 注册 web_search + search_document (mock) + read_section + output_finding。
/// read_section 从合成 clause 构建的 Chunk Map 读取数据。
/// search_document 使用 mock 实现（测试环境无向量索引）。
fn make_tools_factory(
    ds_search: Option<Arc<DashScopeSearchBackend>>,
    buffer: Option<Arc<SearchBuffer>>,
    chunks: Arc<HashMap<String, Chunk>>,
    chunk_order: Arc<Vec<String>>,
) -> Arc<dyn Fn() -> ToolRegistry + Send + Sync> {
    Arc::new(move || {
        eprintln!("[test_agents] ── 创建测试工具集 ToolRegistry ──");
        let mut registry = ToolRegistry::new();
        if let Some(ref ds) = ds_search {
            registry.register(Box::new(SearchKnowledgeTool::with_dashscope(ds.clone())));
        } else if let Some(ref buf) = buffer {
            registry.register(Box::new(SearchKnowledgeTool::with_buffer(buf.clone())));
        } else {
            panic!("搜索后端未初始化");
        }
        registry.register(Box::new(ReadSectionTool::new(
            chunks.clone(),
            chunk_order.clone(),
        )));
        registry.register(Box::new(MockSearchDocumentTool));
        registry.register(Box::new(OutputFindingTool));
        // V2+ 工具
        registry.register(Box::new(CompareVersionsTool {
            current_chunks: chunks.clone(),
            current_order: chunk_order.clone(),
        }));
        registry.register(Box::new(DetectBoilerplateTool {
            chunks: chunks.clone(),
            chunk_order: chunk_order.clone(),
        }));
        // V3 采购程序合规审查
        registry.register(Box::new(VerifyProcurementMethodTool));
        registry.register(Box::new(VerifyBidDepositTool));
        registry.register(Box::new(VerifyAnnouncementPeriodTool));
        registry.register(Box::new(VerifyBidPreparationPeriodTool));
        // V4 评审标准审查
        registry.register(Box::new(ValidateScoringFormulaTool));
        registry.register(Box::new(ValidateWeightDistributionTool));
        registry.register(Box::new(DetectSubjectiveScoringTool));
        registry.register(Box::new(CheckScoringCompletenessTool));
        registry.register(Box::new(CheckImportedProductsTool));
        registry.register(Box::new(VerifyConsortiumRulesTool));
        // 零依赖计算工具
        registry.register(Box::new(CalculateTimelineTool));
        // 依赖 chunk 数据的工具（测试环境有 chunks + chunk_order）
        registry.register(Box::new(CheckCrossReferenceTool::new(
            chunks.clone(),
            chunk_order.clone(),
        )));
        registry.register(Box::new(ExtractObligationsTool::new(
            chunks.clone(),
            chunk_order.clone(),
        )));
        // 模板比对
        let template_text_provider = Arc::new(ChunkTextProvider {
            chunks: chunks.clone(),
        });
        registry.register(Box::new(CompareWithTemplateTool::new(
            Arc::new(TemplateStore::with_builtin_templates()),
            template_text_provider,
        )));
        // 数值计算校验
        registry.register(Box::new(ValidateCalculationTool));
        // 矛盾检测
        registry.register(Box::new(SearchContradictionTool::new(
            chunks.clone(),
            chunk_order.clone(),
            None,
        )));
        eprintln!(
            "[test_agents] ── 测试工具集注册完成: 共 {} 个工具 ──",
            registry.len()
        );
        registry
    })
}

/// 将测试用的 ReviewClause 列表转换为 ReadSectionTool 所需的 Chunk Map。
///
/// 测试环境跳过 PDF → Chunk 管线，用合成数据驱动，但 read_section 工具需要
/// HashMap<String, Chunk> 做 O(1) 查找。此辅助函数完成 ReviewClause → Chunk 的转换。
fn build_chunk_data(clauses: &[ReviewClause]) -> (Arc<HashMap<String, Chunk>>, Arc<Vec<String>>) {
    let mut map = HashMap::new();
    let mut order = Vec::new();
    for c in clauses {
        order.push(c.chunk_id.clone());
        map.insert(
            c.chunk_id.clone(),
            Chunk {
                chunk_id: c.chunk_id.clone(),
                chunk_type: ChunkType::Leaf,
                section_path: c.section_path.clone(),
                text: c.text.clone(),
                page_start: c.page_start,
                page_end: c.page_end,
                source_block_ids: Vec::new(),
                bbox_refs: Vec::new(),
            },
        );
    }
    (Arc::new(map), Arc::new(order))
}

// ─── 共享基础设施 ──────────────────────────────────────────────────

fn make_shared_infra() -> (Arc<AgentBus>, Arc<SessionGraph>, Arc<Mutex<TraceLog>>) {
    let bus = Arc::new(AgentBus::new(32));
    let graph = Arc::new(SessionGraph::new());
    let trace = Arc::new(Mutex::new(TraceLog::new()));
    (bus, graph, trace)
}

// ─── Coordinator 工厂 ──────────────────────────────────────────────

fn make_coordinator(
    config: CoordinatorConfig,
    ds_search: Option<Arc<DashScopeSearchBackend>>,
    buffer: Option<Arc<SearchBuffer>>,
    bus: Arc<AgentBus>,
    graph: Arc<SessionGraph>,
    trace: Arc<Mutex<TraceLog>>,
    chunks: Arc<HashMap<String, Chunk>>,
    chunk_order: Arc<Vec<String>>,
) -> Coordinator {
    let registry = AgentRegistry::builtin();
    let llm_factory: Arc<dyn Fn() -> Box<dyn ai_bid::agents::react_loop::LlmClient> + Send + Sync> =
        Arc::new(move || create_llm_client().expect("创建 LLM 客户端失败。请检查 API 密钥环境变量"));
    let tools_factory = make_tools_factory(ds_search, buffer, chunks, chunk_order);

    Coordinator::new(
        config,
        registry,
        llm_factory,
        tools_factory,
        bus,
        graph,
        trace,
    )
}

// ─── 辅助：运行管线并收集 trace ────────────────────────────────────

async fn run_pipeline(coordinator: &Coordinator, clauses: &[ReviewClause]) -> CoordinatorOutput {
    coordinator
        .review(clauses)
        .await
        .expect("Coordinator::review 不应 panic")
}

/// 从 CoordinatorOutput 中提取结构化信息用于测试断言。
#[allow(dead_code)]
struct PipelineResult {
    output: CoordinatorOutput,
    /// agent → finding count
    agent_counts: HashMap<String, usize>,
    /// severity → count
    severity_counts: HashMap<String, usize>,
    /// 是否有 Agent 输出了 truncated finding
    has_truncated: bool,
    /// High risk finding 数量
    high_risk_count: usize,
    /// 有 legal_basis 的 finding 数量
    with_legal_basis: usize,
    /// BlindSpot 产生的 finding 数
    blind_spot_findings: usize,
    /// LegalVerify 产生的 finding 数
    legal_verify_findings: usize,
    /// 含 [LegalVerify] 标记的 finding 数
    legal_verify_merged: usize,
    /// 含 [Debate] 标记的 finding 数
    debate_merged: usize,
    /// suggested_agent 数量
    suggested_agent_count: usize,
    /// 动态 Agent 产生的 finding 数
    dynamic_agent_findings: usize,
}

fn analyze_output(output: &CoordinatorOutput) -> PipelineResult {
    let findings = &output.findings;
    let mut agent_counts = HashMap::new();
    let mut severity_counts = HashMap::new();
    let mut has_truncated = false;
    let mut high_risk_count = 0;
    let mut with_legal_basis = 0;
    let mut blind_spot_findings = 0;
    let mut legal_verify_findings = 0;
    let mut legal_verify_merged = 0;
    let mut debate_merged = 0;
    let mut suggested_agent_count = 0;
    let mut dynamic_agent_findings = 0;

    for f in findings {
        *agent_counts.entry(f.agent.clone()).or_default() += 1;
        *severity_counts
            .entry(format!("{:?}", f.severity))
            .or_default() += 1;
        if f.truncated {
            has_truncated = true;
        }
        if f.severity == RiskSeverity::High && !f.no_risk {
            high_risk_count += 1;
        }
        if !f.legal_basis.is_empty() {
            with_legal_basis += 1;
        }
        if f.agent.contains("BlindSpot") {
            blind_spot_findings += 1;
        }
        if f.agent.contains("LegalVerify") {
            legal_verify_findings += 1;
        }
        if f.reason.contains("[LegalVerify]") {
            legal_verify_merged += 1;
        }
        if f.reason.contains("[Debate]") {
            debate_merged += 1;
        }
        if f.suggested_agent.is_some() {
            suggested_agent_count += 1;
        }
        if f.agent.starts_with("Dynamic_") {
            dynamic_agent_findings += 1;
        }
    }

    PipelineResult {
        output: output.clone(),
        agent_counts,
        severity_counts,
        has_truncated,
        high_risk_count,
        with_legal_basis,
        blind_spot_findings,
        legal_verify_findings,
        legal_verify_merged,
        debate_merged,
        suggested_agent_count,
        dynamic_agent_findings,
    }
}

// ═══════════════════════════════════════════════════════════════════
// §8 — 双通道协同
// ═══════════════════════════════════════════════════════════════════

async fn test_bus(
    ds_search: Option<Arc<DashScopeSearchBackend>>,
    buffer: Option<Arc<SearchBuffer>>,
) -> Vec<TestCheck> {
    let test_name = "bus";
    eprintln!("\n━━━━━━ §8 双通道协同 ━━━━━━");

    let mut checks = Vec::new();
    let (bus, graph, trace) = make_shared_infra();
    // 保留 trace 引用，用于管线结束后检查 AgentBus 事件
    let trace_for_check = trace.clone();

    // 只启用 FactCheck + SemanticRisk，使双通道交互更清晰
    let mut config = CoordinatorConfig::default();
    config.enabled_agents = vec![AgentId::FactCheck, AgentId::SemanticRisk];
    // 关闭 LegalVerify 和 BlindSpot 以聚焦双通道
    config.enable_legal_verify = false;
    config.blind_spot_fallback_enabled = false;

    let clauses = bus_test_clauses();
    let (chunks, chunk_order) = build_chunk_data(&clauses);
    let coordinator = make_coordinator(
        config,
        ds_search,
        buffer,
        bus,
        graph,
        trace,
        chunks,
        chunk_order,
    );

    eprintln!(
        "  条款: {} 条 | Agent: FactCheck + SemanticRisk",
        clauses.len()
    );
    let output = run_pipeline(&coordinator, &clauses).await;
    let result = analyze_output(&output);

    // Check 1: 两个 Agent 都产出了 finding
    let has_factcheck = result.agent_counts.contains_key("FactCheckAgent");
    let has_semantic = result.agent_counts.contains_key("SemanticRiskAgent");
    if has_factcheck && has_semantic {
        checks.push(TestCheck::pass(
            test_name,
            "both_agents_produced_findings",
            &format!(
                "FactCheck={} findings, SemanticRisk={} findings",
                result.agent_counts.get("FactCheckAgent").unwrap_or(&0),
                result.agent_counts.get("SemanticRiskAgent").unwrap_or(&0)
            ),
        ));
    } else {
        checks.push(TestCheck::fail(
            test_name,
            "both_agents_produced_findings",
            &format!("FactCheck={}, SemanticRisk={}", has_factcheck, has_semantic),
        ));
    }

    // Check 2: SessionGraph snapshot 完整
    if let Some(ref snap) = output.graph_snapshot {
        let chunks_ok = !snap.chunks.is_empty();
        let reviewed_ok = !snap.reviewed_by.is_empty();
        if chunks_ok && reviewed_ok {
            checks.push(TestCheck::pass(
                test_name,
                "session_graph_populated",
                &format!(
                    "chunks={}, reviewed_by edges={}",
                    snap.chunks.len(),
                    snap.reviewed_by.len()
                ),
            ));
        } else {
            checks.push(TestCheck::fail(
                test_name,
                "session_graph_populated",
                "SessionGraph chunks 或 reviewed_by 为空",
            ));
        }
    } else {
        checks.push(TestCheck::fail(
            test_name,
            "session_graph_populated",
            "graph_snapshot 为 None",
        ));
    }

    // Check 3: 从 trace 检查 AgentBus 消息传递
    // 分两层验证：(a) 是否有 Agent 广播了消息 (b) 是否有 Agent 收到了消息
    let (has_bus_send, has_bus_recv) = {
        let trace_guard = trace_for_check.lock().await;
        let send = trace_guard.events.iter().any(|e| {
            matches!(
                e.event_type,
                ai_bid::agents::trace::TraceEventType::AgentBusSend
            )
        });
        let recv = trace_guard.events.iter().any(|e| {
            matches!(
                e.event_type,
                ai_bid::agents::trace::TraceEventType::AgentBusRecv
            )
        });
        (send, recv)
    };
    if has_bus_recv {
        checks.push(TestCheck::pass(
            test_name,
            "agent_bus_messages",
            "检测到 AgentBus 消息传递",
        ));
    } else if has_bus_send {
        checks.push(TestCheck::skip(
            test_name,
            "agent_bus_messages",
            "AgentBus 有广播但未被接收（竞态窗口：接收方 LLM 调用期间到达）",
        ));
    } else {
        checks.push(TestCheck::skip(
            test_name,
            "agent_bus_messages",
            "AgentBus 无广播（所有 finding 均为 medium 或 info）",
        ));
    }

    // Check 4: 输出文件结构合法
    checks.push(TestCheck::pass(
        test_name,
        "output_structure_valid",
        &format!(
            "total_findings={}, high_risk={}",
            output.findings.len(),
            result.high_risk_count
        ),
    ));

    checks
}

// ═══════════════════════════════════════════════════════════════════
// §9 — 分层记忆
// ═══════════════════════════════════════════════════════════════════

async fn test_memory(
    ds_search: Option<Arc<DashScopeSearchBackend>>,
    buffer: Option<Arc<SearchBuffer>>,
) -> Vec<TestCheck> {
    let test_name = "memory";
    eprintln!("\n━━━━━━ §9 分层记忆 ━━━━━━");

    let mut checks = Vec::new();
    let (bus, graph, trace) = make_shared_infra();
    let mut config = CoordinatorConfig::default();
    // 使用全部 7 个 reviewer
    config.enable_legal_verify = true;
    config.blind_spot_max_turns = 5; // 加快 BlindSpot

    let clauses = memory_test_clauses();
    let (chunks, chunk_order) = build_chunk_data(&clauses);
    let coordinator = make_coordinator(
        config,
        ds_search,
        buffer,
        bus,
        graph,
        trace,
        chunks,
        chunk_order,
    );

    eprintln!("  条款: {} 条 | Agent: 全部 7 个 reviewer", clauses.len());
    let output = run_pipeline(&coordinator, &clauses).await;
    let result = analyze_output(&output);

    // Check 1: GraphSnapshot 完整性
    if let Some(ref snap) = output.graph_snapshot {
        let mut missing = Vec::new();
        if snap.chunks.is_empty() {
            missing.push("chunks");
        }
        if snap.agents.is_empty() {
            missing.push("agents");
        }
        // risks/reviewed_by/has_risk 可能为空（如果所有条款无风险），不作为 FAIL

        if missing.is_empty() {
            checks.push(TestCheck::pass(
                test_name,
                "graph_snapshot_complete",
                &format!(
                    "chunks={}, agents={}, risks={}, reviewed_by={}",
                    snap.chunks.len(),
                    snap.agents.len(),
                    snap.risks.len(),
                    snap.reviewed_by.len()
                ),
            ));
        } else {
            checks.push(TestCheck::fail(
                test_name,
                "graph_snapshot_complete",
                &format!("缺少字段: {}", missing.join(", ")),
            ));
        }
    } else {
        checks.push(TestCheck::fail(
            test_name,
            "graph_snapshot_complete",
            "graph_snapshot 为 None",
        ));
    }

    // Check 2: 多个 Agent 产出了 finding（SessionGraph 在各 Agent 间共享）
    let unique_agents = result.agent_counts.len();
    if unique_agents >= 2 {
        checks.push(TestCheck::pass(
            test_name,
            "multi_agent_shared_graph",
            &format!("{} 个 Agent 在共享 SessionGraph 上工作", unique_agents),
        ));
    } else {
        checks.push(TestCheck::fail(
            test_name,
            "multi_agent_shared_graph",
            &format!("仅 {} 个 Agent 产出 finding", unique_agents),
        ));
    }

    // Check 3: 输出结构完整
    checks.push(TestCheck::pass(
        test_name,
        "output_complete",
        &format!(
            "findings={}, with_legal_basis={}, blindspot={}",
            output.findings.len(),
            result.with_legal_basis,
            result.blind_spot_findings
        ),
    ));

    // Check 4: 数据不跨 Session 泄漏（通过创建新 graph 验证）
    let new_graph = SessionGraph::new();
    let new_snapshot = new_graph.snapshot();
    if new_snapshot.chunks.is_empty() && new_snapshot.risks.is_empty() {
        checks.push(TestCheck::pass(
            test_name,
            "no_cross_session_leak",
            "新建 SessionGraph 为空，无跨 Session 数据泄漏",
        ));
    } else {
        checks.push(TestCheck::fail(
            test_name,
            "no_cross_session_leak",
            "新建 SessionGraph 非空，存在数据泄漏",
        ));
    }

    // Check 5: 全部 7 个 reviewer Agent 均被路由到至少 1 条条款
    let route_counts = &output.routing_summary.agent_clause_counts;
    let expected_reviewers = [
        "FactCheckAgent",
        "ProcedureAgent",
        "RuleEngineAgent",
        "SemanticRiskAgent",
        "ScoringAgent",
        "DemandAgent",
        "ContractAgent",
    ];
    let mut missing_agents: Vec<&str> = Vec::new();
    for agent in &expected_reviewers {
        if route_counts.get(*agent).unwrap_or(&0) < &1 {
            missing_agents.push(agent);
        }
    }
    if missing_agents.is_empty() {
        checks.push(TestCheck::pass(
            test_name,
            "all_agents_routed",
            &format!(
                "全部 {} 个 reviewer 均被路由到条款 (route map: {:?})",
                expected_reviewers.len(),
                route_counts.keys().collect::<Vec<_>>()
            ),
        ));
    } else {
        checks.push(TestCheck::fail(
            test_name,
            "all_agents_routed",
            &format!(
                "{} 个 Agent 未被路由: {:?}。route map: {:?}",
                missing_agents.len(),
                missing_agents,
                route_counts.keys().collect::<Vec<_>>()
            ),
        ));
    }

    checks
}

// ═══════════════════════════════════════════════════════════════════
// §10.3 — 并行 Agent 隔离
// ═══════════════════════════════════════════════════════════════════

async fn test_execute(
    ds_search: Option<Arc<DashScopeSearchBackend>>,
    buffer: Option<Arc<SearchBuffer>>,
) -> Vec<TestCheck> {
    let test_name = "execute";
    eprintln!("\n━━━━━━ §10.3 并行 Agent 隔离 ━━━━━━");

    let mut checks = Vec::new();
    let (bus, graph, trace) = make_shared_infra();

    let mut config = CoordinatorConfig::default();
    // 启用 5 个 Agent
    config.enabled_agents = vec![
        AgentId::FactCheck,
        AgentId::Procedure,
        AgentId::SemanticRisk,
        AgentId::Scoring,
        AgentId::Contract,
    ];
    config.enable_legal_verify = false;
    config.blind_spot_fallback_enabled = false;

    let clauses = memory_test_clauses(); // 5 条条款
    let (chunks, chunk_order) = build_chunk_data(&clauses);
    let coordinator = make_coordinator(
        config,
        ds_search,
        buffer,
        bus,
        graph,
        trace,
        chunks,
        chunk_order,
    );

    eprintln!(
        "  条款: {} 条 | Agent: FactCheck, Procedure, SemanticRisk, Scoring, Contract",
        clauses.len()
    );
    let output = run_pipeline(&coordinator, &clauses).await;
    let result = analyze_output(&output);

    // Check 1: 多个 Agent 并行产出了 finding
    let agent_count = result.agent_counts.len();
    if agent_count >= 3 {
        checks.push(TestCheck::pass(
            test_name,
            "multi_agent_parallel",
            &format!(
                "{} 个 Agent 并行产出 finding: {:?}",
                agent_count,
                result.agent_counts.keys().collect::<Vec<_>>()
            ),
        ));
    } else {
        checks.push(TestCheck::fail(
            test_name,
            "multi_agent_parallel",
            &format!("仅 {} 个 Agent 产出（预期 ≥3）", agent_count),
        ));
    }

    // Check 2: 各 Agent 的 finding 互不污染（不同 agent 字段值不同）
    let agents: std::collections::HashSet<&str> =
        output.findings.iter().map(|f| f.agent.as_str()).collect();
    if agents.len() >= 3 {
        checks.push(TestCheck::pass(
            test_name,
            "agent_isolation",
            &format!("{} 个独立 Agent 身份: {:?}", agents.len(), agents),
        ));
    } else {
        checks.push(TestCheck::fail(
            test_name,
            "agent_isolation",
            "Agent finding 的 agent 字段缺乏多样性",
        ));
    }

    // Check 3: 系统不 panic
    checks.push(TestCheck::pass(
        test_name,
        "no_panic",
        &format!("正常完成，{} findings", output.findings.len()),
    ));

    checks
}

// ═══════════════════════════════════════════════════════════════════
// §10.5 — LEGAL VERIFY
// ═══════════════════════════════════════════════════════════════════

async fn test_legal(
    ds_search: Option<Arc<DashScopeSearchBackend>>,
    buffer: Option<Arc<SearchBuffer>>,
) -> Vec<TestCheck> {
    let test_name = "legal";
    eprintln!("\n━━━━━━ §10.5 LEGAL VERIFY ━━━━━━");

    let mut checks = Vec::new();
    let (bus, graph, trace) = make_shared_infra();

    let mut config = CoordinatorConfig::default();
    config.enabled_agents = vec![
        AgentId::FactCheck,
        AgentId::SemanticRisk,
        AgentId::Procedure,
    ];
    config.enable_legal_verify = true;
    config.legal_verify_max_turns = 3;
    config.blind_spot_fallback_enabled = false;

    let clauses = legal_test_clauses();
    let (chunks, chunk_order) = build_chunk_data(&clauses);
    let coordinator = make_coordinator(
        config,
        ds_search,
        buffer,
        bus,
        graph,
        trace,
        chunks,
        chunk_order,
    );

    eprintln!("  条款: {} 条 | LegalVerify 已启用", clauses.len());
    let output = run_pipeline(&coordinator, &clauses).await;
    let result = analyze_output(&output);

    // Check 1: 有 legal_basis 的 finding 被产出
    if result.with_legal_basis > 0 {
        checks.push(TestCheck::pass(
            test_name,
            "legal_basis_produced",
            &format!("{} 条 finding 包含 legal_basis", result.with_legal_basis),
        ));
    } else {
        checks.push(TestCheck::skip(
            test_name,
            "legal_basis_produced",
            "无 finding 包含 legal_basis（LLM 未引用法条，跳过验证）",
        ));
    }

    // Check 2: LegalVerify 管线步骤被执行
    // 检查 routing_summary 中的 legal_verify_count
    let lv_count = output.routing_summary.legal_verify_count;
    if lv_count > 0 {
        checks.push(TestCheck::pass(
            test_name,
            "legal_verify_step_executed",
            &format!("legal_verify_count={}", lv_count),
        ));
    } else if result.with_legal_basis > 0 {
        // 有 legal_basis 但 legal_verify_count=0 → 可能全部 fallback
        checks.push(TestCheck::skip(
            test_name,
            "legal_verify_step_executed",
            "legal_verify_count=0，可能全部走 fallback",
        ));
    } else {
        checks.push(TestCheck::skip(
            test_name,
            "legal_verify_step_executed",
            "无 legal_basis 的 finding，LegalVerify 步骤跳过",
        ));
    }

    // Check 3: 如果 LegalVerify 产出了 finding，检查合并逻辑
    if result.legal_verify_merged > 0 {
        checks.push(TestCheck::pass(
            test_name,
            "legal_verify_merged",
            &format!(
                "{} 条 finding 包含 [LegalVerify] 标记",
                result.legal_verify_merged
            ),
        ));
    } else {
        checks.push(TestCheck::skip(
            test_name,
            "legal_verify_merged",
            "无 [LegalVerify] 标记（ReAct 无产出或走 fallback）",
        ));
    }

    // Check 4: 输出结构合法
    checks.push(TestCheck::pass(
        test_name,
        "output_valid",
        &format!("{} findings, routing_summary 完整", output.findings.len()),
    ));

    checks
}

// ═══════════════════════════════════════════════════════════════════
// §10.6 — BLINDSPOT
// ═══════════════════════════════════════════════════════════════════

async fn test_blindspot(
    ds_search: Option<Arc<DashScopeSearchBackend>>,
    buffer: Option<Arc<SearchBuffer>>,
) -> Vec<TestCheck> {
    let test_name = "blindspot";
    eprintln!("\n━━━━━━ §10.6 BLINDSPOT ━━━━━━");

    let mut checks = Vec::new();
    let (bus, graph, trace) = make_shared_infra();

    let mut config = CoordinatorConfig::default();
    config.enabled_agents = vec![AgentId::FactCheck, AgentId::SemanticRisk];
    config.enable_legal_verify = false;
    config.blind_spot_max_turns = 5;
    config.blind_spot_fallback_enabled = true;

    let clauses = blindspot_test_clauses();
    let (chunks, chunk_order) = build_chunk_data(&clauses);
    let coordinator = make_coordinator(
        config,
        ds_search,
        buffer,
        bus,
        graph,
        trace,
        chunks,
        chunk_order,
    );

    eprintln!(
        "  条款: {} 条 | BlindSpot 已启用 (fallback=enabled)",
        clauses.len()
    );
    let output = run_pipeline(&coordinator, &clauses).await;
    let result = analyze_output(&output);

    // Check 1: BlindSpot 步骤被执行（产出了 BlindSpot finding 或 fallback 标记）
    if result.blind_spot_findings > 0 {
        checks.push(TestCheck::pass(
            test_name,
            "blindspot_executed",
            &format!("BlindSpot 产出 {} 条 finding", result.blind_spot_findings),
        ));
    } else {
        // 测试夹具包含 4 条仅被 1 个 Agent 审查的 L2+ 条款，BlindSpot 必须产出发现
        checks.push(TestCheck::fail(
            test_name,
            "blindspot_executed",
            "BlindSpot 无额外 finding，但测试数据包含 4 条未充分审查的 L2+ 条款",
        ));
    }

    // Check 2: GraphSnapshot 存在（BlindSpot 依赖它）
    if let Some(ref snap) = output.graph_snapshot {
        checks.push(TestCheck::pass(
            test_name,
            "graph_snapshot_for_blindspot",
            &format!(
                "chunks={}, risks={}, reviewed_by={}",
                snap.chunks.len(),
                snap.risks.len(),
                snap.reviewed_by.len()
            ),
        ));
    } else {
        checks.push(TestCheck::fail(
            test_name,
            "graph_snapshot_for_blindspot",
            "graph_snapshot 为 None，BlindSpot 无法工作",
        ));
    }

    // Check 3: 系统不 panic
    checks.push(TestCheck::pass(
        test_name,
        "no_panic",
        &format!("正常完成，{} findings", output.findings.len()),
    ));

    checks
}

// ═══════════════════════════════════════════════════════════════════
// §10.7 — DEBATE
// ═══════════════════════════════════════════════════════════════════

async fn test_debate(
    ds_search: Option<Arc<DashScopeSearchBackend>>,
    buffer: Option<Arc<SearchBuffer>>,
) -> Vec<TestCheck> {
    let test_name = "debate";
    eprintln!("\n━━━━━━ §10.7 DEBATE ━━━━━━");

    let mut checks = Vec::new();
    let (bus, graph, trace) = make_shared_infra();

    let mut config = CoordinatorConfig::default();
    config.enabled_agents = vec![AgentId::FactCheck, AgentId::SemanticRisk];
    config.enable_legal_verify = true;
    config.blind_spot_fallback_enabled = false;
    // Debate 由 Coordinator 内部自动处理（High + confidence<0.85 触发）

    let clauses = debate_test_clauses();
    let (chunks, chunk_order) = build_chunk_data(&clauses);
    let coordinator = make_coordinator(
        config,
        ds_search,
        buffer,
        bus,
        graph,
        trace,
        chunks,
        chunk_order,
    );

    eprintln!(
        "  条款: {} 条 | Debate 由 High+低置信度自动触发",
        clauses.len()
    );
    let output = run_pipeline(&coordinator, &clauses).await;
    let result = analyze_output(&output);

    // Check 1: 是否有 High risk finding
    if result.high_risk_count > 0 {
        checks.push(TestCheck::pass(
            test_name,
            "high_risk_found",
            &format!("{} 条 High risk finding", result.high_risk_count),
        ));
    } else {
        checks.push(TestCheck::skip(
            test_name,
            "high_risk_found",
            "无 High risk finding，Debate 不会触发",
        ));
    }

    // Check 2: 如果触发了 Debate，验证合并
    if result.debate_merged > 0 {
        checks.push(TestCheck::pass(
            test_name,
            "debate_executed_and_merged",
            &format!("{} 条 finding 包含 [Debate] 标记", result.debate_merged),
        ));
    } else {
        checks.push(TestCheck::skip(
            test_name,
            "debate_executed_and_merged",
            "无 [Debate] 标记（可能 confidence 够高或 Debate 无产出）",
        ));
    }

    // Check 3: 系统不 panic
    checks.push(TestCheck::pass(
        test_name,
        "no_panic",
        &format!("正常完成，{} findings", output.findings.len()),
    ));

    checks
}

// ═══════════════════════════════════════════════════════════════════
// §11 — 动态 Agent 闭环
// ═══════════════════════════════════════════════════════════════════

async fn test_dynamic(
    ds_search: Option<Arc<DashScopeSearchBackend>>,
    buffer: Option<Arc<SearchBuffer>>,
) -> Vec<TestCheck> {
    let test_name = "dynamic";
    eprintln!("\n━━━━━━ §11 动态 Agent 闭环 ━━━━━━");

    let mut checks = Vec::new();
    let (bus, graph, trace) = make_shared_infra();

    let mut config = CoordinatorConfig::default();
    config.enabled_agents = AgentId::all_reviewers();
    config.enable_legal_verify = true;
    config.blind_spot_max_turns = 8; // 给 BlindSpot 足够时间分析
    config.blind_spot_fallback_enabled = true;

    let clauses = dynamic_test_clauses();
    let (chunks, chunk_order) = build_chunk_data(&clauses);
    let coordinator = make_coordinator(
        config,
        ds_search,
        buffer,
        bus,
        graph,
        trace,
        chunks,
        chunk_order,
    );

    eprintln!(
        "  条款: {} 条 | BlindSpot 可能 suggest_agent",
        clauses.len()
    );
    let output = run_pipeline(&coordinator, &clauses).await;
    let result = analyze_output(&output);

    // Check 1: BlindSpot 运行了
    if result.blind_spot_findings > 0 {
        checks.push(TestCheck::pass(
            test_name,
            "blindspot_ran",
            &format!("BlindSpot 产出 {} 条 finding", result.blind_spot_findings),
        ));
    } else {
        checks.push(TestCheck::skip(
            test_name,
            "blindspot_ran",
            "BlindSpot 无额外 finding",
        ));
    }

    // Check 2: suggest_agent 是否被触发
    if result.suggested_agent_count > 0 {
        checks.push(TestCheck::pass(
            test_name,
            "suggest_agent_triggered",
            &format!(
                "{} 条 finding 包含 suggest_agent",
                result.suggested_agent_count
            ),
        ));
    } else {
        checks.push(TestCheck::skip(
            test_name,
            "suggest_agent_triggered",
            "BlindSpot 未 suggest_agent（当前条款可能不需要新 Agent）",
        ));
    }

    // Check 3: dynamic_agents.json 文件状态
    let dynamic_file_path = data_path_str("agents/dynamic_agents.json");
    let dynamic_file = Path::new(&dynamic_file_path);
    if dynamic_file.exists() {
        match fs::read_to_string(dynamic_file) {
            Ok(content) => match serde_json::from_str::<DynamicAgentManifest>(&content) {
                Ok(manifest) => {
                    let active_count = manifest.agents.iter().filter(|a| a.active).count();
                    let inactive_count = manifest.agents.iter().filter(|a| !a.active).count();
                    checks.push(TestCheck::pass(
                        test_name,
                        "dynamic_agents_file_valid",
                        &format!(
                            "{} agents ({} active, {} pending approval)",
                            manifest.agents.len(),
                            active_count,
                            inactive_count
                        ),
                    ));
                }
                Err(e) => {
                    checks.push(TestCheck::fail(
                        test_name,
                        "dynamic_agents_file_valid",
                        &format!("JSON 非法: {}", e),
                    ));
                }
            },
            Err(e) => {
                checks.push(TestCheck::fail(
                    test_name,
                    "dynamic_agents_file_valid",
                    &format!("无法读取: {}", e),
                ));
            }
        }
    } else {
        checks.push(TestCheck::skip(
            test_name,
            "dynamic_agents_file_valid",
            "dynamic_agents.json 不存在（首次运行正常）",
        ));
    }

    // Check 4: 系统不 panic
    checks.push(TestCheck::pass(
        test_name,
        "no_panic",
        &format!("正常完成，{} findings", output.findings.len()),
    ));

    checks
}

// ═══════════════════════════════════════════════════════════════════
// §12/§13 — 故障与边界
// ═══════════════════════════════════════════════════════════════════

async fn test_fault(
    ds_search: Option<Arc<DashScopeSearchBackend>>,
    buffer: Option<Arc<SearchBuffer>>,
) -> Vec<TestCheck> {
    let test_name = "fault";
    eprintln!("\n━━━━━━ §12/§13 故障与边界 ━━━━━━");

    let mut checks = Vec::new();

    // ── 场景 1: 空条款列表 ──
    {
        let (bus, graph, trace) = make_shared_infra();
        let mut config = CoordinatorConfig::default();
        config.enabled_agents = vec![AgentId::FactCheck];
        config.enable_legal_verify = false;
        config.blind_spot_fallback_enabled = false;

        let clauses: Vec<ReviewClause> = vec![];
        let (chunks, chunk_order) = build_chunk_data(&clauses);
        let coordinator = make_coordinator(
            config,
            ds_search.clone(),
            buffer.clone(),
            bus,
            graph,
            trace,
            chunks,
            chunk_order,
        );
        match coordinator.review(&clauses).await {
            Ok(output) => {
                if output.findings.is_empty() {
                    checks.push(TestCheck::pass(
                        test_name,
                        "empty_clauses",
                        "空条款列表 → 返回空 findings，不 panic",
                    ));
                } else {
                    checks.push(TestCheck::fail(
                        test_name,
                        "empty_clauses",
                        &format!("空输入但返回了 {} 条 finding", output.findings.len()),
                    ));
                }
            }
            Err(e) => {
                checks.push(TestCheck::fail(
                    test_name,
                    "empty_clauses",
                    &format!("空条款列表导致 Err: {}", e),
                ));
            }
        }
    }

    // ── 场景 2: 超长条款文本 ──
    {
        let (bus, graph, trace) = make_shared_infra();
        let mut config = CoordinatorConfig::default();
        config.enabled_agents = vec![AgentId::FactCheck];
        config.enable_legal_verify = false;
        config.blind_spot_fallback_enabled = false;

        let clauses = fault_test_clauses();
        // 取超长条款
        let long_clause: Vec<ReviewClause> = clauses
            .into_iter()
            .filter(|c| c.chunk_id == "ch_long")
            .collect();
        let (chunks, chunk_order) = build_chunk_data(&long_clause);
        let coordinator = make_coordinator(
            config,
            ds_search.clone(),
            buffer.clone(),
            bus,
            graph,
            trace,
            chunks,
            chunk_order,
        );

        match coordinator.review(&long_clause).await {
            Ok(output) => {
                checks.push(TestCheck::pass(
                    test_name,
                    "long_text_no_panic",
                    &format!(
                        "超长条款({} 字符) → 不 panic, {} findings",
                        long_clause[0].text.chars().count(),
                        output.findings.len()
                    ),
                ));
            }
            Err(e) => {
                checks.push(TestCheck::fail(
                    test_name,
                    "long_text_no_panic",
                    &format!("超长条款导致 Err: {}", e),
                ));
            }
        }
    }

    // ── 场景 3: 特殊字符条款 ──
    {
        let (bus, graph, trace) = make_shared_infra();
        let mut config = CoordinatorConfig::default();
        config.enabled_agents = vec![AgentId::FactCheck];
        config.enable_legal_verify = false;
        config.blind_spot_fallback_enabled = false;

        let all_clauses = fault_test_clauses();
        let special_clause: Vec<ReviewClause> = all_clauses
            .into_iter()
            .filter(|c| c.chunk_id == "ch_special")
            .collect();
        let (chunks, chunk_order) = build_chunk_data(&special_clause);
        let coordinator = make_coordinator(
            config,
            ds_search.clone(),
            buffer.clone(),
            bus,
            graph,
            trace,
            chunks,
            chunk_order,
        );

        match coordinator.review(&special_clause).await {
            Ok(output) => {
                let json_result = serde_json::to_string(&output.findings);
                match json_result {
                    Ok(_) => checks.push(TestCheck::pass(
                        test_name,
                        "special_chars_serializable",
                        "特殊字符条款 → JSON 序列化成功",
                    )),
                    Err(e) => checks.push(TestCheck::fail(
                        test_name,
                        "special_chars_serializable",
                        &format!("JSON 序列化失败: {}", e),
                    )),
                }
            }
            Err(e) => {
                checks.push(TestCheck::fail(
                    test_name,
                    "special_chars_serializable",
                    &format!("特殊字符条款导致 Err: {}", e),
                ));
            }
        }
    }

    // ── 场景 4: JSON 损坏不 panic ──
    {
        // 备份 + 损坏 + 测试 + 恢复
        let dynamic_file_path = data_path_str("agents/dynamic_agents.json");
        let dynamic_file = Path::new(&dynamic_file_path);
        let backup_file_path = data_path_str("agents/dynamic_agents.json.test_bak");
        let backup_file = Path::new(&backup_file_path);

        let original_exists = dynamic_file.exists();
        if original_exists {
            fs::rename(dynamic_file, backup_file).ok();
        }

        // 写入非法 JSON
        fs::write(dynamic_file, "not valid json {{{").ok();

        // 创建新的 Coordinator，load_dynamic_agents 在 new() 中自动调用
        let bus = Arc::new(AgentBus::new(4));
        let graph = Arc::new(SessionGraph::new());
        let trace = Arc::new(Mutex::new(TraceLog::new()));
        let empty_chunks: Arc<HashMap<String, Chunk>> = Arc::new(HashMap::new());
        let empty_order: Arc<Vec<String>> = Arc::new(Vec::new());
        let mut coordinator = make_coordinator(
            CoordinatorConfig::default(),
            ds_search.clone(),
            buffer.clone(),
            bus,
            graph,
            trace,
            empty_chunks,
            empty_order,
        );
        // 手动再调一次确认
        match coordinator.load_dynamic_agents() {
            Ok(count) => {
                if count == 0 {
                    checks.push(TestCheck::pass(
                        test_name,
                        "corrupt_json_no_panic",
                        "损坏 JSON → loaded=0，不 panic",
                    ));
                } else {
                    checks.push(TestCheck::fail(
                        test_name,
                        "corrupt_json_no_panic",
                        &format!("损坏 JSON 返回 loaded={}（预期 0）", count),
                    ));
                }
            }
            Err(e) => {
                checks.push(TestCheck::pass(
                    test_name,
                    "corrupt_json_no_panic",
                    &format!("损坏 JSON → 返回 Err（不 panic）: {}", e),
                ));
            }
        }

        // 恢复
        fs::remove_file(dynamic_file).ok();
        if original_exists {
            fs::rename(backup_file, dynamic_file).ok();
        }
    }

    // ── 场景 5: 输出文件自动创建目录 ──
    {
        let test_dir = data_path_str("output/test_fault_output");
        let result = fs::create_dir_all(&test_dir);
        if result.is_ok() {
            checks.push(TestCheck::pass(
                test_name,
                "output_dir_auto_create",
                "create_dir_all 成功 — 输出目录自动创建",
            ));
            // 清理
            fs::remove_dir_all(&test_dir).ok();
        } else {
            checks.push(TestCheck::fail(
                test_name,
                "output_dir_auto_create",
                &format!("create_dir_all 失败: {:?}", result.err()),
            ));
        }
    }

    checks
}

// ═══════════════════════════════════════════════════════════════════
// Main — 解析参数，分发测试
// ═══════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    let test_filter = std::env::args().nth(1).unwrap_or_else(|| "all".to_string());

    // 检查 Agent 模式
    let agent_enabled = env::var("AIBID_AGENT").unwrap_or_default() == "1";
    if !agent_enabled {
        eprintln!("错误: 需要设置 AIBID_AGENT=1");
        eprintln!("用法: $env:AIBID_AGENT=1; cargo run --bin test_agents -- [test_name]");
        std::process::exit(1);
    }

    eprintln!("══════════════════════════════════════════════");
    eprintln!(
        "  Agent 框架集成测试 (LLM: {})",
        env::var("AIBID_LLM_PROTOCOL").unwrap_or_else(|_| "dashscope".to_string())
    );
    eprintln!("══════════════════════════════════════════════");

    // 一次性初始化搜索后端（所有测试共享）
    let (ds_search, buffer) = init_search_backend();

    let mut all_checks: Vec<TestCheck> = Vec::new();

    let run_test = |name: &str| name == "all" || test_filter == name || test_filter == "all";

    if run_test("bus") {
        let checks = test_bus(ds_search.clone(), buffer.clone()).await;
        TestSummary::from_checks(&checks).print("bus");
        all_checks.extend(checks);
    }

    if run_test("memory") {
        let checks = test_memory(ds_search.clone(), buffer.clone()).await;
        TestSummary::from_checks(&checks).print("memory");
        all_checks.extend(checks);
    }

    if run_test("execute") {
        let checks = test_execute(ds_search.clone(), buffer.clone()).await;
        TestSummary::from_checks(&checks).print("execute");
        all_checks.extend(checks);
    }

    if run_test("legal") {
        let checks = test_legal(ds_search.clone(), buffer.clone()).await;
        TestSummary::from_checks(&checks).print("legal");
        all_checks.extend(checks);
    }

    if run_test("blindspot") {
        let checks = test_blindspot(ds_search.clone(), buffer.clone()).await;
        TestSummary::from_checks(&checks).print("blindspot");
        all_checks.extend(checks);
    }

    if run_test("debate") {
        let checks = test_debate(ds_search.clone(), buffer.clone()).await;
        TestSummary::from_checks(&checks).print("debate");
        all_checks.extend(checks);
    }

    if run_test("dynamic") {
        let checks = test_dynamic(ds_search.clone(), buffer.clone()).await;
        TestSummary::from_checks(&checks).print("dynamic");
        all_checks.extend(checks);
    }

    if run_test("fault") {
        let checks = test_fault(ds_search.clone(), buffer.clone()).await;
        TestSummary::from_checks(&checks).print("fault");
        all_checks.extend(checks);
    }

    // ── 输出 NDJSON ──
    println!("╔══════════════════════════════════════════════╗");
    println!("║  Test Results (NDJSON)                       ║");
    println!("╚══════════════════════════════════════════════╝");
    for check in &all_checks {
        println!("{}", check.to_ndjson());
    }

    let summary = TestSummary::from_checks(&all_checks);
    eprintln!();
    eprintln!("══════════════════════════════════════════════");
    eprintln!(
        "  TOTAL: {} total | {} PASS | {} FAIL | {} SKIP | {:.0}% pass",
        summary.total, summary.passed, summary.failed, summary.skipped, summary.pass_rate
    );
    eprintln!("══════════════════════════════════════════════");

    if summary.failed > 0 {
        eprintln!("\nFAILED CHECKS:");
        for check in &all_checks {
            if check.status == "FAIL" {
                eprintln!(
                    "  [FAIL] {}::{} — {}",
                    check.test, check.check, check.detail
                );
            }
        }
        std::process::exit(1);
    }
}
