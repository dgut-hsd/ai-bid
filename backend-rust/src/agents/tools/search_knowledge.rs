//! `search_knowledge` 工具 — 搜索外部知识库（法规/案例/负面清单/标准范本）。
//!
//! ## 架构 (Phase 2.5 — 搜索缓冲池)
//!
//! 多 Agent 并发审查场景下，每个 Agent 独立搜索 SearXNG 会触发下游引擎
//! (Baidu/Google) 的并发限流。`SearchBuffer` 将跨 Agent 的搜索请求合并到
//! 单后台 worker 串行执行，实现：
//!
//! 1. **查询去重** — 相似 query 自动归一化，合并为一次请求
//! 2. **串行执行** — 单线程消费队列，请求间固定间隔
//! 3. **结果广播** — 等待同一 query 的所有 Agent 同时收到结果
//! 4. **退避重试** — 空结果时自动冷却 2s 后重试一次
//!
//! 原有 `KnowledgeSearch` trait 保留，用于 Tavily / Mock 等替代后端。

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, broadcast, mpsc};

use super::AgentTool;

// ─── 知识搜索结果 ──────────────────────────────────────────────

/// 单条知识搜索结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeResult {
    /// 标题
    pub title: String,
    /// 内容摘要（前 500 字）
    pub snippet: String,
    /// 来源 URL
    pub url: String,
    /// 内容正文（如有）
    pub content: Option<String>,
    /// 相关性分数 [0.0, 1.0]
    pub score: f32,
}

// ─── KnowledgeSearch trait（保留，用于 Tavily/Mock 后端）────────

#[async_trait::async_trait]
pub trait KnowledgeSearch: Send + Sync {
    async fn search(&self, query: &str, category: &str) -> Result<Vec<KnowledgeResult>>;
}

// ─── Mock 实现 ─────────────────────────────────────────────────

pub struct MockKnowledgeSearch;

#[async_trait::async_trait]
impl KnowledgeSearch for MockKnowledgeSearch {
    async fn search(&self, query: &str, category: &str) -> Result<Vec<KnowledgeResult>> {
        Ok(vec![KnowledgeResult {
            title: format!("Mock 搜索结果 — {}: {}", category, query),
            snippet: format!(
                "Mock 搜索引擎返回。实际部署时调用 SearXNG 搜索 '{}'。",
                query
            ),
            url: "https://example.com/mock-result".to_string(),
            content: None,
            score: 0.8,
        }])
    }
}

// ─── Tavily 实现（保留兼容）────────────────────────────────────

pub struct TavilyKnowledgeSearch {
    api_key: String,
    client: reqwest::Client,
}

impl TavilyKnowledgeSearch {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl KnowledgeSearch for TavilyKnowledgeSearch {
    async fn search(&self, query: &str, category: &str) -> Result<Vec<KnowledgeResult>> {
        let search_query = match category {
            "法规" => format!("{} site:gov.cn", query),
            "案例" => format!("财政部投诉处理决定 {}", query),
            "负面清单" => format!("政府采购负面清单 {}", query),
            "标准范本" => format!("政府采购 标准招标文件 {}", query),
            _ => query.to_string(),
        };

        let response = self
            .client
            .post("https://api.tavily.com/search")
            .json(&serde_json::json!({
                "api_key": self.api_key,
                "query": search_query,
                "search_depth": "advanced",
                "max_results": 5,
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Tavily API 返回错误: {} {}",
                response.status(),
                response.text().await.unwrap_or_default()
            ));
        }

        let body: serde_json::Value = response.json().await?;
        let results: Vec<KnowledgeResult> = body
            .get("results")
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|r| KnowledgeResult {
                        title: r["title"].as_str().unwrap_or("").to_string(),
                        snippet: r["content"]
                            .as_str()
                            .unwrap_or("")
                            .chars()
                            .take(500)
                            .collect(),
                        url: r["url"].as_str().unwrap_or("").to_string(),
                        content: r["raw_content"].as_str().map(|s| s.to_string()),
                        score: r["score"].as_f64().unwrap_or(0.0) as f32,
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(results)
    }
}

// ─── SearXNG HTTP 客户端（SearchBuffer 内部使用）────────────────

/// SearXNG 自托管元搜索引擎的底层 HTTP 客户端。
///
/// 不直接暴露给 Agent——由 `SearchBuffer` 的 background worker 独占使用。
struct SearXNGClient {
    base_url: String,
    http: reqwest::Client,
}

impl SearXNGClient {
    fn new(base_url: &str) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("构建 SearXNG HTTP 客户端失败");

        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
        }
    }

    async fn search(&self, query: &str, category: &str) -> Vec<KnowledgeResult> {
        let search_query = match category {
            "法规" => format!("{} 政府采购 法律法规", query),
            "案例" => format!("{} 政府采购 投诉处理 案例", query),
            "负面清单" => format!("{} 政府采购负面清单", query),
            "标准范本" => format!("{} 政府采购 标准招标文件", query),
            _ => query.to_string(),
        };

        let url = format!("{}/search", self.base_url);
        eprintln!(
            "  [SearXNG] 搜索: category={} query=\"{}\" → q=\"{}\"",
            category, query, search_query
        );

        let response = match self
            .http
            .get(&url)
            .query(&[("q", search_query.as_str()), ("format", "json")])
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("  [SearXNG] 请求失败: {}", e);
                return Vec::new();
            }
        };

        if !response.status().is_success() {
            eprintln!(
                "  [SearXNG] 返回错误 {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            );
            return Vec::new();
        }

        let body: serde_json::Value = match response.json().await {
            Ok(b) => b,
            Err(e) => {
                eprintln!("  [SearXNG] 解析响应失败: {}", e);
                return Vec::new();
            }
        };

        let raw_count = body
            .get("results")
            .and_then(|r| r.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        eprintln!("  [SearXNG] 原始返回 {} 条结果", raw_count);

        let mut results: Vec<KnowledgeResult> = body
            .get("results")
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|r| {
                        let engine = r["engine"].as_str().unwrap_or("unknown");
                        let title = r["title"].as_str().unwrap_or("").to_string();
                        let snippet = r["content"]
                            .as_str()
                            .or_else(|| r["snippet"].as_str())
                            .unwrap_or("")
                            .chars()
                            .take(500)
                            .collect();
                        let url = r["url"].as_str().unwrap_or("").to_string();
                        let score = r["score"].as_f64().unwrap_or(0.0) as f32;
                        KnowledgeResult {
                            title: format!("[{}] {}", engine, title),
                            snippet,
                            url,
                            content: None,
                            score,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        if results.len() > 5 {
            results.retain(|r| is_relevant_result(&r.url, &r.snippet));
        }

        eprintln!("  [SearXNG] 过滤后保留 {} 条结果", results.len());
        results
    }
}

fn is_relevant_result(url: &str, snippet: &str) -> bool {
    const BLOCKLIST: &[&str] = &[
        "google.com/accounts",
        "google.com/signin",
        "accounts.google",
        "apple.com",
        "discussions.apple",
        "reddit.com",
        "forbes.com",
        "wikihow.com",
        "gmail.com",
        "inbodyusa",
        "malwaretips",
    ];

    for blocked in BLOCKLIST {
        if url.contains(blocked) {
            return false;
        }
    }

    if snippet.chars().count() < 10 {
        return false;
    }

    let has_chinese = snippet.contains(|c: char| ('\u{4e00}'..='\u{9fff}').contains(&c));
    let has_gov_keyword = snippet.contains("procurement")
        || snippet.contains("government")
        || url.contains("gov.cn")
        || url.contains("gov.");

    if !has_chinese && !has_gov_keyword {
        return false;
    }

    true
}

// ─── SearXNGSearch 兼容包装（保留旧接口）────────────────────────

/// 向后兼容的 SearXNG 客户端（不使用 SearchBuffer，直接搜索）。
///
/// 用于单 Agent 模式（非 Coordinator）或测试场景。
pub struct SearXNGSearch {
    client: SearXNGClient,
}

impl SearXNGSearch {
    pub fn new(base_url: String) -> Self {
        Self {
            client: SearXNGClient::new(&base_url),
        }
    }

    pub fn from_env() -> Self {
        let url =
            std::env::var("SEARXNG_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
        Self::new(url)
    }
}

#[async_trait::async_trait]
impl KnowledgeSearch for SearXNGSearch {
    async fn search(&self, query: &str, category: &str) -> Result<Vec<KnowledgeResult>> {
        Ok(self.client.search(query, category).await)
    }
}

// ─── SearchBuffer: 跨 Agent 搜索缓冲池 ─────────────────────────

/// 搜索任务——从 Agent 端发送给后台 worker。
struct SearchTask {
    /// 归一化后的去重 key
    key: String,
    /// 原始 query
    query: String,
    /// 搜索类别
    category: String,
}

/// 跨 Agent 共享的搜索缓冲池。
///
/// ## 工作原理
///
/// ```text
/// Agent A ─┐
/// Agent B ─┤   ┌──────────────────────┐
/// Agent C ─┼──→│ SearchBuffer          │
/// Agent D ─┤   │ · 归一化 query 去重    │──→ SearXNG (串行, 500ms 间隔)
/// Agent E ─┘   │ · 单 worker 串行消费   │
///              │ · 空结果退避重试       │
///              │ · 结果广播给等待者     │
///              └──────────────────────┘
/// ```
///
/// ## 线程安全
///
/// `SearchBuffer` 内部使用 `tokio::sync::Mutex`（非阻塞），适合高并发 async 场景。
pub struct SearchBuffer {
    /// 发送搜索任务给后台 worker
    tx: mpsc::UnboundedSender<SearchTask>,
    /// 等待中的查询：key → broadcast sender
    /// 多个 Agent 等待同一 key 时，worker 执行一次搜索，结果广播给所有人
    pending: Mutex<HashMap<String, broadcast::Sender<Vec<KnowledgeResult>>>>,
    /// 跨 Session 法规缓存（可选）
    law_cache: Option<Arc<Mutex<LawCache>>>,
}

impl SearchBuffer {
    /// 创建 SearchBuffer 并启动后台 worker。
    ///
    /// * `searxng_base_url` — SearXNG 服务地址，如 `"http://localhost:8080"`
    /// * `law_cache` — 可选的跨 Session 法规缓存，提供 `Arc<Mutex<LawCache>>` 以启用持久化缓存
    pub fn new(
        searxng_base_url: String,
        law_cache: Option<Arc<Mutex<LawCache>>>,
    ) -> Arc<Self> {
        let (tx, mut rx) = mpsc::unbounded_channel::<SearchTask>();

        let buf = Arc::new(Self {
            tx,
            pending: Mutex::new(HashMap::new()),
            law_cache,
        });

        // 启动后台 worker（独立 spawned task）
        let worker = buf.clone();
        let worker_url = searxng_base_url.trim_end_matches('/').to_string();
        tokio::spawn(async move {
            let client = SearXNGClient::new(&worker_url);
            eprintln!(
                "  [SearchBuffer] 后台 worker 已启动，SearXNG: {}",
                worker_url
            );

            while let Some(task) = rx.recv().await {
                // ★ 先查缓存（跨 Session 持久化）
                let mut results = if let Some(ref cache) = worker.law_cache {
                    let c = cache.lock().await;
                    c.get(&task.key)
                } else {
                    None
                };

                if let Some(ref cached_results) = results {
                    eprintln!(
                        "  [SearchBuffer] 缓存命中: key={} → {} 条结果",
                        task.key,
                        cached_results.len()
                    );
                } else {
                    // ★ 执行搜索
                    eprintln!(
                        "  [SearchBuffer] 执行搜索: key={} query=\"{}\"",
                        task.key, task.query
                    );
                    let mut search_results = client.search(&task.query, &task.category).await;

                    // ★ 空结果退避重试（冷却 2s 后重试一次）
                    if search_results.is_empty() {
                        eprintln!("  [SearchBuffer] 空结果，2s 后退避重试: {}", task.key);
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        search_results = client.search(&task.query, &task.category).await;
                        if search_results.is_empty() {
                            eprintln!("  [SearchBuffer] 重试仍为空: {}", task.key);
                        } else {
                            eprintln!(
                                "  [SearchBuffer] 重试成功: {} → {} 条结果",
                                task.key,
                                search_results.len()
                            );
                        }
                    }

                    // ★ 搜索结果写入缓存
                    if !search_results.is_empty() {
                        if let Some(ref cache) = worker.law_cache {
                            let mut c = cache.lock().await;
                            c.put(task.key.clone(), search_results.clone());
                            eprintln!("  [SearchBuffer] 已缓存: key={}", task.key);
                        }
                    }

                    results = Some(search_results);
                }

                // ★ 广播结果给所有等待的 Agent
                let mut pending = worker.pending.lock().await;
                if let Some(tx) = pending.remove(&task.key) {
                    let _ = tx.send(results.unwrap_or_default());
                    // broadcast sender 随 drop 自动清理
                }

                // ★ 请求间隔（保护 SearXNG 下游引擎不被限流）
                tokio::time::sleep(Duration::from_millis(500)).await;
            }

            eprintln!("  [SearchBuffer] 后台 worker 已退出");
        });

        buf
    }

    /// Agent 调用入口：搜索知识库（可能等待去重）。
    ///
    /// 如果另一个 Agent 正在执行相同（或高度相似）的搜索，当前 Agent
    /// 将等待该搜索完成并共享结果，而非发起重复请求。
    pub async fn search(&self, raw_query: &str, category: &str) -> Vec<KnowledgeResult> {
        let key = normalize_query(raw_query, category);

        // 1. 检查是否有相同的搜索正在进行中 → 订阅广播
        {
            let pending = self.pending.lock().await;
            if let Some(tx) = pending.get(&key) {
                let mut rx = tx.subscribe();
                drop(pending);
                eprintln!("  [SearchBuffer] 去重命中: key={} (等待已有搜索完成)", key);
                return rx.recv().await.unwrap_or_default();
            }
        }

        // 2. 新查询 → 注册 pending，发送给 worker，等待结果
        let (btx, _) = broadcast::channel::<Vec<KnowledgeResult>>(1);
        let mut rx = btx.subscribe();

        {
            let mut pending = self.pending.lock().await;
            pending.insert(key.clone(), btx);
        }

        // 发送任务给 worker（忽略 channel closed 错误）
        let _ = self.tx.send(SearchTask {
            key,
            query: raw_query.to_string(),
            category: category.to_string(),
        });

        rx.recv().await.unwrap_or_default()
    }
}

/// 查询归一化：去除标点、合并空白、统一小写。
///
/// 目的：让 "不接受联合体响应 禁止 政府采购" 和
/// "不接受联合体, 禁止-政府采购" 被视为同一查询。
fn normalize_query(query: &str, category: &str) -> String {
    let normalized: String = query
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    format!("{}|{}", category, normalized.to_lowercase())
}

// ─── search_knowledge / web_search 工具 ──────────────────────────

/// 搜索工具参数（DashScope 模式 — 自然语言问题）。
#[derive(Debug, Deserialize)]
pub struct WebSearchArgs {
    /// 完整的自然语言问题，描述要查什么 + 为什么查
    pub question: String,
    /// 搜索上下文：法规、案例、负面清单、标准范本、历史审查记录
    #[serde(default)]
    pub search_context: String,
}

/// 搜索工具参数（SearXNG 兼容模式 — 关键词）。
#[derive(Debug, Deserialize)]
pub struct SearchKnowledgeArgs {
    pub query: String,
    pub category: String,
}

/// DashScope 联网搜索返回的单条来源。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSource {
    pub title: String,
    pub url: String,
    pub site_name: String,
}

/// DashScope 联网搜索返回结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchResult {
    /// 模型基于搜索结果生成的综合回答
    pub answer: String,
    /// 引用来源列表
    pub sources: Vec<WebSource>,
}

/// 搜索后端模式。
#[derive(Debug, Clone, Copy, PartialEq)]
enum SearchMode {
    /// DashScope 原生 API — AI 联网搜索（默认）
    DashScope,
    /// SearXNG 自托管元搜索
    SearXNG,
}

/// `search_knowledge` / `web_search` 工具实现。
///
/// 在 DashScope 模式下：改名 `web_search`，输入自然语言问题，
/// 返回 AI 综合回答 + 引用来源。
/// 在 SearXNG 模式下：保持 `search_knowledge`，输入关键词，返回原始搜索列表。
pub struct SearchKnowledgeTool {
    /// 共享搜索缓冲池（SearXNG 模式专用）
    pub buffer: Option<Arc<SearchBuffer>>,
    /// 独立搜索后端（SearXNG 单 Agent 模式 / 测试）
    pub backend: Option<Box<dyn KnowledgeSearch>>,
    /// DashScope 搜索后端
    pub dashscope: Option<Arc<DashScopeSearchBackend>>,
    /// 当前搜索模式
    mode: SearchMode,
    /// 上次搜索的 source URLs（用于检测 DashScope 返回重复结果）
    last_source_urls: std::sync::Mutex<Vec<String>>,
}

impl SearchKnowledgeTool {
    /// 使用 DashScope 搜索后端创建。
    pub fn with_dashscope(dashscope: Arc<DashScopeSearchBackend>) -> Self {
        Self {
            buffer: None,
            backend: None,
            dashscope: Some(dashscope),
            mode: SearchMode::DashScope,
            last_source_urls: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// 使用 SearchBuffer 创建（SearXNG Coordinator 模式）。
    pub fn with_buffer(buffer: Arc<SearchBuffer>) -> Self {
        Self {
            buffer: Some(buffer),
            backend: None,
            dashscope: None,
            mode: SearchMode::SearXNG,
            last_source_urls: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// 使用独立 KnowledgeSearch 后端创建（SearXNG 单 Agent 模式 / 测试）。
    pub fn with_backend(backend: Box<dyn KnowledgeSearch>) -> Self {
        Self {
            buffer: None,
            backend: Some(backend),
            dashscope: None,
            mode: SearchMode::SearXNG,
            last_source_urls: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl AgentTool for SearchKnowledgeTool {
    fn name(&self) -> &str {
        // 统一工具名——不管后端是 DashScope 还是 SearXNG，
        // Agent 都看到同一个 `web_search` 接口。
        "web_search"
    }

    fn definition(&self) -> serde_json::Value {
        match self.mode {
            SearchMode::DashScope => self.dashscope_definition(),
            SearchMode::SearXNG => self.searxng_definition(),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        match self.mode {
            SearchMode::DashScope => self.execute_dashscope(args).await,
            SearchMode::SearXNG => self.execute_searxng(args).await,
        }
    }
}

impl SearchKnowledgeTool {
    /// DashScope 模式工具定义 — AI 联网研究助手。
    fn dashscope_definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "联网搜索——调用 AI 研究助手查找实时信息。\n\
                    你可以向它提出完整的自然语言问题，它会搜索互联网并返回综合回答 + 引用来源链接。\n\
                    \n\
                    【🛑 硬性限制】web_search 最多调用 5 次。超过 5 次会直接导致审查截断。\n\
                    如果连续 2 次搜索结果不相关或回答中未提供有效引用 → 立即停止搜索，\n\
                    基于条款原文 + 已知法规常识输出 output_finding。\n\
                    \n\
                    【与旧版 search_knowledge 的区别】\n\
                    这是 AI 研究助手而非关键词检索引擎——\n\
                    直接把你的问题和背景说清楚，不用提炼关键词。\n\
                    好: '这条条款要求投标人在东莞设有常驻服务机构且有本地业绩，\n\
                      请查是否有法规禁止这种地域限制性条款？'\n\
                    坏: '资格条件 地域限制 禁止 常驻服务机构'\n\
                    它会把搜索结果消化后给你分析结论，而不是丢一堆链接让你自己看。\n\
                    \n\
                    【search_context 选择指南】\n\
                    · 法规: 查找法律、行政法规、部门规章中的禁止性条款\n\
                    · 案例: 查找财政部投诉处理决定等实际判例\n\
                    · 负面清单: 查找各级政府发布的政府采购负面行为清单\n\
                    · 标准范本: 对比财政部标准招标文件模板\n\
                    · 历史审查记录: 查找本系统之前审查过的同类条款",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "question": {
                            "type": "string",
                            "description": "完整的自然语言问题。描述你要查什么、为什么查、有什么背景上下文。\n\
                                好: '这条条款规定XXX，是否存在法规禁止这种限制？'\n\
                                坏: '地域限制 禁止'"
                        },
                        "search_context": {
                            "type": "string",
                            "enum": ["法规", "案例", "负面清单", "标准范本", "历史审查记录"],
                            "description": "搜索领域上下文，帮助搜索引擎更精准定位。"
                        }
                    },
                    "required": ["question"]
                }
            }
        })
    }

    /// SearXNG 模式工具定义 — 接口与 DashScope 保持一致。
    fn searxng_definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "搜索外部知识库——法规条文、财政部案例、负面清单、标准范本。\n\
                    这是你最核心的检索工具——法规是判定风险的依据，案例是验证判断的佐证。\n\
                    \n\
                    【🛑 硬性限制】web_search 最多调用 5 次。超过 5 次会直接导致审查截断。\n\
                    如果连续 2 次搜索返回 0 条结果 → 立即停止搜索，基于条款原文+已知法规常识输出 output_finding。\n\
                    \n\
                    【提问技巧】用自然语言描述你要查什么、为什么查。\n\
                    好: '这条条款要求投标人在东莞设有常驻服务机构，请查是否有法规禁止这种地域限制？'\n\
                    坏: '资格条件 地域限制 禁止'\n\
                    \n\
                    【search_context 选择指南】\n\
                    · 法规: 查找法律、行政法规、部门规章中的禁止性条款\n\
                    · 案例: 查找财政部投诉处理决定等实际判例\n\
                    · 负面清单: 查找各级政府发布的政府采购负面行为清单\n\
                    · 标准范本: 对比财政部标准招标文件模板\n\
                    · 历史审查记录: 查找本系统之前审查过的同类条款",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "question": {
                            "type": "string",
                            "description": "完整的自然语言问题。描述你要查什么、为什么查。"
                        },
                        "search_context": {
                            "type": "string",
                            "enum": ["法规", "案例", "负面清单", "标准范本", "历史审查记录"],
                            "description": "搜索领域上下文，帮助搜索引擎更精准定位。"
                        }
                    },
                    "required": ["question"]
                }
            }
        })
    }

    /// DashScope 模式执行：调用 AI 联网搜索。
    async fn execute_dashscope(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: WebSearchArgs = serde_json::from_value(args)?;
        let backend = self
            .dashscope
            .as_ref()
            .ok_or_else(|| anyhow!("DashScope 搜索后端未配置"))?;

        let mut result = backend
            .search(&parsed.question, &parsed.search_context)
            .await?;

        // ★ 客户端去重检测：对比上次搜索的 source URLs
        // 如果 >70% 重叠 → 在 answer 前插入止损警告，让 Agent 立即停止搜索
        {
            let mut last_urls = self.last_source_urls.lock().unwrap();
            let current_urls: Vec<String> = result.sources.iter().map(|s| s.url.clone()).collect();

            if !last_urls.is_empty() && !current_urls.is_empty() {
                let overlap = current_urls
                    .iter()
                    .filter(|u| last_urls.contains(u))
                    .count();
                let overlap_ratio = overlap as f64 / current_urls.len() as f64;
                if overlap_ratio > 0.7 {
                    result.answer = format!(
                        "⚠️ 搜索结果与上次高度重复（{:.0}% 重叠）。\
                         搜索引擎对该主题信息有限，请立即基于已知法规常识输出结论，不要再次搜索。\n\n{}",
                        overlap_ratio * 100.0,
                        result.answer
                    );
                }
            }

            *last_urls = current_urls;
        }

        Ok(serde_json::to_value(&result)?)
    }

    /// SearXNG 模式执行：将自然语言问题转为关键词搜索，结果包装为 WebSearchResult 格式。
    async fn execute_searxng(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: WebSearchArgs = serde_json::from_value(args)?;

        // SearXNG 需要关键词而非自然语言，这里做简单转换：
        // 将 question 直接作为搜索词（SearXNG 也能处理自然语言），
        // 并通过 category 前缀注入领域关键词。
        let category = if parsed.search_context.is_empty() {
            "法规".to_string()
        } else {
            parsed.search_context.clone()
        };

        let raw_results = if let Some(ref buffer) = self.buffer {
            buffer.search(&parsed.question, &category).await
        } else if let Some(ref backend) = self.backend {
            backend.search(&parsed.question, &category).await?
        } else {
            return Err(anyhow!("SearchKnowledgeTool: 未配置搜索后端"));
        };

        // 映射到统一的 WebSearchResult 格式
        let result = WebSearchResult {
            answer: String::new(), // SearXNG 不生成 AI 回答
            sources: raw_results
                .into_iter()
                .map(|r| WebSource {
                    title: r.title,
                    url: r.url,
                    site_name: String::new(),
                })
                .collect(),
        };

        Ok(serde_json::to_value(&result)?)
    }
}

// ─── DashScope 搜索后端 ──────────────────────────────────────────

/// DashScope 原生 API 联网搜索后端。
///
/// 调用 DashScope Text Generation API（流式 + enable_search），
/// 从 SSE 流中提取 `search_info.search_results[]` 和模型回答。
pub struct DashScopeSearchBackend {
    http: reqwest::Client,
    api_key: String,
    model: String,
    endpoint: String,
}

impl DashScopeSearchBackend {
    const DEFAULT_ENDPOINT: &'static str =
        "https://dashscope.aliyuncs.com/api/v1/services/aigc/text-generation/generation";

    /// 创建新的 DashScope 搜索后端。
    pub fn new(api_key: &str, model: &str) -> Self {
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("DashScope 搜索 HTTP 客户端构建失败");
        Self {
            http,
            api_key: api_key.to_string(),
            model: model.to_string(),
            endpoint: Self::DEFAULT_ENDPOINT.to_string(),
        }
    }

    /// 从环境变量创建。
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("DASHSCOPE_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .context(
                "DashScope 搜索后端需要 API 密钥。请设置 DASHSCOPE_API_KEY 或 OPENAI_API_KEY",
            )?;
        let model = std::env::var("DASHSCOPE_SEARCH_MODEL")
            .or_else(|_| std::env::var("DASHSCOPE_MODEL"))
            .unwrap_or_else(|_| "qwen-plus".to_string());
        Ok(Self::new(&api_key, &model))
    }

    /// 执行 AI 联网搜索。
    ///
    /// 1. 构造搜索提示（包含 search_context 限定）
    /// 2. 调用 DashScope API（流式 + enable_search + forced_search）
    /// 3. 从 SSE 第一个 chunk 提取 search_info.search_results[]
    /// 4. 拼接所有 chunk 中的 content 作为 answer
    /// 5. 返回 WebSearchResult
    pub async fn search(&self, question: &str, search_context: &str) -> Result<WebSearchResult> {
        // 构造搜索提示：将 search_context 融入 system prompt
        let context_hint = match search_context {
            "法规" => {
                "你需要找到具体的政府采购法律法规原文或官方释义。\n\
                        搜索策略：① 用文件全称+文号精确搜索（如\"财库〔2020〕46号 全文\"）；\
                        ② 在 gov.cn / mof.gov.cn 站内搜索；③ 优先返回法规原文链接，而非新闻或公告。\
                        常见法规索引：政府采购法、政府采购法实施条例、财库〔2020〕46号、\
                        财库〔2014〕214号、第87号令、财库〔2019〕38号。"
            }
            "案例" => {
                "你需要找到财政部政府采购投诉处理决定或行政处罚案例原文。\n\
                       搜索策略：① 搜索\"财政部政府采购信息公告 第X号\"；\
                       ② 搜索\"投诉处理决定书 政府采购\"；③ 优先 gov.cn 域名下的处理决定原文。"
            }
            "负面清单" => {
                "你需要找到政府采购负面行为清单原文。\n\
                         搜索策略：① 搜索\"政府采购负面清单 XX省\"或\"政府采购禁止行为清单\"；\
                         ② 优先找省级财政厅发布的清单文件（PDF/网页）。"
            }
            "标准范本" => {
                "你需要找到财政部政府采购标准招标文件范本。\n\
                         搜索策略：① 搜索\"政府采购招标文件标准文本 财政部\"；\
                         ② 搜索\"政府采购竞争性磋商文件范本\"。"
            }
            "历史审查记录" => "你需要查找类似条款的历史审查记录或合规分析。",
            _ => "",
        };

        let system_msg = format!(
            "你是政府采购合规研究助手。\n\
             【核心任务】根据用户问题找到准确的法规/政策原文，提供可验证的引用。\n\
             【搜索要求】① 优先搜索法规全称+文号（如\"政府采购促进中小企业发展管理办法 财库〔2020〕46号\"）；\
             ② 不要返回不相关的政府采购公告/招标信息——这些不是法规；\
             ③ 优先返回 .gov.cn 域名的官方原文或政策解读；\
             ④ 如果搜索到法规全文，摘录与问题直接相关的条款。\n\
             {}\n\
             用[ref_<数字>]标注引用来源。回答要具体引用法条原文，不要泛泛总结。",
            context_hint
        );

        let body = serde_json::json!({
            "model": self.model,
            "input": {
                "messages": [
                    {"role": "system", "content": system_msg},
                    {"role": "user", "content": question}
                ]
            },
            "parameters": {
                "result_format": "message",
                "max_tokens": 2000,
                "enable_search": true,
                "search_options": {
                    "enable_source": true,
                    "enable_citation": true,
                    "citation_format": "[ref_<number>]",
                    "search_strategy": "standard",
                    "forced_search": true,
                },
                "stream": true,
                "incremental_output": true,
            }
        });

        let response = self
            .http
            .post(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .context("DashScope 搜索请求失败")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "DashScope 搜索 API 返回错误 {}: {}",
                status,
                error_text
            ));
        }

        // 读取完整 SSE 流
        let sse_text = response.text().await.context("读取搜索 SSE 流失败")?;

        // 解析 SSE
        let mut search_info: Option<serde_json::Value> = None;
        let mut answer = String::new();

        for line in sse_text.lines() {
            let line = line.trim();
            if line.is_empty() || line == "data:[DONE]" {
                continue;
            }
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim();
                if data.is_empty() {
                    continue;
                }
                if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) {
                    if search_info.is_none()
                        && let Some(si) = chunk["output"]["search_info"].as_object()
                    {
                        search_info = Some(serde_json::json!(si));
                    }
                    if let Some(content) =
                        chunk["output"]["choices"][0]["message"]["content"].as_str()
                    {
                        answer.push_str(content);
                    }
                }
            }
        }

        // 提取来源
        let sources: Vec<WebSource> = search_info
            .as_ref()
            .and_then(|si| si["search_results"].as_array())
            .map(|arr| {
                arr.iter()
                    .map(|item| WebSource {
                        title: item["title"].as_str().unwrap_or("").to_string(),
                        url: item["url"].as_str().unwrap_or("").to_string(),
                        site_name: item["site_name"].as_str().unwrap_or("").to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        // 如果没有 answer（模型只搜不说），基于来源构造摘要
        if answer.trim().is_empty() && !sources.is_empty() {
            answer = format!("搜索到 {} 条相关结果，详见来源链接。", sources.len());
        }

        eprintln!(
            "  [DashScope搜索] question=\"{:.80}...\" → answer={}字, sources={}条",
            question,
            answer.chars().count(),
            sources.len()
        );

        Ok(WebSearchResult { answer, sources })
    }
}

// ─── 法规缓存持久化层 ──────────────────────────────────────────

/// 单条法规缓存条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedLawItem {
    /// 缓存 key（归一化后的查询）
    key: String,
    /// 搜索返回的知识结果
    results: Vec<KnowledgeResult>,
    /// 缓存写入时间戳（Unix 秒）
    created_at: u64,
    /// TTL（秒），超期后缓存失效。默认 30 天 = 2592000 秒。
    ttl_seconds: u64,
}

/// 跨 Session 的法规缓存。
///
/// 持久化到 JSON 文件，Key=归一化查询，Value=搜索结果，
/// TTL=30 天（法规变更周期以月为单位，30 天合理）。
///
/// ## 使用方式
///
/// 1. 搜索前：`cache.get(&key)` → 命中且未过期 → 直接返回
/// 2. 搜索后：`cache.put(key, results)`  → 写入内存 + 异步刷盘
pub struct LawCache {
    /// 缓存内容（内存 HashMap for O(1) 查找）
    items: HashMap<String, CachedLawItem>,
    /// 持久化文件路径
    cache_path: String,
    /// 默认 TTL（秒）
    default_ttl: u64,
}

impl LawCache {
    /// 默认 TTL：30 天
    pub const DEFAULT_TTL: u64 = 30 * 24 * 3600;

    /// 创建/加载法规缓存。
    ///
    /// 从 `cache_path` 加载已有缓存文件（JSON），
    /// 自动淘汰已过期条目。
    pub fn new(cache_path: &str) -> Self {
        let mut cache = Self {
            items: HashMap::new(),
            cache_path: cache_path.to_string(),
            default_ttl: Self::DEFAULT_TTL,
        };
        cache.load();
        cache
    }

    /// 从 JSON 文件加载缓存。
    fn load(&mut self) {
        match std::fs::read_to_string(&self.cache_path) {
            Ok(content) => {
                match serde_json::from_str::<Vec<CachedLawItem>>(&content) {
                    Ok(items) => {
                        let now = Self::now_secs();
                        let mut valid_count = 0;
                        let mut expired_count = 0;
                        for item in items {
                            if now - item.created_at < item.ttl_seconds {
                                self.items.insert(item.key.clone(), item);
                                valid_count += 1;
                            } else {
                                expired_count += 1;
                            }
                        }
                        eprintln!(
                            "  [LawCache] 加载完成: {} 条有效, {} 条已过期",
                            valid_count, expired_count
                        );
                    }
                    Err(e) => {
                        eprintln!("  [LawCache] 缓存文件损坏，从头开始: {}", e);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("  [LawCache] 缓存文件不存在，将新建: {}", self.cache_path);
            }
            Err(e) => {
                eprintln!("  [LawCache] 读取缓存文件失败: {}", e);
            }
        }
    }

    /// 查询缓存。
    ///
    /// 返回 `Some(results)` 表示命中且未过期，`None` 表示未命中或已过期。
    pub fn get(&self, key: &str) -> Option<Vec<KnowledgeResult>> {
        self.items.get(key).and_then(|item| {
            let age = Self::now_secs() - item.created_at;
            if age < item.ttl_seconds {
                Some(item.results.clone())
            } else {
                None
            }
        })
    }

    /// 写入缓存（内存）并异步刷盘。
    pub fn put(&mut self, key: String, results: Vec<KnowledgeResult>) {
        let item = CachedLawItem {
            key: key.clone(),
            results,
            created_at: Self::now_secs(),
            ttl_seconds: self.default_ttl,
        };
        self.items.insert(key, item);
        self.flush_async();
    }

    /// 异步刷盘到 JSON 文件（spawn blocking task）。
    fn flush_async(&self) {
        let path = self.cache_path.clone();
        let items: Vec<CachedLawItem> = self.items.values().cloned().collect();
        tokio::task::spawn_blocking(move || {
            if let Ok(json) = serde_json::to_string(&items) {
                if let Err(e) = std::fs::write(&path, &json) {
                    eprintln!("  [LawCache] 写入缓存文件失败: {}", e);
                }
            }
        });
    }

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}
