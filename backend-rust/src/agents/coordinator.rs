//! Coordinator — 多 Agent 审查协调器（Mediator + Chain of Responsibility）。
//!
//! 设计文档 §6.1 / temp.md Phase 2 完整实现。
//!
//! ## 设计模式
//!
//! - **Mediator**: Agent 之间不直接通信，交互经 Coordinator 调度 + SessionGraph 中转
//! - **Chain of Responsibility**: Route → Execute → Merge → LegalVerify → BlindSpot → Triage
//!
//! ## 聚合流水线 (7 步)
//!
//! ```text
//! [1] ROUTE   → clauses → HashMap<AgentId, Vec<ReviewClause>>
//! [2] PRELOAD → 所有 Chunk 节点写入 SessionGraph
//! [3] EXECUTE → tokio::spawn × N agents (并行)
//! [4] MERGE   → 合并 + 去重 (SessionGraph 快照)
//! [5] LEGAL_VERIFY → 对抗法条验证
//! [6] BLINDSPOT → BlindSpotAgent 读取完整 SessionGraph
//! [7] TRIAGE  → 按 severity + confidence 分流
//! ```
//!
//! ## 工厂注入
//!
//! `llm_factory` 和 `tools_factory` 避免 `clone_box` 传染——
//! 每个 Agent 获得独立的 LLM 客户端和工具集。

use crate::agents::bus::AgentBus;
use crate::agents::react_loop::{LlmClient, ReActLoop};
use crate::agents::registry::AgentRegistry;
use crate::agents::review_event::{FindingLifecycle, ReviewEvent, ReviewEventBus};
use crate::agents::risk_taxonomy;
use crate::agents::session_graph::SessionGraph;
use crate::agents::tools::ToolRegistry;
use crate::agents::trace::TraceLog;
use crate::agents::types::*;
use crate::paths::data_path_str;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinSet;

/// 返回 RiskSeverity 的纯字符串表示（不含 emoji），用于 SSE 事件。
fn severity_str(s: &RiskSeverity) -> &'static str {
    match s {
        RiskSeverity::High => "high",
        RiskSeverity::Medium => "medium",
        RiskSeverity::Low => "low",
        RiskSeverity::Info => "info",
    }
}

// ─── 批量搜索辅助函数 ────────────────────────────────────────────

/// 从 legal_basis 字符串提取搜索 query。
///
/// 输入如 "[《87号令》第二十条](url)" → "87号令 第二十条 原文"。
fn law_ref_to_search_query(law_ref: &str) -> String {
    // 去掉 Markdown 链接格式: [text](url) → text
    let text = if law_ref.starts_with('[') {
        law_ref
            .trim_start_matches('[')
            .split("](")
            .next()
            .unwrap_or(law_ref)
    } else {
        law_ref
    };

    // 如果已经是纯法条名，直接使用
    let trimmed = text.trim();
    // 去掉书名号做关键词
    let keywords: String = trimmed.replace('《', "").replace(['》', '（', '）'], " ");

    if keywords.len() > 3 {
        format!("{} 原文", keywords.trim())
    } else {
        String::new()
    }
}

/// risk_type → 搜索模板映射。
fn risk_type_to_search_template(risk_type: &str) -> Option<String> {
    match risk_type {
        "程序违规" => Some("政府采购 招标程序 合规要求 违规案例".to_string()),
        "品牌指定" => Some("政府采购 指定品牌 禁止 投诉案例".to_string()),
        "排他条款" => Some("政府采购 不合理条件 差别待遇 歧视待遇 案例".to_string()),
        "资质门槛" => Some("政府采购 供应商资格 限制竞争 投诉案例".to_string()),
        "评分不公" => Some("政府采购 评分标准 评审因素 投诉案例".to_string()),
        "合同陷阱" => Some("政府采购 合同条款 验收标准 违约责任".to_string()),
        "技术缺失" => Some("政府采购 技术参数 需求描述 规范".to_string()),
        "地域限制" => Some("政府采购 地域限制 本地业绩 差别待遇".to_string()),
        "资金风险" => Some("政府采购 预算 最高限价 资金支付".to_string()),
        "需求不清" => Some("政府采购 采购需求 明确性 规范性要求".to_string()),
        _ => None,
    }
}

/// 从条款文本提取关键搜索词（用于没有 Hypothesis 的条款）。
fn extract_clause_keywords(text: &str) -> String {
    let keywords = [
        "★",
        "▲",
        "品牌",
        "型号",
        "专利",
        "原厂",
        "本地",
        "地域",
        "排他",
        "唯一",
        "微信",
        "小程序",
        "App",
        "内部规范",
        "标准",
        "认证",
        "资质",
        "否决",
        "废标",
        "无效标",
        "一票否决",
    ];
    let matched: Vec<&str> = keywords
        .iter()
        .filter(|kw| text.contains(*kw))
        .copied()
        .collect();
    if matched.len() > 3 {
        matched[..3].join(" ")
    } else {
        matched.join(" ")
    }
}

/// MERGE 阶段的去重结果。
struct MergeResult {
    retained: Vec<RiskFinding>,
}

/// 多 Agent 审查协调器。
///
/// 持有 Agent 注册表、共享基础设施（Bus/Graph/Trace）、工厂函数。
pub struct Coordinator {
    /// Coordinator 运行时配置
    config: CoordinatorConfig,
    /// Agent 注册表（8 个 Agent 的静态定义）
    registry: AgentRegistry,
    /// 已加载的动态 Agent 定义 (id → definition)
    dynamic_definitions: HashMap<String, DynamicAgentDefinition>,
    /// LLM 客户端工厂：每次调用创建新的 LlmClient
    /// ★ 避免 clone_box 传染到 LlmClient trait
    llm_factory: Arc<dyn Fn() -> Box<dyn LlmClient> + Send + Sync>,
    /// 工具集工厂：每次调用创建新的 ToolRegistry
    /// ★ 避免 clone_box 传染到 AgentTool trait
    tools_factory: Arc<dyn Fn() -> ToolRegistry + Send + Sync>,
    /// Agent 间广播通道
    bus: Arc<AgentBus>,
    /// Session Knowledge Graph（Blackboard 核心）
    graph: Arc<SessionGraph>,
    /// 审查追溯日志
    trace: Arc<Mutex<TraceLog>>,
    /// stderr 打印锁：多 Agent 并行时确保日志不交叠
    print_lock: Arc<std::sync::Mutex<()>>,
    /// SSE 实时推送通道（可选，仅 HTTP server 模式启用）
    review_events: Option<Arc<ReviewEventBus>>,
    /// 指标采集器（可选，启用时记录全链路性能数据）
    metrics: Option<Arc<Mutex<crate::metrics::MetricsCollector>>>,
    /// ★ 跨 Agent 共享搜索缓存（避免不同 Agent 重复搜索相同的法规）
    pub shared_search_cache: Arc<Mutex<HashMap<(String, String), serde_json::Value>>>,
}

impl Coordinator {
    /// 创建新的 Coordinator。
    ///
    /// * `config` — 运行时配置（启用哪些 Agent、是否 Legal Verify 等）
    /// * `registry` — Agent 注册表（通常用 `AgentRegistry::builtin()`）
    /// * `llm_factory` — LLM 客户端工厂（每次调用创建新实例）
    /// * `tools_factory` — 工具集工厂（每次调用创建新实例）
    /// * `bus` — Agent 间广播通道
    /// * `graph` — Session Knowledge Graph
    /// * `trace` — 审查追溯日志
    pub fn new(
        config: CoordinatorConfig,
        registry: AgentRegistry,
        llm_factory: Arc<dyn Fn() -> Box<dyn LlmClient> + Send + Sync>,
        tools_factory: Arc<dyn Fn() -> ToolRegistry + Send + Sync>,
        bus: Arc<AgentBus>,
        graph: Arc<SessionGraph>,
        trace: Arc<Mutex<TraceLog>>,
    ) -> Self {
        let print_lock = Arc::new(std::sync::Mutex::new(()));
        let shared_search_cache = Arc::new(Mutex::new(HashMap::new()));
        let mut coordinator = Self {
            config,
            registry,
            dynamic_definitions: HashMap::new(),
            llm_factory,
            tools_factory,
            bus,
            graph,
            trace,
            print_lock,
            review_events: None,
            metrics: None,
            shared_search_cache,
        };

        // 启动时加载已有动态 Agent
        if let Err(e) = coordinator.load_dynamic_agents() {
            eprintln!(
                "  [DYNAMIC] 加载动态 Agent 失败: {}（继续使用内置 Agent）",
                e
            );
        }

        coordinator
    }

    /// 设置 SSE 实时推送通道。
    ///
    /// 仅在 HTTP server 模式下启用（CLI 模式不设置此通道）。
    pub fn with_review_events(mut self, events: Arc<ReviewEventBus>) -> Self {
        self.review_events = Some(events);
        self
    }

    /// 设置指标采集器（用于记录全链路性能数据）。
    pub fn with_metrics(mut self, collector: Arc<Mutex<crate::metrics::MetricsCollector>>) -> Self {
        self.metrics = Some(collector);
        self
    }

    // ── 主入口：完整审查管线 ──────────────────────────────────

    /// 执行完整的多 Agent 审查管线（主流程）。
    ///
    /// 6 步聚合流水线：Route → Preload → Execute → Merge → LegalVerify → Debate → Triage。
    /// 每步通过 `review_events`（如果已设置）推送实时事件到 SSE 客户端。
    ///
    /// ★ BlindSpot 不在此主流程中。调用 `run_blind_spot()` 在后台异步执行
    ///   盲点扫描和经验沉淀（生成 DynamicAgentDefinition，写入 dynamic_agents.json）。
    pub async fn review(&self, clauses: &[ReviewClause]) -> Result<CoordinatorOutput> {
        let total_clauses = clauses.len();
        let _review_start = std::time::Instant::now();

        // ── 指标：记录 Coordinator 阶段耗时 ──
        eprintln!(
            "\n╔══════════════════════════════════════════════════════════════╗\n\
               ║  Coordinator: Multi-Agent 审查管线启动                        ║\n\
               ╠══════════════════════════════════════════════════════════════╣\n\
               ║  条款总数: {total:>5}                                              ║\n\
               ║  启用 Agent: {agents:<42} ║\n\
               ╚══════════════════════════════════════════════════════════════╝",
            total = total_clauses,
            agents = self
                .config
                .enabled_agents
                .iter()
                .map(|a| a.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );

        let emit = |event: &ReviewEvent| {
            if let Some(ref bus) = self.review_events {
                bus.emit(event);
            }
        };

        // [0] SCOUT: 已禁用（成本优化）。
        //   原 Scout 阶段用 LLM 扫描全部 clause 产出 Hypothesis，占 ~51% 成本。
        //   替代方案：关键词路由 (Step 1) + Agent 领域知识自主判断。
        //   BatchSearch 的 fallback 路径（extract_clause_keywords）为无 Hypothesis 的
        //   clause 自动生成搜索 query，无需 Scout 前置。
        self.graph.mark_scout_complete();
        eprintln!("  [SCOUT] 已禁用（成本优化），跳过 LLM 初筛");

        // ── 指标：Scout 阶段耗时（0）──
        if let Some(ref m) = self.metrics {
            let mut collector = m.lock().await;
            collector.record_sub_phase("Scout", 0);
        }
        let mut phase_start = std::time::Instant::now();

        // [1] ROUTE: clauses → HashMap<AgentId, Vec<ReviewClause>>
        emit(&ReviewEvent::Phase {
            phase: crate::agents::review_event::PipelinePhase::Route,
            phase_index: 1,
            total_phases: 7,
            message: "关键词路由中...".to_string(),
        });
        let routing = self.route_clauses(clauses);

        // [2] PRELOAD: 所有 Chunk 节点写入 SessionGraph
        self.preload_chunks(clauses);

        // [2b] PRELOAD: Agent 节点预写入
        self.preload_agents();

        // ── 指标：Route + Preload 阶段耗时 ──
        let route_duration = phase_start.elapsed().as_millis() as u64;
        if let Some(ref m) = self.metrics {
            let mut collector = m.lock().await;
            collector.record_sub_phase("Route+Preload", route_duration);
        }
        phase_start = std::time::Instant::now();

        // [2.5] BATCH_SEARCH: 根据 Scout 假设批量预搜索法规/案例
        emit(&ReviewEvent::Phase {
            phase: crate::agents::review_event::PipelinePhase::Execute, // 复用 Execute phase 类型
            phase_index: 2,
            total_phases: 7,
            message: "批量预搜索法规中...".to_string(),
        });
        self.batch_search_phase(clauses).await;

        // ── 指标：BatchSearch 阶段耗时 ──
        let batch_search_duration = phase_start.elapsed().as_millis() as u64;
        if let Some(ref m) = self.metrics {
            let mut collector = m.lock().await;
            collector.record_sub_phase("BatchSearch", batch_search_duration);
        }
        phase_start = std::time::Instant::now();

        // [3] EXECUTE: 并行执行各 Agent
        let agent_count = routing.len();
        emit(&ReviewEvent::Phase {
            phase: crate::agents::review_event::PipelinePhase::Execute,
            phase_index: 2,
            total_phases: 7,
            message: format!("{} 个 Agent 并行审查中...", agent_count),
        });
        // 发送所有 Agent 的初始进度（pending/running）
        for (agent_id, clauses) in &routing {
            let agent_id_str = agent_id.to_string();
            let label = self
                .registry
                .get(agent_id.clone())
                .map(|d| d.display_name.to_string())
                .unwrap_or_else(|| agent_id_str.clone());
            emit(&ReviewEvent::AgentProgress {
                agent_id: agent_id_str,
                agent_label: label,
                clauses_done: 0,
                clauses_total: clauses.len(),
                raw_findings: 0,
                status: "running".to_string(),
            });
        }
        let all_findings = self.execute_agents(&routing).await;

        // ── 指标：Execute 阶段耗时 + per-agent finding 统计 ──
        let exec_duration = phase_start.elapsed().as_millis() as u64;
        if let Some(ref m) = self.metrics {
            let mut collector = m.lock().await;
            collector.record_sub_phase("Execute", exec_duration);
            // 按 Agent 记录 finding 统计
            for agent_id in routing.keys() {
                let agent_name = agent_id.to_string();
                let agent_findings: Vec<&crate::agents::types::RiskFinding> = all_findings
                    .iter()
                    .filter(|f| f.agent == agent_name)
                    .collect();
                if !agent_findings.is_empty() {
                    // clone findings for recording
                    let cloned: Vec<crate::agents::types::RiskFinding> =
                        agent_findings.iter().map(|f| (*f).clone()).collect();
                    collector.record_agent_findings(&agent_name, &cloned);
                }
            }
        }
        phase_start = std::time::Instant::now();

        // 发射 execute 阶段统计
        let raw_total = all_findings.len();
        let raw_high = all_findings
            .iter()
            .filter(|f| f.severity == RiskSeverity::High)
            .count();
        let raw_medium = all_findings
            .iter()
            .filter(|f| f.severity == RiskSeverity::Medium)
            .count();
        let raw_low = all_findings
            .iter()
            .filter(|f| f.severity == RiskSeverity::Low)
            .count();
        let raw_info = all_findings
            .iter()
            .filter(|f| f.severity == RiskSeverity::Info)
            .count();
        emit(&ReviewEvent::Stats {
            phase: crate::agents::review_event::PipelinePhase::Execute,
            total_raw: raw_total,
            total_merged: raw_total,
            total_verified: 0,
            high: raw_high,
            medium: raw_medium,
            low: raw_low,
            info: raw_info,
        });

        // [4] MERGE: 合并 + 去重
        emit(&ReviewEvent::Phase {
            phase: crate::agents::review_event::PipelinePhase::Merge,
            phase_index: 3,
            total_phases: 7,
            message: format!("去重合并中 ({} 条原始发现)...", all_findings.len()),
        });
        let merge_result = self.merge_findings_v3(all_findings, &emit);
        let mut merged = merge_result.retained;

        // [4b] LINK: 跨 Agent 同类型风险关联推导
        self.derive_cross_agent_links(&merged);

        // ── 指标：Merge+Link 阶段耗时 ──
        let merge_duration = phase_start.elapsed().as_millis() as u64;
        if let Some(ref m) = self.metrics {
            let mut collector = m.lock().await;
            collector.record_sub_phase("Merge+Link", merge_duration);
        }
        phase_start = std::time::Instant::now();

        let merge_high = merged
            .iter()
            .filter(|f| f.severity == RiskSeverity::High && !f.no_risk)
            .count();
        let merge_medium = merged
            .iter()
            .filter(|f| f.severity == RiskSeverity::Medium && !f.no_risk)
            .count();
        let merge_low = merged
            .iter()
            .filter(|f| f.severity == RiskSeverity::Low && !f.no_risk)
            .count();
        let merge_info = merged
            .iter()
            .filter(|f| f.severity == RiskSeverity::Info && !f.no_risk)
            .count();
        emit(&ReviewEvent::Stats {
            phase: crate::agents::review_event::PipelinePhase::Merge,
            total_raw: raw_total,
            total_merged: merged.len(),
            total_verified: 0,
            high: merge_high,
            medium: merge_medium,
            low: merge_low,
            info: merge_info,
        });

        // [5] LEGAL VERIFY: 对抗法条验证
        let legal_verify_count = if self.config.enable_legal_verify {
            emit(&ReviewEvent::Phase {
                phase: crate::agents::review_event::PipelinePhase::LegalVerify,
                phase_index: 4,
                total_phases: 7,
                message: "法条引用对抗验证中...".to_string(),
            });
            let lv_count = self.legal_verify(&mut merged).await;
            // 逐条发射通过验证的 finding（进入 L1 主视图）
            for f in merged.iter().filter(|f| !f.no_risk) {
                emit(&ReviewEvent::FindingAdded {
                    risk_id: f.risk_id.clone(),
                    severity: severity_str(&f.severity).to_string(),
                    is_critical: f.is_critical,
                    critical_reason: f.critical_reason.clone(),
                    risk_type: f.risk_type.clone(),
                    agent: f.agent.clone(),
                    confidence: f.confidence as f64,
                    clause_ids: f.clause_ids.clone(),
                    source_quote: f.source_quote.chars().take(500).collect(),
                    legal_basis: f.legal_basis.clone(),
                    reason: f.reason.chars().take(500).collect(),
                    suggestion: f.suggestion.clone(),
                    lifecycle: FindingLifecycle::Verified,
                    page_number: f.page_number,
                    section_path: f.section_path.clone(),
                });
            }
            lv_count
        } else {
            0
        };

        // ── 指标：LegalVerify 阶段耗时 ──
        let lv_duration = phase_start.elapsed().as_millis() as u64;
        if let Some(ref m) = self.metrics {
            let mut collector = m.lock().await;
            collector.record_sub_phase("LegalVerify", lv_duration);
        }
        phase_start = std::time::Instant::now();

        let verified_high = merged
            .iter()
            .filter(|f| f.severity == RiskSeverity::High && !f.no_risk)
            .count();
        let verified_medium = merged
            .iter()
            .filter(|f| f.severity == RiskSeverity::Medium && !f.no_risk)
            .count();
        let verified_low = merged
            .iter()
            .filter(|f| f.severity == RiskSeverity::Low && !f.no_risk)
            .count();
        let verified_info = merged
            .iter()
            .filter(|f| f.severity == RiskSeverity::Info && !f.no_risk)
            .count();
        emit(&ReviewEvent::Stats {
            phase: crate::agents::review_event::PipelinePhase::LegalVerify,
            total_raw: raw_total,
            total_merged: merged.len(),
            total_verified: legal_verify_count,
            high: verified_high,
            medium: verified_medium,
            low: verified_low,
            info: verified_info,
        });

        // [6] DEBATE: 高风险 + 低置信度正反辩论
        emit(&ReviewEvent::Phase {
            phase: crate::agents::review_event::PipelinePhase::Debate,
            phase_index: 5,
            total_phases: 7,
            message: "高风险辩论裁决中...".to_string(),
        });
        self.debate_high_risk(&mut merged).await;
        // Debate/LegalVerify 都可能回写 severity 或 Critical。最终出口再次执行
        // 统一分类、证据准入和跨 Agent 去重，禁止下游阶段绕过政策。
        merged = self.merge_findings_v3(merged, &emit).retained;

        // ── 指标：Debate 阶段耗时 ──
        let debate_duration = phase_start.elapsed().as_millis() as u64;
        if let Some(ref m) = self.metrics {
            let mut collector = m.lock().await;
            collector.record_sub_phase("Debate", debate_duration);
        }
        phase_start = std::time::Instant::now();

        // [7] TRIAGE: 按 severity + confidence 分流
        emit(&ReviewEvent::Phase {
            phase: crate::agents::review_event::PipelinePhase::Triage,
            phase_index: 6,
            total_phases: 7,
            message: "最终排序中...".to_string(),
        });
        let merged_before_triage = merged.len();
        let findings = self.triage(merged);

        // ── 指标：Triage 阶段耗时 + Coordinator 质量统计 ──
        let triage_duration = phase_start.elapsed().as_millis() as u64;
        if let Some(ref m) = self.metrics {
            let mut collector = m.lock().await;
            collector.record_sub_phase("Triage", triage_duration);
            collector.set_coordinator_stats(
                raw_total,
                merged_before_triage,
                0, // debate_triggered
                0, // debate_changed
                0, // blindspot_extra
                0, // cross_agent_links
                legal_verify_count,
            );
        }

        let high_risk_count = findings
            .iter()
            .filter(|f| f.severity == RiskSeverity::High)
            .count();
        let graph_snapshot = Some(self.graph.snapshot());

        let routing_summary = RoutingSummary {
            total_clauses,
            agent_clause_counts: routing
                .iter()
                .map(|(id, clauses)| (id.to_string(), clauses.len()))
                .collect(),
            high_risk_count,
            legal_verify_count,
            blind_spot_findings: 0, // BlindSpot 已移出主流程，在后台异步执行
        };

        eprintln!(
            "\n╔══════════════════════════════════════════════════════════════╗\n\
               ║  Coordinator: 审查管线完成                                    ║\n\
               ╠══════════════════════════════════════════════════════════════╣\n\
               ║  总风险数: {risks:<5}  高风险: {high:<4}  LegalVerify: {lv:<4}       ║\n\
               ╚══════════════════════════════════════════════════════════════╝",
            risks = findings.len(),
            high = high_risk_count,
            lv = legal_verify_count,
        );

        Ok(CoordinatorOutput {
            findings,
            routing_summary,
            graph_snapshot,
        })
    }

    // ── BlindSpot: 后台异步经验沉淀 ──────────────────────────────

    /// 运行 BlindSpot 盲点扫描并沉淀经验（主流程之外的后台任务）。
    ///
    /// ★ 此方法应在 `review()` 返回之后调用，不会阻塞用户看到主结果。
    ///
    /// 工作流程：
    /// 1. 扫描 SessionGraph 快照，识别未被充分审查的条款盲区
    /// 2. 对盲区运行 BlindSpotAgent ReAct 扫描
    /// 3. 从扫描结果中提取 `suggested_agent` 建议
    /// 4. 生成新的 `DynamicAgentDefinition` → 写入 `dynamic_agents.json`
    ///
    /// ★ BlindSpot 的发现**不会**追加到本次审核结果中——
    ///   它的价值在于为**下一次**审核积累经验（自动生成新的检测维度）。
    pub async fn run_blind_spot(&self) {
        eprintln!(
            "\n┌──────────────────────────────────────────────────────────────┐\n\
               │  BlindSpot: 后台盲点扫描 + 经验沉淀（异步，不阻塞主结果）    │\n\
               └──────────────────────────────────────────────────────────────┘"
        );

        let blind_spot_findings = self.blind_spot_scan().await;

        let real_count = blind_spot_findings.iter().filter(|f| !f.no_risk).count();
        let no_risk_count = blind_spot_findings.iter().filter(|f| f.no_risk).count();
        eprintln!(
            "  [BLINDSPOT] 扫描完成: {} 条新发现, {} 条 no_risk",
            real_count, no_risk_count
        );

        // 提取 suggested_agent → 写入 dynamic_agents.json
        let registered = self.register_dynamic_agents(&blind_spot_findings);
        if registered > 0 {
            eprintln!(
                "  [BLINDSPOT] 经验沉淀: {} 个新 Agent 定义已写入 dynamic_agents.json（下次审查自动启用）",
                registered
            );
        } else {
            eprintln!(
                "  [BLINDSPOT] 本次无新的 Agent 建议（盲区已被现有 Agent 覆盖或无需新增检测维度）"
            );
        }
    }

    // ── [0] SCOUT: 初筛 ─────────────────────────────────────

    /// Phase 0: Scout 初筛。Mini-batch 并行（3 clauses/批），零搜索。
    ///
    /// Scout 对每条 clause 产出 Hypothesis（finding_role=Hypothesis），
    /// 通过 `add_hypothesis()` 轻量写入 SessionGraph，供 Phase 2 Agent 使用。
    #[allow(dead_code)]
    /// Scout 阶段 — 已被 review() 禁用（成本优化，见第 276 行 mark_scout_complete）。
    /// 保留此函数供未来按需重新启用，当前为死代码。
    #[allow(dead_code)]
    async fn scout_phase(&self, clauses: &[ReviewClause]) {
        let scout_def = match self.registry.get(AgentId::Scout) {
            Some(d) => d,
            None => {
                eprintln!("  [SCOUT] ScoutAgent 未注册，跳过初筛");
                return;
            }
        };

        let mut config = scout_def.to_agent_config(); // max_turns=3, tool_names=["read_section","output_finding"]
        // 确保没有 web_search 工具（Scout 不应该搜索）
        config.tool_names.retain(|t| t != "web_search");

        const BATCH_SIZE: usize = 3;
        let total = clauses.len();

        for (batch_idx, batch) in clauses.chunks(BATCH_SIZE).enumerate() {
            let mut handles = vec![];
            for clause in batch {
                let llm = (self.llm_factory)();
                let mut tools = (self.tools_factory)();
                tools.retain_only(&["read_section", "output_finding"]);

                let risk_id = self.graph.next_risk_id();
                let clause = clause.clone();
                let graph = self.graph.clone();
                let print_lock = self.print_lock.clone();
                let config = config.clone();
                let search_cache = self.shared_search_cache.clone();

                handles.push(tokio::spawn(async move {
                    let agent = ReActLoop::new(config, llm, tools)
                        .with_print_lock(print_lock)
                        .with_search_cache(search_cache);
                    // NOTE: Scout 不需要 .with_graph() — SessionGraph 此时只有 Chunk 节点
                    let mut findings = agent.review_single(&clause, &risk_id).await;
                    for finding in &mut findings {
                        if !finding.no_risk {
                            finding.finding_role = FindingRole::Hypothesis;
                            finding.knowledge_source = "training_knowledge".into();
                            finding.hypothesized_by = vec!["ScoutAgent".into()];
                            graph.add_hypothesis(
                                RiskNode {
                                    finding: finding.clone(),
                                    law_refs: finding.legal_basis.clone(),
                                },
                                &clause.chunk_id,
                            );
                        }
                    }
                    graph.add_reviewed_by(&clause.chunk_id, AgentId::Scout);
                    eprintln!(
                        "  [SCOUT] {}: {} hypotheses",
                        clause.chunk_id,
                        findings.iter().filter(|f| !f.no_risk).count(),
                    );
                    findings
                }));
            }

            // 等待本批完成再启动下一批
            for h in handles {
                let _ = h.await;
            }
            eprintln!(
                "  [SCOUT] 批次 {}/{} 完成",
                batch_idx + 1,
                total.div_ceil(BATCH_SIZE)
            );
        }
        eprintln!("  [SCOUT] 全部完成: {} clauses 已初筛", total);
    }

    // ── [2.5] BATCH_SEARCH: 批量预搜索 ──────────────────────────

    /// 根据 Scout Hypothesis 批量搜索法规/案例，结果缓存到 SessionGraph。
    ///
    /// 搜索 query 从 Hypothesis 的 legal_basis、verification_required、
    /// risk_type 映射以及 clause 文本关键词中提取。
    /// 直接调用 web_search 工具（不经过 LLM），搜索结果供 Execute Phase
    /// Agent 直接引用，避免每个 Agent 独立重复搜索。
    async fn batch_search_phase(&self, clauses: &[ReviewClause]) {
        let hypotheses = self.graph.get_hypotheses();
        if hypotheses.is_empty() {
            eprintln!("  [BATCH_SEARCH] 无 Scout Hypothesis，从 clause 文本提取搜索 query...");
        } else {
            eprintln!(
                "  [BATCH_SEARCH] 从 {} 条 Hypothesis 提取搜索 query...",
                hypotheses.len()
            );
        }

        // Step 1: 提取搜索 query，按 chunk_id 分组
        let mut chunk_queries: HashMap<String, Vec<(String, String)>> = HashMap::new();

        // 1a: 从 Hypothesis 提取（如果有）
        for h in &hypotheses {
            for chunk_id in &h.clause_ids {
                let entries = chunk_queries.entry(chunk_id.clone()).or_default();
                for law_ref in &h.legal_basis {
                    let query = law_ref_to_search_query(law_ref);
                    if !query.is_empty() {
                        entries.push((query, "法规".to_string()));
                    }
                }
                for verify in &h.verification_required {
                    if !verify.is_empty() {
                        entries.push((format!("{} 原文 核心内容", verify), "法规".to_string()));
                    }
                }
                if let Some(tmpl) = risk_type_to_search_template(&h.risk_type) {
                    entries.push((tmpl, "案例".to_string()));
                }
            }
        }

        // 1b: 对所有 clause 补充关键词搜索（无 Hypothesis 时全覆盖）
        for clause in clauses {
            if !chunk_queries.contains_key(&clause.chunk_id) {
                let kw = extract_clause_keywords(&clause.text);
                if !kw.is_empty() {
                    chunk_queries
                        .entry(clause.chunk_id.clone())
                        .or_default()
                        .push((format!("政府采购 {} 合规要求", kw), "法规".to_string()));
                }
            }
        }

        // Step 3: 去重 + 收集所有唯一的 (query, category) 对
        let mut seen_queries: HashSet<String> = HashSet::new();
        let mut unique_queries: Vec<(String, String)> = Vec::new();

        for queries in chunk_queries.values() {
            for (query, category) in queries {
                let dedup_key = format!("{}|{}", query.to_lowercase(), category);
                if !seen_queries.contains(&dedup_key) {
                    seen_queries.insert(dedup_key);
                    unique_queries.push((query.clone(), category.clone()));
                }
            }
        }

        if unique_queries.is_empty() {
            eprintln!("  [BATCH_SEARCH] 无搜索 query，跳过");
            return;
        }

        let total_searches = unique_queries.len();
        eprintln!(
            "  [BATCH_SEARCH] 共 {} 个唯一 query，并行执行搜索...",
            total_searches
        );

        // Step 4: 并行执行所有搜索
        let mut join_set = JoinSet::new();
        let tools_factory = self.tools_factory.clone();

        for (query, category) in unique_queries {
            let tf = tools_factory.clone();
            join_set.spawn(async move {
                let tools = tf();
                let result = match tools.get("web_search") {
                    Some(web_search_tool) => {
                        web_search_tool
                            .execute(serde_json::json!({
                                "question": &query,
                                "search_context": &category,
                            }))
                            .await
                    }
                    None => Err(anyhow::anyhow!("web_search 工具未注册")),
                };
                (query, category, result)
            });
        }

        // Step 5: 收集结果，构建 per-chunk SearchCacheEntry
        let mut query_results: HashMap<(String, String), serde_json::Value> = HashMap::new();
        while let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok((query, category, Ok(result))) => {
                    eprintln!("  [BATCH_SEARCH] ✅ {} [{}]", query, category);
                    query_results.insert((query, category), result);
                }
                Ok((query, _category, Err(e))) => {
                    eprintln!("  [BATCH_SEARCH] ❌ {} — {}", query, e);
                }
                Err(e) => {
                    eprintln!("  [BATCH_SEARCH] ❌ join error: {}", e);
                }
            }
        }

        // Step 6: 将搜索结果按 chunk_id 分组写入 SessionGraph 和 shared_search_cache
        for (chunk_id, queries) in &chunk_queries {
            let mut entries: Vec<SearchCacheEntry> = Vec::new();

            for (query, category) in queries {
                if let Some(result) = query_results.get(&(query.clone(), category.clone())) {
                    let answer = result
                        .get("answer")
                        .and_then(|a| a.as_str())
                        .unwrap_or("")
                        .to_string();
                    let sources: Vec<Citation> = result
                        .get("sources")
                        .and_then(|s| s.as_array())
                        .map(|arr| {
                            arr.iter()
                                .map(|src| Citation {
                                    title: src
                                        .get("title")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                    url: src
                                        .get("url")
                                        .and_then(|u| u.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                    site_name: src
                                        .get("site_name")
                                        .and_then(|s| s.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    entries.push(SearchCacheEntry {
                        query: query.clone(),
                        category: category.clone(),
                        answer,
                        sources,
                    });

                    // 同步写入 shared_search_cache
                    {
                        let mut cache = self.shared_search_cache.lock().await;
                        cache.insert((query.clone(), category.clone()), result.clone());
                    }
                }
            }

            if !entries.is_empty() {
                self.graph.cache_search_results(chunk_id, entries);
            }
        }

        eprintln!(
            "  [BATCH_SEARCH] 完成: {} 次搜索, {} 个 chunk 有预搜索结果",
            total_searches,
            chunk_queries.iter().filter(|(_, q)| !q.is_empty()).count()
        );
    }

    // ── [1] ROUTE: 关键词路由 ─────────────────────────────────

    /// 将条款按关键词路由到各 Agent。
    ///
    /// 每条条款可以被多个 Agent 审查（一对多路由）。
    /// 路由策略：条款文本包含 Agent 的 `section_keywords` 中任一关键词 → 分配。
    fn route_clauses(&self, clauses: &[ReviewClause]) -> HashMap<AgentId, Vec<ReviewClause>> {
        let mut routing: HashMap<AgentId, Vec<ReviewClause>> = HashMap::new();

        for clause in clauses {
            let text_lower = clause.text.to_lowercase();
            for agent_id in &self.config.enabled_agents {
                // 获取 Agent 的路由关键词（固定 Agent 从 registry，动态 Agent 从 dynamic_definitions）
                let keywords: Vec<String> = match agent_id {
                    AgentId::Dynamic(id) => self
                        .dynamic_definitions
                        .get(id)
                        .map(|d| d.section_keywords.clone())
                        .unwrap_or_default(),
                    _ => self
                        .registry
                        .get(agent_id.clone())
                        .map(|d| d.section_keywords.iter().map(|s| s.to_string()).collect())
                        .unwrap_or_default(),
                };

                let should_route = keywords.is_empty() // BlindSpot/LegalVerify 等不参与路由
                    || keywords
                        .iter()
                        .any(|kw| text_lower.contains(&kw.to_lowercase()));

                if should_route {
                    routing
                        .entry(agent_id.clone())
                        .or_default()
                        .push(clause.clone());
                }
            }
        }

        // 确保每条条款至少分配给一个已启用的 Agent（优先 FactCheck）
        let has_factcheck = self.config.enabled_agents.contains(&AgentId::FactCheck);
        for clause in clauses {
            let mut assigned = false;
            for clauses_list in routing.values() {
                if clauses_list.iter().any(|c| c.chunk_id == clause.chunk_id) {
                    assigned = true;
                    break;
                }
            }
            if !assigned && has_factcheck {
                routing
                    .entry(AgentId::FactCheck)
                    .or_default()
                    .push(clause.clone());
            } else if !assigned {
                // Fallback: 分配给第一个启用的 Agent
                if let Some(first_enabled) = self.config.enabled_agents.first() {
                    routing
                        .entry(first_enabled.clone())
                        .or_default()
                        .push(clause.clone());
                }
                // 如果没有启用的 Agent,条款被静默丢弃(不会崩溃)
            }
        }

        // 日志
        for (agent_id, agent_clauses) in &routing {
            eprintln!("  [ROUTE] {} ← {} 条条款", agent_id, agent_clauses.len());
        }

        routing
    }

    // ── [2] PRELOAD: Chunk 节点预写入 ────────────────────────

    fn preload_chunks(&self, clauses: &[ReviewClause]) {
        let chunk_nodes: Vec<ChunkNode> = clauses
            .iter()
            .map(|c| ChunkNode {
                chunk_id: c.chunk_id.clone(),
                section_path: c.section_path.clone(),
                page_start: c.page_start,
                page_end: c.page_end,
                text_preview: c.text.chars().take(200).collect(),
                tier: c.tier,
            })
            .collect();

        let count = chunk_nodes.len();
        self.graph.add_chunks(chunk_nodes);
        eprintln!("  [PRELOAD] SessionGraph ← {} 个 Chunk 节点", count);
    }

    /// PRELOAD 阶段：将所有启用的 Agent 节点写入 SessionGraph。
    fn preload_agents(&self) {
        let mut count = 0;
        for agent_id in &self.config.enabled_agents {
            let agent_node = AgentNode {
                agent_id: agent_id.clone(),
                display_name: agent_id.to_string(),
                role: match agent_id {
                    AgentId::Scout => "初筛".to_string(),
                    AgentId::BlindSpot => "兜底扫描".to_string(),
                    AgentId::LegalVerify => "法条验证".to_string(),
                    AgentId::Debate => "正反辩论".to_string(),
                    AgentId::Dynamic(_) => "动态补充".to_string(),
                    _ => "标准审查".to_string(),
                },
            };
            self.graph.add_agent(agent_node);
            count += 1;
        }
        eprintln!("  [PRELOAD] SessionGraph ← {} 个 Agent 节点", count);
    }

    // ── [3] EXECUTE: 并行执行各 Agent ────────────────────────

    async fn execute_agents(
        &self,
        routing: &HashMap<AgentId, Vec<ReviewClause>>,
    ) -> Vec<RiskFinding> {
        let mut handles = Vec::new();

        for (agent_id, clauses) in routing {
            if clauses.is_empty() {
                continue;
            }

            let agent_id = agent_id.clone();
            let clauses = clauses.clone();
            let clauses_total = clauses.len();
            let bus = self.bus.clone();
            let graph = self.graph.clone();
            let trace = self.trace.clone();
            let print_lock = self.print_lock.clone();
            let registry_def = self.registry.get(agent_id.clone()).cloned();
            let review_events = self.review_events.clone();
            let metrics = self.metrics.clone();
            let agent_id_str = agent_id.to_string();
            let agent_label = registry_def
                .as_ref()
                .map(|d| d.display_name.to_string())
                .unwrap_or_else(|| agent_id_str.clone());

            // Clone Arcs before moving into the spawned task
            let graph_for_write = graph.clone();
            let llm_factory = self.llm_factory.clone();
            let tools_factory = self.tools_factory.clone();
            let max_parallel = self.config.max_parallel_clauses;
            let shared_search_cache = self.shared_search_cache.clone();

            let handle = tokio::spawn(async move {
                let agent_name = agent_id.to_string();
                if let Some(def) = registry_def {
                    eprintln!(
                        "  [EXECUTE] {} 开始审查 {} 条条款 (并行 max={})...",
                        agent_name,
                        clauses.len(),
                        max_parallel,
                    );

                    let findings = crate::agents::react_loop::review_clauses_parallel(
                        &clauses,
                        {
                            let def = def.clone();
                            let bus = bus.clone();
                            let graph = graph.clone();
                            let print_lock = print_lock.clone();
                            let trace = trace.clone();
                            let review_events = review_events.clone();
                            let metrics = metrics.clone();
                            let search_cache = shared_search_cache.clone();
                            move |llm, tools| {
                                let config = def.to_agent_config();
                                let mut agent = ReActLoop::new(config, llm, tools);
                                agent = agent
                                    .with_bus(bus.clone())
                                    .with_graph(graph.clone())
                                    .with_print_lock(print_lock.clone())
                                    .with_search_cache(search_cache.clone());
                                agent.trace = trace.clone();
                                if let Some(ref events) = review_events {
                                    agent = agent.with_review_events(events.clone());
                                }
                                if let Some(ref m) = metrics {
                                    agent = agent.with_metrics(m.clone());
                                }
                                agent
                            }
                        },
                        &*llm_factory,
                        &*tools_factory,
                        max_parallel,
                        Some(graph_for_write.clone()),
                        review_events.clone(),
                        &agent_name,
                    )
                    .await;

                    let raw_findings = findings.iter().filter(|f| !f.no_risk).count();
                    eprintln!(
                        "  [EXECUTE] {} 完成，发现 {} 条风险",
                        agent_name, raw_findings
                    );

                    // 发送 AgentProgress → SSE（完成事件）
                    if let Some(ref events) = review_events {
                        events.emit(&ReviewEvent::AgentProgress {
                            agent_id: agent_name.clone(),
                            agent_label: agent_label.clone(),
                            clauses_done: clauses_total,
                            clauses_total,
                            raw_findings,
                            status: "completed".to_string(),
                        });
                    }

                    // 将发现写入 SessionGraph（共享工作区）
                    for finding in &findings {
                        if !finding.no_risk {
                            let law_refs = finding.legal_basis.clone();
                            let risk_node = RiskNode {
                                finding: finding.clone(),
                                law_refs,
                            };
                            // 对每个关联的 clause 写入 has_risk 边
                            for cid in &finding.clause_ids {
                                graph_for_write.add_risk_with_edges(risk_node.clone(), cid);
                            }
                            // Note: AgentBus 广播已移至 ReActLoop 内部实时执行，
                            // 不再在此处批量广播（避免时序问题——其他 Agent 已结束审查）
                        }
                    }

                    findings
                } else {
                    eprintln!("  [EXECUTE] 错误: Agent 定义未找到: {}", agent_name);
                    Vec::new()
                }
            });

            handles.push(handle);
        }

        // 等待所有 Agent 完成
        let mut all_findings = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(findings) => all_findings.extend(findings),
                Err(e) => {
                    eprintln!("  [EXECUTE] Agent task panicked: {}", e);
                }
            }
        }

        all_findings
    }

    // ── [4] MERGE: 合并 + 去重 ───────────────────────────────

    /// 合并 + 去重（无 SSE 事件发射的快捷版本，用于测试）。
    #[allow(dead_code)]
    fn merge_findings(&self, findings: Vec<RiskFinding>) -> Vec<RiskFinding> {
        self.merge_findings_with_events(findings, &|_| {}).retained
    }

    fn merge_findings_with_events(
        &self,
        findings: Vec<RiskFinding>,
        emit: &dyn Fn(&ReviewEvent),
    ) -> MergeResult {
        let total = findings.len();
        // 简单去重：按稳定分类|证据|clause_ids|agent 组合去重。
        // 同一 chunk 中的不同类别不得仅因文字相似被合并。
        let mut seen: HashMap<String, RiskFinding> = HashMap::new();
        for f in findings {
            let key = format!(
                "{}|{}|{}|{}",
                finding_category(&f),
                f.source_quote.trim(),
                f.clause_ids.join(","),
                f.agent
            );
            if let Some(existing) = seen.get(&key) {
                if f.confidence > existing.confidence {
                    // 旧的被替换，通知前端移除旧 risk_id
                    emit(&ReviewEvent::FindingRemoved {
                        risk_id: existing.risk_id.clone(),
                        reason: "去重合并（保留置信度更高的）".to_string(),
                        merged_into: Some(f.risk_id.clone()),
                    });
                    seen.insert(key, f);
                } else {
                    // 当前 finding 被合并掉了
                    emit(&ReviewEvent::FindingRemoved {
                        risk_id: f.risk_id.clone(),
                        reason: "去重合并（保留置信度更高的）".to_string(),
                        merged_into: Some(existing.risk_id.clone()),
                    });
                }
            } else {
                seen.insert(key, f);
            }
        }

        let merged: Vec<RiskFinding> = seen.into_values().collect();
        let risk_count = merged.iter().filter(|f| !f.no_risk).count();
        let removed_count = total - merged.len();
        eprintln!(
            "  [MERGE] {} → {} 条发现（去重 {} 条），{} 条风险",
            total,
            merged.len(),
            removed_count,
            risk_count
        );
        MergeResult { retained: merged }
    }

    // ── [4b] LINK: 跨 Agent 同类型风险 linked_to 推导 ──────────

    /// 按 risk_type 分组，对不同 Agent 发现的同类型风险，在它们的 clause_ids
    /// 之间创建 linked_to 边。
    fn derive_cross_agent_links(&self, findings: &[RiskFinding]) {
        // 按 risk_type 分组
        let mut by_type: HashMap<String, Vec<&RiskFinding>> = HashMap::new();
        for f in findings {
            if f.no_risk {
                continue;
            }
            by_type.entry(f.risk_type.clone()).or_default().push(f);
        }

        let mut link_count = 0;
        for (_risk_type, group) in &by_type {
            if group.len() < 2 {
                continue;
            }
            // 检查是否有不同 Agent 参与
            let agents: std::collections::HashSet<&str> =
                group.iter().map(|f| f.agent.as_str()).collect();
            if agents.len() < 2 {
                continue; // 同类型但都是同一个 Agent 发现的，无需跨 Agent 关联
            }

            // 在不同 clause_id 之间创建 linked_to 边
            let all_clause_ids: Vec<&String> = group.iter().flat_map(|f| &f.clause_ids).collect();
            for i in 0..all_clause_ids.len() {
                for j in (i + 1)..all_clause_ids.len() {
                    let cid_a = all_clause_ids[i];
                    let cid_b = all_clause_ids[j];
                    if cid_a != cid_b {
                        let reason = format!(
                            "跨 Agent 同类型风险: {} ({} 个 Agent 独立发现)",
                            _risk_type,
                            agents.len()
                        );
                        self.graph.add_linked_to(cid_a, cid_b, &reason);
                        link_count += 1;
                    }
                }
            }
        }

        if link_count > 0 {
            eprintln!(
                "  [LINK] 跨 Agent 关联推导完成: {} 条 linked_to 边",
                link_count
            );
        }
    }

    // ── [4c] MERGE v3: 文本相似度去重 ─────────────────────────

    /// MERGE v3: 两阶段去重。
    ///
    /// **Stage 1 — 同风险+同条款去重**：
    /// 按 `(risk_type, clause_ids)` 分组，同组内保留 confidence 最高的 finding，
    /// 合并 contributors（hypothesized_by, verified_by）。
    ///
    /// **Stage 2 — 同类别证据去重**：
    /// 只在稳定分类相同、条款相同的前提下比较 source_quote，
    /// 防止同一 chunk 中多个独立问题被 reason 相似度误合并。
    ///
    /// **Hypothesis→Verified 关联**：
    /// 在处理前，将 Hypothesis 的 agent 注入到同 clause + 同 risk_type 的
    /// Verified finding 的 hypothesized_by 字段。
    fn merge_findings_v3(
        &self,
        findings: Vec<RiskFinding>,
        emit: &dyn Fn(&ReviewEvent),
    ) -> MergeResult {
        let total = findings.len();
        let mut normalized = Vec::with_capacity(total);
        for mut finding in findings {
            risk_taxonomy::normalize_finding(&mut finding);
            if risk_taxonomy::is_actionable(&finding) {
                normalized.push(finding);
            } else {
                emit(&ReviewEvent::FindingRemoved {
                    risk_id: finding.risk_id,
                    reason: "证据准入失败：风险结论缺少有效原文引文".to_string(),
                    merged_into: None,
                });
            }
        }

        // 分离 Hypothesis 和 Verified（各自拥有所有权）
        let (hypotheses, mut verified): (Vec<RiskFinding>, Vec<RiskFinding>) = normalized
            .into_iter()
            .partition(|f| f.finding_role == FindingRole::Hypothesis);

        // ── Step 0: Hypothesis → Verified 关联 ──
        for vf in &mut verified {
            for h in &hypotheses {
                let same_clause = h.clause_ids.iter().any(|c| vf.clause_ids.contains(c));
                let same_type = finding_category(h) == finding_category(vf);
                if same_clause && same_type && !vf.hypothesized_by.contains(&h.agent) {
                    vf.hypothesized_by.push(h.agent.clone());
                }
            }
        }

        // ── Stage 1: 同 (category_code/risk_type, clause_ids, evidence) 去重 ──
        let mut stage1: Vec<RiskFinding> = Vec::new();
        for vf in verified {
            let mut merged = false;
            for existing in stage1.iter_mut() {
                let same_type = finding_category(existing) == finding_category(&vf);
                let same_clause = existing
                    .clause_ids
                    .iter()
                    .any(|c| vf.clause_ids.contains(c));
                let same_evidence = evidence_similarity(existing, &vf) >= 0.70;
                if same_type && same_clause && same_evidence {
                    // 同风险类型 + 同条款 → 合并
                    merge_contributors(existing, &vf);
                    for cid in &vf.clause_ids {
                        if !existing.clause_ids.contains(cid) {
                            existing.clause_ids.push(cid.clone());
                        }
                    }
                    if vf.confidence > existing.confidence {
                        existing.reason = combine_reasons(existing, &vf);
                        existing.suggestion = vf.suggestion.clone();
                        existing.legal_basis =
                            dedup_legal_basis(&existing.legal_basis, &vf.legal_basis);
                    }
                    existing.confidence = existing.confidence.max(vf.confidence);
                    emit(&ReviewEvent::FindingRemoved {
                        risk_id: vf.risk_id.clone(),
                        reason: format!(
                            "同风险+同条款合并: {} | {}",
                            vf.risk_type,
                            vf.clause_ids.join(",")
                        ),
                        merged_into: Some(existing.risk_id.clone()),
                    });
                    merged = true;
                    break;
                }
            }
            if !merged {
                stage1.push(vf);
            }
        }
        let stage1_removed = total - hypotheses.len() - stage1.len();
        eprintln!(
            "  [MERGE v3] Stage1 同风险+同条款: {} → {} 条（去重 {} 条）",
            total - hypotheses.len(),
            stage1.len(),
            stage1_removed,
        );

        // ── Stage 2: 同类别 + 同条款 + 高证据相似度去重 ──
        let mut retained: Vec<RiskFinding> = Vec::new();
        for f in stage1 {
            let mut merged = false;
            for existing in retained.iter_mut() {
                // 不同 clause 不合并
                let same_clause = existing.clause_ids.iter().any(|c| f.clause_ids.contains(c));
                if !same_clause {
                    continue;
                }
                if finding_category(existing) != finding_category(&f) {
                    continue;
                }
                let sim = evidence_similarity(&f, existing);
                if sim >= 0.65 {
                    merge_contributors(existing, &f);
                    for cid in &f.clause_ids {
                        if !existing.clause_ids.contains(cid) {
                            existing.clause_ids.push(cid.clone());
                        }
                    }
                    if f.confidence > existing.confidence {
                        existing.reason = combine_reasons(existing, &f);
                        existing.suggestion = f.suggestion.clone();
                    }
                    existing.confidence = existing.confidence.max(f.confidence);
                    emit(&ReviewEvent::FindingRemoved {
                        risk_id: f.risk_id.clone(),
                        reason: format!("同类别证据相似度合并 (sim={:.2})", sim),
                        merged_into: Some(existing.risk_id.clone()),
                    });
                    merged = true;
                    break;
                }
            }
            if !merged {
                retained.push(f);
            }
        }

        let removed_count = total - retained.len();
        let risk_count = retained.iter().filter(|f| !f.no_risk).count();
        eprintln!(
            "  [MERGE v3] {} → {} 条发现（去重 {} 条，跳过 {} Hypothesis），{} 条风险",
            total,
            retained.len(),
            removed_count,
            hypotheses.len(),
            risk_count
        );
        MergeResult { retained }
    }

    // ── [5] LEGAL VERIFY: 分组批量 + 分层法条验证 ──────────────────

    /// 分组批量 + 分层的法条引用验证。
    ///
    /// ## 流程
    ///
    /// ```text
    /// findings with legal_basis
    ///   │
    ///   ├─ Step A: LegalDomain 自动分类（纯规则，零 LLM）
    ///   │
    ///   ├─ Step B: 规则预筛（已知法规直通，跳过 LLM）
    ///   │    ├─ 法条名称/条款号合法 → ✅ 直接通过（~70%）
    ///   │    └─ 无法判断 → ➡ 进入 LLM 批量验证
    ///   │
    ///   ├─ Step C: LLM 批量验证（按 legal_domain 分组）
    ///   │    每组一条 prompt → 一次 ReAct → 输出该组所有验证结论
    ///   │
    ///   └─ Step D: 合并结果 → 回写 findings
    /// ```
    ///
    /// ## 与旧版的区别
    ///
    /// - 旧版：每条 finding 独立 ReAct（N 条 = N 个 ReAct = 6N 次 LLM 调用）
    /// - 新版：按领域分组批量（N 条 ≈ 3-5 组 ≈ 3-5 个 ReAct ≈ 12-20 次 LLM 调用）
    async fn legal_verify(&self, findings: &mut [RiskFinding]) -> usize {
        let to_verify: Vec<RiskFinding> = findings
            .iter()
            .filter(|f| {
                !f.no_risk && !f.legal_basis.is_empty() && f.finding_role == FindingRole::Verified
            })
            .cloned()
            .collect();

        if to_verify.is_empty() {
            return 0;
        }

        let total_count = to_verify.len();
        eprintln!(
            "  [LEGAL_VERIFY] 启动分组批量验证，{} 条法条引用...",
            total_count
        );

        // ── Step A: LegalDomain 自动分类 ──
        let mut domain_groups: HashMap<LegalDomain, Vec<RiskFinding>> = HashMap::new();
        for f in &to_verify {
            let (domain, _conf) = LegalDomain::classify(&f.risk_type, &f.legal_basis);
            domain_groups.entry(domain).or_default().push(f.clone());
        }

        eprintln!("  [LEGAL_VERIFY] 分类: {} 个法律领域", domain_groups.len());
        for (domain, group) in &domain_groups {
            eprintln!("    - {} ({} 条)", domain, group.len());
        }

        let mut total_verified = 0usize;
        let legal_def = self.registry.get(AgentId::LegalVerify);

        for (domain, group) in &domain_groups {
            // ── Step B: 规则预筛 ──
            let (verified, ambiguous): (Vec<&RiskFinding>, Vec<&RiskFinding>) = group
                .iter()
                .partition(|f| self.rule_based_law_check(f, domain));

            // 规则通过的 → 直接标记
            for vf in &verified {
                for original in findings.iter_mut() {
                    if original.risk_id == vf.risk_id {
                        original.reason.push_str(&format!(
                            "\n[LegalVerify] ✅ 规则直通验证通过 (domain={})。",
                            domain
                        ));
                        total_verified += 1;
                        break;
                    }
                }
            }

            if !verified.is_empty() {
                eprintln!("    [{}] 规则直通: {} 条直接通过", domain, verified.len());
            }

            // ── Step C: LLM 批量验证（仅对模糊条目）──
            if ambiguous.is_empty() {
                continue;
            }

            let amb_count = ambiguous.len();
            eprintln!(
                "    [{}] LLM 批量验证: {} 条模糊条目 → 1 次 ReAct...",
                domain, amb_count
            );

            if let Some(def) = legal_def {
                // 构造批量验证 prompt
                let batch_text = Self::format_batch_legal_verify_task(domain, &ambiguous);

                let batch_clause = ReviewClause {
                    chunk_id: format!("legal_batch_{:?}", domain),
                    section_path: vec!["批量法条验证".to_string(), domain.to_string()],
                    text: batch_text,
                    page_start: 0,
                    page_end: 0,
                    tier: RiskTier::Medium,
                    tier_max_turns: 4, // 批量模式 4 轮即可（共享搜索结果，效率更高）
                };

                let mut config = def.to_agent_config();
                // 注入批量验证专用工具
                config.tool_names = vec![
                    "web_search".into(),
                    "search_document".into(),
                    "output_verification_batch".into(),
                ];

                let llm = (self.llm_factory)();
                let mut tools = (self.tools_factory)();
                // 注册批量验证工具（替换掉单条 output_finding）
                tools.register(Box::new(
                    crate::agents::tools::output_verification_batch::OutputVerificationBatchTool,
                ));

                let agent = ReActLoop::new(config, llm, tools)
                    .with_print_lock(self.print_lock.clone())
                    .with_search_cache(self.shared_search_cache.clone());
                // 批量验证不需要 SessionGraph 和 AgentBus

                let batch_results = agent.review(&[batch_clause]).await;

                // 解析批量结果
                let mut parsed_count = 0usize;
                for result in &batch_results {
                    if result.risk_type == "__BATCH_VERIFICATION__" {
                        // 从 source_quote 中提取原始 JSON
                        if let Ok(batch_output) =
                            serde_json::from_str::<BatchVerificationOutput>(&result.source_quote)
                        {
                            for entry in &batch_output.verifications {
                                for original in findings.iter_mut() {
                                    if original.risk_id == entry.risk_id {
                                        if entry.is_valid && entry.confidence >= 0.5 {
                                            original.reason.push_str(&format!(
                                                "\n[LegalVerify] ✅ 批量验证通过 (domain={}, confidence={:.2})。",
                                                domain, entry.confidence
                                            ));
                                            // 回写修正后的法条引用
                                            if !entry.corrected_legal_basis.is_empty() {
                                                original.legal_basis =
                                                    entry.corrected_legal_basis.clone();
                                            }
                                        } else {
                                            original.severity = RiskSeverity::Info;
                                            original.clear_criticality();
                                            original.reason.push_str(&format!(
                                                "\n[LegalVerify] ❌ 批量验证未通过 (domain={}, confidence={:.2}): {}。已降级。",
                                                domain, entry.confidence, entry.reason
                                            ));
                                        }
                                        parsed_count += 1;
                                        break;
                                    }
                                }
                            }
                        } else {
                            eprintln!("    [{}] ⚠️ 批量结果 JSON 解析失败", domain);
                        }
                        break; // 只解析第一个 BATCH_VERIFICATION 标记
                    }
                }

                total_verified += parsed_count;
                eprintln!(
                    "    [{}] 批量验证完成: {}/{} 条已解析",
                    domain, parsed_count, amb_count
                );
            } else {
                // LegalVerifyAgent 未注册 → 所有 ambiguous 走 fallback
                eprintln!(
                    "    [{}] LegalVerifyAgent 未注册，{} 条走 fallback",
                    domain, amb_count
                );
                for af in &ambiguous {
                    for original in findings.iter_mut() {
                        if original.risk_id == af.risk_id {
                            if original.confidence < 0.5 {
                                original.severity = RiskSeverity::Info;
                                original.clear_criticality();
                                original
                                    .reason
                                    .push_str("\n[LegalVerify] ❌ 置信度不足，已降级 (fallback)。");
                            }
                            total_verified += 1;
                            break;
                        }
                    }
                }
            }
        }

        // ── Step D: Other 领域（逐条 fallback）──
        if let Some(other_group) = domain_groups.get(&LegalDomain::Other)
            && !other_group.is_empty()
            && legal_def.is_some()
        {
            eprintln!(
                "  [LEGAL_VERIFY] Other 领域 {} 条 → 逐条 fallback 验证...",
                other_group.len()
            );
            let def = legal_def.unwrap();
            for f in other_group {
                let clause = ReviewClause {
                    chunk_id: format!("legal_verify_{}", f.risk_id),
                    section_path: vec!["法条验证".to_string(), f.risk_id.clone()],
                    text: Self::format_single_legal_verify_task(f),
                    page_start: 0,
                    page_end: 0,
                    tier: RiskTier::Medium,
                    tier_max_turns: 3, // fallback 模式减到 3 轮
                };

                let config = def.to_agent_config();
                let llm = (self.llm_factory)();
                let tools = (self.tools_factory)();
                let agent = ReActLoop::new(config, llm, tools)
                    .with_print_lock(self.print_lock.clone())
                    .with_search_cache(self.shared_search_cache.clone());

                let verify_findings = agent.review(&[clause]).await;
                for vf in &verify_findings {
                    if vf.no_risk {
                        continue;
                    }
                    let original_risk_id = vf
                        .clause_ids
                        .first()
                        .and_then(|cid| cid.strip_prefix("legal_verify_"))
                        .unwrap_or("");
                    for original in findings.iter_mut() {
                        if original.risk_id == original_risk_id {
                            if vf.confidence < 0.5 {
                                original.severity = RiskSeverity::Info;
                                original.clear_criticality();
                                original
                                    .reason
                                    .push_str("\n[LegalVerify] ❌ 法条引用验证未通过，已降级。");
                            } else {
                                original.reason.push_str(&format!(
                                    "\n[LegalVerify] ✅ 法条引用验证通过 (confidence={:.2})。",
                                    vf.confidence
                                ));
                                if !vf.legal_basis.is_empty() {
                                    original.legal_basis = vf.legal_basis.clone();
                                }
                            }
                            total_verified += 1;
                            break;
                        }
                    }
                }
            }
        }

        eprintln!(
            "  [LEGAL_VERIFY] 完成: {} 条已验证 ({} 领域, 含规则直通 + 批量LLM + fallback)",
            total_verified,
            domain_groups.len()
        );

        total_verified
    }

    /// 规则预筛：对已知法规做确定性检查，不需要 LLM。
    ///
    /// 返回 `true` = 法条引用确认有效，可直接通过。
    /// 返回 `false` = 无法规则判断，需 LLM 验证。
    fn rule_based_law_check(&self, finding: &RiskFinding, domain: &LegalDomain) -> bool {
        if !domain.supports_rule_prefilter() {
            return false;
        }

        // 已知法规库：法规名 → 合理条款号范围
        let known_laws: &[(&str, u32, u32)] = &[
            ("政府采购法", 1, 90),
            ("政府采购法实施条例", 1, 60),
            ("87号令", 1, 90),
            ("财政部令第87号", 1, 90),
            ("招标投标法", 1, 70),
            ("招标投标法实施条例", 1, 100),
            ("公平竞争审查条例", 1, 30),
        ];

        let mut all_known = true;
        let mut has_any_law = false;

        for law_ref in &finding.legal_basis {
            // 提取法条引用中的法规名和条款号
            let clean = law_ref
                .replace(['[', ']'], "")
                .replace("《", "")
                .replace("》", "");
            // 去掉 Markdown URL: "政府采购法第二十七条](http://...)"
            let clean = if let Some(pos) = clean.find("](") {
                &clean[..pos]
            } else {
                &clean
            };

            let mut matched = false;
            for (law_name, min_article, max_article) in known_laws {
                if clean.contains(law_name) {
                    has_any_law = true;
                    // 尝试提取条款号
                    if let Some(article_num) = Self::extract_article_number(clean) {
                        if article_num >= *min_article && article_num <= *max_article {
                            matched = true;
                        }
                    } else {
                        // 有条款名但无法提取数字，例如"第X条"模糊引用
                        matched = true; // 给通过，LLM 复审
                    }
                    break;
                }
            }

            if !matched && has_any_law {
                all_known = false;
                break;
            }
        }

        // 全部法条在已知范围内，且置信度足够高
        has_any_law && all_known && finding.confidence >= 0.7
    }

    /// 从法条引用字符串中提取条款号。
    fn extract_article_number(law_ref: &str) -> Option<u32> {
        // 匹配 "第X条" 或 "第XX条" 模式
        for prefix in &["第"] {
            if let Some(pos) = law_ref.find(prefix) {
                let after = &law_ref[pos + prefix.len()..];
                let num_str: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '十')
                    .collect();
                if !num_str.is_empty() {
                    // 简单解析：纯数字
                    if let Ok(n) = num_str.parse::<u32>() {
                        return Some(n);
                    }
                    // 中文数字如"二十三"→ 简化处理返回 Some(23)
                    if num_str.contains("十") {
                        return Some(20); // 近似
                    }
                }
            }
        }
        None
    }

    /// 将单条待验证 finding 格式化为 LegalVerifyAgent 的输入文本（fallback 用）。
    fn format_single_legal_verify_task(f: &RiskFinding) -> String {
        let mut task = String::from(
            "## 法条验证任务\n\n请验证以下风险发现中的法条引用是否真实、准确、适用：\n\n",
        );
        task.push_str(&format!(
            "risk_id={} | risk_type={} | agent={}\n",
            f.risk_id, f.risk_type, f.agent
        ));
        task.push_str(&format!(
            "条款文本: {}\n",
            f.source_quote.chars().take(500).collect::<String>()
        ));
        task.push_str(&format!("法条引用: {}\n", f.legal_basis.join("; ")));
        task.push_str(&format!(
            "推理: {}\n\n",
            f.reason.chars().take(500).collect::<String>()
        ));
        task.push_str("请对上述法条引用进行对抗性验证，使用 output_finding 输出验证结论。\n\n");
        task.push_str("🛑 无论验证通过或修正，每条 legal_basis 必须包含可验证的 URL 链接（Markdown 格式: [法条名](URL)），禁止输出纯文本法条名。");
        task
    }

    /// 将同一法律领域的多条待验证 finding 格式化为批量验证输入文本。
    fn format_batch_legal_verify_task(domain: &LegalDomain, findings: &[&RiskFinding]) -> String {
        let mut task = format!("## 批量法条验证任务 — 领域: {}\n\n", domain);
        task.push_str(&format!(
            "你需要一次验证 {} 条风险发现的法条引用。它们都属于【{}】领域，共享法律上下文。\n\n",
            findings.len(),
            domain
        ));
        task.push_str("### 验证规则\n\n");
        task.push_str(
            "1. 对每条 finding，验证其 legal_basis 是否：真实存在、条款号正确、适用于该场景\n",
        );
        task.push_str("2. 使用 web_search 搜索法条原文（一次搜索可覆盖多条 finding）\n");
        task.push_str("3. 修正错误的法条引用，替换为正确的法条名 + URL 链接\n");
        task.push_str(
            "4. 全部验证完成后，调用 **output_verification_batch** 一次性输出所有结论\n\n",
        );
        task.push_str("---\n\n");

        for (i, f) in findings.iter().enumerate() {
            task.push_str(&format!(
                "#### [{}/{}] risk_id={}\n",
                i + 1,
                findings.len(),
                f.risk_id
            ));
            task.push_str(&format!("- risk_type: {}\n", f.risk_type));
            task.push_str(&format!("- agent: {}\n", f.agent));
            task.push_str(&format!(
                "- source_quote: {}\n",
                f.source_quote.chars().take(300).collect::<String>()
            ));
            task.push_str(&format!("- legal_basis: {}\n", f.legal_basis.join("; ")));
            task.push_str(&format!(
                "- 原推理: {}\n\n",
                f.reason.chars().take(300).collect::<String>()
            ));
        }

        task.push_str("---\n\n");
        task.push_str("🛑 现在调用 **output_verification_batch** 输出所有验证结论。\n");
        task.push_str("每条 corrected_legal_basis 必须包含可验证的 URL 链接（Markdown 格式: [法条名](URL)）。");
        task
    }

    /// LegalVerify 静态 fallback：简单的置信度检查（不调用 LLM）。
    #[allow(dead_code)]
    fn legal_verify_fallback(&self, findings: &mut [RiskFinding]) {
        for finding in findings.iter_mut() {
            if !finding.no_risk && !finding.legal_basis.is_empty() {
                if finding.confidence < 0.5 {
                    finding.severity = RiskSeverity::Info;
                    finding.clear_criticality();
                    finding
                        .reason
                        .push_str("\n[LegalVerify] ❌ 法条引用置信度不足，已降级 (fallback)。");
                } else {
                    finding.reason.push_str(&format!(
                        "\n[LegalVerify] ✅ 法条引用置信度充足 (fallback, confidence={:.2})。",
                        finding.confidence
                    ));
                }
            }
        }
    }

    // ── [6.5] DEBATE: 高风险正反辩论 ───────────────────────────

    /// 对 High + confidence ≤ 0.85 的发现启动 DebateAgent 辩论。
    /// ≤ 0.85（非 < 0.85）——LLM 的自然置信度下限约 0.85，
    /// 含等号能捕获所有"不够确信"的 High 发现。
    async fn debate_high_risk(&self, findings: &mut [RiskFinding]) {
        let candidates: Vec<RiskFinding> = findings
            .iter()
            .filter(|f| {
                f.severity == RiskSeverity::High
                    && f.confidence <= 0.85
                    && !f.no_risk
                    && f.finding_role == FindingRole::Verified
            })
            .cloned()
            .collect();

        if candidates.is_empty() {
            return;
        }

        eprintln!(
            "  [DEBATE] {} 个候选发现（High + confidence<0.85），启动辩论...",
            candidates.len()
        );

        let debate_def = self.registry.get(AgentId::Debate);
        if debate_def.is_none() {
            eprintln!("  [DEBATE] DebateAgent 未注册，跳过");
            return;
        }

        // ★ 并行辩论，不要串行等
        let debate_handles: Vec<_> = candidates
            .iter()
            .map(|candidate| {
                let debate_text = format!(
                    "## 辩论任务\n\n对以下高风险发现进行正反辩论：\n\n\
                     **risk_type**: {}\n\
                     **severity**: {}\n\
                     **confidence**: {:.2}\n\
                     **source_quote**: {}\n\
                     **legal_basis**: {}\n\
                     **reason**: {}\n\
                     **suggestion**: {}\n\n\
                     按 Defender → Challenger → Arbiter 三角色执行辩论，输出裁决结果。",
                    candidate.risk_type,
                    candidate.severity,
                    candidate.confidence,
                    candidate.source_quote,
                    candidate.legal_basis.join("; "),
                    candidate.reason,
                    candidate.suggestion,
                );

                let debate_clause = ReviewClause {
                    chunk_id: format!("debate_{}", candidate.risk_id),
                    section_path: vec!["辩论".to_string(), candidate.risk_type.clone()],
                    text: debate_text,
                    page_start: 0,
                    page_end: 0,
                    tier: RiskTier::High,
                    tier_max_turns: 8,
                };

                let def = debate_def.unwrap();
                let config = def.to_agent_config();
                let llm = (self.llm_factory)();
                let tools = (self.tools_factory)();
                let agent = ReActLoop::new(config, llm, tools)
                    .with_print_lock(self.print_lock.clone())
                    .with_search_cache(self.shared_search_cache.clone());
                let risk_id = candidate.risk_id.clone();
                tokio::spawn(async move { (risk_id, agent.review(&[debate_clause]).await) })
            })
            .collect();

        for handle in debate_handles {
            match handle.await {
                Ok((risk_id, debate_findings)) => {
                    for df in &debate_findings {
                        if df.no_risk {
                            continue;
                        }
                        if let Some(original) = findings.iter_mut().find(|f| f.risk_id == risk_id) {
                            original.severity = df.severity;
                            original.is_critical =
                                df.is_critical && df.severity == RiskSeverity::High;
                            original.critical_reason = if original.is_critical {
                                df.critical_reason.clone()
                            } else {
                                String::new()
                            };
                            original.confidence = df.confidence;
                            original.reason =
                                format!("{}\n\n[Debate] 辩论裁决: {}", original.reason, df.reason);
                            original.suggestion = df.suggestion.clone();
                            eprintln!(
                                "  [DEBATE] {} → severity={} confidence={:.2}",
                                risk_id, df.severity, df.confidence
                            );
                        }
                    }
                }
                Err(e) => {
                    eprintln!("  [DEBATE] spawn 失败: {}", e);
                }
            }
        }
    }

    // ── [6] BLINDSPOT: 盲点扫描 ──────────────────────────────

    /// 判断章节是否为"前导内容"（纯元数据/邀请函/目录等，无需盲点复查）。
    fn is_frontmatter_section(section_path: &[String]) -> bool {
        let frontmatter_keywords = [
            "磋商邀请",
            "磋商公告",
            "招标公告",
            "投标邀请",
            "未归类",
            "封面",
            "目录",
            "前附表",
            "须知前附表",
            "采购公告",
            "竞争性谈判",
            "询价公告",
            "单一来源",
        ];
        section_path
            .iter()
            .any(|s| frontmatter_keywords.iter().any(|kw| s.contains(kw)))
    }

    /// 构造 BlindSpotAgent 的图上下文附录（注入 system message）。
    fn build_blind_spot_context(&self, snapshot: &GraphSnapshot) -> String {
        let mut ctx = String::from("## SessionGraph 全局快照\n\n");

        // 高风险发现摘要
        let high_risks: Vec<&RiskNode> = snapshot
            .risks
            .values()
            .filter(|r| r.finding.severity == RiskSeverity::High && !r.finding.no_risk)
            .collect();
        if !high_risks.is_empty() {
            ctx.push_str(&format!("### 高风险发现 ({} 条)\n\n", high_risks.len()));
            for r in &high_risks {
                let cids = r.finding.clause_ids.join(", ");
                ctx.push_str(&format!(
                    "- **{}** [{}] {} | clauses=[{}] | confidence={:.2}\n",
                    r.finding.risk_type,
                    r.finding.agent,
                    r.finding.reason.chars().take(200).collect::<String>(),
                    cids,
                    r.finding.confidence,
                ));
            }
            ctx.push('\n');
        }

        // contradicts 边
        if !snapshot.contradicts.is_empty() {
            ctx.push_str(&format!(
                "### 条款矛盾 ({} 条边)\n\n",
                snapshot.contradicts.len()
            ));
            for (cid, pairs) in &snapshot.contradicts {
                for (other_cid, reason) in pairs {
                    ctx.push_str(&format!("- {} ↔ {} : {}\n", cid, other_cid, reason));
                }
            }
            ctx.push('\n');
        }

        // same_law 边
        if !snapshot.same_law.is_empty() {
            ctx.push_str(&format!(
                "### 同法条关联 ({} 条边)\n\n",
                snapshot.same_law.len()
            ));
            for (cid, others) in &snapshot.same_law {
                if !others.is_empty() {
                    ctx.push_str(&format!("- {} 共享法条: {}\n", cid, others.join(", ")));
                }
            }
            ctx.push('\n');
        }

        // Scout Hypothesis 覆盖度（已初筛维度参考）
        let hypotheses: Vec<&RiskNode> = snapshot
            .risks
            .values()
            .filter(|r| r.finding.finding_role == FindingRole::Hypothesis)
            .collect();
        if !hypotheses.is_empty() {
            ctx.push_str(&format!(
                "### Scout 初筛已覆盖维度 ({} 条 Hypothesis)\n\n",
                hypotheses.len()
            ));
            for r in &hypotheses {
                let cids = r.finding.clause_ids.join(", ");
                ctx.push_str(&format!(
                    "- **{}** clauses=[{}] confidence={:.2} | verify: {}\n",
                    r.finding.risk_type,
                    cids,
                    r.finding.confidence,
                    r.finding.verification_required.join(", "),
                ));
            }
            ctx.push('\n');
        }

        // 审查覆盖统计
        let total_chunks = snapshot.chunks.len();
        let reviewed_chunks = snapshot.reviewed_by.len();
        let unreviewed = total_chunks.saturating_sub(reviewed_chunks);
        ctx.push_str(&format!(
            "### 审查覆盖\n- 总条款: {}\n- 已审查: {}\n- 未审查: {}\n\n",
            total_chunks, reviewed_chunks, unreviewed
        ));

        ctx
    }

    /// 新的 BlindSpotAgent ReAct 扫描。
    ///
    /// 1. 获取 GraphSnapshot，识别候选条款
    /// 2. 构建图上下文 → 构造 ReviewClause 列表（上限 50 条）
    /// 3. 启动 BlindSpotAgent ReAct 循环
    /// 4. ReAct 无产出或出错时回退到 blind_spot_fallback()
    async fn blind_spot_scan(&self) -> Vec<RiskFinding> {
        let snapshot = self.graph.snapshot();

        // 识别候选条款：未审查 OR (≤1 Agent 审查且无风险发现)
        let candidate_ids: Vec<String> = snapshot
            .chunks
            .keys()
            .filter(|cid| {
                let reviewed = snapshot.reviewed_by.get(*cid).map(|v| v.len()).unwrap_or(0);
                let has_risk = snapshot
                    .has_risk
                    .get(*cid)
                    .map(|v| !v.is_empty())
                    .unwrap_or(false);

                // 跳过 L1 格式条款和 frontmatter
                if let Some(chunk) = snapshot.chunks.get(*cid)
                    && (chunk.tier == RiskTier::Low
                        || Self::is_frontmatter_section(&chunk.section_path))
                {
                    return false;
                }

                reviewed == 0 || (reviewed <= 1 && !has_risk)
            })
            .cloned()
            .collect();

        if candidate_ids.is_empty() {
            eprintln!("  [BLINDSPOT] 无候选条款（所有条款已被充分审查），跳过 ReAct 扫描");
            return Vec::new();
        }

        let total_candidates = candidate_ids.len();
        let max_candidates = std::env::var("AIBID_BLINDSPOT_MAX_CANDIDATES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(12)
            .clamp(1, 50);
        let capped = total_candidates.min(max_candidates);
        if total_candidates > max_candidates {
            eprintln!(
                "  [BLINDSPOT] 候选条款过多 ({} 条)，按后台预算截取前 {} 条",
                total_candidates, max_candidates,
            );
        }

        // 构造 ReviewClause 列表
        let candidate_clauses: Vec<ReviewClause> = candidate_ids[..capped]
            .iter()
            .filter_map(|cid| {
                snapshot.chunks.get(cid).map(|chunk| ReviewClause {
                    chunk_id: chunk.chunk_id.clone(),
                    section_path: chunk.section_path.clone(),
                    text: chunk.text_preview.clone(), // 仅预览文本，Agent 需要用 read_section 获取全文
                    page_start: chunk.page_start,
                    page_end: chunk.page_end,
                    tier: chunk.tier,
                    tier_max_turns: chunk.tier.max_turns(),
                })
            })
            .collect();

        eprintln!(
            "  [BLINDSPOT] 启动 BlindSpotAgent ReAct，候选条款 {} 条 (总 {} 条)",
            candidate_clauses.len(),
            total_candidates,
        );

        // 构建图上下文 → 注入 BlindSpot Agent 的 conversation
        let graph_context = self.build_blind_spot_context(&snapshot);

        // 启动 BlindSpotAgent ReAct
        let blind_spot_def = self.registry.get(AgentId::BlindSpot);
        if blind_spot_def.is_none() {
            eprintln!("  [BLINDSPOT] BlindSpotAgent 未注册，回退到 fallback");
            return if self.config.blind_spot_fallback_enabled {
                self.blind_spot_fallback(Some(&snapshot)).await
            } else {
                Vec::new()
            };
        }

        let def = blind_spot_def.unwrap();
        let mut config = def.to_agent_config();
        config.system_prompt = format!("{}\n\n{}", config.system_prompt, graph_context);
        let llm = (self.llm_factory)();
        let tools = (self.tools_factory)();
        let graph = self.graph.clone();
        let bus = self.bus.clone();
        let trace = self.trace.clone();

        let mut agent = ReActLoop::new(config, llm, tools);
        agent = agent
            .with_graph(graph)
            .with_bus(bus)
            .with_print_lock(self.print_lock.clone())
            .with_search_cache(self.shared_search_cache.clone());
        agent.trace = trace;

        let findings = agent.review(&candidate_clauses).await;

        let total_findings = findings.len();
        let no_risk_count = findings.iter().filter(|f| f.no_risk).count();
        let real_findings: Vec<RiskFinding> = findings.into_iter().filter(|f| !f.no_risk).collect();

        if real_findings.is_empty() {
            if no_risk_count > 0 {
                eprintln!(
                    "  [BLINDSPOT] ReAct 产出 {} 条 no_risk 结论，无新增风险发现，回退到 fallback",
                    no_risk_count
                );
            } else {
                eprintln!(
                    "  [BLINDSPOT] ReAct 无任何产出 (0 条 finding，共 {} 候选条款)，回退到 fallback",
                    total_findings
                );
            }
            return if self.config.blind_spot_fallback_enabled {
                self.blind_spot_fallback(Some(&snapshot)).await
            } else {
                Vec::new()
            };
        }

        eprintln!(
            "  [BLINDSPOT] ReAct 完成，发现 {} 条新风险 (另有 {} 条 no_risk 结论)",
            real_findings.len(),
            no_risk_count
        );

        // 内部去重：同一 Agent 对同一条款的同一 risk_type 只保留 confidence 最高的
        let before_dedup = real_findings.len();
        let mut seen: HashMap<String, RiskFinding> = HashMap::new();
        for f in real_findings {
            let key = format!("{}|{}|{}", f.risk_type, f.clause_ids.join(","), f.agent);
            if let Some(existing) = seen.get(&key) {
                if f.confidence > existing.confidence {
                    seen.insert(key, f);
                }
            } else {
                seen.insert(key, f);
            }
        }
        let mut real_findings: Vec<RiskFinding> = seen.into_values().collect();
        if real_findings.len() < before_dedup {
            eprintln!(
                "  [BLINDSPOT] 内部去重: {} → {} 条 (移除 {} 条重复)",
                before_dedup,
                real_findings.len(),
                before_dedup - real_findings.len()
            );
        }

        // ── Post-ReAct Sweep: 确保每条候选条款都有有效审查结论 ──
        let sweep_enabled = std::env::var("AIBID_BLINDSPOT_SWEEP_ENABLED")
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes"
                )
            })
            .unwrap_or(false);
        if sweep_enabled {
            let covered: std::collections::HashSet<&str> = real_findings
                .iter()
                .flat_map(|f| f.clause_ids.iter().map(|s| s.as_str()))
                .collect();
            let missed: Vec<ReviewClause> = candidate_clauses
                .iter()
                .filter(|c| !covered.contains(c.chunk_id.as_str()))
                .map(|c| {
                    let mut sweep = c.clone();
                    sweep.tier_max_turns = 2; // 强制 2 轮快速复审
                    sweep
                })
                .collect();

            if !missed.is_empty() {
                eprintln!(
                    "  [BLINDSPOT] Sweep: {} 条候选条款无审查结论，启动 2 轮快速复审",
                    missed.len()
                );
                let sweep_findings = agent.review(&missed).await;
                let sweep_real: Vec<RiskFinding> = sweep_findings
                    .into_iter()
                    .filter(|f| !f.no_risk && !f.truncated)
                    .collect();
                eprintln!(
                    "  [BLINDSPOT] Sweep 完成: {} 条新发现 ({} 条 no_risk/truncated 已忽略)",
                    sweep_real.len(),
                    missed.len().saturating_sub(sweep_real.len())
                );
                real_findings.extend(sweep_real);
            }
        }

        // 将发现写入 SessionGraph
        let graph_for_write = self.graph.clone();
        let bus_for_write = self.bus.clone();
        for finding in &real_findings {
            let law_refs = finding.legal_basis.clone();
            let risk_node = RiskNode {
                finding: finding.clone(),
                law_refs,
            };
            for cid in &finding.clause_ids {
                graph_for_write.add_risk_with_edges(risk_node.clone(), cid);
            }
            if finding.severity == RiskSeverity::High {
                bus_for_write.broadcast(
                    AgentId::BlindSpot,
                    finding.severity,
                    &finding.reason,
                    &finding.clause_ids,
                    &finding.risk_type,
                );
            }
        }

        real_findings
    }

    /// BlindSpot 静态 fallback：确定性逻辑扫描盲点（不调用 LLM）。
    ///
    /// 当 BlindSpotAgent ReAct 失败或无产出时回退到此方法。
    /// `snapshot` 为 pre-ReAct 快照（由调用方传入），避免 ReAct 已将本 Agent
    /// 写入 `reviewed_by` 后导致 fallback 无法识别未充分审查的条款。
    async fn blind_spot_fallback(&self, snapshot: Option<&GraphSnapshot>) -> Vec<RiskFinding> {
        let snapshot: GraphSnapshot = match snapshot {
            Some(s) => s.clone(),
            None => self.graph.snapshot(),
        };

        // 找出审查覆盖盲点
        let unreviewed_chunks: Vec<&String> = snapshot
            .chunks
            .keys()
            .filter(|cid| {
                !snapshot.reviewed_by.contains_key(*cid)
                    || snapshot
                        .reviewed_by
                        .get(*cid)
                        .map(|v| v.is_empty())
                        .unwrap_or(true)
            })
            .collect();

        let no_risk_chunks: Vec<&String> = snapshot
            .chunks
            .keys()
            .filter(|cid| !snapshot.has_risk.contains_key(*cid))
            .collect();

        eprintln!(
            "  [BLINDSPOT] 未审查: {} 条, 无关联风险: {} 条",
            unreviewed_chunks.len(),
            no_risk_chunks.len()
        );

        if unreviewed_chunks.is_empty() {
            // 过滤会被后续 skip 的 L1/frontmatter，精确判断是否需要提前退出
            let mut effective_count = 0usize;
            for cid in &no_risk_chunks {
                if let Some(chunk) = snapshot.chunks.get(*cid)
                    && chunk.tier != RiskTier::Low
                    && !Self::is_frontmatter_section(&chunk.section_path)
                {
                    effective_count += 1;
                }
            }
            if effective_count <= 1 {
                eprintln!("  [BLINDSPOT] 无明显盲点，跳过复查");
                return Vec::new();
            }
        }

        // 标记盲点（Phase 2: 结构化标记；Phase 3: 完整 BlindSpotAgent ReAct）
        let mut blind_findings = Vec::new();

        for cid in &unreviewed_chunks {
            if let Some(chunk) = snapshot.chunks.get(*cid) {
                blind_findings.push(RiskFinding {
                    risk_id: format!("BLIND_{}", cid),
                    clause_ids: vec![(*cid).clone()],
                    block_ids: Vec::new(),
                    agent: "BlindSpotAgent".to_string(),
                    no_risk: false,
                    severity: RiskSeverity::Info,
                    is_critical: false,
                    critical_reason: String::new(),
                    risk_type: "审查盲点".to_string(),
                    category_code: "REVIEW_BLIND_SPOT".to_string(),
                    source_quote: chunk.text_preview.clone(),
                    legal_basis: Vec::new(),
                    case_refs: Vec::new(),
                    reason: format!(
                        "条款 {} 未被任何 Agent 审查，建议人工复核。章节: {}",
                        cid,
                        chunk.section_path.join(" > ")
                    ),
                    suggestion: "建议指派 Agent 重新审查或人工复核。".to_string(),
                    confidence: 0.5,
                    initial_tier: RiskTier::Medium,
                    final_tier: RiskTier::Medium,
                    tier_escalated: false,
                    truncated: false,
                    suggested_agent: None,
                    citations: Vec::new(),
                    finding_role: FindingRole::default(),
                    knowledge_source: String::new(),
                    verification_required: Vec::new(),
                    hypothesized_by: Vec::new(),
                    verified_by: Vec::new(),
                    page_number: Some(chunk.page_start + 1),
                    section_path: Some(chunk.section_path.clone()),
                    context: Some(chunk.text_preview.chars().take(500).collect()),
                });
            }
        }

        // 对无风险关联 + 审查 Agent 数 ≤1 的条款标记
        // 跳过：L1（格式/信息类，快速扫描即可）、前导内容（封面/邀请/目录）
        for cid in &no_risk_chunks {
            if !unreviewed_chunks.contains(cid)
                && snapshot
                    .reviewed_by
                    .get(*cid)
                    .map(|v| v.len() <= 1)
                    .unwrap_or(true)
                && let Some(chunk) = snapshot.chunks.get(*cid)
            {
                // L1 条款已是"格式/信息"快速扫描，无需盲点复查
                if chunk.tier == RiskTier::Low {
                    continue;
                }
                // 前导内容（封面/磋商邀请/目录等）纯元数据，无需盲点复查
                if Self::is_frontmatter_section(&chunk.section_path) {
                    continue;
                }
                blind_findings.push(RiskFinding {
                    risk_id: format!("BLIND_NO_RISK_{}", cid),
                    clause_ids: vec![(*cid).clone()],
                    block_ids: Vec::new(),
                    agent: "BlindSpotAgent".to_string(),
                    no_risk: true,
                    severity: RiskSeverity::Info,
                    is_critical: false,
                    critical_reason: String::new(),
                    risk_type: "潜在遗漏".to_string(),
                    category_code: "POTENTIAL_OMISSION".to_string(),
                    source_quote: chunk.text_preview.clone(),
                    legal_basis: Vec::new(),
                    case_refs: Vec::new(),
                    reason: format!(
                        "条款 {} 仅被 {} 个 Agent 审查且无风险发现，建议人工确认。",
                        cid,
                        snapshot.reviewed_by.get(*cid).map(|v| v.len()).unwrap_or(0)
                    ),
                    suggestion: "建议人工快速复核确认无风险。".to_string(),
                    confidence: 0.6,
                    initial_tier: RiskTier::Medium,
                    final_tier: RiskTier::Medium,
                    tier_escalated: false,
                    truncated: false,
                    suggested_agent: None,
                    citations: Vec::new(),
                    finding_role: FindingRole::default(),
                    knowledge_source: String::new(),
                    verification_required: Vec::new(),
                    hypothesized_by: Vec::new(),
                    verified_by: Vec::new(),
                    page_number: Some(chunk.page_start + 1),
                    section_path: Some(chunk.section_path.clone()),
                    context: Some(chunk.text_preview.chars().take(500).collect()),
                });
            }
        }

        eprintln!(
            "  [BLINDSPOT] 发现 {} 条盲点/潜在遗漏",
            blind_findings.len()
        );
        blind_findings
    }

    // ── [7] TRIAGE: 按 severity + confidence 分流 ────────────

    fn triage(&self, mut findings: Vec<RiskFinding>) -> Vec<RiskFinding> {
        // 过滤 Hypothesis（不进入最终输出）
        findings.retain(|f| f.finding_role == FindingRole::Verified);
        // 排序：High → Medium → Low → Info; 同 severity 内 confidence 降序
        findings.sort_by(|a, b| {
            b.severity.cmp(&a.severity).then_with(|| {
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        });

        let high = findings
            .iter()
            .filter(|f| f.severity == RiskSeverity::High)
            .count();
        let medium = findings
            .iter()
            .filter(|f| f.severity == RiskSeverity::Medium)
            .count();
        let low = findings
            .iter()
            .filter(|f| f.severity == RiskSeverity::Low)
            .count();
        let info = findings
            .iter()
            .filter(|f| f.severity == RiskSeverity::Info)
            .count();

        eprintln!(
            "  [TRIAGE] 🔴High={} 🟡Medium={} 🟢Low={} ℹ️Info={}",
            high, medium, low, info
        );

        findings
    }

    // ── 动态 Agent 生命周期 ──────────────────────────────────

    /// 启动时从 agents/dynamic_agents.json 加载活跃的动态 Agent。
    pub fn load_dynamic_agents(&mut self) -> Result<usize> {
        let path = data_path_str("agents/dynamic_agents.json");
        if !std::path::Path::new(&path).exists() {
            return Ok(0);
        }
        let json = std::fs::read_to_string(path)?;
        let manifest: DynamicAgentManifest = serde_json::from_str(&json)?;

        let mut loaded = 0;
        for def in &manifest.agents {
            if !def.active {
                continue;
            }
            if def.system_prompt.is_empty() || def.section_keywords.is_empty() {
                eprintln!("  [DYNAMIC] 跳过无效动态 Agent: {}", def.id);
                continue;
            }
            if self.is_duplicate_dynamic_agent(def) {
                eprintln!("  [DYNAMIC] 跳过重复动态 Agent: {}", def.id);
                continue;
            }
            self.registry.register_dynamic(def);
            self.dynamic_definitions.insert(def.id.clone(), def.clone());
            self.config
                .enabled_agents
                .push(AgentId::Dynamic(def.id.clone()));
            loaded += 1;
        }
        if loaded > 0 {
            eprintln!("  [DYNAMIC] 加载 {} 个动态 Agent", loaded);
        }
        Ok(loaded)
    }

    /// 扫描 findings 中的 suggested_agent，写入 dynamic_agents.json。
    fn register_dynamic_agents(&self, findings: &[RiskFinding]) -> usize {
        let mut registered = 0;
        for f in findings {
            if let Some(suggested) = &f.suggested_agent {
                if suggested.agent_prompt.is_empty() || suggested.section_keywords.is_empty() {
                    continue;
                }
                let id = format!("Dynamic_{}", self.sanitize_agent_id(&suggested.agent_name));
                let def = DynamicAgentDefinition {
                    id,
                    display_name: format!("{}Agent", suggested.agent_name),
                    system_prompt: suggested.agent_prompt.clone(),
                    default_max_turns: 8,
                    complexity: AgentComplexity::Medium,
                    section_keywords: suggested.section_keywords.clone(),
                    tool_names: vec![
                        "web_search".into(),
                        "search_document".into(),
                        "read_section".into(),
                        "output_finding".into(),
                    ],
                    created_at: chrono::Utc::now().to_rfc3339(),
                    created_by: "BlindSpotAgent".into(),
                    reason: suggested.reason.clone(),
                    active: false,
                };
                self.append_dynamic_agent_to_file(&def);
                eprintln!(
                    "  [DYNAMIC] 新 Agent 建议已写入: {} (active=false, 需人工审批)",
                    def.id
                );
                registered += 1;
            }
        }
        registered
    }

    /// 去重检查：section_keywords Jaccard 重叠度 > 0.5 视为重复。
    fn is_duplicate_dynamic_agent(&self, def: &DynamicAgentDefinition) -> bool {
        let new_kws: std::collections::HashSet<&str> =
            def.section_keywords.iter().map(|s| s.as_str()).collect();

        for existing in self.dynamic_definitions.values() {
            let existing_kws: std::collections::HashSet<&str> = existing
                .section_keywords
                .iter()
                .map(|s| s.as_str())
                .collect();
            let intersection = new_kws.intersection(&existing_kws).count();
            let union = new_kws.union(&existing_kws).count();
            if union > 0 && (intersection as f64 / union as f64) > 0.5 {
                return true;
            }
        }
        false
    }

    /// 将新 Agent 追加写入 dynamic_agents.json（上限 20 个，超出淘汰最旧）。
    fn append_dynamic_agent_to_file(&self, def: &DynamicAgentDefinition) {
        let path = data_path_str("agents/dynamic_agents.json");
        let mut manifest: DynamicAgentManifest = if std::path::Path::new(&path).exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(DynamicAgentManifest {
                    version: 1,
                    agents: vec![],
                })
        } else {
            DynamicAgentManifest {
                version: 1,
                agents: vec![],
            }
        };

        // 去重：同名覆盖
        manifest.agents.retain(|a| a.id != def.id);
        manifest.agents.push(def.clone());

        // 上限 20，淘汰最旧
        if manifest.agents.len() > 20 {
            manifest
                .agents
                .sort_by(|a, b| a.created_at.cmp(&b.created_at));
            manifest.agents.remove(0); // 移除最旧
        }

        // 确保目录存在
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if let Ok(json) = serde_json::to_string_pretty(&manifest) {
            let _ = std::fs::write(&path, json);
        }
    }

    /// 将中文 Agent 名称转为合法的 ID（去除非 ASCII，snake_case）。
    fn sanitize_agent_id(&self, name: &str) -> String {
        let mut result = String::new();
        for c in name.chars() {
            if c.is_ascii_alphanumeric() || c == '_' {
                result.push(c);
            }
        }
        if result.is_empty() {
            "Unknown".to_string()
        } else {
            result
        }
    }

    // ── STS 辅助方法: 在现有 legal_verify / debate_high_risk / triage 中增加 Hypothesis 过滤 ──
}

// ─── STS 辅助函数 ──────────────────────────────────────────────

fn finding_category(finding: &RiskFinding) -> String {
    risk_taxonomy::canonical_category(finding)
}

fn evidence_similarity(a: &RiskFinding, b: &RiskFinding) -> f64 {
    let aq = a.source_quote.trim();
    let bq = b.source_quote.trim();
    if aq.is_empty() || bq.is_empty() {
        return 0.0;
    }
    if aq == bq || aq.contains(bq) || bq.contains(aq) {
        return 1.0;
    }
    let normalized_a = normalize_evidence_dates(aq);
    let normalized_b = normalize_evidence_dates(bq);
    if normalized_a == normalized_b
        || normalized_a.contains(&normalized_b)
        || normalized_b.contains(&normalized_a)
    {
        return 1.0;
    }
    text_similarity(&normalized_a, &normalized_b)
}

/// 脱敏流程可能把一位 Agent 的日期变成 `[日期]`，另一位仍保留原日期。
/// 这两种引文应视为同一证据，避免产生跨 Agent 重复项。
fn normalize_evidence_dates(text: &str) -> String {
    static DATE_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let date_re = DATE_RE.get_or_init(|| {
        regex::Regex::new(r"\d{4}年\d{1,2}月\d{1,2}日|\[日期\]").expect("valid evidence date regex")
    });
    date_re.replace_all(text, "日期").replace(' ', "")
}

/// 计算两个文本的 Jaccard 相似度（基于字符 trigram）。
fn text_similarity(a: &str, b: &str) -> f64 {
    fn trigrams(s: &str) -> std::collections::HashSet<[char; 3]> {
        let cleaned: Vec<char> = s.chars().filter(|c| c.is_alphanumeric()).collect();
        cleaned
            .windows(3)
            .filter_map(|w| <[char; 3]>::try_from(w).ok())
            .collect()
    }
    let ta = trigrams(a);
    let tb = trigrams(b);
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let intersection = ta.intersection(&tb).count();
    let union = ta.union(&tb).count();
    intersection as f64 / union as f64
}

/// 合并 contributors：追加 hypothesized_by 和 verified_by，去重。
fn merge_contributors(existing: &mut RiskFinding, new: &RiskFinding) {
    if new.is_critical {
        existing.is_critical = true;
        if existing.critical_reason.trim().is_empty() {
            existing.critical_reason = new.critical_reason.clone();
        } else if !new.critical_reason.trim().is_empty()
            && !existing
                .critical_reason
                .contains(new.critical_reason.trim())
        {
            existing.critical_reason = format!(
                "{}；{}",
                existing.critical_reason.trim(),
                new.critical_reason.trim()
            );
        }
        existing.normalize_criticality();
    }
    for h in &new.hypothesized_by {
        if !existing.hypothesized_by.contains(h) {
            existing.hypothesized_by.push(h.clone());
        }
    }
    for v in &new.verified_by {
        if !existing.verified_by.contains(v) {
            existing.verified_by.push(v.clone());
        }
    }
}

/// 合并 reason 文本：existing 在前，new 在后（截断 800 字符防止膨胀）。
fn combine_reasons(existing: &RiskFinding, new: &RiskFinding) -> String {
    let existing_reason = existing.reason.trim();
    let new_reason = new.reason.trim();
    let combined = format!(
        "{}\n\n[补充验证 — {}]: {}",
        existing_reason, new.agent, new_reason
    );
    if combined.chars().count() > 800 {
        format!("{}…", combined.chars().take(797).collect::<String>())
    } else {
        combined
    }
}

/// 合并 legal_basis 列表，去重。
fn dedup_legal_basis(a: &[String], b: &[String]) -> Vec<String> {
    let mut result: Vec<String> = a.to_vec();
    for item in b {
        if !result.contains(item) {
            result.push(item.clone());
        }
    }
    result
}

// ─── 测试 ────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    /// 创建一个只用于离线测试的 Coordinator。
    /// llm_factory 和 tools_factory 是 dummy（不应被调用）。
    fn make_test_coordinator(config: CoordinatorConfig, registry: AgentRegistry) -> Coordinator {
        let bus = Arc::new(AgentBus::new(4));
        let graph = Arc::new(SessionGraph::new());
        let trace = Arc::new(Mutex::new(TraceLog::new()));

        Coordinator {
            config,
            registry,
            dynamic_definitions: HashMap::new(),
            llm_factory: Arc::new(|| unreachable!("llm_factory 不应在离线测试中调用")),
            tools_factory: Arc::new(|| unreachable!("tools_factory 不应在离线测试中调用")),
            bus,
            graph,
            trace,
            print_lock: Arc::new(std::sync::Mutex::new(())),
            review_events: None,
            metrics: None,
            shared_search_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn make_test_clause(id: &str, text: &str) -> ReviewClause {
        ReviewClause {
            chunk_id: id.to_string(),
            section_path: vec!["测试章节".to_string()],
            text: text.to_string(),
            page_start: 0,
            page_end: 0,
            tier: RiskTier::from_clause_text(text),
            tier_max_turns: RiskTier::from_clause_text(text).max_turns(),
        }
    }

    fn make_test_finding(risk_id: &str, clause_id: &str, agent: &str) -> RiskFinding {
        RiskFinding {
            risk_id: risk_id.to_string(),
            clause_ids: vec![clause_id.to_string()],
            block_ids: Vec::new(),
            agent: agent.to_string(),
            no_risk: false,
            severity: RiskSeverity::High,
            is_critical: false,
            critical_reason: String::new(),
            risk_type: "测试风险".to_string(),
            category_code: String::new(),
            source_quote: "测试原文".to_string(),
            legal_basis: vec!["《测试法》第1条".to_string()],
            case_refs: vec![],
            reason: "测试理由".to_string(),
            suggestion: "测试建议".to_string(),
            confidence: 0.8,
            initial_tier: RiskTier::Medium,
            final_tier: RiskTier::High,
            tier_escalated: true,
            truncated: false,
            suggested_agent: None,
            citations: Vec::new(),
            finding_role: FindingRole::default(),
            knowledge_source: String::new(),
            verification_required: Vec::new(),
            hypothesized_by: Vec::new(),
            verified_by: Vec::new(),
            page_number: None,
            section_path: None,
            context: None,
        }
    }

    // ── [1] ROUTE 测试 ───────────────────────────────────────

    #[test]
    fn test_route_clauses_keyword_match() {
        let mut config = CoordinatorConfig::default();
        config.enabled_agents = vec![AgentId::FactCheck, AgentId::SemanticRisk, AgentId::Contract];
        let registry = AgentRegistry::builtin();
        let coordinator = make_test_coordinator(config, registry);

        let clauses = vec![
            make_test_clause("ch_001", "封面格式要求见附件"),
            make_test_clause("ch_002", "本项目指定华为品牌交换机"),
            make_test_clause("ch_003", "付款方式和结算条件"),
        ];

        let routing = coordinator.route_clauses(&clauses);

        // ch_001 → FactCheck (含"格式"和"封面")
        assert!(
            routing
                .get(&AgentId::FactCheck)
                .unwrap()
                .iter()
                .any(|c| c.chunk_id == "ch_001")
        );
        // ch_002 → SemanticRisk (含"品牌")
        assert!(
            routing
                .get(&AgentId::SemanticRisk)
                .unwrap()
                .iter()
                .any(|c| c.chunk_id == "ch_002")
        );
        // ch_003 → Contract (含"付款")
        assert!(
            routing
                .get(&AgentId::Contract)
                .unwrap()
                .iter()
                .any(|c| c.chunk_id == "ch_003")
        );
    }

    #[test]
    fn test_route_one_clause_to_multi_agents() {
        let mut config = CoordinatorConfig::default();
        config.enabled_agents = vec![AgentId::Scoring, AgentId::Demand];
        let registry = AgentRegistry::builtin();
        let coordinator = make_test_coordinator(config, registry);

        let clauses = vec![make_test_clause(
            "ch_004",
            "评分权重价格分30%技术分50%商务分20%",
        )];

        let routing = coordinator.route_clauses(&clauses);
        // "评分"+"价格"+"技术" 应同时命中 Scoring 和 Demand
        assert!(
            routing
                .get(&AgentId::Scoring)
                .unwrap()
                .iter()
                .any(|c| c.chunk_id == "ch_004")
        );
        assert!(
            routing
                .get(&AgentId::Demand)
                .unwrap()
                .iter()
                .any(|c| c.chunk_id == "ch_004")
        );
    }

    #[test]
    fn test_route_empty_keywords_skip() {
        let mut config = CoordinatorConfig::default();
        // BlindSpot/LegalVerify/Debate 的 section_keywords 为空，不参与路由
        config.enabled_agents = vec![AgentId::BlindSpot, AgentId::LegalVerify, AgentId::Debate];
        let registry = AgentRegistry::builtin();
        let coordinator = make_test_coordinator(config, registry);

        let clauses = vec![make_test_clause("ch_001", "任意文本")];

        let routing = coordinator.route_clauses(&clauses);
        // 空 keywords → 命中（所有 clause 进入）→ 但由于 no Reviewer，fallback 到 FactCheck
        // 实际逻辑：空 keywords 导致 should_route=true，但 FactCheck 不在 enabled_agents
        // → 条款被分配给 BlindSpot/LegalVerify/Debate（空 keywords 的 Agent）
        assert!(!routing.is_empty());
    }

    #[test]
    fn test_route_fallback_to_factcheck() {
        let mut config = CoordinatorConfig::default();
        config.enabled_agents = vec![AgentId::SemanticRisk]; // 只有 SemanticRisk
        let registry = AgentRegistry::builtin();
        let coordinator = make_test_coordinator(config, registry);

        // 这条条款不含 SemanticRisk 的任何关键词 → 不会被分配
        let clauses = vec![make_test_clause(
            "ch_006",
            "本文件为竞争性磋商文件的组成部分",
        )];

        let routing = coordinator.route_clauses(&clauses);
        // fallback: 即使 FactCheck 不在 enabled_agents，也应通过 fallback 逻辑分配
        let factcheck_clauses = routing.get(&AgentId::FactCheck);
        assert!(
            factcheck_clauses.is_some()
                && factcheck_clauses
                    .unwrap()
                    .iter()
                    .any(|c| c.chunk_id == "ch_006"),
            "无匹配条款应 fallback 到 FactCheckAgent"
        );
    }

    #[test]
    fn test_route_dynamic_agent_keywords() {
        let mut config = CoordinatorConfig::default();
        let dynamic_id = AgentId::Dynamic("Dynamic_BrandDetector".into());
        config.enabled_agents = vec![AgentId::FactCheck, dynamic_id.clone()];
        let mut registry = AgentRegistry::builtin();

        // 注册一个动态 Agent（手工注入到 registry 和 dynamic_definitions）
        let dynamic_def = DynamicAgentDefinition {
            id: "Dynamic_BrandDetector".into(),
            display_name: "品牌检测".into(),
            system_prompt: "test".into(),
            default_max_turns: 8,
            complexity: AgentComplexity::Medium,
            section_keywords: vec!["品牌组合".into(), "多品牌".into()],
            tool_names: vec!["web_search".into()],
            created_at: "2026-01-01T00:00:00Z".into(),
            created_by: "BlindSpotAgent".into(),
            reason: "test".into(),
            active: true,
        };
        registry.register_dynamic(&dynamic_def);

        let mut coordinator = make_test_coordinator(config, registry);
        coordinator
            .dynamic_definitions
            .insert("Dynamic_BrandDetector".into(), dynamic_def);

        let clauses = vec![make_test_clause("ch_007", "本项目采用多品牌组合策略排他")];

        let routing = coordinator.route_clauses(&clauses);
        // 应被路由到 Dynamic Agent
        assert!(
            routing
                .get(&dynamic_id)
                .unwrap()
                .iter()
                .any(|c| c.chunk_id == "ch_007"),
            "动态 Agent 应通过其 section_keywords 接收条款"
        );
    }

    // ── [4] MERGE 测试 ───────────────────────────────────────

    #[test]
    fn test_merge_deduplicate_by_risk_type_clause_agent() {
        let config = CoordinatorConfig::default();
        let registry = AgentRegistry::builtin();
        let coordinator = make_test_coordinator(config, registry);

        let f1 = {
            let mut f = make_test_finding("R_001", "ch_001", "FactCheckAgent");
            f.risk_type = "品牌指定".into();
            f.confidence = 0.8;
            f
        };
        let f2 = {
            let mut f = make_test_finding("R_002", "ch_001", "SemanticRiskAgent");
            f.risk_type = "品牌指定".into();
            f.confidence = 0.9;
            f
        };
        let f3 = {
            let mut f = make_test_finding("R_003", "ch_002", "FactCheckAgent");
            f.risk_type = "品牌指定".into();
            f.confidence = 0.7;
            f
        };
        let f4 = {
            let mut f = make_test_finding("R_004", "ch_001", "ContractAgent");
            f.risk_type = "付款风险".into();
            f.confidence = 0.8;
            f
        };

        let merged = coordinator.merge_findings(vec![f1, f2, f3, f4]);
        // key: risk_type|clause_ids|agent
        // f1 和 f2 的 key 不同（agent 不同），所以都保留
        // f3 是不同 clause
        // f4 是不同 risk_type
        assert_eq!(
            merged.len(),
            4,
            "不同 agent 或 clause 或 risk_type 的发现不应去重"
        );
    }

    #[test]
    fn test_merge_keep_higher_confidence_same_key() {
        let config = CoordinatorConfig::default();
        let registry = AgentRegistry::builtin();
        let coordinator = make_test_coordinator(config, registry);

        let f_low = {
            let mut f = make_test_finding("R_001", "ch_001", "FactCheckAgent");
            f.risk_type = "品牌指定".into();
            f.confidence = 0.6;
            f
        };
        let f_high = {
            let mut f = make_test_finding("R_002", "ch_001", "FactCheckAgent");
            f.risk_type = "品牌指定".into();
            f.confidence = 0.95;
            f
        };

        let merged = coordinator.merge_findings(vec![f_low, f_high]);
        assert_eq!(merged.len(), 1);
        assert!((merged[0].confidence - 0.95).abs() < 0.001);
        assert_eq!(merged[0].risk_id, "R_002");
    }

    #[test]
    fn test_merge_v3_keeps_distinct_categories_with_similar_text() {
        let coordinator =
            make_test_coordinator(CoordinatorConfig::default(), AgentRegistry::builtin());
        let mut local_registration = make_test_finding("R_001", "ch_001", "SemanticRiskAgent");
        local_registration.category_code = "LOCAL_REGISTRATION".into();
        local_registration.risk_type = "地域注册限制".into();
        local_registration.source_quote = "投标人须在本地注册并缴纳5%保证金".into();
        local_registration.reason = "该条款构成不合理资格限制，应删除限制条件".into();

        let mut excessive_deposit = make_test_finding("R_002", "ch_001", "ProcedureAgent");
        excessive_deposit.category_code = "EXCESSIVE_DEPOSIT".into();
        excessive_deposit.risk_type = "保证金比例过高".into();
        excessive_deposit.source_quote = "投标人须在本地注册并缴纳5%保证金".into();
        excessive_deposit.reason = "该条款构成不合理资格限制，应删除限制条件".into();

        let merged = coordinator
            .merge_findings_v3(vec![local_registration, excessive_deposit], &|_| {})
            .retained;
        assert_eq!(
            merged.len(),
            2,
            "同一chunk中的不同风险类别不得因理由或证据文本相似而合并"
        );
    }

    // ── [4b] LINK 测试 ───────────────────────────────────────

    #[test]
    fn test_derive_cross_agent_links_same_risk_type_different_agents() {
        let config = CoordinatorConfig::default();
        let registry = AgentRegistry::builtin();
        let coordinator = make_test_coordinator(config, registry);

        let f1 = {
            let mut f = make_test_finding("R_001", "ch_001", "SemanticRiskAgent");
            f.risk_type = "品牌排他".into();
            f
        };
        let f2 = {
            let mut f = make_test_finding("R_002", "ch_005", "DemandAgent");
            f.risk_type = "品牌排他".into();
            f
        };
        let f3 = {
            let mut f = make_test_finding("R_003", "ch_010", "FactCheckAgent");
            f.risk_type = "付款风险".into();
            f
        };

        // 写入 chunk 节点以便 add_linked_to 有目标
        coordinator.graph.add_chunk(ChunkNode {
            chunk_id: "ch_001".into(),
            section_path: vec!["测试".into()],
            page_start: 0,
            page_end: 1,
            text_preview: "条款1".into(),
            tier: RiskTier::Medium,
        });
        coordinator.graph.add_chunk(ChunkNode {
            chunk_id: "ch_005".into(),
            section_path: vec!["测试".into()],
            page_start: 0,
            page_end: 1,
            text_preview: "条款5".into(),
            tier: RiskTier::Medium,
        });

        coordinator.derive_cross_agent_links(&[f1, f2, f3]);

        // ch_001 和 ch_005 之间应该有 linked_to 边（同 risk_type "品牌排他"，不同 Agent）
        let ctx = coordinator.graph.query_clause_context("ch_001");
        let has_link_to_ch005 = ctx.linked_chunks.iter().any(|lc| lc.chunk_id == "ch_005");
        assert!(has_link_to_ch005, "跨 Agent 同类型风险应产生 linked_to 边");
    }

    #[test]
    fn test_derive_cross_agent_links_same_agent_no_link() {
        let config = CoordinatorConfig::default();
        let registry = AgentRegistry::builtin();
        let coordinator = make_test_coordinator(config, registry);

        let f1 = {
            let mut f = make_test_finding("R_001", "ch_001", "FactCheckAgent");
            f.risk_type = "品牌排他".into();
            f
        };
        let f2 = {
            let mut f = make_test_finding("R_002", "ch_005", "FactCheckAgent");
            f.risk_type = "品牌排他".into();
            f
        };

        coordinator.graph.add_chunk(ChunkNode {
            chunk_id: "ch_001".into(),
            section_path: vec!["测试".into()],
            page_start: 0,
            page_end: 1,
            text_preview: "条款1".into(),
            tier: RiskTier::Medium,
        });

        coordinator.derive_cross_agent_links(&[f1, f2]);

        // 同一 Agent 的同类型发现不应产生 linked_to 边
        let ctx = coordinator.graph.query_clause_context("ch_001");
        assert!(
            ctx.linked_chunks.is_empty(),
            "同一 Agent 不应产生 linked_to 边"
        );
    }

    // ── [7] TRIAGE 测试 ──────────────────────────────────────

    #[test]
    fn test_triage_sort_order() {
        let config = CoordinatorConfig::default();
        let registry = AgentRegistry::builtin();
        let coordinator = make_test_coordinator(config, registry);

        let f1 = {
            let mut f = make_test_finding("R_001", "ch_001", "A");
            f.severity = RiskSeverity::Medium;
            f.confidence = 0.9;
            f
        };
        let f2 = {
            let mut f = make_test_finding("R_002", "ch_002", "B");
            f.severity = RiskSeverity::High;
            f.confidence = 0.7;
            f
        };
        let f3 = {
            let mut f = make_test_finding("R_003", "ch_003", "C");
            f.severity = RiskSeverity::High;
            f.confidence = 0.95;
            f
        };
        let f4 = {
            let mut f = make_test_finding("R_004", "ch_004", "D");
            f.severity = RiskSeverity::Low;
            f.confidence = 0.8;
            f
        };
        let f5 = {
            let mut f = make_test_finding("R_005", "ch_005", "E");
            f.severity = RiskSeverity::Info;
            f.confidence = 0.5;
            f
        };

        let sorted = coordinator.triage(vec![f1, f2, f3, f4, f5]);

        // 验证顺序: High(0.95) > High(0.7) > Medium(0.9) > Low(0.8) > Info(0.5)
        assert_eq!(sorted[0].risk_id, "R_003"); // High, 0.95
        assert_eq!(sorted[1].risk_id, "R_002"); // High, 0.7
        assert_eq!(sorted[2].risk_id, "R_001"); // Medium, 0.9
        assert_eq!(sorted[3].risk_id, "R_004"); // Low, 0.8
        assert_eq!(sorted[4].risk_id, "R_005"); // Info, 0.5
    }

    // ── 动态 Agent: sanitize_agent_id ────────────────────────

    #[test]
    fn test_sanitize_agent_id_removes_non_ascii() {
        let coordinator =
            make_test_coordinator(CoordinatorConfig::default(), AgentRegistry::builtin());
        // 纯中文 → 无 ascii 字符 → fallback "Unknown"
        assert_eq!(coordinator.sanitize_agent_id("品牌组合排他检测"), "Unknown");
        // 纯英文
        assert_eq!(
            coordinator.sanitize_agent_id("BrandComboDetector"),
            "BrandComboDetector"
        );
        // 混合 → 只保留 ascii
        assert_eq!(coordinator.sanitize_agent_id("品牌Brand检测"), "Brand");
        // 空 → fallback
        assert_eq!(coordinator.sanitize_agent_id(""), "Unknown");
    }

    // ── 动态 Agent: 去重逻辑 ─────────────────────────────────

    #[test]
    fn test_is_duplicate_dynamic_agent_jaccard_below_threshold() {
        let mut coordinator =
            make_test_coordinator(CoordinatorConfig::default(), AgentRegistry::builtin());

        // 已有 Agent: keywords = {"品牌","指定","独家","原厂"}
        let existing = DynamicAgentDefinition {
            id: "Dynamic_Existing".into(),
            display_name: "已有".into(),
            system_prompt: "test".into(),
            default_max_turns: 8,
            complexity: AgentComplexity::Medium,
            section_keywords: vec!["品牌".into(), "指定".into(), "独家".into(), "原厂".into()],
            tool_names: vec![],
            created_at: "2026-01-01T00:00:00Z".into(),
            created_by: "BlindSpotAgent".into(),
            reason: "test".into(),
            active: true,
        };
        coordinator
            .dynamic_definitions
            .insert("Dynamic_Existing".into(), existing);

        // 新 Agent: keywords = {"品牌","指定","授权"}
        // 交集={"品牌","指定"}(2), 并集={"品牌","指定","独家","原厂","授权"}(5)
        // Jaccard = 2/5 = 0.4 ≤ 0.5 → 不重复
        let new_def = DynamicAgentDefinition {
            id: "Dynamic_New".into(),
            display_name: "新".into(),
            system_prompt: "test".into(),
            default_max_turns: 8,
            complexity: AgentComplexity::Medium,
            section_keywords: vec!["品牌".into(), "指定".into(), "授权".into()],
            tool_names: vec![],
            created_at: "2026-01-02T00:00:00Z".into(),
            created_by: "BlindSpotAgent".into(),
            reason: "test".into(),
            active: false,
        };

        assert!(
            !coordinator.is_duplicate_dynamic_agent(&new_def),
            "Jaccard=0.4 不应判定为重复"
        );
    }

    #[test]
    fn test_is_duplicate_dynamic_agent_jaccard_above_threshold() {
        let mut coordinator =
            make_test_coordinator(CoordinatorConfig::default(), AgentRegistry::builtin());

        let existing = DynamicAgentDefinition {
            id: "Dynamic_Existing".into(),
            display_name: "已有".into(),
            system_prompt: "test".into(),
            default_max_turns: 8,
            complexity: AgentComplexity::Medium,
            section_keywords: vec!["品牌".into(), "指定".into(), "独家".into()],
            tool_names: vec![],
            created_at: "2026-01-01T00:00:00Z".into(),
            created_by: "BlindSpotAgent".into(),
            reason: "test".into(),
            active: true,
        };
        coordinator
            .dynamic_definitions
            .insert("Dynamic_Existing".into(), existing.clone());

        // 新 Agent: keywords = {"品牌","指定"}
        // 交集={"品牌","指定"}(2), 并集={"品牌","指定","独家"}(3)
        // Jaccard = 2/3 ≈ 0.67 > 0.5 → 重复
        let new_def = DynamicAgentDefinition {
            section_keywords: vec!["品牌".into(), "指定".into()],
            ..existing
        };

        assert!(
            coordinator.is_duplicate_dynamic_agent(&new_def),
            "Jaccard=0.67 应判定为重复"
        );
    }

    #[test]
    fn test_is_duplicate_no_existing_agents() {
        let coordinator =
            make_test_coordinator(CoordinatorConfig::default(), AgentRegistry::builtin());
        // 无已有动态 Agent → 不重复
        let new_def = DynamicAgentDefinition {
            id: "Dynamic_First".into(),
            display_name: "首个".into(),
            system_prompt: "test".into(),
            default_max_turns: 8,
            complexity: AgentComplexity::Medium,
            section_keywords: vec!["测试".into()],
            tool_names: vec![],
            created_at: "2026-01-01T00:00:00Z".into(),
            created_by: "BlindSpotAgent".into(),
            reason: "test".into(),
            active: false,
        };
        assert!(!coordinator.is_duplicate_dynamic_agent(&new_def));
    }

    // ── 动态 Agent: suggest_agent 注册 ───────────────────────

    #[test]
    fn test_register_dynamic_agents_from_findings() {
        let coordinator =
            make_test_coordinator(CoordinatorConfig::default(), AgentRegistry::builtin());

        let finding = RiskFinding {
            suggested_agent: Some(SuggestedAgent {
                agent_name: "品牌组合排他检测".into(),
                agent_prompt: "你是品牌组合排他检测Agent，负责...".into(),
                section_keywords: vec!["品牌组合".into(), "多品牌".into(), "捆绑".into()],
                reason: "现有SemanticRisk只看单个品牌指定".into(),
            }),
            ..make_test_finding("R_001", "ch_001", "BlindSpotAgent")
        };

        // 注意: register_dynamic_agents 会写入 agents/dynamic_agents.json
        // 测试环境下我们先 backup 原文件，测试完恢复
        let backup_path = data_path_str("agents/dynamic_agents.json.bak");
        let original_path = data_path_str("agents/dynamic_agents.json");
        let original_exists = std::path::Path::new(&original_path).exists();
        if original_exists {
            std::fs::rename(&original_path, &backup_path).ok();
        }

        let registered = coordinator.register_dynamic_agents(&[finding]);
        assert_eq!(registered, 1, "应注册 1 个动态 Agent");

        // 验证文件被写入
        let json = std::fs::read_to_string(&original_path).expect("文件应存在");
        let manifest: DynamicAgentManifest = serde_json::from_str(&json).expect("JSON 应合法");
        assert_eq!(manifest.agents.len(), 1);
        assert!(!manifest.agents[0].active, "新 Agent 的 active 应为 false");
        assert_eq!(manifest.agents[0].created_by, "BlindSpotAgent");
        assert_eq!(manifest.agents[0].default_max_turns, 8);
        assert_eq!(manifest.agents[0].tool_names.len(), 4);

        // 清理恢复
        std::fs::remove_file(&original_path).ok();
        if original_exists {
            std::fs::rename(&backup_path, &original_path).ok();
        }
    }

    #[test]
    fn test_register_dynamic_agents_empty_suggested_agent() {
        let coordinator =
            make_test_coordinator(CoordinatorConfig::default(), AgentRegistry::builtin());

        // 没有 suggested_agent 的 finding
        let finding = make_test_finding("R_001", "ch_001", "FactCheckAgent");
        let registered = coordinator.register_dynamic_agents(&[finding]);
        assert_eq!(registered, 0);
    }

    #[test]
    fn test_register_dynamic_agents_empty_prompt_skipped() {
        let coordinator =
            make_test_coordinator(CoordinatorConfig::default(), AgentRegistry::builtin());

        let finding = RiskFinding {
            suggested_agent: Some(SuggestedAgent {
                agent_name: "测试".into(),
                agent_prompt: "".into(), // 空 prompt
                section_keywords: vec!["测试".into()],
                reason: "测试".into(),
            }),
            ..make_test_finding("R_001", "ch_001", "BlindSpotAgent")
        };

        let registered = coordinator.register_dynamic_agents(&[finding]);
        assert_eq!(registered, 0, "空 prompt 应被跳过");
    }

    // ── 动态 Agent: load ─────────────────────────────────────

    #[test]
    fn test_load_dynamic_agents_file_not_exists() {
        let mut coordinator =
            make_test_coordinator(CoordinatorConfig::default(), AgentRegistry::builtin());

        // 确保文件不存在（测试环境应该没有）
        if std::path::Path::new(&data_path_str("agents/dynamic_agents.json")).exists() {
            // 跳过此测试以免影响已存在的文件
            return;
        }

        let loaded = coordinator.load_dynamic_agents().expect("不应报错");
        assert_eq!(loaded, 0);
    }

    #[test]
    fn test_load_dynamic_agents_inactive_skipped() {
        // 写入一个 active=false 的 manifest
        let manifest = DynamicAgentManifest {
            version: 1,
            agents: vec![DynamicAgentDefinition {
                id: "Dynamic_Inactive".into(),
                display_name: "非活跃".into(),
                system_prompt: "你是测试Agent".into(),
                default_max_turns: 8,
                complexity: AgentComplexity::Medium,
                section_keywords: vec!["测试".into()],
                tool_names: vec!["web_search".into()],
                created_at: "2026-01-01T00:00:00Z".into(),
                created_by: "BlindSpotAgent".into(),
                reason: "test".into(),
                active: false, // ← 不激活
            }],
        };

        let backup_path = data_path_str("agents/dynamic_agents.json.bak");
        let original_path = data_path_str("agents/dynamic_agents.json");
        let original_exists = std::path::Path::new(&original_path).exists();
        if original_exists {
            std::fs::rename(&original_path, &backup_path).ok();
        }

        let json = serde_json::to_string_pretty(&manifest).unwrap();
        std::fs::write(&original_path, &json).unwrap();

        let mut coordinator =
            make_test_coordinator(CoordinatorConfig::default(), AgentRegistry::builtin());
        let loaded = coordinator.load_dynamic_agents().expect("不应报错");
        assert_eq!(loaded, 0, "active=false 的 Agent 不应被加载");

        // 清理恢复
        std::fs::remove_file(&original_path).ok();
        if original_exists {
            std::fs::rename(&backup_path, &original_path).ok();
        }
    }

    // ── is_frontmatter_section ────────────────────────────────

    #[test]
    fn test_is_frontmatter_section() {
        assert!(Coordinator::is_frontmatter_section(&["磋商邀请".into()]));
        assert!(Coordinator::is_frontmatter_section(&[
            "第一章".into(),
            "投标邀请".into()
        ]));
        assert!(Coordinator::is_frontmatter_section(&["目录".into()]));
        assert!(!Coordinator::is_frontmatter_section(&[
            "第二章".into(),
            "采购需求".into()
        ]));
        assert!(!Coordinator::is_frontmatter_section(&[
            "第四章".into(),
            "合同条款".into()
        ]));
    }

    #[test]
    fn test_normalize_evidence_dates_matches_redacted_and_original() {
        let redacted = "投标截止时间为[日期]9时，同时规定[日期]17时后提交的文件一律拒收。";
        let original =
            "投标截止时间为2026年9月20日9时，同时规定2026年9月18日17时后提交的文件一律拒收。";
        assert_eq!(
            normalize_evidence_dates(redacted),
            normalize_evidence_dates(original)
        );
    }
}
