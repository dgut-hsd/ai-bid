//! 指标 Schema —— 稳定的"测量契约"。
//!
//! ## 设计原则
//!
//! 1. **语义阶段，不是函数名**：`SemanticStage` enum 描述业务阶段，不随代码重构变化。
//! 2. **四层独立**：每层指标可独立读取，AI 分析时 `jq '.llm_efficiency.totals.cost_cny'` 即可取单值。
//! 3. **摘要即详情**：`totals` 字段让 GUI 列表页无需深入嵌套。
//! 4. **版本化**：`schema_version` 字段向前兼容，GUI 据此做降级渲染。
//! 5. **by_agent 维度统一**：Layer 2 和 Layer 3 都有 per-agent 数据，支持交叉对比。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 当前 Schema 版本。每次不兼容变更时递增。
pub const SCHEMA_VERSION: &str = "1.0";

// ─── 顶层结构 ─────────────────────────────────────────────────

/// 一次 Review 运行的完整指标。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMetrics {
    pub schema_version: String,
    pub meta: RunMeta,
    pub latency: LatencyReport,
    pub llm_efficiency: LlmEfficiencyReport,
    pub review_quality: ReviewQualityReport,
    pub resources: ResourceReport,
}

// ─── Meta ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMeta {
    pub run_id: String,
    /// 用户自定义标题（为空时 GUI 用 run_id 作为默认名）
    #[serde(default)]
    pub title: Option<String>,
    /// 用户备注（自由文本，用于记录实验背景、改动说明等）
    #[serde(default)]
    pub notes: Option<String>,
    /// 所属实验组（output/runs 下的子目录名，None 表示根目录）
    #[serde(default)]
    pub experiment_group: Option<String>,
    pub timestamp: String,
    pub git_commit: String,
    pub git_branch: String,
    pub tags: Vec<String>,
    pub description: String,
    pub document: DocumentInfo,
    pub config: RunConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentInfo {
    pub name: String,
    pub pages: usize,
    pub file_size_kb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunConfig {
    pub coordinator_enabled: bool,
    pub agent_count: usize,
    pub embed_engine: String,
    pub llm_model: String,
    pub search_backend: String,
    pub max_parallel_clauses: usize,
    /// P2：是否启用 transcript 独白压缩（A/B 审计用）。
    #[serde(default)]
    pub transcript_compression: bool,
}

// ─── Layer 1: 端到端延迟 ──────────────────────────────────────

/// 业务语义阶段 —— 不受代码重构影响。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticStage {
    /// PDF → RawDocument（含 Rust/Python 双引擎）
    DocumentIngestion,
    /// RawDocument → Sections（含表格检测、表格注入、孤儿块兜底）
    DocumentStructure,
    /// Sections → Chunks（智能切分）
    Chunking,
    /// Chunks → Embeddings（本地 BGE-M3 或远程 API）
    Embedding,
    /// Multi-Agent 审核（Coordinator 7 阶段管线 或 单 Agent）
    AgentReview,
    /// 合并 + 去重 + 排序 + 写盘
    PostProcessing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyReport {
    /// 端到端 wall-clock 耗时（秒）
    pub total_wall_clock_secs: f64,
    /// 各语义阶段耗时
    pub stages: Vec<StageRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageRecord {
    pub stage: SemanticStage,
    pub duration_secs: f64,
    pub pct_of_total: f64,
    /// 阶段特有详情
    pub detail: StageDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StageDetail {
    DocumentIngestion {
        pages: usize,
        engine: String,
    },
    DocumentStructure {
        section_count: usize,
    },
    Chunking {
        chunk_count: usize,
        total_chars: usize,
    },
    Embedding {
        chunk_count: usize,
        dimension: usize,
    },
    AgentReview {
        clause_count: usize,
        /// Coordinator 子阶段详情
        coordinator_phases: Option<Vec<CoordinatorPhaseRecord>>,
    },
    PostProcessing {
        output_finding_count: usize,
    },
    /// 未分类阶段
    Generic {
        note: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorPhaseRecord {
    pub phase: String,
    pub duration_secs: f64,
}

// ─── Layer 2: LLM 调用效率 ───────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmEfficiencyReport {
    pub totals: LlmEfficiencyTotals,
    pub by_agent: HashMap<String, AgentLlmStats>,
    pub tool_usage: ToolUsageSummary,
    /// 未产出 finding 的 LLM 调用占比
    pub wasted_call_ratio: f64,
    /// 每次 LLM 调用的详细日志（用于 GUI 审查推理步骤和工具调用质量）
    pub call_log: Vec<LlmCallRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmEfficiencyTotals {
    pub llm_calls: usize,
    pub tokens_input: u64,
    pub tokens_output: u64,
    pub cost_cny: f64,
    pub avg_api_duration_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLlmStats {
    pub turns: usize,
    pub llm_calls: usize,
    pub tokens_input: u64,
    pub tokens_output: u64,
    pub tools: HashMap<String, usize>,
    pub findings_produced: usize,
    /// 未产出有效 finding 的调用数
    pub wasted_calls: usize,
    pub avg_duration_ms: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolUsageSummary {
    pub search_document: usize,
    pub read_section: usize,
    pub search_knowledge: usize,
    pub output_finding: usize,
    pub answer_user: usize,
    pub other: usize,
}

/// 单次 LLM 调用的详细记录（存储在 collector 内部，最终汇总入 Report）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmCallRecord {
    pub agent_name: String,
    pub turn: usize,
    pub tokens_input: u32,
    pub tokens_output: u32,
    pub duration_ms: u64,
    pub tools_called: Vec<String>,
    /// 每个工具调用的关键参数摘要（如 read_section→"ch_003", web_search→"地域限制"）
    pub tool_args: Vec<String>,
    /// LLM 本轮推理/思路文本的前 200 字符
    pub thought_preview: Option<String>,
    pub produced_finding: bool,
    pub finding_parsed_ok: bool,
}

// ─── Layer 3: 审核质量 ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewQualityReport {
    pub findings: FindingSummary,
    /// 每条 RiskFinding 的完整内容（用于 GUI 逐条审查推理质量）
    pub findings_detail: Vec<serde_json::Value>,
    pub by_agent: HashMap<String, AgentQualityStats>,
    pub coordinator: CoordinatorQualityStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingSummary {
    pub total_raw: usize,
    pub after_dedup: usize,
    pub dedup_rate: f64,
    pub by_severity: SeverityBreakdown,
    pub avg_confidence: f64,
    pub median_confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeverityBreakdown {
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub info: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentQualityStats {
    pub raw: usize,
    pub after_dedup: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub info: usize,
    pub avg_confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorQualityStats {
    pub debate_triggered: usize,
    pub debate_changed_verdict: usize,
    pub blindspot_extra_findings: usize,
    pub cross_agent_links: usize,
    pub legal_verify_count: usize,
}

// ─── Layer 4: 资源消耗 ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceReport {
    pub tokens: TokenCostBreakdown,
    pub memory: MemoryUsage,
    pub embedding: EmbeddingStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenCostBreakdown {
    pub total_input: u64,
    pub total_output: u64,
    /// DashScope qwen-plus 定价（CNY / 1M tokens）
    pub pricing_input_per_m: f64,
    pub pricing_output_per_m: f64,
    pub cost_input_cny: f64,
    pub cost_output_cny: f64,
    pub cost_total_cny: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryUsage {
    /// 峰值内存（MB），None 表示未采集
    pub peak_mb: Option<f64>,
    pub onnx_model_mb: Option<f64>,
    pub doc_cache_mb: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingStats {
    pub engine: String,
    pub chunks_embedded: usize,
    pub duration_secs: f64,
    pub chunks_per_sec: f64,
    pub dimension: usize,
}
