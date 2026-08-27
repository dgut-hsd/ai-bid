//! ReAct 循环引擎 — Agent 审查的核心运行时。
//!
//! 设计文档 §7.2-7.3 定义的 while 循环模式：
//! ```text
//! for turn in range(MAX_TURNS):      // ← Rust 代码在循环
//!     response = llm.chat(conversation, tools=[...])
//!     if response.has_tool_call("output_finding"):
//!         return risk_finding          // Agent 认为证据够了，输出
//!     // 否则执行工具调用，结果追加到对话历史
//! ```
//!
//! ## 条款级风险分级 (L1/L2/L3)
//!
//! 每条条款携带 Coordinator 预判的 tier，控制 max_turns：
//! - L1: 5 turns（纯信息/格式条款）
//! - L2: 8 turns（标准审查）
//! - L3: 14 turns（深度审查）
//!
//! 审查过程中支持动态升降级（turn 2 检测）。

use crate::agents::bus::{AgentBus, BusMessage};
use crate::agents::review_event::{ReviewEvent, ReviewEventBus};
use crate::agents::risk_taxonomy;
use crate::agents::session_graph::SessionGraph;
use crate::agents::trace::{TraceEventType, TraceLog};
use crate::agents::types::*;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tokio::task::JoinSet;

// ─── LLM 客户端抽象 ───────────────────────────────────────────

/// LLM 返回的工具调用。
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// LLM API 返回的 Token 使用量。
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
}

/// LLM 的一次响应。
#[derive(Debug, Clone)]
pub struct LlmResponse {
    /// 文本回复（可能为 None，当 LLM 只返回 tool_calls 时）
    pub content: Option<String>,
    /// LLM 在调用工具前的推理/思考文本（ReAct Thought）。
    ///
    /// 来源优先级:
    /// 1. API 响应中的 `reasoning_content` 字段（DeepSeek-R1、qwq 等推理模型）
    /// 2. 当 `content` 与 `tool_calls` 同时存在时，`content` 识别为 thought
    /// 3. 仅 content（无工具调用）→ content 直接作为回答，thought 为 None
    pub thought: Option<String>,
    /// 工具调用列表
    pub tool_calls: Vec<ToolCall>,
    /// Token 使用量（从 API 响应的 usage 字段提取）
    pub usage: Option<TokenUsage>,
}

impl LlmResponse {
    /// 检查是否包含 output_finding 工具调用（触发批量审查循环退出）。
    pub fn has_output_finding(&self) -> bool {
        self.tool_calls.iter().any(|tc| tc.name == "output_finding")
    }

    /// 获取第一个 output_finding 工具调用的 arguments（RiskFinding JSON）。
    pub fn get_finding(&self) -> Option<&serde_json::Value> {
        self.tool_calls
            .iter()
            .find(|tc| tc.name == "output_finding")
            .map(|tc| &tc.arguments)
    }

    /// 检查是否包含 answer_user 工具调用（触发 ChatAgent 循环退出）。
    pub fn has_answer_user(&self) -> bool {
        self.tool_calls.iter().any(|tc| tc.name == "answer_user")
    }

    /// 获取第一个 answer_user 工具调用的 arguments（构建 ChatResponse 用）。
    pub fn get_answer(&self) -> Option<&serde_json::Value> {
        self.tool_calls
            .iter()
            .find(|tc| tc.name == "answer_user")
            .map(|tc| &tc.arguments)
    }

    /// 检查是否包含 output_verification_batch 工具调用（触发批量验证退出）。
    pub fn has_output_verification_batch(&self) -> bool {
        self.tool_calls
            .iter()
            .any(|tc| tc.name == "output_verification_batch")
    }

    /// 获取第一个 output_verification_batch 工具调用的 arguments。
    pub fn get_verification_batch(&self) -> Option<&serde_json::Value> {
        self.tool_calls
            .iter()
            .find(|tc| tc.name == "output_verification_batch")
            .map(|tc| &tc.arguments)
    }
}

/// ★ 工具选择策略 —— 控制 LLM 是否必须调用工具。
///
/// 用于解决 LLM 在 react_loop 中以文本输出结论、拒绝调用 `output_finding`
/// 工具的问题。引擎可以通过此参数主动收回终止控制权。
#[derive(Debug, Clone)]
pub enum ToolChoice {
    /// 不限制 —— LLM 自由选择文本回复或工具调用（默认行为）
    Auto,
    /// 必须调用某个工具（不限具体哪个）
    Required,
    /// 只能调用指定的工具（强制终止 —— 用于最后一轮）
    Specific { name: String },
}

impl ToolChoice {
    /// 序列化为 DashScope API 的 `tool_choice` 字段值。
    pub fn to_dashscope_value(&self) -> serde_json::Value {
        match self {
            ToolChoice::Auto => serde_json::Value::Null,
            ToolChoice::Required => serde_json::json!("required"),
            ToolChoice::Specific { name } => serde_json::json!({
                "type": "function",
                "function": { "name": name }
            }),
        }
    }

    /// 序列化为 OpenAI 兼容 API 的 `tool_choice` 字段值。
    pub fn to_openai_value(&self) -> serde_json::Value {
        match self {
            ToolChoice::Auto => serde_json::json!("auto"),
            ToolChoice::Required => serde_json::json!("required"),
            ToolChoice::Specific { name } => serde_json::json!({
                "type": "function",
                "function": { "name": name }
            }),
        }
    }
}

/// LLM 客户端抽象 trait。
///
/// 解耦 ReAct 循环与具体 LLM 提供商。
/// MVP 使用 OpenAI 兼容 API，后续可添加 Anthropic 原生实现。
#[async_trait::async_trait]
pub trait LlmClient: Send + Sync {
    /// 发送消息到 LLM，返回响应。
    ///
    /// * `messages` — 对话历史（system/user/assistant/tool 消息）
    /// * `tools` — 可用工具的 JSON Schema 定义列表
    /// * `tool_choice` — ★ 工具选择策略（Auto/Required/Specific）
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
        tool_choice: &ToolChoice,
    ) -> Result<LlmResponse>;
}

// ─── 对话消息类型 ─────────────────────────────────────────────

/// ReAct 循环中使用的对话消息（与提供商无关）。
#[derive(Debug, Clone)]
pub enum ChatMessage {
    System {
        content: String,
    },
    User {
        content: String,
    },
    Assistant {
        content: Option<String>,
        tool_calls: Option<Vec<ToolCall>>,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}

const MAX_FINDINGS_PER_CHUNK: usize = 5;

#[derive(Debug, Default)]
struct ChunkReviewOutput {
    findings: Vec<RiskFinding>,
    has_more: bool,
    coverage: Vec<String>,
}

impl ChunkReviewOutput {
    fn single(finding: RiskFinding) -> Self {
        Self {
            findings: vec![finding],
            has_more: false,
            coverage: Vec::new(),
        }
    }
}

// ─── 共享 Helper ───────────────────────────────────────────────

/// 执行 LLM 返回的工具调用并将结果追加到对话历史。
///
/// ★ 这是 ReActLoop 和 ChatAgent 共享的公共逻辑。
/// 批量审查特有的逻辑（搜索缓存、空结果升级、打印日志）
/// 保留在 ReActLoop::react_loop() 内部，不纳入此 helper。
pub async fn execute_tool_calls(
    response: &LlmResponse,
    tools: &crate::agents::tools::ToolRegistry,
    conversation: &mut Vec<ChatMessage>,
) -> Result<()> {
    let assistant_tool_calls = response.tool_calls.clone();
    conversation.push(ChatMessage::Assistant {
        content: response.content.clone(),
        tool_calls: if assistant_tool_calls.is_empty() {
            None
        } else {
            Some(assistant_tool_calls.clone())
        },
    });

    // 如果没有工具调用，提示继续
    if assistant_tool_calls.is_empty() {
        conversation.push(ChatMessage::User {
            content: "请继续——调用工具搜索证据或输出结论。".to_string(),
        });
        return Ok(());
    }

    for tc in &assistant_tool_calls {
        let result = if let Some(tool) = tools.get(&tc.name) {
            match tool.execute(tc.arguments.clone()).await {
                Ok(val) => val,
                Err(e) => serde_json::json!({ "error": format!("{}", e) }),
            }
        } else {
            serde_json::json!({
                "error": format!("工具 '{}' 未注册", tc.name)
            })
        };
        conversation.push(ChatMessage::Tool {
            tool_call_id: tc.id.clone(),
            content: serde_json::to_string(&result).unwrap_or_default(),
        });
    }
    Ok(())
}

/// 提取工具调用的关键参数摘要（用于指标采集）。
///
/// 不同工具提取不同的关键字段：
/// - read_section → section_id 或 "全文"
/// - search_knowledge / web_search → 搜索问题
/// - search_document → 查询文本
/// - output_finding → risk_type + severity（如果有）
/// - 其他 → arguments 前 80 字符
fn summarize_tool_arg(name: &str, args: &serde_json::Value) -> String {
    match name {
        "read_section" => args
            .get("section_id")
            .or_else(|| args.get("chunk_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "全文".to_string()),
        "search_knowledge" | "web_search" => args
            .get("question")
            .or_else(|| args.get("query"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "?".to_string()),
        "search_document" => args
            .get("query")
            .or_else(|| args.get("question"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "?".to_string()),
        "output_finding" => {
            if let Some(items) = args.get("findings").and_then(|v| v.as_array()) {
                return format!("批量结论:{}条", items.len());
            }
            let risk_type = args
                .get("risk_type")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let no_risk = args
                .get("no_risk")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if no_risk {
                format!("无风险:{}", risk_type)
            } else {
                let severity = args.get("severity").and_then(|v| v.as_str()).unwrap_or("?");
                format!("{}:{}", severity, risk_type)
            }
        }
        _ => args.to_string(),
    }
}

fn severity_name(severity: RiskSeverity) -> &'static str {
    match severity {
        RiskSeverity::High => "high",
        RiskSeverity::Medium => "medium",
        RiskSeverity::Low => "low",
        RiskSeverity::Info => "info",
    }
}

/// 解析新批量信封，同时兼容迁移前的单 Finding 对象。
///
/// 批量模式按元素独立解析：一条格式错误不会连带丢弃其他合法发现。
fn parse_finding_batch(
    args: &serde_json::Value,
) -> std::result::Result<(Vec<RiskFinding>, bool, Vec<String>), String> {
    if let Some(items) = args.get("findings").and_then(|v| v.as_array()) {
        let mut findings = Vec::new();
        let mut errors = Vec::new();
        for (idx, item) in items.iter().take(MAX_FINDINGS_PER_CHUNK).enumerate() {
            match serde_json::from_value::<RiskFinding>(item.clone()) {
                Ok(finding) if !finding.no_risk => findings.push(finding),
                Ok(_) => {
                    // 新协议用空数组表达无风险；忽略模型误放入数组的 no_risk 占位项。
                }
                Err(e) => errors.push(format!("findings[{}]: {}", idx, e)),
            }
        }
        if findings.is_empty() && !items.is_empty() && !errors.is_empty() {
            return Err(errors.join("; "));
        }
        let overflowed = items.len() > MAX_FINDINGS_PER_CHUNK;
        let has_more = args
            .get("has_more")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
            || overflowed
            || !errors.is_empty();
        let coverage = args
            .get("coverage")
            .and_then(|v| v.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        return Ok((findings, has_more, coverage));
    }

    // 兼容旧模型/测试桩：单对象响应继续可用。
    let mut fixed = args.clone();
    if let Some(cids) = fixed.get("clause_ids")
        && let Some(raw) = cids.as_str()
    {
        if let Ok(parsed) = serde_json::from_str::<Vec<String>>(raw) {
            fixed["clause_ids"] = serde_json::json!(parsed);
        } else if let Some(object) = fixed.as_object_mut() {
            object.remove("clause_ids");
        }
    }
    serde_json::from_value::<RiskFinding>(fixed)
        .map(|finding| {
            if finding.no_risk {
                (Vec::new(), false, Vec::new())
            } else {
                (vec![finding], false, Vec::new())
            }
        })
        .map_err(|e| e.to_string())
}

fn numbered_item_count(text: &str) -> usize {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            let mut chars = trimmed.chars();
            let Some(first) = chars.next() else {
                return false;
            };
            first.is_ascii_digit() && matches!(chars.next(), Some('.' | '、' | ')' | '）'))
        })
        .count()
}

fn same_finding_identity(a: &RiskFinding, b: &RiskFinding) -> bool {
    let a_category = if a.category_code.trim().is_empty() {
        a.risk_type.trim()
    } else {
        a.category_code.trim()
    };
    let b_category = if b.category_code.trim().is_empty() {
        b.risk_type.trim()
    } else {
        b.category_code.trim()
    };
    if canonical_category(a_category) != canonical_category(b_category) {
        return false;
    }
    let aq = a.source_quote.trim();
    let bq = b.source_quote.trim();
    !aq.is_empty() && !bq.is_empty() && (aq == bq || aq.contains(bq) || bq.contains(aq))
}

fn canonical_category(value: &str) -> String {
    let upper = value.trim().to_uppercase();
    if let Some((prefix, remainder)) = upper.split_once('_')
        && prefix.len() >= 2
        && prefix.starts_with(|c: char| c.is_ascii_alphabetic())
        && prefix[1..].chars().all(|c| c.is_ascii_digit())
    {
        return remainder.to_string();
    }
    upper
}

// ─── ReActLoop ─────────────────────────────────────────────────

/// ReAct 循环引擎 — Agent 审查的运行时。
///
/// 持有 LLM 客户端、工具注册表、AgentBus 引用、SessionGraph 引用、TraceLog 引用。
/// 每个 ReActLoop 实例对应一个 Agent 类型（FactCheck / Procedure / SemanticRisk / ...）。
pub struct ReActLoop {
    /// Agent 配置
    pub config: AgentConfig,
    /// LLM 客户端
    pub llm: Box<dyn LlmClient>,
    /// 工具注册表
    pub tools: crate::agents::tools::ToolRegistry,
    /// AgentBus 发送端（可选，MVP 阶段可省略）
    pub bus: Option<Arc<AgentBus>>,
    /// ★ Agent 持有的专属 Receiver（通过 bus.subscribe() 获取）
    /// 每轮 try_recv() 循环排空，避免多 Agent 并发下消息丢失
    pub bus_rx: Option<Mutex<broadcast::Receiver<BusMessage>>>,
    /// ★ SessionGraph 引用（Blackboard 拉取侧）
    pub graph: Option<Arc<SessionGraph>>,
    /// 搜索缓存：(query, category) → 搜索结果 JSON
    /// ★ 使用 Arc 支持跨 Agent 共享（Coordinator 注入同一实例）
    pub search_cache: Arc<Mutex<HashMap<(String, String), serde_json::Value>>>,
    /// 审查追溯日志
    pub trace: Arc<Mutex<TraceLog>>,
    /// ★ stderr 打印锁：多个 Agent 并行时，确保每个 Agent 的多行日志块不交叠。
    /// 仅用于 eprintln 序列化，不在 await 期间持有。
    pub print_lock: Option<Arc<std::sync::Mutex<()>>>,
    /// SSE 实时推送通道（可选，仅 HTTP server 模式启用）
    pub review_events: Option<Arc<ReviewEventBus>>,
    /// 指标采集器（可选，启用时记录所有 LLM 调用明细）
    pub metrics: Option<Arc<Mutex<crate::metrics::MetricsCollector>>>,
}

impl ReActLoop {
    /// 创建新的 ReActLoop 实例。
    pub fn new(
        config: AgentConfig,
        llm: Box<dyn LlmClient>,
        tools: crate::agents::tools::ToolRegistry,
    ) -> Self {
        Self {
            config,
            llm,
            tools,
            bus: None,
            bus_rx: None,
            graph: None,
            search_cache: Arc::new(Mutex::new(HashMap::new())),
            trace: Arc::new(Mutex::new(TraceLog::new())),
            print_lock: None,
            review_events: None,
            metrics: None,
        }
    }

    /// 设置 AgentBus（同时持有 Sender + 专属 Receiver）。
    ///
    /// ★ Phase 2 增强：调用 `bus.subscribe()` 获取 Agent 专属 Receiver，
    /// 存入 `bus_rx`。此后每轮 `try_recv()` 循环排空，避免多 Agent 并发丢消息。
    pub fn with_bus(mut self, bus: Arc<AgentBus>) -> Self {
        let rx = bus.subscribe();
        self.bus_rx = Some(Mutex::new(rx));
        self.bus = Some(bus);
        self
    }

    /// ★ 新增: 设置 SessionGraph（Blackboard 拉取侧）。
    pub fn with_graph(mut self, graph: Arc<SessionGraph>) -> Self {
        self.graph = Some(graph);
        self
    }

    /// ★ 设置 stderr 打印锁，确保并行 Agent 的多行日志不交叠。
    pub fn with_print_lock(mut self, lock: Arc<std::sync::Mutex<()>>) -> Self {
        self.print_lock = Some(lock);
        self
    }

    /// 设置 SSE 实时推送通道（仅在 HTTP server 模式下启用）。
    pub fn with_review_events(mut self, events: Arc<ReviewEventBus>) -> Self {
        self.review_events = Some(events);
        self
    }

    /// 设置指标采集器（用于记录 LLM 调用明细）。
    pub fn with_metrics(mut self, collector: Arc<Mutex<crate::metrics::MetricsCollector>>) -> Self {
        self.metrics = Some(collector);
        self
    }

    /// 注入共享搜索缓存（跨 Agent 复用搜索结果）。
    ///
    /// 如果不设置，每个 ReActLoop 实例使用独立的空缓存。
    /// Coordinator 应创建共享缓存并通过此方法注入到所有 Agent。
    pub fn with_search_cache(
        mut self,
        cache: Arc<Mutex<HashMap<(String, String), serde_json::Value>>>,
    ) -> Self {
        self.search_cache = cache;
        self
    }

    // ── 主入口 ──────────────────────────────────────────────

    /// 审查一组条款。每个条款运行独立的 ReAct 循环。
    pub async fn review(&self, clauses: &[ReviewClause]) -> Vec<RiskFinding> {
        let mut findings = Vec::new();
        let total = clauses.len();

        for (idx, clause) in clauses.iter().enumerate() {
            // 优先从 SessionGraph 获取全局唯一 risk_id，避免多 Agent 并发下 ID 碰撞。
            // 无 graph 时（LegalVerify/Debate 等独立 ReActLoop）回退到 per-agent 编号。
            let risk_id = self
                .graph
                .as_ref()
                .map(|g| g.next_risk_id())
                .unwrap_or_else(|| format!("R_{:03}", idx + 1));
            findings.extend(self.review_single(clause, &risk_id).await);

            // 每审完一条条款后，发送 AgentProgress（SSE 实时推送）
            if let Some(ref events) = self.review_events {
                let raw_findings = findings.iter().filter(|f| !f.no_risk).count();
                events.emit(&ReviewEvent::AgentProgress {
                    agent_id: self.config.name.clone(),
                    agent_label: self.config.name.clone(),
                    clauses_done: idx + 1,
                    clauses_total: total,
                    raw_findings,
                    status: "running".to_string(),
                });
            }
        }

        findings
    }

    /// 审查单条条款（公开入口，供并行调度使用）。
    ///
    /// 与 `react_loop` 功能相同，但作为公开 API 暴露，
    /// 使外部并行调度器可以为每条条款创建独立 task。
    pub async fn review_single(&self, clause: &ReviewClause, risk_id: &str) -> Vec<RiskFinding> {
        let mut output = self.react_loop(clause, risk_id).await;
        let numbered_items = numbered_item_count(&clause.text);
        let supports_supplement = !matches!(
            self.config.name.as_str(),
            "LegalVerifyAgent" | "DebateAgent"
        );
        let should_supplement = supports_supplement
            && (output.has_more
                || output.findings.len() >= MAX_FINDINGS_PER_CHUNK
                || (numbered_items >= 2
                    && output.findings.len() < numbered_items.min(MAX_FINDINGS_PER_CHUNK)));

        if should_supplement {
            let already_found = output
                .findings
                .iter()
                .map(|f| {
                    format!(
                        "{}：{}",
                        if f.category_code.is_empty() {
                            &f.risk_type
                        } else {
                            &f.category_code
                        },
                        f.source_quote
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            let mut supplement_clause = clause.clone();
            supplement_clause.text = format!(
                "[自适应补充扫描]\n\
                 第一遍已覆盖风险域：{}\n\
                 第一遍已发现（禁止重复输出）：\n{}\n\n\
                 请重新逐段检查下列原条款，只输出尚未覆盖的独立问题。\
                 如果没有遗漏，返回空findings。\n\n[原条款]\n{}",
                output.coverage.join(", "),
                if already_found.is_empty() {
                    "（无）"
                } else {
                    &already_found
                },
                clause.text
            );
            let supplement_id = self
                .graph
                .as_ref()
                .map(|g| g.next_risk_id())
                .unwrap_or_else(|| format!("{}_S", risk_id));
            let supplement = self.react_loop(&supplement_clause, &supplement_id).await;
            for finding in supplement.findings {
                if !output
                    .findings
                    .iter()
                    .any(|existing| same_finding_identity(existing, &finding))
                {
                    output.findings.push(finding);
                }
            }
        }

        output.findings
    }

    // ── 核心 ReAct 循环 ─────────────────────────────────────

    /// 单条款 ReAct 循环。
    ///
    /// ```text
    /// conversation = [system_prompt, user(clause_text)]
    /// while turn < max_turns:
    ///     poll AgentBus → inject bus messages
    ///     response = llm.chat(conversation, tools)
    ///     if output_finding → parse RiskFinding, exit
    ///     execute tool_calls → append results
    /// max_turns exhausted → force_output
    /// ```
    async fn react_loop(&self, clause: &ReviewClause, risk_id: &str) -> ChunkReviewOutput {
        let agent_name = &self.config.name;
        let initial_tier = clause.tier;
        let max_turns = clause.effective_max_turns(self.config.default_max_turns);
        let mut tier = initial_tier;
        let mut tier_escalated = false;
        let mut consecutive_empty_searches = 0u32;
        let mut web_search_count = 0u32; // 硬性限制 web_search 调用次数
        let mut consecutive_duplicate_searches = 0u32; // 连续重复搜索结果计数
        let mut last_search_urls: Vec<String> = Vec::new(); // 上次搜索的 top-3 URL
        // ★ 新增：read_section 重复检测 + 确认搜索拦截
        let mut read_section_count: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new(); // chunk_id → 读取次数
        let mut found_actionable_law = false; // 是否已找到可直接支撑判断的法规
        let mut post_law_search_count = 0u32; // 找到法规后仍继续搜索的次数
        let mut seen_law_refs: std::collections::HashSet<String> = std::collections::HashSet::new(); // 已见过的法规引用（用于判断搜索是否带来新信息）
        // ★ 优化：法规证据充分后，下一轮强制锁定 output_finding，避免 Auto 模式下 LLM 继续调用工具空转
        let mut force_output_next = false;
        // ★ 强化强制收尾（AIBID_STALL_FORCE_OUTPUT）：连续 N 轮仅请求探索类工具且未产出 finding → 下一轮强制 output_finding
        let mut consecutive_stall: u32 = 0;

        // ── 条款头日志 ──
        let _print_lock = self.print_lock.as_ref().map(|l| l.lock().unwrap());
        let sep = "═".repeat(60);
        eprintln!(
            "\n{sep}\n{rid} | {cid} | tier={tier} | max_turns={max} | pages {ps}-{pe}\n{sep}",
            sep = sep,
            rid = risk_id,
            cid = clause.chunk_id,
            tier = initial_tier,
            max = max_turns,
            ps = clause.page_start + 1,
            pe = clause.page_end + 1
        );
        eprintln!("章节: {}", clause.section_path.join(" > "));
        let text_preview = if clause.text.chars().count() > 500 {
            format!(
                "{}…[截断]",
                clause.text.chars().take(500).collect::<String>()
            )
        } else {
            clause.text.clone()
        };
        eprintln!(
            "条款文本 ({} 字符):\n{}\n",
            clause.text.chars().count(),
            text_preview
        );
        drop(_print_lock);

        // 构建初始对话
        let mut conversation: Vec<ChatMessage> = vec![
            ChatMessage::System {
                content: self.config.system_prompt.clone(),
            },
            ChatMessage::System {
                content: "【多问题输出协议】一个 chunk 可能同时包含多个相互独立的问题。\
                    在调用 output_finding 前必须逐段复核，不得只挑最严重的一条。\
                    使用 findings 数组逐条输出；不同事实、不同风险类别或不同修改建议应拆成不同 finding。\
                    无风险返回 findings=[]；最多5条，仍有遗漏可能时 has_more=true。\
                    每条必须填写稳定 category_code 和只支撑该问题的 source_quote。"
                    .to_string(),
            },
            ChatMessage::User {
                content: self.format_clause_prompt(clause),
            },
        ];
        let rule_candidates =
            risk_taxonomy::review_candidates_for_agent(&clause.text, agent_name.as_str());
        if !rule_candidates.is_empty() {
            let checklist = rule_candidates
                .iter()
                .map(|category| {
                    format!(
                        "- {}（{}）",
                        category,
                        risk_taxonomy::display_name(category).unwrap_or("未命名风险")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            conversation.insert(
                2,
                ChatMessage::System {
                    content: format!(
                        "【规则预检候选｜责任 Agent 必查】\n{checklist}\n\
                         这些是关键词预检候选，不等于最终定罪。你必须逐项核对原文：\
                         成立则在 findings 中分别输出；不成立则不要输出。禁止遗漏候选，\
                         禁止把多个候选合并成一条，source_quote 必须只引用对应问题。"
                    ),
                },
            );
        }

        let mut turn = 0u32;
        while turn < max_turns as u32 {
            turn += 1;

            // SSE: turn_start
            if let Some(ref events) = self.review_events {
                events.emit(&ReviewEvent::Trace {
                    event_type: "turn_start".to_string(),
                    agent_name: agent_name.clone(),
                    turn,
                    clause_id: Some(clause.chunk_id.clone()),
                    summary: format!("{} 第 {} 轮审查", agent_name, turn),
                    payload: None,
                });
            }

            // ── Step 0a: Query SessionGraph — 拉取已知上下文 ──
            if let Some(graph) = &self.graph {
                let ctx = graph.query_clause_context(&clause.chunk_id);
                if ctx.has_prior_risks() || !ctx.reviewed_by.is_empty() {
                    let mut graph_msg =
                        String::from("[Session 记忆] 以下条款已被审查或存在已知发现:\n");

                    if !ctx.reviewed_by.is_empty() {
                        graph_msg.push_str(&format!(
                            "已审查 Agent: {}\n",
                            ctx.reviewed_by
                                .iter()
                                .map(|a| a.to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }

                    if ctx.has_prior_risks() {
                        graph_msg.push_str("已知风险:\n");
                        graph_msg.push_str(&ctx.risk_summary());
                    }

                    if !ctx.linked_chunks.is_empty() {
                        graph_msg.push_str("\n关联条款:\n");
                        for lc in &ctx.linked_chunks {
                            graph_msg.push_str(&format!("- {} ({})\n", lc.chunk_id, lc.reason));
                        }
                    }

                    if !ctx.same_law_chunks.is_empty() {
                        graph_msg.push_str("\n引用相同法条的其他条款:\n");
                        for cid in &ctx.same_law_chunks {
                            graph_msg.push_str(&format!("- {}\n", cid));
                        }
                    }

                    if !ctx.contradictions.is_empty() {
                        graph_msg.push_str("\n⚠️ 已知条款矛盾:\n");
                        for lc in &ctx.contradictions {
                            graph_msg
                                .push_str(&format!("- 与 {} 矛盾: {}\n", lc.chunk_id, lc.reason));
                        }
                    }

                    conversation.push(ChatMessage::System { content: graph_msg });
                }

                // 记录当前 Agent 已审查此条款
                if let Some(agent_id) = AgentId::parse(agent_name) {
                    graph.add_reviewed_by(&clause.chunk_id, agent_id);
                }
            }

            // ── Step 0b: AgentBus poll — 使用 Agent 持有的 Receiver 增量拉取 ──
            if let Some(rx) = &self.bus_rx {
                let mut rx_guard = rx.lock().await;
                while let Ok(msg) = rx_guard.try_recv() {
                    // 不接收自己发送的消息
                    let own_id = AgentId::parse(agent_name);
                    if own_id.is_none_or(|oid| msg.from != oid) {
                        // Trace: 记录接收事件
                        {
                            let mut trace = self.trace.lock().await;
                            trace.log(
                                TraceEventType::AgentBusRecv,
                                turn,
                                Some(&clause.chunk_id),
                                &format!("Received bus msg from {}: {}", msg.from, msg.summary),
                                serde_json::json!({
                                    "from": msg.from.to_string(),
                                    "risk_type": msg.risk_type,
                                    "clause_ids": msg.clause_ids,
                                    "topic": format!("{:?}", msg.topic),
                                }),
                            );
                        }
                        conversation.push(ChatMessage::System {
                            content: format!(
                                "[AgentBus] {} 发现 {} 风险: {}\n涉及条款: {}\n如果你审查的条款与此相关，用 search_document 和 read_section 做交叉验证。",
                                msg.from, msg.severity, msg.summary,
                                msg.clause_ids.join(", ")
                            ),
                        });
                    }
                }
            }

            // ── Step 2: LLM 推理 ──

            // ── Turn 剩余轮次预警 + tool_choice 控制 ──
            let remaining = max_turns as u32 - turn;
            let tool_choice = if force_output_next {
                // ★ 优化：法规证据已充分 → 本轮直接强制 output_finding，提前收尾
                force_output_next = false;
                ToolChoice::Specific {
                    name: "output_finding".to_string(),
                }
            } else if remaining <= 1 {
                // 最后一轮：锁定 output_finding，引擎收回终止控制权
                ToolChoice::Specific {
                    name: "output_finding".to_string(),
                }
            } else if remaining == 2 {
                // 倒数第二轮：要求必须调用工具，阻止纯文本输出
                ToolChoice::Required
            } else {
                ToolChoice::Auto
            };

            if remaining == 3 {
                conversation.push(ChatMessage::System {
                    content: format!(
                        "⏳ 剩余 {} 轮审查机会（条款 {}）。请开始汇总已收集的信息，减少探索性搜索。\n如果已有足够证据判定风险（或无风险），即可准备调用 output_finding。",
                        remaining, clause.chunk_id
                    ),
                });
            } else if remaining == 2 {
                conversation.push(ChatMessage::System {
                    content: format!(
                        "⚠️ 剩余 {} 轮审查机会（条款 {}）。请汇总已收集的信息，准备调用 output_finding。\n不要再开启新的搜索方向——基于已有信息做出判定即可。",
                        remaining, clause.chunk_id
                    ),
                });
            } else if remaining <= 1 {
                conversation.push(ChatMessage::System {
                    content: format!(
                        "🛑 这是对条款 {} 的最后一轮审查！本轮**只能**调用 output_finding 输出结论。\n\
                         no_risk=true 也比被截断好——截断会丢失所有已完成的审查工作。\n\
                         立即基于已收集的信息 + 条款原文 + 已知法规常识，调用 output_finding。",
                        clause.chunk_id
                    ),
                });
            }

            let tool_defs = self.tools.definitions_filtered(&self.config.tool_names);
            let api_start = std::time::Instant::now();
            let response = match self.llm.chat(&conversation, &tool_defs, &tool_choice).await {
                Ok(r) => {
                    let api_duration_ms = api_start.elapsed().as_millis() as u64;

                    // ── 指标采集：记录 LLM 调用 ──
                    if let Some(ref metrics) = self.metrics {
                        let tools_called: Vec<String> =
                            r.tool_calls.iter().map(|tc| tc.name.clone()).collect();
                        let tool_args: Vec<String> = r
                            .tool_calls
                            .iter()
                            .map(|tc| summarize_tool_arg(&tc.name, &tc.arguments))
                            .collect();
                        let thought_preview = r
                            .thought
                            .as_ref()
                            .or(r.content.as_ref())
                            .map(|t| t.to_string());
                        let usage = r.usage.as_ref();
                        let mut collector = metrics.lock().await;
                        collector.record_llm_call(crate::metrics::schema::LlmCallRecord {
                            agent_name: agent_name.clone(),
                            turn: turn as usize,
                            tokens_input: usage.map(|u| u.input_tokens).unwrap_or(0),
                            tokens_output: usage.map(|u| u.output_tokens).unwrap_or(0),
                            duration_ms: api_duration_ms,
                            tools_called: tools_called.clone(),
                            tool_args,
                            thought_preview,
                            produced_finding: r.has_output_finding(),
                            finding_parsed_ok: false, // 由后续 output_finding 解析更新
                        });
                    }

                    // ★ 强化强制收尾（AIBID_STALL_FORCE_OUTPUT=N）：
                    // 连续 N 轮 LLM 只请求探索类工具（read_section/search_*）且未产出 finding → 视为空转，
                    // 下一轮强制锁定 output_finding，避免在多 agent 协作 / 复杂条款场景下无限探索。
                    let stall_threshold: u32 = std::env::var("AIBID_STALL_FORCE_OUTPUT")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    if stall_threshold > 0 {
                        let produced = r.has_output_finding();
                        let explore_only = !r.tool_calls.is_empty()
                            && r.tool_calls.iter().all(|tc| {
                                matches!(
                                    tc.name.as_str(),
                                    "read_section"
                                        | "search_document"
                                        | "search_knowledge"
                                        | "web_search"
                                )
                            });
                        if produced {
                            consecutive_stall = 0;
                        } else if explore_only {
                            consecutive_stall += 1;
                            if consecutive_stall >= stall_threshold {
                                force_output_next = true;
                                eprintln!(
                                    "[STALL-FORCE] 条款 {} 连续 {} 轮仅探索未产出 → 强制 output_finding",
                                    clause.chunk_id, consecutive_stall
                                );
                                consecutive_stall = 0;
                            }
                        } else {
                            consecutive_stall = 0;
                        }
                    }

                    // SSE: call_log — 每次 LLM 调用的统计信息
                    if let Some(ref events) = self.review_events {
                        let usage = r.usage.as_ref();
                        let tools_called: Vec<String> =
                            r.tool_calls.iter().map(|tc| tc.name.clone()).collect();
                        events.emit(&ReviewEvent::Trace {
                            event_type: "call_log".to_string(),
                            agent_name: agent_name.clone(),
                            turn,
                            clause_id: Some(clause.chunk_id.clone()),
                            summary: format!(
                                "{}K+{} {}ms {}",
                                usage.map(|u| u.input_tokens).unwrap_or(0) / 1000,
                                usage.map(|u| u.output_tokens).unwrap_or(0),
                                api_duration_ms,
                                tools_called.join(", ")
                            ),
                            payload: Some(serde_json::json!({
                                "tokens_input": usage.map(|u| u.input_tokens).unwrap_or(0),
                                "tokens_output": usage.map(|u| u.output_tokens).unwrap_or(0),
                                "duration_ms": api_duration_ms,
                                "tools_called": tools_called,
                                "produced_finding": r.has_output_finding(),
                            })),
                        });
                    }

                    // SSE: agent_thought — 发送完整推理内容到前端
                    if let Some(ref events) = self.review_events {
                        let full_content = r.content.clone().unwrap_or_default();
                        let thought_summary = full_content.chars().take(200).collect::<String>();
                        if !full_content.is_empty() {
                            events.emit(&ReviewEvent::Trace {
                                event_type: "agent_thought".to_string(),
                                agent_name: agent_name.clone(),
                                turn,
                                clause_id: Some(clause.chunk_id.clone()),
                                summary: if thought_summary.is_empty() {
                                    "(推理内容省略)".to_string()
                                } else {
                                    thought_summary
                                },
                                payload: Some(serde_json::json!({
                                    "content": full_content,
                                })),
                            });
                        }
                    }

                    // ── 详细日志：LLM 响应（加锁避免并行 Agent 交叠）──
                    {
                        let _lock = self.print_lock.as_ref().map(|l| l.lock().unwrap());
                        eprintln!(
                            "\n── [{agent} turn {turn}/{max}] ─────────────────────────────────────────────",
                            agent = agent_name,
                            turn = turn,
                            max = max_turns
                        );
                        // 完整输出 LLM 的推理内容（不截断）
                        if let Some(ref content) = r.content
                            && !content.is_empty()
                        {
                            eprintln!("💭 推理内容 ({} 字符):", content.chars().count());
                            for line in content.lines() {
                                eprintln!("   {}", line);
                            }
                        }
                        // 工具调用及参数
                        if !r.tool_calls.is_empty() {
                            eprintln!("🔧 工具调用 ({} 个):", r.tool_calls.len());
                            for tc in &r.tool_calls {
                                let args_str = serde_json::to_string(&tc.arguments)
                                    .unwrap_or_else(|_| "(序列化失败)".to_string());
                                eprintln!("   → {} (id={})", tc.name, tc.id);
                                eprintln!("      args: {}", args_str);
                            }
                        } else {
                            eprintln!("🔧 工具调用: (无)");
                        }
                    } // 释放打印锁
                    r
                }
                Err(e) => {
                    // LLM 调用失败 → 输出错误 finding
                    return ChunkReviewOutput::single(RiskFinding {
                        risk_id: risk_id.to_string(),
                        clause_ids: vec![clause.chunk_id.clone()],
                        block_ids: Vec::new(),
                        agent: agent_name.clone(),
                        no_risk: true,
                        severity: RiskSeverity::Info,
                        is_critical: false,
                        critical_reason: String::new(),
                        risk_type: "LLM调用失败".to_string(),
                        category_code: "ENGINE_ERROR".to_string(),
                        source_quote: String::new(),
                        legal_basis: Vec::new(),
                        case_refs: Vec::new(),
                        reason: format!("LLM API 调用失败: {}", e),
                        suggestion: "请检查 API 配置后重试。".to_string(),
                        confidence: 0.0,
                        initial_tier,
                        final_tier: tier,
                        tier_escalated,
                        // 基础设施错误必须进入条款失败统计，不能伪装成“无风险”。
                        truncated: true,
                        suggested_agent: None,
                        citations: Vec::new(),
                        finding_role: FindingRole::default(),
                        knowledge_source: String::new(),
                        verification_required: Vec::new(),
                        hypothesized_by: Vec::new(),
                        verified_by: Vec::new(),
                        evidence_verdict: None,
                        verifier_reason: None,
                        page_number: None,
                        section_path: None,
                        context: None,
                    });
                }
            };

            // ── Step 2.5: 二次 AgentBus poll ──
            // Step 0b 的 poll 发生在 LLM 调用之前。如果其他 Agent 的广播
            // 恰好在 LLM 调用期间到达，Step 0b 会错过。此处补 poll 一次，
            // 确保在 output_finding 之前能感知到最新的跨 Agent 消息。
            if let Some(rx) = &self.bus_rx {
                let mut rx_guard = rx.lock().await;
                while let Ok(msg) = rx_guard.try_recv() {
                    let own_id = AgentId::parse(agent_name);
                    if own_id.is_none_or(|oid| msg.from != oid) {
                        {
                            let mut trace = self.trace.lock().await;
                            trace.log(
                                TraceEventType::AgentBusRecv,
                                turn,
                                Some(&clause.chunk_id),
                                &format!("Late bus msg from {}: {}", msg.from, msg.summary),
                                serde_json::json!({
                                    "from": msg.from.to_string(),
                                    "risk_type": msg.risk_type,
                                    "clause_ids": msg.clause_ids,
                                    "topic": format!("{:?}", msg.topic),
                                    "stage": "pre_output",
                                }),
                            );
                        }
                        conversation.push(ChatMessage::System {
                            content: format!(
                                "[AgentBus] {} 发现 {} 风险: {}\n涉及条款: {}\n如果你审查的条款与此相关，用 search_document 和 read_section 做交叉验证。",
                                msg.from, msg.severity, msg.summary,
                                msg.clause_ids.join(", ")
                            ),
                        });
                    }
                }
            }

            // ── Step 3: 检查 output_verification_batch（批量法条验证模式）──
            if response.has_output_verification_batch()
                && let Some(args) = response.get_verification_batch()
            {
                // 将批量验证结果编码为特殊 finding，由 Coordinator 解析
                let raw_pretty = serde_json::to_string_pretty(args);
                {
                    let _lock = self.print_lock.as_ref().map(|l| l.lock().unwrap());
                    eprintln!("📤 output_verification_batch 原始参数:");
                    eprintln!(
                        "{}",
                        raw_pretty.as_deref().unwrap_or(&format!("{:?}", args))
                    );
                }

                return ChunkReviewOutput::single(RiskFinding {
                    risk_id: risk_id.to_string(),
                    clause_ids: vec![clause.chunk_id.clone()],
                    block_ids: Vec::new(),
                    agent: agent_name.clone(),
                    no_risk: false,
                    severity: RiskSeverity::Info,
                    is_critical: false,
                    critical_reason: String::new(),
                    risk_type: "__BATCH_VERIFICATION__".to_string(),
                    category_code: "BATCH_VERIFICATION".to_string(),
                    source_quote: serde_json::to_string(args).unwrap_or_default(),
                    legal_basis: Vec::new(),
                    case_refs: Vec::new(),
                    reason: "批量法条验证结果（由 Coordinator 解析）".to_string(),
                    suggestion: String::new(),
                    confidence: 1.0,
                    initial_tier,
                    final_tier: tier,
                    tier_escalated,
                    truncated: false,
                    suggested_agent: None,
                    citations: Vec::new(),
                    finding_role: FindingRole::default(),
                    knowledge_source: String::new(),
                    verification_required: Vec::new(),
                    hypothesized_by: Vec::new(),
                    verified_by: Vec::new(),
                        evidence_verdict: None,
                        verifier_reason: None,
                    page_number: None,
                    section_path: None,
                    context: None,
                });
            }

            // ── Step 3: 检查 output_finding ──
            if response.has_output_finding()
                && let Some(args) = response.get_finding()
            {
                // ── 始终打印 output_finding 原始参数（加锁）──
                let raw_pretty = serde_json::to_string_pretty(args);
                {
                    let _lock = self.print_lock.as_ref().map(|l| l.lock().unwrap());
                    eprintln!("📤 output_finding 原始参数:");
                    eprintln!(
                        "{}",
                        raw_pretty.as_deref().unwrap_or(&format!("{:?}", args))
                    );
                }

                match parse_finding_batch(args) {
                    Ok((mut findings, has_more, coverage)) => {
                        let citations = self.extract_citations().await;
                        let citation_text = if citations.is_empty() {
                            None
                        } else {
                            Some(
                                citations
                                    .iter()
                                    .enumerate()
                                    .map(|(i, c)| {
                                        if c.site_name.is_empty() {
                                            format!("[{}] {} — {}", i + 1, c.title, c.url)
                                        } else {
                                            format!(
                                                "[{}] {} — {} ({})",
                                                i + 1,
                                                c.title,
                                                c.url,
                                                c.site_name
                                            )
                                        }
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                            )
                        };

                        for (idx, finding) in findings.iter_mut().enumerate() {
                            finding.no_risk = false;
                            finding.normalize_criticality();
                            finding.clause_ids = vec![clause.chunk_id.clone()];
                            finding.agent = agent_name.clone();
                            finding.initial_tier = initial_tier;
                            finding.final_tier = tier;
                            finding.tier_escalated = tier_escalated;
                            finding.truncated = false;
                            finding.risk_id = if idx == 0 {
                                risk_id.to_string()
                            } else {
                                self.graph
                                    .as_ref()
                                    .map(|g| g.next_risk_id())
                                    .unwrap_or_else(|| format!("{}_{:02}", risk_id, idx + 1))
                            };
                            finding.page_number = Some(clause.page_start + 1);
                            finding.section_path = Some(clause.section_path.clone());
                            finding.context = Some(clause.text.chars().take(500).collect());
                            finding.citations = citations.clone();
                            if let Some(ref refs) = citation_text {
                                finding.reason =
                                    format!("{}\n\n📎 搜索来源:\n{}", finding.reason, refs);
                            }

                            if let Some(ref events) = self.review_events {
                                let sev_str = severity_name(finding.severity);
                                events.emit(&ReviewEvent::Trace {
                                    event_type: "output_finding".to_string(),
                                    agent_name: agent_name.clone(),
                                    turn,
                                    clause_id: Some(clause.chunk_id.clone()),
                                    summary: format!("发现: {} ({})", finding.risk_type, sev_str),
                                    payload: Some(serde_json::json!({
                                        "risk_id": finding.risk_id,
                                        "severity": sev_str,
                                        "is_critical": finding.is_critical,
                                        "critical_reason": finding.critical_reason,
                                        "risk_type": finding.risk_type,
                                        "category_code": finding.category_code,
                                        "confidence": finding.confidence,
                                        "no_risk": finding.no_risk,
                                        "reason": finding.reason,
                                        "suggestion": finding.suggestion,
                                        "source_quote": finding.source_quote,
                                        "legal_basis": finding.legal_basis,
                                        "case_refs": finding.case_refs,
                                        "citations": finding.citations,
                                        "truncated": finding.truncated,
                                        "tier_escalated": finding.tier_escalated,
                                        "initial_tier": finding.initial_tier.to_string(),
                                        "final_tier": finding.final_tier.to_string(),
                                        "page_number": finding.page_number,
                                        "section_path": finding.section_path,
                                    })),
                                });
                            }

                            if finding.severity == RiskSeverity::High
                                && let Some(bus) = &self.bus
                                && let Some(agent_id) = AgentId::parse(agent_name)
                            {
                                bus.broadcast(
                                    agent_id.clone(),
                                    finding.severity,
                                    &finding.reason,
                                    &finding.clause_ids,
                                    &finding.risk_type,
                                );
                                let mut trace = self.trace.lock().await;
                                trace.log(
                                    TraceEventType::AgentBusSend,
                                    turn,
                                    Some(&clause.chunk_id),
                                    &format!(
                                        "High risk broadcast: {} ({})",
                                        finding.risk_type, finding.severity
                                    ),
                                    serde_json::json!({
                                        "from": agent_id.to_string(),
                                        "risk_type": finding.risk_type,
                                        "category_code": finding.category_code,
                                        "clause_ids": finding.clause_ids,
                                        "severity": "high",
                                        "is_critical": finding.is_critical,
                                    }),
                                );
                            }
                        }

                        {
                            let _lock = self.print_lock.as_ref().map(|l| l.lock().unwrap());
                            eprintln!(
                                "✅ output_finding 解析成功：{} 条发现，has_more={}",
                                findings.len(),
                                has_more
                            );
                        }
                        if let Some(ref metrics) = self.metrics {
                            metrics.lock().await.mark_last_finding_parsed_ok();
                        }

                        return ChunkReviewOutput {
                            findings,
                            has_more,
                            coverage,
                        };
                    }
                    Err(e) => {
                        // ── 关键调试：打印原始 JSON + 解析错误（加锁）──
                        let raw_json = serde_json::to_string_pretty(args)
                            .unwrap_or_else(|_| format!("{:?}", args));
                        {
                            let _lock = self.print_lock.as_ref().map(|l| l.lock().unwrap());
                            eprintln!(
                                "⚠️  output_finding JSON 解析失败!\n\
                                     ─── LLM 原始 output_finding arguments ───\n\
                                     {raw}\n\
                                     ─── END ───\n\
                                     解析错误: {err}\n\
                                     期望批量信封: findings(最多5条), has_more, coverage",
                                raw = raw_json,
                                err = e,
                            );
                        }
                        // 追加详细的重试提示
                        conversation.push(ChatMessage::Tool {
                                tool_call_id: "output_finding".to_string(),
                                content: format!(
                                    "output_finding 参数解析错误: {}\n\
                                     请返回 {{\"findings\":[...],\"has_more\":false,\"coverage\":[...]}}。\n\
                                     每条 finding 必须包含 no_risk, severity, is_critical, critical_reason,\n\
                                     risk_type, category_code, source_quote, legal_basis, reason, suggestion, confidence。\n\
                                     请修正后重新调用 output_finding。",
                                    e
                                ),
                            });
                        continue;
                    }
                }
            }

            // ── Step 4: 执行工具调用 ──
            // 先追加 assistant 消息
            let assistant_tool_calls: Vec<ToolCall> = response.tool_calls.clone();
            conversation.push(ChatMessage::Assistant {
                content: response.content,
                tool_calls: if assistant_tool_calls.is_empty() {
                    None
                } else {
                    Some(assistant_tool_calls.clone())
                },
            });

            // 如果没有工具调用且没有 output_finding，LLM 只是回复了文本
            // 追加一个提示让它继续
            if assistant_tool_calls.is_empty() {
                conversation.push(ChatMessage::User {
                    content: "请继续审查——调用工具搜索证据或输出结论。如果证据已充分，请调用 output_finding。"
                        .to_string(),
                });
                continue;
            }

            // 执行每个工具调用
            for tc in &assistant_tool_calls {
                let tool_name = &tc.name;

                // SSE: tool_call — 发送完整工具参数到前端
                if let Some(ref events) = self.review_events {
                    let query = tc
                        .arguments
                        .get("query")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let summary = if query.is_empty() {
                        format!("调用工具: {}", tool_name)
                    } else {
                        format!(
                            "{}: {}",
                            tool_name,
                            query.chars().take(80).collect::<String>()
                        )
                    };
                    // 构建精简的 payload：含完整参数 + 人类可读描述
                    let mut tc_payload = tc.arguments.clone();
                    // 为 read_section 补充人类可读的 clause_id 描述
                    if tool_name == "read_section"
                        && let Some(cid) = tc.arguments.get("clause_id").and_then(|v| v.as_str())
                    {
                        tc_payload["_clause_label"] =
                            serde_json::Value::String(format!("条款 {}", cid));
                    }
                    events.emit(&ReviewEvent::Trace {
                        event_type: "tool_call".to_string(),
                        agent_name: agent_name.clone(),
                        turn,
                        clause_id: Some(clause.chunk_id.clone()),
                        summary,
                        payload: Some(serde_json::json!({
                            "tool_name": tool_name,
                            "arguments": tc_payload,
                        })),
                    });
                }

                // 搜索缓存逻辑（search_knowledge / web_search 共用）
                let result = if self.is_search_tool(tool_name) {
                    self.cached_search_knowledge(&tc.arguments).await
                } else if let Some(tool) = self.tools.get(tool_name) {
                    match tool.execute(tc.arguments.clone()).await {
                        Ok(val) => val,
                        Err(e) => serde_json::json!({ "error": format!("{}", e) }),
                    }
                } else {
                    let available: Vec<String> = self
                        .tools
                        .definitions_filtered(&self.config.tool_names)
                        .iter()
                        .filter_map(|d| {
                            d.get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(|n| n.as_str())
                                .map(String::from)
                        })
                        .collect();
                    serde_json::json!({
                        "error": format!("工具 '{}' 未注册。当前可用工具: {}。请只使用以上工具。", tool_name, available.join(", "))
                    })
                };

                // ── 工具结果摘要（加锁避免并行 Agent 交叠）──
                {
                    let _lock = self.print_lock.as_ref().map(|l| l.lock().unwrap());
                    match tool_name.as_str() {
                        "read_section" => {
                            let title = result
                                .get("section_path")
                                .and_then(|p| p.as_array())
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|v| v.as_str())
                                        .collect::<Vec<_>>()
                                        .join(" > ")
                                })
                                .unwrap_or_else(|| "(未知)".to_string());
                            let chars = result
                                .get("char_count")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0);
                            eprintln!("📖 read_section → {} ({} 字符)", title, chars);
                        }
                        "search_knowledge" | "web_search" | "search_document" => {
                            let hit_count = Self::count_search_hits(&result);
                            let query = tc
                                .arguments
                                .get("query")
                                .and_then(|q| q.as_str())
                                .unwrap_or("?");
                            let cat = tc
                                .arguments
                                .get("category")
                                .and_then(|c| c.as_str())
                                .unwrap_or("");
                            let cat_str = if cat.is_empty() {
                                String::new()
                            } else {
                                format!(" [{}]", cat)
                            };
                            eprintln!(
                                "🔍 {} → \"{}\"{} = {} 条结果",
                                tool_name, query, cat_str, hit_count
                            );
                            // 打印前几条标题（兼容 sources 和 hits 两种格式）
                            let items: Option<&Vec<serde_json::Value>> = result
                                .get("sources")
                                .and_then(|s| s.as_array())
                                .or_else(|| result.get("hits").and_then(|h| h.as_array()))
                                .or_else(|| result.as_array());
                            if let Some(arr) = items {
                                for (i, h) in arr.iter().take(3).enumerate() {
                                    let t = h.get("title").and_then(|t| t.as_str()).unwrap_or("?");
                                    // WebSource 格式 (DashScope/SearXNG): title + url
                                    let url = h.get("url").and_then(|u| u.as_str()).unwrap_or("");
                                    if !url.is_empty() {
                                        eprintln!("   #{}. {} — {}", i + 1, t, url);
                                    } else {
                                        // SearchHit 格式 (search_document): title + score + snippet
                                        let s =
                                            h.get("score").and_then(|s| s.as_f64()).unwrap_or(0.0);
                                        let snip =
                                            h.get("snippet").and_then(|s| s.as_str()).unwrap_or("");
                                        let snip_short: String = snip.chars().take(200).collect();
                                        eprintln!(
                                            "   #{}. [score={:.2}] {} — {}",
                                            i + 1,
                                            s,
                                            t,
                                            snip_short
                                        );
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                } // 释放打印锁

                // SSE: tool_result — 发送工具返回的实际数据到前端
                if let Some(ref events) = self.review_events {
                    let summary = if self.is_search_tool(tool_name) {
                        let hits = Self::count_search_hits(&result);
                        format!("{} 返回 {} 条结果", tool_name, hits)
                    } else if tool_name == "read_section" {
                        let chars = result
                            .get("char_count")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        let section_title = result
                            .get("section_path")
                            .and_then(|p| p.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str())
                                    .collect::<Vec<_>>()
                                    .join(" > ")
                            })
                            .unwrap_or_else(|| "(未知)".to_string());
                        format!("read_section → {} ({} 字符)", section_title, chars)
                    } else {
                        format!("{} 执行完成", tool_name)
                    };

                    // 构建 payload：根据工具类型提取前端需要的数据
                    let payload = if self.is_search_tool(tool_name)
                        || tool_name == "search_document"
                    {
                        let sources = Self::extract_search_sources_for_sse(&result, 5);
                        let items: Vec<serde_json::Value> = result
                            .get("sources")
                            .and_then(|s| s.as_array())
                            .or_else(|| result.get("hits").and_then(|h| h.as_array()))
                            .map(|a| a.iter().take(5).cloned().collect())
                            .unwrap_or_default();
                        if sources.is_empty() && items.is_empty() {
                            None
                        } else {
                            Some(serde_json::json!({
                                "sources": sources,
                                "items": items,
                                "hit_count": Self::count_search_hits(&result),
                            }))
                        }
                    } else if tool_name == "read_section" {
                        let text = result.get("text").and_then(|t| t.as_str()).unwrap_or("");
                        // 发送前 2000 字符的文本预览（足够前端判断内容）
                        let text_preview: String = text.chars().take(2000).collect();
                        Some(serde_json::json!({
                            "char_count": result.get("char_count").and_then(|v| v.as_i64()).unwrap_or(0),
                            "section_path": result.get("section_path"),
                            "page_start": result.get("page_start"),
                            "page_end": result.get("page_end"),
                            "text_preview": text_preview,
                            "truncated": text.chars().count() > 2000,
                        }))
                    } else {
                        // 其他工具：发送完整返回结果（限制大小）
                        let result_str = serde_json::to_string(&result).unwrap_or_default();
                        let preview: String = result_str.chars().take(2000).collect();
                        Some(serde_json::json!({
                            "raw_preview": preview,
                            "truncated": result_str.chars().count() > 2000,
                        }))
                    };

                    events.emit(&ReviewEvent::Trace {
                        event_type: "tool_result".to_string(),
                        agent_name: agent_name.clone(),
                        turn,
                        clause_id: Some(clause.chunk_id.clone()),
                        summary,
                        payload,
                    });
                }

                // ★ read_section 重复读取检测
                if tool_name == "read_section"
                    && let Some(chunk_id) = tc
                        .arguments
                        .get("chunk_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                {
                    let count = read_section_count.entry(chunk_id.clone()).or_insert(0);
                    *count += 1;
                    if *count >= 3 {
                        // 同一 chunk 读了 3 次 → 强制停止
                        conversation.push(ChatMessage::Tool {
                            tool_call_id: tc.id.clone(),
                            content: serde_json::to_string(&result).unwrap_or_default(),
                        });
                        conversation.push(ChatMessage::System {
                                content: format!(
                                    "🛑 你已经 {} 次读取 chunk_id={} 的原文。不要再重复读了。\n\
                                    基于已掌握的原文信息 + 搜索到的法规依据，立即调用 output_finding 输出结论。",
                                    count, chunk_id
                                ),
                            });
                        continue;
                    } else if *count >= 2 {
                        // 同一 chunk 读了 2 次 → 温和提示
                        conversation.push(ChatMessage::Tool {
                            tool_call_id: tc.id.clone(),
                            content: serde_json::to_string(&result).unwrap_or_default(),
                        });
                        conversation.push(ChatMessage::System {
                            content: format!(
                                "⚠️ 你已经 {} 次读取 chunk_id={} 的原文。\
                                    如果原文信息已经够用，请尽快 output_finding，不要反复精读。",
                                count, chunk_id
                            ),
                        });
                        continue;
                    }
                }

                // 检测空搜索结果
                if self.is_search_tool(tool_name) || tool_name == "search_document" {
                    let hit_count = Self::count_search_hits(&result);

                    if hit_count == 0 {
                        consecutive_empty_searches += 1;
                        // ── 三级空结果升级策略 ──
                        if consecutive_empty_searches == 1 {
                            // L1: 第 1 次空 → 温和提示换策略
                            conversation.push(ChatMessage::Tool {
                                tool_call_id: tc.id.clone(),
                                content: serde_json::to_string(&result).unwrap_or_default(),
                            });
                            conversation.push(ChatMessage::System {
                                content: "搜索返回 0 条结果。换一组关键词或换 category 再试一次。若下次仍为空，必须基于已读原文+已知法规常识直接 output_finding。"
                                    .to_string(),
                            });
                            continue;
                        } else if consecutive_empty_searches == 2 {
                            // L2: 连续 2 次空 → 强硬指令：禁止再搜
                            conversation.push(ChatMessage::Tool {
                                tool_call_id: tc.id.clone(),
                                content: serde_json::to_string(&result).unwrap_or_default(),
                            });
                            conversation.push(ChatMessage::System {
                                content: "🛑 连续 2 次搜索返回空结果。你已用完搜索机会。\n\
                                    禁止调用 web_search 或 search_document。\n\
                                    基于已读的条款原文 + 已知的法规常识，立即调用 output_finding 输出结论。\n\
                                    在 reason 开头标注：『搜索未返回结果，以下判定基于已知法规常识。』\n\
                                    不要再搜索了。现在调用 output_finding。"
                                    .to_string(),
                            });
                            force_output_next = true;
                            continue;
                        } else {
                            // L3: 连续 3+ 次空（Agent 无视了 L2 指令）→ 最后通牒
                            conversation.push(ChatMessage::Tool {
                                tool_call_id: tc.id.clone(),
                                content: serde_json::to_string(&result).unwrap_or_default(),
                            });
                            conversation.push(ChatMessage::System {
                                content:
                                    "⛔ 这是第 3 次空搜索。你的下一个动作必须是 output_finding。\n\
                                    不调用 output_finding 将导致 max_turns 耗尽、审查截断。\n\
                                    立即输出 output_finding，no_risk 设为 true 亦可。"
                                        .to_string(),
                            });
                            force_output_next = true;
                            // 不 continue——让正常流程追加 tool result（保持对话一致性）
                        }
                    } else {
                        consecutive_empty_searches = 0;

                        // ── 搜索重复检测：若连续 2 次搜索返回相同 top-3 URL，触发强制 output ──
                        if self.is_search_tool(tool_name) {
                            let current_urls = Self::extract_top_urls(&result, 3);
                            if !current_urls.is_empty() && current_urls == last_search_urls {
                                consecutive_duplicate_searches += 1;
                                if consecutive_duplicate_searches >= 2 {
                                    conversation.push(ChatMessage::Tool {
                                        tool_call_id: tc.id.clone(),
                                        content: serde_json::to_string(&result).unwrap_or_default(),
                                    });
                                    conversation.push(ChatMessage::System {
                                        content: "🛑 连续 2 次搜索返回相同的结果列表，搜索引擎对不同 query 返回了相同内容。\n\
                                            你已用完有效的搜索机会。禁止再调用 web_search。\n\
                                            基于已搜索到的信息 + 条款原文 + 已知法规常识，立即调用 output_finding 输出结论。\n\
                                            在 reason 开头标注：『联网搜索未返回差异化结果，以下判定基于已知法规常识。』\n\
                                            不要再搜索了。现在调用 output_finding。"
                                            .to_string(),
                                    });
                                    force_output_next = true;
                                    continue;
                                }
                            } else {
                                consecutive_duplicate_searches = 0;
                                last_search_urls = current_urls;
                            }
                        }

                        // 硬性限制: web_search 按 tier 分级限制
                        if self.is_search_tool(tool_name) {
                            web_search_count += 1;

                            // ★ 法规引用提取：检查本次搜索是否带来了新的法规引用
                            let new_law_refs = Self::extract_law_refs(&result);
                            let novel_refs: Vec<_> = new_law_refs
                                .iter()
                                .filter(|r| !seen_law_refs.contains(*r))
                                .collect();
                            if !novel_refs.is_empty() {
                                // 有新法规引用 → 重置确认搜索计数器
                                seen_law_refs.extend(new_law_refs);
                                // 搜索带来了新法规 → 标记"已找到可用的法规"
                                found_actionable_law = true;
                                post_law_search_count = 0;
                            } else if found_actionable_law {
                                // 已找到法规，但本次搜索没有新法规引用 → 确认搜索
                                post_law_search_count += 1;
                            }

                            // ★ 确认搜索拦截：已找到法规 + 又做了 1 次无新法规的搜索 → 停
                            // ★ 优化：同时设置 force_output_next，下一轮强制锁定 output_finding，
                            //   避免 Auto 模式下 LLM 又调用其他工具，浪费额外 LLM 调用。
                            if found_actionable_law && post_law_search_count >= 1 {
                                conversation.push(ChatMessage::Tool {
                                    tool_call_id: tc.id.clone(),
                                    content: serde_json::to_string(&result).unwrap_or_default(),
                                });
                                conversation.push(ChatMessage::System {
                                    content: "🛑 你已经找到了可以支撑判断的法规依据，本次搜索没有带来新的法规引用。\n\
                                        法条本身就是最高依据，不需要案例「佐证」或重复搜索确认。\n\
                                        下一轮将强制你输出结论。立即整理已有信息，调用 output_finding。"
                                        .to_string(),
                                });
                                if tier != RiskTier::High {
                                    force_output_next = true;
                                }
                                continue;
                            }

                            // ★ Tier 分级硬上限
                            let search_limit = match tier {
                                RiskTier::Low => 1,    // L1 纯格式/信息，基本不需要搜索
                                RiskTier::Medium => 2, // L2 标准审查，1-2 次法规搜索够用
                                RiskTier::High => 4,   // L3 深度审查，需要多角度搜索
                            };
                            if web_search_count >= search_limit {
                                conversation.push(ChatMessage::Tool {
                                    tool_call_id: tc.id.clone(),
                                    content: serde_json::to_string(&result).unwrap_or_default(),
                                });
                                let limit_msg = match tier {
                                    RiskTier::Low => {
                                        "此条款为 L1 格式/信息类，无需搜索。\n".to_string()
                                    }
                                    RiskTier::Medium => {
                                        format!(
                                            "你已调用 web_search {} 次，达到此级别条款的上限。\n",
                                            search_limit
                                        )
                                    }
                                    RiskTier::High => {
                                        format!(
                                            "你已调用 web_search {} 次，达到硬性上限。\n",
                                            search_limit
                                        )
                                    }
                                };
                                conversation.push(ChatMessage::System {
                                    content: format!(
                                        "🛑 {}禁止再调用 web_search。\n\
                                        基于已搜索到的信息 + 条款原文 + 已知法规常识，立即调用 output_finding 输出结论。\n\
                                        不要再搜索了。现在调用 output_finding。",
                                        limit_msg
                                    ),
                                });
                                force_output_next = true;
                                continue;
                            }
                        }
                    }
                }

                conversation.push(ChatMessage::Tool {
                    tool_call_id: tc.id.clone(),
                    content: serde_json::to_string(&result).unwrap_or_default(),
                });
            }

            // ── Step 5: Turn 2 动态升降级检测 ──
            if turn == 2 {
                let (new_tier, escalated) =
                    self.check_tier_escalation(&conversation, initial_tier, tier_escalated);
                if new_tier != tier {
                    let _lock = self.print_lock.as_ref().map(|l| l.lock().unwrap());
                    eprintln!(
                        "🔄 分级变化: {} → {} (escalated={})",
                        tier, new_tier, escalated
                    );
                }
                tier = new_tier;
                tier_escalated = escalated;
            }
        }

        // ── max_turns 耗尽 → 强制输出 ──
        let summary = format!(
            "执行了 {} 轮审查（上限 {} 轮），Agent: {}，条款: {}",
            turn, max_turns, agent_name, clause.chunk_id
        );
        {
            let _lock = self.print_lock.as_ref().map(|l| l.lock().unwrap());
            eprintln!("⛔ max_turns 耗尽! {}", summary);
        }
        ChunkReviewOutput::single(RiskFinding::truncated_finding(
            risk_id.to_string(),
            clause.chunk_id.clone(),
            agent_name,
            initial_tier,
            tier,
            &summary,
        ))
    }

    // ── 辅助方法 ────────────────────────────────────────────

    /// 格式化为发送给 Agent 的条款审查提示。
    /// 从搜索答案中提取关键摘要（第一句实质性内容，最长 120 字）。
    /// 去掉常见 LLM 引导语，在第一个句号/换行处截断。
    fn extract_snippet(answer: &str) -> String {
        let text = answer.trim();
        if text.is_empty() {
            return String::new();
        }

        // 去掉常见的 LLM 引导语
        let mut cleaned = text;
        let boilerplate = [
            "根据搜索结果，",
            "根据搜索结果：",
            "根据您的要求，",
            "根据您的要求：",
            "根据查询结果，",
            "根据查询结果：",
            "搜索结果显示，",
            "搜索结果显示：",
            "根据相关法规，",
            "好的，",
            "根据提供的搜索结果，",
        ];
        for prefix in &boilerplate {
            if let Some(s) = cleaned.strip_prefix(prefix) {
                cleaned = s;
                break;
            }
        }
        cleaned = cleaned.trim();

        // 如果去掉引导语后以 "以下是" 开头，再剥一层
        if let Some(s) = cleaned.strip_prefix("以下是") {
            cleaned = s.trim();
        }

        // 取第一句（到第一个句号、换行，最长 120 字）
        const MAX_LEN: usize = 120;
        let mut snippet = String::new();
        for ch in cleaned.chars() {
            if snippet.chars().count() >= MAX_LEN {
                break;
            }
            snippet.push(ch);
            if ch == '。' || ch == '\n' {
                break;
            }
        }

        snippet.trim().to_string()
    }

    fn format_clause_prompt(&self, clause: &ReviewClause) -> String {
        let tier_hint = match clause.tier {
            RiskTier::Low => {
                "【L1 - 快速扫描】此条款为格式/信息类，风险极低。条款原文已在上方给出，直接分析即可——一般在 1-2 轮内输出结论（no_risk=true 或若有格式缺失则标记）。不要调用 read_section（原文已在上下文中），不要调用 web_search（没有可核查的阈值）。"
            }
            RiskTier::Medium => {
                "【L2 - 标准审查】条款原文已在上方给出，请直接分析原文并搜索法规进行对照。需要关联条款时用 search_document → read_section。web_search 最多 2 次。"
            }
            RiskTier::High => {
                "【L3 - 深度审查】此条款含高风险关键词（品牌/地域/排他性），请深度审查：分析原文 → web_search(法规→案例) → search_document(跨条款联动) → 需要时 read_section(精读确认关联条款) → 输出结论。web_search 最多 4 次。"
            }
        };

        let mut prompt = format!(
            "{}\n\n【条款信息】\nchunk_id: {}\n章节路径: {}\n页码: {}-{}\n\n【条款文本】\n{}",
            tier_hint,
            clause.chunk_id,
            clause.section_path.join(" > "),
            clause.page_start + 1,
            clause.page_end + 1,
            clause.text
        );

        // ★ 注入预搜索结果摘要（V5：紧凑摘要 — 每条 1 句关键条款内容）
        //   在 V2"法规索引"(仅URL) 和 V1"全文摘要"(300字) 之间取中间地带：
        //   从搜索答案中提取第一句实质性内容（最长 120 字），Agent 看到法规核心
        //   条款后可直接判断，无需为了"看法规写了什么"而 Turn 1 搜索。
        if let Some(graph) = &self.graph {
            let entries = graph.get_search_results_for_clause(&clause.chunk_id);
            if !entries.is_empty() {
                let mut seen_queries: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                let mut summary_items: Vec<String> = Vec::new();
                const MAX_ITEMS: usize = 8;

                for entry in &entries {
                    if summary_items.len() >= MAX_ITEMS {
                        break;
                    }
                    // 按 (query, category) 去重
                    let dedup_key = format!("{}|{}", entry.query.to_lowercase(), entry.category);
                    if !seen_queries.insert(dedup_key) {
                        continue;
                    }

                    let snippet = Self::extract_snippet(&entry.answer);
                    if snippet.is_empty() {
                        continue;
                    }

                    // 法规简称：优先用 source title（最规范），回退到 query 前 40 字
                    let label = entry
                        .sources
                        .first()
                        .filter(|s| !s.title.is_empty())
                        .map(|s| s.title.chars().take(50).collect::<String>())
                        .unwrap_or_else(|| entry.query.chars().take(40).collect::<String>());

                    // 收集来源 URL（最多 2 个）
                    let urls: Vec<&str> = entry
                        .sources
                        .iter()
                        .filter(|s| !s.url.is_empty())
                        .map(|s| s.url.as_str())
                        .take(2)
                        .collect();

                    let url_str = if urls.is_empty() {
                        String::new()
                    } else if urls.len() == 1 {
                        format!(" [来源]({})", urls[0])
                    } else {
                        format!(" [来源1]({}) [来源2]({})", urls[0], urls[1])
                    };

                    summary_items.push(format!("- {}：_{}_{}", label, snippet, url_str));
                }

                if !summary_items.is_empty() {
                    prompt.push_str("\n\n📋 法规摘要（已预载入搜索缓存，可直接引用）:\n");
                    prompt.push_str(&summary_items.join("\n"));
                    prompt.push('\n');
                }
            }
        }

        prompt
    }

    /// Turn 2 动态升降级检测。
    ///
    /// 检查前 2 轮对话内容，判断是否需要升级或降级。
    /// - 升级触发：Agent 表达了高风险怀疑（"可能存在"/"值得深挖"/"疑似"）
    /// - 降级触发：无高风险信号
    fn check_tier_escalation(
        &self,
        conversation: &[ChatMessage],
        current_tier: RiskTier,
        already_escalated: bool,
    ) -> (RiskTier, bool) {
        if already_escalated {
            return (current_tier, true);
        }

        // 拼接前 2 轮对话检查 Agent 是否表达了高风险怀疑
        let suspicious_phrases = [
            "可能存在",
            "值得深挖",
            "需要进一步",
            "不排除",
            "疑似",
            "涉嫌",
            "潜在风险",
            "值得关注",
            "需进一步核实",
        ];

        let combined: String = conversation
            .iter()
            .filter_map(|msg| match msg {
                ChatMessage::Assistant { content, .. } => content.clone(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");

        let has_suspicious = suspicious_phrases
            .iter()
            .any(|phrase| combined.contains(phrase));

        match (current_tier, has_suspicious) {
            // L1/L2 + 可疑信号 → 升级到 L3
            (RiskTier::Low, true) | (RiskTier::Medium, true) => (RiskTier::High, true),
            // L3 + 无信号 → 降级到 L2
            (RiskTier::High, false) => (RiskTier::Medium, false),
            _ => (current_tier, false),
        }
    }

    /// 从搜索结果中提取法规引用（用于判断搜索是否带来了新的法律信息）。
    ///
    /// 匹配模式包括：
    /// - "第X条" / "第XX条" / "第XXX条"
    /// - "〔20XX〕XX号" / "令第X号"
    /// - "《...》第X条"
    ///
    /// 返回去重后的法规引用集合。
    fn extract_law_refs(result: &serde_json::Value) -> std::collections::HashSet<String> {
        let mut refs = std::collections::HashSet::new();
        // 拼接 answer + sources 中的 title/snippet 作为提取源
        let mut text = String::new();
        if let Some(answer) = result.get("answer").and_then(|a| a.as_str()) {
            text.push_str(answer);
        }
        if let Some(sources) = result.get("sources").and_then(|s| s.as_array()) {
            for src in sources {
                if let Some(title) = src.get("title").and_then(|t| t.as_str()) {
                    text.push(' ');
                    text.push_str(title);
                }
                if let Some(snippet) = src.get("snippet").and_then(|s| s.as_str()) {
                    text.push(' ');
                    text.push_str(snippet);
                }
            }
        }
        // 提取法规引用模式
        use regex::Regex;
        // 模式1: 第X条 (含中文数字)
        if let Ok(re) = Regex::new(r"第[零一二三四五六七八九十百]+条") {
            for m in re.find_iter(&text) {
                refs.insert(m.as_str().to_string());
            }
        }
        // 模式2: 《...》第X条
        if let Ok(re) = Regex::new(r"《[^》]+》第[零一二三四五六七八九十百]+条") {
            for m in re.find_iter(&text) {
                refs.insert(m.as_str().to_string());
            }
        }
        // 模式3: 令第X号
        if let Ok(re) = Regex::new(r"\S+令第[零一二三四五六七八九十百]+号") {
            for m in re.find_iter(&text) {
                refs.insert(m.as_str().to_string());
            }
        }
        // 模式4: 〔20XX〕XX号
        if let Ok(re) = Regex::new(r"〔\d{4}〕\d+号") {
            for m in re.find_iter(&text) {
                refs.insert(m.as_str().to_string());
            }
        }
        refs
    }

    /// 判断是否为搜索类工具（兼容新旧工具名）。
    fn is_search_tool(&self, name: &str) -> bool {
        name == "web_search" || name == "search_knowledge" || name == "search_knowledge_base"
    }

    /// 统计搜索结果条数，兼容多种后端格式。
    ///
    /// - DashScope/SearXNG (web_search): JSON 中包含 `sources` 数组
    /// - search_document: JSON 中包含 `hits` 数组
    /// - 旧格式: 结果本身是顶层数组
    ///
    /// ★ DashScope 特殊处理：模型可能返回详尽的 AI 回答但无显式来源 URL。
    /// 此时 answer 本身即为有效搜索结果，不应视为"空搜索"。
    fn count_search_hits(result: &serde_json::Value) -> usize {
        // 1) DashScope / SearXNG 统一格式: { "answer": "...", "sources": [...] }
        if let Some(arr) = result.get("sources").and_then(|s| s.as_array()) {
            let source_count = arr.len();
            if source_count > 0 {
                return source_count;
            }
            // sources 为空，检查 answer 是否有实质内容
            // DashScope 可能返回详细的 AI 回答但没有显式来源 URL——
            // 此时 answer 本身就是有效搜索结果，不应触发空搜索拦截
            if let Some(answer) = result.get("answer").and_then(|a| a.as_str())
                && answer.chars().count() > 100
            {
                return 1; // 有实质回答 → 视为 1 条有效结果
            }
            return 0;
        }
        // 2) search_document 格式: { "hits": [...] }
        if let Some(arr) = result.get("hits").and_then(|h| h.as_array()) {
            return arr.len();
        }
        // 3) 旧/未知格式：顶层数组
        result.as_array().map(|a| a.len()).unwrap_or(0)
    }

    /// 从搜索结果中提取 top-N 的 source URL，用于重复检测。
    ///
    /// 兼容 DashScope / SearXNG 格式的 `sources` 数组。
    fn extract_top_urls(result: &serde_json::Value, n: usize) -> Vec<String> {
        if let Some(arr) = result.get("sources").and_then(|s| s.as_array()) {
            arr.iter()
                .filter_map(|item| {
                    item.get("url")
                        .or_else(|| item.get("link"))
                        .and_then(|u| u.as_str())
                        .map(|s| s.to_string())
                })
                .take(n)
                .collect()
        } else {
            Vec::new()
        }
    }

    /// 从搜索结果中提取 top-N 条 {title, url}，用于 SSE 推送前端展示。
    fn extract_search_sources_for_sse(
        result: &serde_json::Value,
        n: usize,
    ) -> Vec<serde_json::Value> {
        let mut items: Vec<serde_json::Value> = Vec::new();
        // 1) DashScope / SearXNG: sources 数组
        if let Some(arr) = result.get("sources").and_then(|s| s.as_array()) {
            for item in arr.iter().take(n) {
                let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let url = item
                    .get("url")
                    .or_else(|| item.get("link"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !url.is_empty() {
                    items.push(serde_json::json!({ "title": title, "url": url }));
                }
            }
        }
        // 2) search_document: hits 数组（没有 URL，用 score + snippet 代替）
        if items.is_empty()
            && let Some(arr) = result.get("hits").and_then(|h| h.as_array())
        {
            for item in arr.iter().take(n) {
                let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let score = item.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
                if !title.is_empty() {
                    items.push(serde_json::json!({
                        "title": title,
                        "score": format!("{:.2}", score)
                    }));
                }
            }
        }
        items
    }

    /// 带缓存的 search_knowledge / web_search 调用。
    async fn cached_search_knowledge(&self, args: &serde_json::Value) -> serde_json::Value {
        let query = args.get("query").and_then(|q| q.as_str()).unwrap_or("");
        let category = args
            .get("category")
            .and_then(|c| c.as_str())
            .unwrap_or("法规");

        let cache_key = (query.to_string(), category.to_string());

        // 检查缓存：精确匹配 → 模糊 bigram 匹配（仅匹配缓存 key）
        //   ★ 经验：不匹配 answer 文本（缓存命中后 answer 注入对话历史，prompt 膨胀）。
        //     不做截断提示（"需要完整内容请重新搜索"诱使 Agent 多搜）。
        {
            let cache = self.search_cache.lock().await;
            // 1) 精确 (query, category) 匹配
            if let Some(cached) = cache.get(&cache_key) {
                return cached.clone();
            }
            // 2) 模糊 bigram Jaccard（query vs cache key），阈值 ≥ 0.4
            //    ★ 优化：0.25 → 0.4，收紧模糊命中，避免语义相距较远的查询
            //      误命中缓存返回不相关结果，诱导 Agent 重复搜索。
            if !query.is_empty() && !cache.is_empty() {
                let q_chars: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).collect();
                let q_bigrams: std::collections::HashSet<String> = q_chars
                    .windows(2)
                    .map(|w| w.iter().collect::<String>())
                    .collect();
                if !q_bigrams.is_empty() {
                    let mut best_score: f64 = 0.0;
                    let mut best_value: Option<&serde_json::Value> = None;
                    for ((k, _cat), v) in cache.iter() {
                        let k_chars: Vec<char> = k.chars().filter(|c| !c.is_whitespace()).collect();
                        let k_bigrams: std::collections::HashSet<String> = k_chars
                            .windows(2)
                            .map(|w| w.iter().collect::<String>())
                            .collect();
                        let intersection = q_bigrams.intersection(&k_bigrams).count();
                        let union = q_bigrams.union(&k_bigrams).count();
                        if union > 0 {
                            let score = intersection as f64 / union as f64;
                            if score > best_score && score >= 0.4 {
                                best_score = score;
                                best_value = Some(v);
                            }
                        }
                    }
                    if let Some(cached) = best_value {
                        return cached.clone();
                    }
                }
            }
        }

        // 执行实际搜索
        // 同时兼容旧名 search_knowledge 和新名 web_search
        let result = if let Some(tool) = self
            .tools
            .get("web_search")
            .or_else(|| self.tools.get("search_knowledge"))
        {
            match tool.execute(args.clone()).await {
                Ok(val) => val,
                Err(e) => serde_json::json!({ "error": format!("{}", e) }),
            }
        } else {
            serde_json::json!({ "error": "web_search / search_knowledge 工具未注册" })
        };

        // 写入缓存
        {
            let mut cache = self.search_cache.lock().await;
            cache.insert(cache_key, result.clone());
        }

        result
    }

    /// 从 search_cache 中提取所有搜索来源 URL，去重后返回 Citation 列表。
    ///
    /// 遍历 search_cache 中每条搜索结果，提取 `sources` 数组中的
    /// `(title, url, site_name)` 三元组，按 URL 去重（同一 URL 只保留首次出现）。
    /// 用于自动填充 RiskFinding.citations 字段。
    async fn extract_citations(&self) -> Vec<Citation> {
        let cache = self.search_cache.lock().await;
        let mut seen_urls: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut citations: Vec<Citation> = Vec::new();

        for value in cache.values() {
            if let Some(sources) = value.get("sources").and_then(|s| s.as_array()) {
                for source in sources {
                    let url = source
                        .get("url")
                        .and_then(|u| u.as_str())
                        .unwrap_or("")
                        .to_string();
                    // 跳过空 URL 或已见过的 URL
                    if url.is_empty() || seen_urls.contains(&url) {
                        continue;
                    }
                    seen_urls.insert(url.clone());
                    citations.push(Citation {
                        title: source
                            .get("title")
                            .and_then(|t| t.as_str())
                            .unwrap_or("")
                            .to_string(),
                        url,
                        site_name: source
                            .get("site_name")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string(),
                    });
                }
            }
        }

        citations
    }
}

// ─── 并行条款审查调度器 ────────────────────────────────────────

/// 并行审查多条条款。
///
/// 为每条条款创建独立的 LLM 客户端和工具集，通过 `tokio::task::JoinSet`
/// 并行执行 ReAct 循环，受 `Semaphore` 控制最大并发数。
///
/// # 参数
///
/// * `clauses` — 待审查条款列表
/// * `make_agent` — Agent 工厂闭包，接收 (LLM客户端, 工具集) → 返回完全配置的 ReActLoop
/// * `llm_factory` — LLM 客户端工厂（每条 task 调用一次，创建独立实例）
/// * `tools_factory` — 工具集工厂（每条 task 调用一次，创建独立实例）
/// * `max_parallel` — 最大并行审查条款数（Semaphore permits）
/// * `graph` — SessionGraph（用于生成全局唯一 risk_id，None 时回退到索引编号）
/// * `review_events` — SSE 推送通道（None 时不推送）
/// * `agent_name` — Agent 名称（用于日志和进度事件）
#[derive(Debug)]
pub struct ClauseReviewFailure {
    pub clause_id: String,
    pub message: String,
}

#[derive(Debug)]
pub struct ClauseReviewReport {
    pub findings: Vec<RiskFinding>,
    pub successful_clauses: usize,
    pub failed_clauses: Vec<ClauseReviewFailure>,
}

#[allow(clippy::too_many_arguments)]
pub async fn review_clauses_parallel_report<F>(
    clauses: &[ReviewClause],
    make_agent: F,
    llm_factory: Arc<dyn Fn() -> Box<dyn LlmClient> + Send + Sync>,
    tools_factory: Arc<dyn Fn() -> crate::agents::tools::ToolRegistry + Send + Sync>,
    max_parallel: usize,
    graph: Option<Arc<SessionGraph>>,
    review_events: Option<Arc<ReviewEventBus>>,
    agent_name: &str,
    execution_control: Option<Arc<crate::agents::execution_control::ReviewExecutionControl>>,
) -> ClauseReviewReport
where
    F: Fn(Box<dyn LlmClient>, crate::agents::tools::ToolRegistry) -> ReActLoop
        + Send
        + Sync
        + 'static,
{
    if clauses.is_empty() {
        return ClauseReviewReport {
            findings: Vec::new(),
            successful_clauses: 0,
            failed_clauses: Vec::new(),
        };
    }

    let sem = Arc::new(tokio::sync::Semaphore::new(max_parallel.max(1)));
    let total = clauses.len();
    let done = Arc::new(AtomicUsize::new(0));
    let raw_findings_total = Arc::new(AtomicUsize::new(0));
    let make_agent = Arc::new(make_agent);
    let mut join_set = JoinSet::new();

    for (idx, clause) in clauses.iter().enumerate() {
        let clause = clause.clone();
        let sem = sem.clone();
        let llm_factory = llm_factory.clone();
        let tools_factory = tools_factory.clone();
        let make_agent = make_agent.clone();
        let graph = graph.clone();
        let events = review_events.clone();
        let name = agent_name.to_string();
        let done = done.clone();
        let raw_findings_total = raw_findings_total.clone();
        let execution_control = execution_control.clone();

        join_set.spawn(async move {
            let _permit = sem.acquire_owned().await;
            let _global_permit = if let Some(ref control) = execution_control {
                Some(control.acquire().await?)
            } else {
                None
            };
            let mut llm = llm_factory();
            let mut tools = tools_factory();
            if let Some(ref control) = execution_control {
                llm = crate::agents::execution_control::ControlledLlmClient::wrap(
                    llm,
                    control.clone(),
                );
                tools = tools.into_controlled(control.clone());
            }
            let agent = make_agent(llm, tools);
            let risk_id = graph
                .as_ref()
                .map(|g| g.next_risk_id())
                .unwrap_or_else(|| format!("R_{:03}", idx + 1));
            let findings = if let Some(ref control) = execution_control {
                match tokio::time::timeout(
                    control.limits().clause_timeout,
                    agent.review_single(&clause, &risk_id),
                )
                .await
                {
                    Ok(findings) => findings,
                    Err(_) => vec![RiskFinding::truncated_finding(
                        risk_id.clone(),
                        clause.chunk_id.clone(),
                        &name,
                        clause.tier,
                        clause.tier,
                        "单条条款审查超过 180 秒",
                    )],
                }
            } else {
                agent.review_single(&clause, &risk_id).await
            };

            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            let risk_count = findings.iter().filter(|f| !f.no_risk).count();
            raw_findings_total.fetch_add(risk_count, Ordering::Relaxed);

            // SSE 实时进度推送
            if let Some(ref events) = events {
                events.emit(&ReviewEvent::AgentProgress {
                    agent_id: name.clone(),
                    agent_label: name.clone(),
                    clauses_done: n,
                    clauses_total: total,
                    raw_findings: raw_findings_total.load(Ordering::Relaxed),
                    status: if n >= total {
                        "completed".to_string()
                    } else {
                        "running".to_string()
                    },
                });
            }

            Ok::<_, anyhow::Error>((idx, findings))
        });
    }

    // 收集结果，按原始顺序排列
    let mut findings: Vec<Option<Vec<RiskFinding>>> = (0..total).map(|_| None).collect();
    while let Some(result) = join_set.join_next().await {
        match result {
            Ok(Ok((idx, clause_findings))) => {
                findings[idx] = Some(clause_findings);
            }
            Ok(Err(e)) => {
                eprintln!("[PARALLEL] 获取全局并发名额失败: {}", e);
            }
            Err(e) => {
                // task panic — 为该 clause 生成占位 finding
                eprintln!("[PARALLEL] 条款审查 task 异常: {}", e);
            }
        }
    }

    // 补齐缺失 finding，并显式记录条款级失败，避免占位结果被误判为成功。
    let mut collected = Vec::new();
    let mut successful_clauses = 0;
    let mut failed_clauses = Vec::new();
    for (i, result) in findings.into_iter().enumerate() {
        match result {
            Some(clause_findings) if clause_findings.iter().any(|finding| finding.truncated) => {
                failed_clauses.push(ClauseReviewFailure {
                    clause_id: clauses[i].chunk_id.clone(),
                    message: "条款审查未完整结束".to_string(),
                });
                collected.extend(clause_findings);
            }
            Some(clause_findings) => {
                successful_clauses += 1;
                collected.extend(clause_findings);
            }
            None => {
                failed_clauses.push(ClauseReviewFailure {
                    clause_id: clauses[i].chunk_id.clone(),
                    message: "并行审查 task 异常终止".to_string(),
                });
                collected.push(RiskFinding::truncated_finding(
                    format!("R_{:03}", i + 1),
                    clauses[i].chunk_id.clone(),
                    agent_name,
                    clauses[i].tier,
                    clauses[i].tier,
                    "并行审查 task 异常终止",
                ));
            }
        }
    }

    ClauseReviewReport {
        findings: collected,
        successful_clauses,
        failed_clauses,
    }
}

/// 兼容单 Agent 调用方，仅返回 finding；Coordinator 应使用带完整性报告的版本。
#[allow(clippy::too_many_arguments)]
pub async fn review_clauses_parallel<F>(
    clauses: &[ReviewClause],
    make_agent: F,
    llm_factory: Arc<dyn Fn() -> Box<dyn LlmClient> + Send + Sync>,
    tools_factory: Arc<dyn Fn() -> crate::agents::tools::ToolRegistry + Send + Sync>,
    max_parallel: usize,
    graph: Option<Arc<SessionGraph>>,
    review_events: Option<Arc<ReviewEventBus>>,
    agent_name: &str,
    execution_control: Option<Arc<crate::agents::execution_control::ReviewExecutionControl>>,
) -> Vec<RiskFinding>
where
    F: Fn(Box<dyn LlmClient>, crate::agents::tools::ToolRegistry) -> ReActLoop
        + Send
        + Sync
        + 'static,
{
    review_clauses_parallel_report(
        clauses,
        make_agent,
        llm_factory,
        tools_factory,
        max_parallel,
        graph,
        review_events,
        agent_name,
        execution_control,
    )
    .await
    .findings
}

#[cfg(test)]
mod multi_finding_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    use tokio::sync::Notify;

    struct GatedNoRiskLlm {
        started: Arc<Notify>,
        released: Arc<AtomicBool>,
        release_notify: Arc<Notify>,
    }

    struct ConditionalSlowLlm;

    struct AlwaysFailLlm;

    #[async_trait::async_trait]
    impl LlmClient for AlwaysFailLlm {
        async fn chat(
            &self,
            _messages: &[ChatMessage],
            _tools: &[serde_json::Value],
            _tool_choice: &ToolChoice,
        ) -> Result<LlmResponse> {
            Err(anyhow::anyhow!("模拟 LLM 基础设施故障"))
        }
    }

    #[async_trait::async_trait]
    impl LlmClient for ConditionalSlowLlm {
        async fn chat(
            &self,
            messages: &[ChatMessage],
            _tools: &[serde_json::Value],
            _tool_choice: &ToolChoice,
        ) -> Result<LlmResponse> {
            let should_timeout = messages.iter().any(|message| match message {
                ChatMessage::User { content } => content.contains("模拟超时"),
                _ => false,
            });
            if should_timeout {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            Ok(LlmResponse {
                content: None,
                thought: None,
                tool_calls: vec![ToolCall {
                    id: "test-output".to_string(),
                    name: "output_finding".to_string(),
                    arguments: serde_json::json!({
                        "findings": [],
                        "has_more": false,
                        "coverage": [],
                    }),
                }],
                usage: None,
            })
        }
    }

    #[tokio::test]
    async fn llm_failure_is_reported_as_clause_failure() {
        let clauses = vec![ReviewClause {
            chunk_id: "ch_llm_error".to_string(),
            section_path: vec!["测试".to_string()],
            text: "格式要求".to_string(),
            page_start: 0,
            page_end: 0,
            tier: RiskTier::Low,
            tier_max_turns: 1,
            source_block_ids: vec![],
        }];

        let report = review_clauses_parallel_report(
            &clauses,
            |llm, tools| {
                ReActLoop::new(
                    AgentConfig {
                        name: "TestAgent".to_string(),
                        system_prompt: "测试".to_string(),
                        default_max_turns: 1,
                        tool_names: vec!["output_finding".to_string()],
                    },
                    llm,
                    tools,
                )
            },
            Arc::new(|| Box::new(AlwaysFailLlm)),
            Arc::new(crate::agents::tools::ToolRegistry::new),
            1,
            None,
            None,
            "TestAgent",
            None,
        )
        .await;

        assert_eq!(report.successful_clauses, 0);
        assert_eq!(report.failed_clauses.len(), 1);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| { finding.category_code == "ENGINE_ERROR" && finding.truncated })
        );
    }

    #[async_trait::async_trait]
    impl LlmClient for GatedNoRiskLlm {
        async fn chat(
            &self,
            _messages: &[ChatMessage],
            _tools: &[serde_json::Value],
            _tool_choice: &ToolChoice,
        ) -> Result<LlmResponse> {
            self.started.notify_one();
            while !self.released.load(Ordering::SeqCst) {
                self.release_notify.notified().await;
            }
            Ok(LlmResponse {
                content: None,
                thought: None,
                tool_calls: vec![ToolCall {
                    id: "test-output".to_string(),
                    name: "output_finding".to_string(),
                    arguments: serde_json::json!({
                        "findings": [],
                        "has_more": false,
                        "coverage": [],
                    }),
                }],
                usage: None,
            })
        }
    }

    #[tokio::test]
    async fn parallel_review_creates_clients_only_after_permit_is_acquired() {
        let factory_calls = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(Notify::new());
        let released = Arc::new(AtomicBool::new(false));
        let release_notify = Arc::new(Notify::new());
        let clauses = vec![
            ReviewClause {
                chunk_id: "ch_001".to_string(),
                section_path: vec!["测试".to_string()],
                text: "格式要求一".to_string(),
                page_start: 0,
                page_end: 0,
                tier: RiskTier::Low,
                tier_max_turns: 1,
                source_block_ids: vec![],
            },
            ReviewClause {
                chunk_id: "ch_002".to_string(),
                section_path: vec!["测试".to_string()],
                text: "格式要求二".to_string(),
                page_start: 0,
                page_end: 0,
                tier: RiskTier::Low,
                tier_max_turns: 1,
                source_block_ids: vec![],
            },
        ];
        let factory = {
            let factory_calls = factory_calls.clone();
            let started = started.clone();
            let released = released.clone();
            let release_notify = release_notify.clone();
            move || {
                factory_calls.fetch_add(1, Ordering::SeqCst);
                Box::new(GatedNoRiskLlm {
                    started: started.clone(),
                    released: released.clone(),
                    release_notify: release_notify.clone(),
                }) as Box<dyn LlmClient>
            }
        };

        let task = tokio::spawn(async move {
            review_clauses_parallel_report(
                &clauses,
                |llm, tools| {
                    ReActLoop::new(
                        AgentConfig {
                            name: "TestAgent".to_string(),
                            system_prompt: "测试".to_string(),
                            default_max_turns: 1,
                            tool_names: vec!["output_finding".to_string()],
                        },
                        llm,
                        tools,
                    )
                },
                Arc::new(factory),
                Arc::new(crate::agents::tools::ToolRegistry::new),
                1,
                None,
                None,
                "TestAgent",
                None,
            )
            .await
        });

        started.notified().await;
        assert_eq!(
            factory_calls.load(Ordering::SeqCst),
            1,
            "等待并发名额的条款不得提前创建 LLM 客户端"
        );
        released.store(true, Ordering::SeqCst);
        release_notify.notify_waiters();
        let report = task.await.expect("并行审查任务应正常结束");
        assert_eq!(report.successful_clauses, 2);
    }

    #[tokio::test]
    async fn one_clause_timeout_preserves_other_clause_result() {
        let clauses = vec![
            ReviewClause {
                chunk_id: "ch_ok".to_string(),
                section_path: vec!["测试".to_string()],
                text: "正常条款".to_string(),
                page_start: 0,
                page_end: 0,
                tier: RiskTier::Low,
                tier_max_turns: 1,
                source_block_ids: vec![],
            },
            ReviewClause {
                chunk_id: "ch_timeout".to_string(),
                section_path: vec!["测试".to_string()],
                text: "模拟超时".to_string(),
                page_start: 0,
                page_end: 0,
                tier: RiskTier::Low,
                tier_max_turns: 1,
                source_block_ids: vec![],
            },
        ];
        let limiter = Arc::new(
            crate::agents::execution_control::GlobalExecutionLimiter::new(
                crate::agents::execution_control::ExecutionLimits {
                    global_concurrency: 2,
                    document_concurrency: 2,
                    clause_timeout: std::time::Duration::from_millis(20),
                    ..crate::agents::execution_control::ExecutionLimits::default()
                },
            ),
        );
        let control = limiter.start_review(2, 2);

        let report = review_clauses_parallel_report(
            &clauses,
            |llm, tools| {
                ReActLoop::new(
                    AgentConfig {
                        name: "TestAgent".to_string(),
                        system_prompt: "测试".to_string(),
                        default_max_turns: 1,
                        tool_names: vec!["output_finding".to_string()],
                    },
                    llm,
                    tools,
                )
            },
            Arc::new(|| Box::new(ConditionalSlowLlm)),
            Arc::new(crate::agents::tools::ToolRegistry::new),
            2,
            None,
            None,
            "TestAgent",
            Some(control),
        )
        .await;

        assert_eq!(report.successful_clauses, 1);
        assert_eq!(report.failed_clauses.len(), 1);
        assert_eq!(report.failed_clauses[0].clause_id, "ch_timeout");
    }

    fn finding_json(category: &str, quote: &str) -> serde_json::Value {
        serde_json::json!({
            "no_risk": false,
            "severity": "high",
            "is_critical": false,
            "critical_reason": "",
            "risk_type": category,
            "category_code": category,
            "source_quote": quote,
            "legal_basis": [],
            "reason": "测试理由",
            "suggestion": "测试建议",
            "confidence": 0.9
        })
    }

    #[test]
    fn parses_multiple_findings_from_one_chunk() {
        let args = serde_json::json!({
            "findings": [
                finding_json("LOCAL_REGISTRATION", "须在本地注册"),
                finding_json("EXCESSIVE_DEPOSIT", "保证金为预算的5%"),
                finding_json("UNILATERAL_CHANGE", "采购人可单方变更")
            ],
            "has_more": false,
            "coverage": ["qualification", "procedure", "contract"]
        });
        let (findings, has_more, coverage) = parse_finding_batch(&args).unwrap();
        assert_eq!(findings.len(), 3);
        assert!(!has_more);
        assert_eq!(coverage.len(), 3);
    }

    #[test]
    fn keeps_valid_items_when_one_item_is_invalid() {
        let args = serde_json::json!({
            "findings": [
                finding_json("LOCAL_REGISTRATION", "须在本地注册"),
                {"severity": "high"}
            ],
            "has_more": false,
            "coverage": []
        });
        let (findings, has_more, _) = parse_finding_batch(&args).unwrap();
        assert_eq!(findings.len(), 1);
        assert!(has_more, "局部解析失败应触发选择性补扫");
    }

    #[test]
    fn empty_findings_replaces_no_risk_placeholder() {
        let args = serde_json::json!({
            "findings": [],
            "has_more": false,
            "coverage": ["procedure"]
        });
        let (findings, has_more, _) = parse_finding_batch(&args).unwrap();
        assert!(findings.is_empty());
        assert!(!has_more);
    }

    #[test]
    fn detects_numbered_multi_issue_chunk() {
        let text = "1.地域注册限制\n须本地注册\n2、保证金超限\n保证金5%\n3）单方变更";
        assert_eq!(numbered_item_count(text), 3);
    }
}
