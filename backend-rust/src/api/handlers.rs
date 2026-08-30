//! HTTP 请求处理函数 — 薄胶水层。
//!
//! 不写业务逻辑，只负责：
//! 1. 解析 HTTP 请求（JSON / multipart）
//! 2. 调用现有核心函数（services / agents）
//! 3. 格式化 HTTP 响应（JSON + 状态码）

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Multipart, Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex as TokioMutex, RwLock as TokioRwLock};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::agents::bus::AgentBus;
use crate::agents::chat_agent::ChatAgent;
use crate::agents::coordinator::Coordinator;
use crate::agents::react_loop::{ChatMessage, LlmClient};
use crate::agents::registry::AgentRegistry;
use crate::agents::review_event::ReviewEventBus;
use crate::agents::session_graph::SessionGraph;
use crate::agents::tools::{
    ToolRegistry,
    answer_user::AnswerUserTool,
    output_finding::OutputFindingTool,
    read_section::ReadSectionTool,
    search_document::SearchDocumentTool,
    search_knowledge::{DashScopeSearchBackend, SearchKnowledgeTool},
    search_knowledge_base::SearchKnowledgeBaseTool,
    // V2+ 工具
    compare_versions::CompareVersionsTool,
    detect_boilerplate::DetectBoilerplateTool,
    // V3 采购程序合规审查
    verify_procurement_method::VerifyProcurementMethodTool,
    verify_bid_deposit::VerifyBidDepositTool,
    verify_announcement_period::VerifyAnnouncementPeriodTool,
    verify_bid_preparation_period::VerifyBidPreparationPeriodTool,
    // V4 评审标准审查
    validate_scoring_formula::ValidateScoringFormulaTool,
    validate_weight_distribution::ValidateWeightDistributionTool,
    detect_subjective_scoring::DetectSubjectiveScoringTool,
    check_scoring_completeness::CheckScoringCompletenessTool,
    check_imported_products::CheckImportedProductsTool,
    verify_consortium_rules::VerifyConsortiumRulesTool,
    // 零依赖计算/检查工具
    calculate_timeline::CalculateTimelineTool,
    // 依赖 chunk 数据的工具
    check_cross_reference::CheckCrossReferenceTool,
    extract_obligations::ExtractObligationsTool,
    compare_with_template::{CompareWithTemplateTool, ChunkTextProvider, TemplateStore},
    validate_calculation::ValidateCalculationTool,
    search_contradiction::SearchContradictionTool,
};
use crate::agents::trace::TraceLog;
use crate::agents::types::{
    AgentId, ChatAgentConfig, ChatResponse, ChatStreamEvent, CoordinatorConfig, CoordinatorOutput,
    HighlightRect, ReviewClause, TextSelection,
};
use crate::domain::chunk::{Chunk, ChunkingConfig};
use crate::domain::raw_document::{BBox, RawDocument};
use crate::domain::vector_index::DocumentVectorIndex;
use crate::paths::data_path_str;
use crate::services::chunking_service::chunk_sections;
use crate::services::desensitize_service::{
    DesensitizationMode, DesensitizationSummary, RedactionVault,
};
use crate::services::docx_convert_service::convert_docx_to_pdf;
use crate::services::embedding_service::EmbeddingClient;
use crate::services::llm_client::create_llm_client;
use crate::services::pdf_extract_service::{extract_pdf_to_raw_json, extract_with_python};

/// 审核期间会产生大量 trace/finding 事件。256 容量在百页文档上很容易
/// 让 SSE 消费者落后；默认扩大到 4096，同时允许部署环境按内存预算调整。
fn review_event_capacity() -> usize {
    std::env::var("AIBID_REVIEW_EVENT_CAPACITY")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(4096)
        .clamp(256, 32768)
}

/// P2 实验开关：`AIBID_COMPRESS_TRANSCRIPT=1` 时压缩 assistant 冗长推理独白。
/// 非法值视为关闭——保守默认，A/B 基线组（control）不受影响。
fn transcript_compression_enabled() -> bool {
    parse_transcript_compression(std::env::var("AIBID_COMPRESS_TRANSCRIPT").ok().as_deref())
}

/// 纯解析函数（便于单测）：仅 `1`/`true`/`on` 视为开启。
fn parse_transcript_compression(value: Option<&str>) -> bool {
    matches!(value, Some("1") | Some("true") | Some("on"))
}

use crate::services::sectionize_service::{self, Section};

/// Authenticated Java → Rust request identity made available to handlers via
/// request extensions by the internal API middleware.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalRequestContext {
    pub tenant_id: String,
    pub user_id: String,
    pub request_id: String,
    pub timestamp: i64,
    pub body_sha256: String,
}

/// Explicit tenant/document composite key used for every in-memory document
/// and review resource. A document ID is only meaningful inside its tenant.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DocumentKey {
    pub tenant_id: String,
    pub document_id: String,
}

impl DocumentKey {
    pub fn new(tenant_id: impl Into<String>, document_id: impl Into<String>) -> Self {
        Self {
            tenant_id: tenant_id.into(),
            document_id: document_id.into(),
        }
    }
}

/// Tenant IDs are decimal strings in the Java/Rust internal contract. Keeping
/// this check next to path construction prevents an untrusted header from
/// becoming a filesystem component.
pub(crate) fn is_valid_tenant_id(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_safe_document_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn document_key(
    context: &InternalRequestContext,
    document_id: &str,
) -> Result<DocumentKey, (StatusCode, Json<ErrorResponse>)> {
    if !is_valid_tenant_id(&context.tenant_id) || !is_safe_document_id(document_id) {
        return Err(document_not_found());
    }
    Ok(DocumentKey::new(
        context.tenant_id.clone(),
        document_id.to_string(),
    ))
}

fn tenant_output_path(tenant_id: &str, relative: &str) -> Option<PathBuf> {
    if !is_valid_tenant_id(tenant_id) || relative.is_empty() {
        return None;
    }
    let root = PathBuf::from(data_path_str("output/tenants"));
    let namespace = root.join(tenant_id);
    let candidate = namespace.join(relative);
    candidate.starts_with(&namespace).then_some(candidate)
}

fn tenant_document_path(
    tenant_id: &str,
    relative_dir: &str,
    document_id: &str,
    suffix: &str,
) -> Option<PathBuf> {
    if !is_safe_document_id(document_id) {
        return None;
    }
    let dir = tenant_output_path(tenant_id, relative_dir)?;
    let candidate = dir.join(format!("{document_id}{suffix}"));
    candidate.starts_with(&dir).then_some(candidate)
}

async fn load_document(
    state: &AppState,
    key: &DocumentKey,
) -> Result<Arc<DocumentState>, (StatusCode, Json<ErrorResponse>)> {
    let docs = state.documents.read().await;
    docs.get(key)
        .filter(|document| document.tenant_id == key.tenant_id)
        .cloned()
        .ok_or_else(document_not_found)
}

// ─── 应用状态 ───────────────────────────────────────────────────────

/// 服务全局共享状态。
#[derive(Clone)]
pub struct AppState {
/// 全进程共享的审核并发额度，所有文档和阶段共同竞争。
    pub review_execution_limiter: Arc<crate::agents::execution_control::GlobalExecutionLimiter>,
    /// 文档缓存：(tenant_id, document_id) → 已处理文档
    pub documents: Arc<TokioRwLock<HashMap<DocumentKey, Arc<DocumentState>>>>,
    /// 嵌入客户端（BGE-M3，启动时加载一次）
    pub embed_client: Arc<StdMutex<Option<Arc<EmbeddingClient>>>>,
    /// DashScope 联网搜索
    pub dashscope_search: Option<Arc<DashScopeSearchBackend>>,
    /// 搜索后端类型（dashscope / searxng）
    pub search_backend: String,
    /// 嵌入引擎类型（local / remote）
    pub embed_engine: String,
/// SSE 实时推送通道：(tenant_id, document_id) → ReviewEventBus
    pub review_event_buses: Arc<TokioMutex<HashMap<DocumentKey, Arc<ReviewEventBus>>>>,
    /// 异步审查结果缓存：(tenant_id, document_id) → CoordinatorOutput
    pub review_results: Arc<TokioMutex<HashMap<DocumentKey, CoordinatorOutput>>>,
    /// 异步审查的 token/成本统计：(tenant_id, document_id) → ReviewUsage
    pub review_usages: Arc<TokioMutex<HashMap<DocumentKey, ReviewUsage>>>,
    /// 异步审查失败信息：(tenant_id, document_id) → 错误消息
    pub review_errors: Arc<TokioMutex<HashMap<DocumentKey, String>>>,
    /// 正在执行的审核任务：(tenant_id, document_id)（用于并发控制）
    pub active_reviews: Arc<TokioMutex<HashSet<DocumentKey>>>,
}

use std::sync::Mutex as StdMutex;

/// 单个文档的处理状态。
pub struct DocumentState {
    pub tenant_id: String,
    pub id: String,
    pub filename: String,
    pub stem: String,
    pub raw_doc: RawDocument,
    pub sections: Vec<Section>,
    pub chunks: Vec<Chunk>,
    /// 仅供远程模型/工具使用的脱敏条款副本。
    pub review_chunks: Vec<Chunk>,
    pub chunk_map: Arc<HashMap<String, Chunk>>,
    pub review_chunk_map: Arc<HashMap<String, Chunk>>,
    pub chunk_order: Arc<Vec<String>>,
    pub doc_index: Arc<DocumentVectorIndex>,
    pub redaction_vault: Arc<RedactionVault>,
    pub desensitization_summary: DesensitizationSummary,
}

impl AppState {
    /// 初始化全局状态。
    pub async fn init() -> anyhow::Result<Self> {
        let embed_engine = std::env::var("EMBED_ENGINE").unwrap_or_else(|_| "local".to_string());

        let embed_client = {
            let client = EmbeddingClient::from_env()?;
            Some(Arc::new(client))
        };

        let search_backend =
            std::env::var("AIBID_SEARCH_BACKEND").unwrap_or_else(|_| "dashscope".to_string());

        let dashscope_search = if search_backend == "dashscope" {
            DashScopeSearchBackend::from_env().map(Arc::new).ok()
        } else {
            None
        };

        Ok(Self {
            review_execution_limiter: Arc::new(
                crate::agents::execution_control::GlobalExecutionLimiter::from_env(),
            ),
            documents: Arc::new(TokioRwLock::new(HashMap::new())),
            embed_client: Arc::new(StdMutex::new(embed_client)),
            dashscope_search,
            search_backend,
            embed_engine,
            review_event_buses: Arc::new(TokioMutex::new(HashMap::new())),
            review_results: Arc::new(TokioMutex::new(HashMap::new())),
            review_usages: Arc::new(TokioMutex::new(HashMap::new())),
            review_errors: Arc::new(TokioMutex::new(HashMap::new())),
            active_reviews: Arc::new(TokioMutex::new(HashSet::new())),
        })
    }
}

/// 单文档「重启后可重建内存态」的持久化清单。
///
/// 补上进程内才有、此前未落盘的字段：原文件名、脱敏副本、脱敏映射。
/// raw_json / sections / chunks / embeddings 已各自落盘，由清单按 stem 串联恢复。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentManifest {
    pub document_id: String,
    pub filename: String,
    pub stem: String,
    pub desensitization_summary: DesensitizationSummary,
    pub review_chunks: Vec<Chunk>,
    pub redaction_vault: RedactionVault,
}

/// 从磁盘整体重建单个文档的内存态。任一份必要文件缺失/损坏即返回 None
/// （跳过该文档，不阻塞其余文档恢复）。
///
/// `data_root` 为 `output/tenants`，目录约定与 process_document 写盘一致：
/// ```text
/// {data_root}/{tenant}/{raw_json|sections|chunks}/{stem}_*.json
/// {data_root}/{tenant}/embeddings/{stem}_embedding_index/
/// {data_root}/{tenant}/documents/{stem}_manifest.json
/// ```
pub(crate) fn rebuild_document_state(
    data_root: &std::path::Path,
    tenant_id: &str,
    stem: &str,
) -> Option<DocumentState> {
    let tenant_dir = data_root.join(tenant_id);

    let manifest_json =
        std::fs::read_to_string(tenant_dir.join("documents").join(format!("{stem}_manifest.json")))
            .ok()?;
    let DocumentManifest {
        document_id,
        filename,
        stem: manifest_stem,
        desensitization_summary,
        review_chunks,
        redaction_vault,
    } = serde_json::from_str(&manifest_json).ok()?;

    let raw_path = tenant_dir.join("raw_json").join(format!("{stem}_raw.json"));
    let sections_path = tenant_dir.join("sections").join(format!("{stem}_sections.json"));
    let chunks_path = tenant_dir.join("chunks").join(format!("{stem}_chunks.json"));

    let raw_doc: RawDocument =
        serde_json::from_str(&std::fs::read_to_string(&raw_path).ok()?).ok()?;
    let sections: Vec<Section> =
        serde_json::from_str(&std::fs::read_to_string(&sections_path).ok()?).ok()?;
    let chunks: Vec<Chunk> =
        serde_json::from_str(&std::fs::read_to_string(&chunks_path).ok()?).ok()?;

    let embeddings_dir = tenant_dir.join("embeddings");
    let doc_index = crate::services::embedding_service::load_index(
        embeddings_dir.to_string_lossy().as_ref(),
        stem,
    )
    .ok()?;

    let chunk_map: HashMap<String, Chunk> = chunks
        .iter()
        .map(|c| (c.chunk_id.clone(), c.clone()))
        .collect();
    let review_chunk_map: HashMap<String, Chunk> = review_chunks
        .iter()
        .map(|c| (c.chunk_id.clone(), c.clone()))
        .collect();
    let chunk_order: Vec<String> = chunks.iter().map(|c| c.chunk_id.clone()).collect();

    Some(DocumentState {
        tenant_id: tenant_id.to_string(),
        id: document_id,
        filename,
        stem: manifest_stem,
        raw_doc,
        sections,
        chunks,
        review_chunks,
        chunk_map: Arc::new(chunk_map),
        review_chunk_map: Arc::new(review_chunk_map),
        chunk_order: Arc::new(chunk_order),
        doc_index: Arc::new(doc_index),
        redaction_vault: Arc::new(redaction_vault),
        desensitization_summary,
    })
}

impl AppState {
    /// 启动时扫描 `output/tenants/*/documents/*_manifest.json`，把已处理文档
    /// 从磁盘恢复到内存注册表，避免容器重启后文档「丢失」触发 Java 重传、
    /// 进而生成新 doc UUID 破坏 A/B 可比性。返回成功恢复的文档数。
    pub async fn reload_persisted_documents(&self) -> usize {
        let base = PathBuf::from(data_path_str("output/tenants"));
        let mut restored = 0usize;
        let Ok(tenants) = std::fs::read_dir(&base) else {
            return 0;
        };
        for tenant in tenants.flatten() {
            let tenant_id = tenant.file_name().to_string_lossy().to_string();
            if !is_valid_tenant_id(&tenant_id) {
                continue;
            }
            let docs_dir = tenant.path().join("documents");
            let Ok(manifests) = std::fs::read_dir(&docs_dir) else {
                continue;
            };
            for entry in manifests.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let Some(stem) = name.strip_suffix("_manifest.json") else {
                    continue;
                };
                if !is_safe_document_id(stem) {
                    continue;
                }
                if let Some(state) = rebuild_document_state(&base, &tenant_id, stem) {
                    let key = DocumentKey::new(tenant_id.clone(), state.id.clone());
                    self.documents.write().await.insert(key, Arc::new(state));
                    restored += 1;
                }
            }
        }
        restored
    }
}

// ─── 请求/响应 DTO ──────────────────────────────────────────────────

#[derive(Debug, Deserialize, ToSchema)]
pub struct ReviewRequest {
    #[serde(default)]
    pub chunk_ids: Vec<String>,
    #[serde(default)]
    pub max_clauses: Option<usize>,
    #[serde(default)]
    pub enabled_agents: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ChatRequest {
    pub user_input: String,
    #[serde(default)]
    pub selection: Option<TextSelection>,
    #[serde(default)]
    pub history: Option<Vec<ChatMessageDto>>,
    #[serde(default)]
    pub max_turns: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ChatMessageDto {
    pub role: String,
    pub content: Option<String>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SearchRequest {
    pub queries: Vec<String>,
    #[serde(default)]
    pub top_k: Option<usize>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ProcessResponse {
    pub document_id: String,
    pub filename: String,
    pub total_pages: usize,
    pub total_blocks: usize,
    pub total_sections: usize,
    pub total_chunks: usize,
    pub avg_chunk_size: f64,
    pub vector_count: usize,
    pub vector_dimension: usize,
    pub desensitization_mode: String,
    pub desensitized_items: usize,
    pub desensitization_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DocumentInfo {
    pub document_id: String,
    pub filename: String,
    pub total_pages: usize,
    pub total_chunks: usize,
    pub vector_count: usize,
    pub desensitization_mode: String,
    pub desensitized_items: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReviewAccepted {
    pub status: String,
    pub document_id: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ReviewResponse {
    pub document_id: String,
    pub findings: Vec<crate::agents::types::RiskFinding>,
    pub routing_summary: crate::agents::types::RoutingSummary,
    #[serde(default)]
    pub execution_summary: crate::agents::types::ExecutionSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot: Option<crate::agents::types::GraphSnapshot>,
}

/// 单份文档一次审核的 LLM 消耗与成本估算（benchmark / 前端统计用）。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReviewUsage {
    /// LLM 调用次数（该文档整次审核聚合）
    pub llm_calls: usize,
    /// 输入 token 总数（该文档整次审核聚合）
    pub tokens_input: u64,
    /// 输出 token 总数（该文档整次审核聚合）
    pub tokens_output: u64,
    /// 估算成本（CNY，按当前模型单价）
    pub cost_cny: f64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ReviewResultResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ReviewResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 该文档审核的 LLM token 消耗与成本估算（审核成功后提供）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ReviewUsage>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SearchResponse {
    pub results: Vec<SearchResultGroup>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SearchResultGroup {
    pub query: String,
    pub hits: Vec<SearchHitDto>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SearchHitDto {
    pub chunk_id: String,
    pub title: String,
    pub score: f32,
    pub snippet: String,
    pub page_start: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
    pub detail: String,
}

fn restore_chat_response(response: &mut ChatResponse, vault: &RedactionVault) {
    response.answer = vault.restore(&response.answer);
    response.reasoning = response
        .reasoning
        .iter()
        .map(|text| vault.restore(text))
        .collect();
    for reference in &mut response.references {
        reference.quote = vault.restore(&reference.quote);
        reference.snippet = vault.restore(&reference.snippet);
    }
    response.suggested_actions = response
        .suggested_actions
        .iter()
        .map(|text| vault.restore(text))
        .collect();
}

// ─── Handlers ───────────────────────────────────────────────────────

/// GET /health
#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Service is healthy", body = serde_json::Value)
    )
)]
pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

/// POST /api/v1/documents
#[utoipa::path(
    post,
    path = "/api/v1/documents",
    request_body(content = Vec<u8>, content_type = "multipart/form-data", description = "Document file (PDF/DOCX/DOC)"),
    responses(
        (status = 200, description = "Document processed successfully", body = ProcessResponse),
        (status = 400, description = "Empty file", body = ErrorResponse),
        (status = 500, description = "Processing failure", body = ErrorResponse)
    )
)]
pub async fn process_document(
    State(state): State<AppState>,
    Extension(context): Extension<InternalRequestContext>,
    mut multipart: Multipart,
) -> Result<Json<ProcessResponse>, (StatusCode, Json<ErrorResponse>)> {
    let tenant_id = context.tenant_id;
    if !is_valid_tenant_id(&tenant_id) {
        return Err(bad_request("tenant context is invalid"));
    }
    println!("[REQ] 收到文件上传请求，开始解析 multipart...");

    let mut file_data: Vec<u8> = Vec::new();
    let mut filename = String::from("upload.pdf");
    let mut desensitization_mode = DesensitizationMode::Low;

    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or_default().to_string();
        let field_filename = field.file_name().map(str::to_string);
        if let Some(name) = field_filename {
            filename = name;
            if let Ok(data) = field.bytes().await {
                file_data = data.to_vec();
            }
        } else if field_name == "desensitize_mode"
            && let Ok(value) = field.text().await
        {
            desensitization_mode = DesensitizationMode::parse(&value)
                .ok_or_else(|| bad_request("desensitize_mode 仅支持 off/low"))?;
        }
    }

    if file_data.is_empty() {
        return Err(bad_request("上传文件为空"));
    }

    println!(
        "[REQ] 收到文件上传: filename={}, size={} bytes",
        filename,
        file_data.len()
    );

    let tmp_dir = tenant_output_path(&tenant_id, "tmp")
        .ok_or_else(|| bad_request("tenant context is invalid"))?;
    std::fs::create_dir_all(&tmp_dir).map_err(|e| server_error("创建临时目录失败", e))?;
    let stem = Uuid::new_v4().to_string();
    let doc_id = stem.clone();
    let ext = std::path::Path::new(&filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("pdf");
    let tmp_path = tmp_dir.join(format!("{}.{}", stem, ext));
    std::fs::write(&tmp_path, &file_data).map_err(|e| server_error("写入临时文件失败", e))?;

    // DOCX → PDF 转换（对齐 CLI 行为）
    let pdf_path = if ext == "docx" || ext == "doc" {
        println!("[STAGE] DOCX → PDF 转换...");
        convert_docx_to_pdf(
            tmp_path.to_string_lossy().as_ref(),
            tmp_dir.to_string_lossy().as_ref(),
        )
        .map_err(|e| server_error("DOCX 转 PDF 失败", e))?
    } else {
        tmp_path.clone()
    };

    // 阶段 1: PDF → RawDocument（Rust 主路径 + Python 兜底）
    println!("[STAGE] PDF 文本提取...");
    let pdf_path_str = pdf_path.to_string_lossy().to_string();
    let raw_doc: RawDocument = match extract_pdf_to_raw_json(&pdf_path_str) {
        Ok(doc) => {
            println!("Rust pdfplumber 解析成功");
            doc
        }
        Err(e) => {
            println!("Rust pdfplumber 失败: {}", e);
            println!("切换到 Python pdfplumber 兜底提取...");
            let fallback_json = tmp_dir.join(format!("{}_python_fallback_raw.json", stem));
            extract_with_python(&pdf_path_str, &fallback_json.to_string_lossy())
                .map_err(|e2| server_error("PDF 解析失败（Rust 和 Python 均失败）", e2))?;
            let json_str = std::fs::read_to_string(&fallback_json)
                .map_err(|e2| server_error("读取 Python 兜底 JSON 失败", e2))?;
            serde_json::from_str(&json_str)
                .map_err(|e2| server_error("解析 Python 兜底 JSON 失败", e2))?
        }
    };
    println!(
        "[STAGE] 提取完成: {} 页, {} 个文本块",
        raw_doc.pages.len(),
        raw_doc.pages.iter().map(|p| p.blocks.len()).sum::<usize>()
    );

    // 构建磁盘输出用的安全 stem：只使用服务端生成的 document_id。
    let disk_stem = doc_id.clone();

    // ── 写盘：raw_json ──
    {
        let dir = tenant_output_path(&tenant_id, "raw_json")
            .ok_or_else(|| bad_request("tenant context is invalid"))?;
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{}_raw.json", disk_stem));
        if let Ok(json) = serde_json::to_string_pretty(&raw_doc) {
            let _ = std::fs::write(&path, json);
            println!("[DISK] raw_json → {}", path.display());
        }
    }

    // 阶段 2: RawDocument → Sections
    let sections_output = sectionize_service::sectionize(&raw_doc);
    let mut raw_doc_mut = {
        // Re-serialize and deserialize to get a mutable copy
        // (RawDocument doesn't implement Clone)
        let json = serde_json::to_value(&raw_doc)
            .map_err(|e| server_error("序列化 RawDocument 失败", e))?;
        serde_json::from_value(json).map_err(|e| server_error("反序列化 RawDocument 失败", e))?
    };
    sectionize_service::detect_pipe_tables(&mut raw_doc_mut);

    let assigned: HashSet<&str> = sections_output
        .sections
        .iter()
        .flat_map(|s| collect_all_block_ids(s))
        .collect();
    let orphan_blocks: Vec<&crate::domain::raw_document::RawBlock> = raw_doc_mut
        .pages
        .iter()
        .flat_map(|p| p.blocks.iter())
        .filter(|b| !assigned.contains(b.id.as_str()))
        .collect();

    let mut all_sections = sections_output.sections.clone();
    if !orphan_blocks.is_empty() {
        let block_page: HashMap<&str, usize> = raw_doc_mut
            .pages
            .iter()
            .flat_map(|p| p.blocks.iter().map(move |b| (b.id.as_str(), p.page_index)))
            .collect();
        let mut page_to_blocks: BTreeMap<usize, Vec<&crate::domain::raw_document::RawBlock>> =
            BTreeMap::new();
        for block in &orphan_blocks {
            if let Some(&page_idx) = block_page.get(block.id.as_str()) {
                page_to_blocks.entry(page_idx).or_default().push(*block);
            }
        }
        let sorted_pages: Vec<usize> = page_to_blocks.keys().copied().collect();
        let mut page_groups: Vec<Vec<usize>> = Vec::new();
        let mut current_group: Vec<usize> = Vec::new();
        for &p in &sorted_pages {
            if current_group.is_empty() || p == current_group.last().unwrap() + 1 {
                current_group.push(p);
            } else {
                page_groups.push(std::mem::take(&mut current_group));
                current_group.push(p);
            }
        }
        if !current_group.is_empty() {
            page_groups.push(current_group);
        }
        for group in &page_groups {
            let group_start = *group.first().unwrap();
            let group_end = *group.last().unwrap();
            let group_blocks: Vec<&&crate::domain::raw_document::RawBlock> = group
                .iter()
                .flat_map(|p| page_to_blocks[p].iter())
                .collect();
            let orphan_ids: Vec<String> = group_blocks.iter().map(|b| b.id.clone()).collect();
            let orphan_text = group_blocks
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            all_sections.push(Section {
                level: 0,
                title: format!("未归类内容 (第{}-{}页)", group_start + 1, group_end + 1),
                pattern: "orphan".to_string(),
                page_start: group_start,
                page_end: group_end,
                block_ids: orphan_ids,
                body_text: orphan_text,
                children: Vec::new(),
                body_page_start: group_start,
                body_page_end: group_end,
            });
        }
    }

    sectionize_service::merge_cross_page_tables(&mut raw_doc_mut);
    sectionize_service::inject_tables_into_sections(&mut all_sections, &raw_doc_mut);

    // ── 写盘：sections ──
    {
        let dir = tenant_output_path(&tenant_id, "sections")
            .ok_or_else(|| bad_request("tenant context is invalid"))?;
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{}_sections.json", disk_stem));
        if let Ok(json) = serde_json::to_string_pretty(&all_sections) {
            let _ = std::fs::write(&path, json);
            println!("[DISK] sections → {}", path.display());
        }
    }

    // 阶段 3: Sections → Chunks
    println!("[STAGE] 章节 → 条款切分 ({} 个章节)...", all_sections.len());
    let chunking_config = ChunkingConfig::default();
    let mut chunks = chunk_sections(&all_sections, &chunking_config);
    crate::services::chunking_service::populate_bbox_refs(&mut chunks, &raw_doc);
    println!("[STAGE] 切分完成: {} 个条款块", chunks.len());

    // 原文与云端审核文本双视图。原始 chunks 只留在本地用于定位和最终展示；
    // review_chunks、远程向量和 read_section 均只包含脱敏文本。
    let mut redaction_vault = RedactionVault::new(desensitization_mode);
    let mut review_chunks = chunks.clone();
    for chunk in &mut review_chunks {
        chunk.text = redaction_vault.redact(&chunk.text);
        chunk.section_path = chunk
            .section_path
            .iter()
            .map(|part| redaction_vault.redact(part))
            .collect();
    }
    let desensitization_summary = redaction_vault.summary();
    println!(
        "[STAGE] 文档脱敏: mode={:?}, replacements={}",
        desensitization_summary.mode, desensitization_summary.total_replacements
    );

    // ── 写盘：chunks ──
    {
        let dir = tenant_output_path(&tenant_id, "chunks")
            .ok_or_else(|| bad_request("tenant context is invalid"))?;
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{}_chunks.json", disk_stem));
        if let Ok(json) = serde_json::to_string_pretty(&chunks) {
            let _ = std::fs::write(&path, json);
            println!("[DISK] chunks → {}", path.display());
        }
    }

    // 阶段 4: Chunks → Embeddings
    println!("[STAGE] 生成嵌入向量 (引擎: {})...", state.embed_engine);
    let doc_index = if state.embed_engine == "remote" {
        let api_client = crate::services::embedding_api_client::EmbeddingApiClient::from_env()
            .map_err(|e| server_error("嵌入 API 客户端初始化失败", e))?;
        crate::services::embedding_service::embed_chunks_remote(
            &review_chunks,
            &chunking_config,
            &sections_output.document_id,
            &api_client,
        )
    } else {
        crate::services::embedding_service::embed_chunks_parallel(
            &review_chunks,
            &chunking_config,
            &sections_output.document_id,
            2,
        )
    }
    .map_err(|e| server_error("嵌入生成失败", e))?;

    let vector_count = doc_index.len();
    let vector_dimension = doc_index.embeddings.first().map(|v| v.len()).unwrap_or(0);

    // ── 写盘：embeddings ──
    {
        let dir = tenant_output_path(&tenant_id, "embeddings")
            .ok_or_else(|| bad_request("tenant context is invalid"))?;
        if let Err(e) = crate::services::embedding_service::save_index(
            &doc_index,
            dir.to_string_lossy().as_ref(),
            &disk_stem,
        ) {
            eprintln!("[DISK] embeddings 写入失败: {}", e);
        } else {
            println!(
                "[DISK] embeddings → {}/{}_embedding_index/",
                dir.display(),
                disk_stem
            );
        }
    }

    // ── 写盘：documents manifest（供重启后恢复内存文档注册表）──
    {
        let dir = tenant_output_path(&tenant_id, "documents")
            .ok_or_else(|| bad_request("tenant context is invalid"))?;
        let _ = std::fs::create_dir_all(&dir);
        let manifest = DocumentManifest {
            document_id: doc_id.clone(),
            filename: filename.clone(),
            stem: disk_stem.clone(),
            desensitization_summary: desensitization_summary.clone(),
            review_chunks: review_chunks.clone(),
            redaction_vault: redaction_vault.clone(),
        };
        let path = dir.join(format!("{}_manifest.json", disk_stem));
        if let Ok(json) = serde_json::to_string_pretty(&manifest) {
            let _ = std::fs::write(&path, json);
            println!("[DISK] manifest → {}", path.display());
        }
    }

    let chunk_map: HashMap<String, Chunk> = chunks
        .iter()
        .map(|c| (c.chunk_id.clone(), c.clone()))
        .collect();
    let review_chunk_map: HashMap<String, Chunk> = review_chunks
        .iter()
        .map(|c| (c.chunk_id.clone(), c.clone()))
        .collect();
    let chunk_order: Vec<String> = chunks.iter().map(|c| c.chunk_id.clone()).collect();

    let total_pages = raw_doc.pages.len();
    let total_blocks: usize = raw_doc.pages.iter().map(|p| p.blocks.len()).sum();
    let total_chars: usize = chunks.iter().map(|c| c.text.len()).sum();

    let doc_state = Arc::new(DocumentState {
        tenant_id: tenant_id.clone(),
        id: doc_id.clone(),
        filename: filename.clone(),
        stem,
        raw_doc,
        sections: all_sections,
        chunks: chunks.clone(),
        review_chunks,
        chunk_map: Arc::new(chunk_map),
        review_chunk_map: Arc::new(review_chunk_map),
        chunk_order: Arc::new(chunk_order),
        doc_index: Arc::new(doc_index),
        redaction_vault: Arc::new(redaction_vault),
        desensitization_summary: desensitization_summary.clone(),
    });
    state
        .documents
        .write()
        .await
        .insert(DocumentKey::new(tenant_id, doc_id.clone()), doc_state);

    let _ = std::fs::remove_file(&tmp_path);

    println!(
        "[OK] 文档处理完成: doc_id={}, pages={}, chunks={}, vectors={}d",
        doc_id,
        total_pages,
        chunks.len(),
        vector_dimension
    );

    Ok(Json(ProcessResponse {
        document_id: doc_id,
        filename,
        total_pages,
        total_blocks,
        total_sections: sections_output.stats.total_sections,
        total_chunks: chunks.len(),
        avg_chunk_size: if chunks.is_empty() {
            0.0
        } else {
            total_chars as f64 / chunks.len() as f64
        },
        vector_count,
        vector_dimension,
        desensitization_mode: format!("{:?}", desensitization_summary.mode).to_ascii_lowercase(),
        desensitized_items: desensitization_summary.total_replacements,
        desensitization_counts: desensitization_summary.counts,
    }))
}

/// GET /api/v1/documents/:id
#[utoipa::path(
    get,
    path = "/api/v1/documents/{id}",
    params(
        ("id" = String, Path, description = "Document UUID")
    ),
    responses(
        (status = 200, description = "Document info", body = DocumentInfo),
        (status = 404, description = "Document not found", body = ErrorResponse)
    )
)]
pub async fn get_document(
    State(state): State<AppState>,
    Extension(context): Extension<InternalRequestContext>,
    Path(doc_id): Path<String>,
) -> Result<Json<DocumentInfo>, (StatusCode, Json<ErrorResponse>)> {
    let key = document_key(&context, &doc_id)?;
    let docs = state.documents.read().await;
    let doc = docs
        .get(&key)
        .filter(|document| document.tenant_id == key.tenant_id)
        .ok_or_else(document_not_found)?;
    Ok(Json(DocumentInfo {
        document_id: doc.id.clone(),
        filename: doc.filename.clone(),
        total_pages: doc.raw_doc.pages.len(),
        total_chunks: doc.chunks.len(),
        vector_count: doc.doc_index.len(),
        desensitization_mode: format!("{:?}", doc.desensitization_summary.mode)
            .to_ascii_lowercase(),
        desensitized_items: doc.desensitization_summary.total_replacements,
    }))
}

/// POST /api/v1/documents/:id/review
///
/// 启动异步 Multi-Agent 审查管线，立即返回 202 Accepted。
/// 审查在后台 Tokio task 中执行，通过 SSE (`GET /review/:doc_id/stream`)
/// 实时推送进度事件，完成后通过 `GET /review/:doc_id/result` 获取结果。
#[utoipa::path(
    post,
    path = "/api/v1/documents/{id}/review",
    params(
        ("id" = String, Path, description = "Document UUID")
    ),
    request_body = ReviewRequest,
    responses(
        (status = 202, description = "Review accepted", body = ReviewAccepted),
        (status = 400, description = "Invalid review parameters", body = ErrorResponse),
        (status = 404, description = "Document not found", body = ErrorResponse),
        (status = 409, description = "Review already in progress", body = ReviewAccepted)
    )
)]
#[axum::debug_handler]
pub async fn review_document(
    State(state): State<AppState>,
    Extension(context): Extension<InternalRequestContext>,
    Path(doc_id): Path<String>,
    Json(req): Json<ReviewRequest>,
) -> Result<(StatusCode, Json<ReviewAccepted>), (StatusCode, Json<ErrorResponse>)> {
    let key = document_key(&context, &doc_id)?;
    let docs = state.documents.read().await;
    let doc = docs
        .get(&key)
        .filter(|document| document.tenant_id == key.tenant_id)
        .ok_or_else(document_not_found)?
        .clone();
    drop(docs);

    // Agent 选择属于公开请求契约，必须在提交后台任务前完整校验。
    let enabled_agents = if let Some(agent_names) = req.enabled_agents.as_ref() {
        let mut parsed_agents = Vec::with_capacity(agent_names.len());
        for agent_name in agent_names {
            let agent_id = AgentId::parse(agent_name)
                .ok_or_else(|| bad_request(&format!("非法 Agent 名称: {}", agent_name)))?;
            parsed_agents.push(agent_id);
        }
        Some(parsed_agents)
    } else {
        None
    };

    println!(
        "[REQ] 启动异步审核: doc_id={}, filename={}",
        doc_id, doc.filename
    );

// 准备 clause 列表。
    //
    // chunk_ids / max_clauses 是公开 API 契约的一部分，基准测试和故障重试
    // 都依赖它们来限定审查范围。此前这里无条件审查 doc.chunks，导致请求
    // 参数被静默忽略，也会让小范围验收产生不必要的模型调用。
    let chunking_config = ChunkingConfig::default();
    let requested_chunk_ids: HashSet<&str> = req.chunk_ids.iter().map(String::as_str).collect();
    let mut selected_chunks: Vec<&Chunk> = doc
        .review_chunks
        .iter()
        .filter(|chunk| {
            requested_chunk_ids.is_empty() || requested_chunk_ids.contains(chunk.chunk_id.as_str())
        })
        .collect();

    if !req.chunk_ids.is_empty() && selected_chunks.is_empty() {
        return Err(bad_request("chunk_ids 未匹配到任何文档条款"));
    }
    if let Some(limit) = req.max_clauses {
        if limit == 0 {
            return Err(bad_request("max_clauses 必须大于 0"));
        }
        selected_chunks.truncate(limit);
    }

    let review_clauses: Vec<ReviewClause> = selected_chunks
        .into_iter()
        .map(|c| {
            ReviewClause::from_chunk(
                c,
                chunking_config.embed_ctx_depth,
                chunking_config.embed_path_max_len,
            )
        })
        .collect();

    // 参数校验完成后再原子占用审核锁，非法请求不得污染任务状态。
    {
        let mut active = state.active_reviews.lock().await;
        if active.contains(&key) {
            return Ok((
                StatusCode::CONFLICT,
                Json(ReviewAccepted {
                    status: "conflict".to_string(),
                    document_id: doc_id,
                    message: "该文档已有进行中的审核任务".to_string(),
                }),
            ));
        }
        active.insert(key.clone());
    }

    // 落盘"审核进行中"状态：进程重启后 get_review_result 可据此识别中断，
    // 让 Java 侧快速失败而不是盲等超时。
    if let Some(findings_dir) = tenant_output_path(&key.tenant_id, "findings") {
        let _ = std::fs::create_dir_all(&findings_dir);
        if let Err(e) = crate::api::review_state::write_running(
            &findings_dir,
            &doc_id,
            || chrono::Utc::now().to_rfc3339(),
        ) {
            eprintln!("[WARN] 审核状态落盘失败: doc_id={}, {}", doc_id, e);
        }
    }

    // 创建或获取 ReviewEventBus（SSE 客户端可能已提前连接）。
    let review_events = {
        let mut buses = state.review_event_buses.lock().await;
        buses
            .entry(key.clone())
            .or_insert_with(|| Arc::new(ReviewEventBus::new(review_event_capacity())))
            .clone()
    };

    println!(
        "[REQ] 审核条款数: {}, 启用 Agent: {:?}",
        review_clauses.len(),
        req.enabled_agents
    );

    // 提取后台任务所需数据（脱离 doc 引用）
    let chunk_map = doc.chunk_map.clone();
    let review_chunk_map = doc.review_chunk_map.clone();
    let doc_index = doc.doc_index.clone();
    let chunk_order = doc.chunk_order.clone();
    let redaction_vault = doc.redaction_vault.clone();
    let dashscope_search = state.dashscope_search.clone();
    let search_backend = state.search_backend.clone();
    let embed_client_for_tools = {
        let ec = state.embed_client.lock().unwrap();
        ec.clone()
    };

    // 提取每页 word 级坐标（审核完成后做 source_quote → 词级精确高亮）。
    // 键为 0-based 页码，值为 (页宽 pt, 阅读顺序的 (词文本, bbox) 列表)。
    let page_words: Arc<HashMap<usize, (f64, Vec<(String, BBox)>)>> = Arc::new(
        doc.raw_doc
            .pages
            .iter()
            .map(|p| {
                (
                    p.page_index,
                    (
                        p.width,
                        p.words
                            .iter()
                            .map(|w| (w.text.clone(), w.bbox.clone()))
                            .collect(),
                    ),
                )
            })
            .collect(),
    );

    // 后台执行管线
    let state_for_task = state.clone();
    let document_key_for_task = key.clone();
    tokio::spawn(async move {
        run_review_pipeline(
            state_for_task,
            document_key_for_task,
            review_clauses,
            enabled_agents,
            chunk_map,
            review_chunk_map,
            doc_index,
            chunk_order,
            redaction_vault,
            dashscope_search,
            search_backend,
            embed_client_for_tools,
            page_words,
            review_events,
        )
        .await;
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(ReviewAccepted {
            status: "accepted".to_string(),
            document_id: doc_id,
            message: "审核任务已提交，通过 SSE 获取实时进度".to_string(),
        }),
    ))
}

/// 后台执行 Multi-Agent 审核管线。
///
/// 成功时：存储结果到 `review_results`，发送 Done SSE 事件。
/// 失败时：存储错误到 `review_errors`，发送 Error SSE 事件。
#[allow(clippy::too_many_arguments)]
async fn run_review_pipeline(
    state: AppState,
    document_key: DocumentKey,
    review_clauses: Vec<ReviewClause>,
    enabled_agents: Option<Vec<AgentId>>,
    chunk_map: Arc<HashMap<String, Chunk>>,
    review_chunk_map: Arc<HashMap<String, Chunk>>,
    doc_index: Arc<DocumentVectorIndex>,
    chunk_order: Arc<Vec<String>>,
    redaction_vault: Arc<RedactionVault>,
    dashscope_search: Option<Arc<DashScopeSearchBackend>>,
    search_backend: String,
    embed_client_for_tools: Option<Arc<EmbeddingClient>>,
    page_words: Arc<HashMap<usize, (f64, Vec<(String, BBox)>)>>,
    review_events: Arc<ReviewEventBus>,
) {
    let tenant_id = document_key.tenant_id.clone();
    let doc_id = document_key.document_id.clone();
    let start_time = std::time::Instant::now();

    // ── 指标采集器 ──
    let llm_model = std::env::var("DASHSCOPE_MODEL")
        .unwrap_or_else(|_| std::env::var("LLM_MODEL").unwrap_or_else(|_| "qwen-plus".to_string()));
    let metrics: Arc<tokio::sync::Mutex<crate::metrics::MetricsCollector>> =
        Arc::new(tokio::sync::Mutex::new(
            crate::metrics::MetricsCollector::new(crate::metrics::SCHEMA_VERSION, &llm_model),
        ));

    let bus = Arc::new(AgentBus::new(32));
    let graph = Arc::new(SessionGraph::new());
    let trace = Arc::new(TokioMutex::new(TraceLog::new()));

    let mut coord_config = CoordinatorConfig::default();
    if let Some(agent_ids) = enabled_agents {
        coord_config.enabled_agents = agent_ids;
    }
    // P2 A/B 开关：同一套流程，on/off 只差独白压缩。分组写入 run meta 便于对比。
    let transcript_compression = transcript_compression_enabled();
    coord_config.transcript_compression = transcript_compression;

    let llm_factory = Arc::new(move || create_llm_client().expect("创建 LLM 客户端失败"));

    let doc_index_for_tools = doc_index.clone();
    let chunk_map_for_tools = review_chunk_map.clone();
    let chunk_order_for_tools = chunk_order.clone();
    let ds_search = dashscope_search.clone();
    let sb = search_backend.clone();
    let ec_for_tools = embed_client_for_tools.clone();

    let tools_factory = Arc::new(move || {
        eprintln!("[handlers] ── 创建 Agent 工具集 ToolRegistry ──");
        let mut registry = ToolRegistry::new();
        if let Some(ref ec) = ec_for_tools {
            registry.register(Box::new(SearchDocumentTool::new(
                doc_index_for_tools.clone(),
                ec.clone(),
            )));
        }
        registry.register(Box::new(ReadSectionTool::new(
            chunk_map_for_tools.clone(),
            chunk_order_for_tools.clone(),
        )));
        if sb == "dashscope"
            && let Some(ref ds) = ds_search
        {
            registry.register(Box::new(SearchKnowledgeTool::with_dashscope(ds.clone())));
        }
        // 本地知识库检索（与入库共享 EmbeddingClient，保证向量空间一致）
        if let Some(ref ec) = ec_for_tools {
            registry.register(Box::new(SearchKnowledgeBaseTool::new(ec.clone())));
        }
        registry.register(Box::new(OutputFindingTool));
        // V2+ 工具（需要 chunk 数据）
        registry.register(Box::new(CompareVersionsTool {
            current_chunks: chunk_map_for_tools.clone(),
            current_order: chunk_order_for_tools.clone(),
        }));
        registry.register(Box::new(DetectBoilerplateTool {
            chunks: chunk_map_for_tools.clone(),
            chunk_order: chunk_order_for_tools.clone(),
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
        // 依赖 chunk 数据的工具
        registry.register(Box::new(CheckCrossReferenceTool::new(
            chunk_map_for_tools.clone(),
            chunk_order_for_tools.clone(),
        )));
        registry.register(Box::new(ExtractObligationsTool::new(
            chunk_map_for_tools.clone(),
            chunk_order_for_tools.clone(),
        )));
        // 模板比对（需要 ChunkTextProvider）
        let template_text_provider = Arc::new(ChunkTextProvider {
            chunks: chunk_map_for_tools.clone(),
        });
        registry.register(Box::new(CompareWithTemplateTool::new(
            Arc::new(TemplateStore::with_builtin_templates()),
            template_text_provider,
        )));
        // 数值计算校验
        registry.register(Box::new(ValidateCalculationTool));
        // 矛盾检测
        registry.register(Box::new(SearchContradictionTool::new(
            chunk_map_for_tools.clone(),
            chunk_order_for_tools.clone(),
            None,
        )));
        eprintln!(
            "[handlers] ── 工具集注册完成: 共 {} 个工具 ──",
            registry.len()
        );
        registry
    });

    let registry = AgentRegistry::builtin();
    let coord_agent_count = coord_config.enabled_agents.len();
    let coord_max_parallel = coord_config.max_parallel_clauses;
    let coordinator = Arc::new(
        Coordinator::new(
            coord_config,
            registry,
            llm_factory,
            tools_factory,
            bus,
            graph,
            trace,
        )
        .with_global_execution_limiter(state.review_execution_limiter.clone())
        .with_review_events(review_events.clone())
        .with_metrics(metrics.clone()),
    );

    println!("[STAGE] Multi-Agent 审核中 (async)...");
    match coordinator.review(&review_clauses).await {
        Ok(mut output) => {
            let duration_secs = start_time.elapsed().as_secs_f64();
            let result_status = output.execution_summary.status.as_str().to_string();
            println!(
                "[OK] 审核完成: {} 条风险发现, 耗时 {:.1}s",
                output.findings.len(),
                duration_secs
            );

            // ★ BlindSpot: 后台异步执行（不阻塞 HTTP 响应）
            let coord_bg = coordinator.clone();
            tokio::spawn(async move {
                coord_bg.run_blind_spot().await;
            });

            // 模型只接触脱敏文本。结果回到本地后恢复原文展示，再填充原始定位。
            for finding in &mut output.findings {
                finding.source_quote = redaction_vault.restore(&finding.source_quote);
                finding.reason = redaction_vault.restore(&finding.reason);
                finding.suggestion = redaction_vault.restore(&finding.suggestion);
                finding.critical_reason = redaction_vault.restore(&finding.critical_reason);
                finding.legal_basis = finding
                    .legal_basis
                    .iter()
                    .map(|item| redaction_vault.restore(item))
                    .collect();
                if let Some(first_clause_id) = finding.clause_ids.first()
                    && let Some(chunk) = chunk_map.get(first_clause_id)
                {
                    finding.page_number = Some(chunk.page_start + 1);
                    finding.section_path = Some(chunk.section_path.clone());
                    finding.context = Some(chunk.text.chars().take(500).collect());

                    // 过滤 block_ids：只保留验证过的非占位 bbox 的 block，
                    // 避免整页高亮导致"框太大"问题。
                    // 占位 bbox 来自 blocks_from_text()（lopdf 失败降级路径），
                    // 特征是 x0==0.0 && x1==400.0 且高度 ≤20pt。
                    //
                    // 同时按 block 真实文本长度累加，估算每个 block 在
                    // chunk.text 中的字符偏移（用 block 中心位置代表该 block），
                    // 替代按 index 比例估算——后者在 block 长度差异大时偏移严重。
                    let source_quote = finding.source_quote.clone();
let max_blocks = 5usize;
                    let mut valid_blocks: Vec<(String, usize)> = Vec::new();
                    let mut offset_acc = 0usize;
                    for r in &chunk.bbox_refs {
                        let is_placeholder =
                            r.bbox.x0 == 0.0 && r.bbox.x1 == 400.0
                                && (r.bbox.bottom - r.bbox.top) <= 20.1;
                        if !is_placeholder {
                            // 用 block 中心偏移代表其位置，避免长 block 的首字符偏移
                            // 无法覆盖落在 block 中后段的证据。
                            valid_blocks
                                .push((r.block_id.clone(), offset_acc + r.char_count / 2));
                        }
                        offset_acc += r.char_count;
                    }

                    // 统一走可靠性匹配：source_quote 匹配不可靠时返回空，
                    // 让前端走文本定位（不再区分「块多/块少」两条路径）。
                    finding.block_ids = select_blocks_by_source_quote(
                        &valid_blocks,
                        &source_quote,
                        &chunk.text,
                        max_blocks,
                    );

                    // ★ 词级精确高亮：用同一份 source_quote 反查该页的词坐标，
                    //   返回紧贴命中原句的逐行矩形（比段落级 block 更精确）。
                    //   chunk.page_start 为 0-based，与 page_words 键一致。
                    finding.highlight_rects = page_words
                        .get(&chunk.page_start)
                        .map(|(width, words)| {
                            match_words_to_quote(words, &source_quote)
                                .into_iter()
                                .map(|bbox| HighlightRect {
                                    page: chunk.page_start,
                                    x0: bbox.x0,
                                    top: bbox.top,
                                    x1: bbox.x1,
                                    bottom: bbox.bottom,
                                    page_width: *width,
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                }
            }
            let findings_with_blocks = output
                .findings
                .iter()
                .filter(|f| !f.block_ids.is_empty())
                .count();
            println!(
                "[OK] block_ids 已填充: {}/{} 条 finding 携带 block 引用",
                findings_with_blocks,
                output.findings.len()
            );

            let high_risk_count = output
                .findings
                .iter()
                .filter(|f| f.severity == crate::agents::types::RiskSeverity::High)
                .count();

            // ── 写盘：findings ──
            {
                let disk_stem = doc_id.clone();
                let Some(dir) = tenant_output_path(&tenant_id, "findings") else {
                    return;
                };
                let _ = std::fs::create_dir_all(&dir);
                let findings_path = dir.join(format!("{}_findings.json", disk_stem));
                if let Ok(json) = serde_json::to_string_pretty(&output.findings) {
                    let _ = std::fs::write(&findings_path, json);
                    println!("[DISK] findings → {}", findings_path.display());
                }
                let summary_path = dir.join(format!("{}_routing_summary.json", disk_stem));
                if let Ok(json) = serde_json::to_string_pretty(&output.routing_summary) {
                    let _ = std::fs::write(&summary_path, json);
                }
                if let Some(ref snap) = output.graph_snapshot {
                    let snap_path = dir.join(format!("{}_graph_snapshot.json", disk_stem));
                    if let Ok(json) = serde_json::to_string_pretty(snap) {
                        let _ = std::fs::write(&snap_path, json);
                    }
                }
            }

// ── 指标：finalize（拿到 token/成本 totals，构造 usage）──
            let usage = {
                let mut collector = metrics.lock().await;
                collector.set_findings_detail(&output.findings);
                collector.record_stage(
                    crate::metrics::SemanticStage::AgentReview,
                    (duration_secs * 1000.0) as u64,
                    crate::metrics::StageDetail::AgentReview {
                        clause_count: review_clauses.len(),
                        coordinator_phases: None,
                    },
                );

                let run_id = format!(
                    "{}-{}",
                    chrono::Local::now().format("%Y%m%dT%H%M%S"),
                    &Uuid::new_v4().to_string()[..8]
                );
                let meta = crate::metrics::RunMeta {
                    run_id: run_id.clone(),
                    title: None,
                    notes: None,
                    experiment_group: Some(
                        if transcript_compression {
                            "transcript_compress"
                        } else {
                            "control"
                        }
                        .to_string(),
                    ),
                    timestamp: chrono::Local::now().to_rfc3339(),
                    git_commit: "unknown".to_string(),
                    git_branch: "unknown".to_string(),
                    tags: vec!["http".to_string()],
                    description: format!("HTTP review: {}", doc_id),
                    document: crate::metrics::schema::DocumentInfo {
                        name: doc_id.clone(),
                        pages: 0,
                        file_size_kb: 0,
                    },
                    config: crate::metrics::schema::RunConfig {
                        coordinator_enabled: true,
                        agent_count: coord_agent_count,
                        embed_engine: "unknown".to_string(),
                        llm_model,
                        search_backend: search_backend.clone(),
                        max_parallel_clauses: coord_max_parallel,
                        transcript_compression,
                    },
                };
                let run_metrics = collector.finalize(meta);

                let Some(runs_dir) = metric_runs_path(&tenant_id, "") else {
                    return;
                };
                let _ = std::fs::create_dir_all(&runs_dir);
                let run_path = runs_dir.join(format!("{}.json", run_id));
                if let Ok(json) = serde_json::to_string_pretty(&run_metrics) {
                    let _ = std::fs::write(&run_path, json);
                    println!("[METRICS] → {}", run_path.display());
                }

                let totals = &run_metrics.llm_efficiency.totals;
                ReviewUsage {
                    llm_calls: totals.llm_calls,
                    tokens_input: totals.tokens_input,
                    tokens_output: totals.tokens_output,
                    cost_cny: totals.cost_cny,
                }
            };

            // 存入 review_results + review_usages 供 GET /result 查询
            {
                let mut results = state.review_results.lock().await;
                results.insert(document_key.clone(), output.clone());
                let mut usages = state.review_usages.lock().await;
                usages.insert(document_key.clone(), usage.clone());
            }

            // 写盘: {doc_id}_result.json — 重启后磁盘 fallback（含 usage，租户命名空间）
            {
                let Some(dir) = tenant_output_path(&tenant_id, "findings") else {
                    return;
                };
                let _ = std::fs::create_dir_all(&dir);
                let result_path = dir.join(format!("{}_result.json", doc_id));
                let persisted = ReviewResultResponse {
                    status: result_status.clone(),
                    result: Some(ReviewResponse {
                        document_id: doc_id.clone(),
                        findings: output.findings.clone(),
                        routing_summary: output.routing_summary.clone(),
                        execution_summary: output.execution_summary.clone(),
                        graph_snapshot: output.graph_snapshot.clone(),
                    }),
                    usage: Some(usage.clone()),
                    error: None,
                };
                if let Ok(json) = serde_json::to_string_pretty(&persisted) {
                    let _ = std::fs::write(&result_path, json);
                    println!("[DISK] result → {}", result_path.display());
                }
                // 结果已落盘，清除"进行中"状态文件
                crate::api::review_state::remove(&dir, &doc_id);
            }

            if output.execution_summary.status
                == crate::agents::types::ReviewExecutionStatus::PartialFailed
            {
                review_events.emit(&crate::agents::review_event::ReviewEvent::PartialDone {
                    total_findings: output.findings.len(),
                    high_risk: high_risk_count,
                    session_id: doc_id.clone(),
                    duration_secs,
                    failed_agents: output.execution_summary.failed_agents.clone(),
                    failed_clauses: output.execution_summary.failed_clauses.clone(),
                    failed_stages: output.execution_summary.failed_stages.clone(),
                    budget: output.execution_summary.budget.clone(),
                });
            } else {
                review_events.emit(&crate::agents::review_event::ReviewEvent::Done {
                    total_findings: output.findings.len(),
                    high_risk: high_risk_count,
                    session_id: doc_id.clone(),
                    duration_secs,
                });
            }
        }
        Err(e) => {
            let msg = format!("审核引擎执行失败: {}", e);
            eprintln!("[ERROR] async review failed: doc_id={}, {}", doc_id, msg);

            // 存入 review_errors
            {
                let mut errors = state.review_errors.lock().await;
                errors.insert(document_key.clone(), msg.clone());
            }

            // 落盘失败状态：重启后 get_review_result 可透传原始失败原因
            if let Some(dir) = tenant_output_path(&tenant_id, "findings") {
                let _ = std::fs::create_dir_all(&dir);
                let _ = crate::api::review_state::write_failed(
                    &dir,
                    &doc_id,
                    &msg,
                    || chrono::Utc::now().to_rfc3339(),
                );
            }

            // 发送 Error 事件
            review_events.emit(&crate::agents::review_event::ReviewEvent::Error {
                message: msg,
                session_id: doc_id.clone(),
            });
        }
    }

    // 延迟清理 ReviewEventBus 和 active_reviews
// （给 SSE 客户端时间接收 Done/PartialDone/Error 事件）
    let cleanup_key = document_key.clone();
    let cleanup_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let mut buses = cleanup_state.review_event_buses.lock().await;
        buses.remove(&cleanup_key);
        let mut active = cleanup_state.active_reviews.lock().await;
        active.remove(&cleanup_key);
    });
}

/// GET /api/v1/review/:doc_id/stream
///
/// SSE 端点：实时推送审查进度事件。
/// 客户端应**先连接此端点**，再调用 POST /review 触发审查，
/// 以确保不丢失早期事件。
#[utoipa::path(
    get,
    path = "/api/v1/review/{doc_id}/stream",
    params(
        ("doc_id" = String, Path, description = "Document UUID")
    ),
    responses(
        (status = 200, description = "SSE stream of review events (text/event-stream)")
    )
)]
pub async fn stream_review_events(
    State(state): State<AppState>,
    Extension(context): Extension<InternalRequestContext>,
    Path(doc_id): Path<String>,
) -> Result<
    axum::response::Sse<
        impl futures_core::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
    >,
    (StatusCode, Json<ErrorResponse>),
> {
    use axum::response::sse::Event;

    let key = document_key(&context, &doc_id)?;
    let _ = load_document(&state, &key).await?;

    // 创建或获取 ReviewEventBus（如果 POST /review 尚未创建）
    let review_events = {
        let mut buses = state.review_event_buses.lock().await;
        buses
            .entry(key)
            .or_insert_with(|| Arc::new(ReviewEventBus::new(review_event_capacity())))
            .clone()
    };

    let mut rx = review_events.subscribe();

    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    // msg 格式: event:{event_type}\n{json} 或 纯 JSON
                    let (event_type, data) = if let Some(rest) = msg.strip_prefix("event:") {
                        if let Some((etype, body)) = rest.split_once('\n') {
                            (etype.to_string(), body.to_string())
                        } else {
                            ("message".to_string(), rest.to_string())
                        }
                    } else {
                        // 旧格式（直接从 emit() 发送的纯 JSON）
                        ("message".to_string(), msg.clone())
                    };

                    // tagged JSON（来自 emit()）: {"event":"phase","data":{...}}
                    // 提取 event 类型 → SSE event type，提取内层 data → SSE data
                    let (final_event_type, final_data) = if event_type == "message" {
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data) {
                            let etype = parsed.get("event")
                                .and_then(|v| v.as_str())
                                .unwrap_or("message")
                                .to_string();
                            // ★ 解包内层 data 字段，避免双重包装
                            let inner = parsed.get("data")
                                .map(|v| v.to_string())
                                .unwrap_or_else(|| data.clone());
                            (etype, inner)
                        } else {
                            ("message".to_string(), data)
                        }
                    } else {
                        // SSE 前缀格式（来自 emit_sse()）: event:phase\n{json}
                        // 已正确分离 event_type 和 data
                        (event_type, data)
                    };

                    yield Ok(Event::default()
                        .event(final_event_type)
                        .data(final_data));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    // 丢失的是实时展示事件，不代表审核失败。最终结果仍由
                    // GET /review/:doc_id/result 提供，因此发送非致命通知。
                    yield Ok(Event::default()
                        .event("stream_lagged")
                        .data(serde_json::json!({
                            "message": "SSE consumer lagged; progress events were dropped",
                            "dropped": n
                        }).to_string()));
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    };

    Ok(axum::response::Sse::new(stream))
}

/// GET /api/v1/review/:doc_id/result
///
/// 查询异步审查的最终结果。
#[utoipa::path(
    get,
    path = "/api/v1/review/{doc_id}/result",
    params(
        ("doc_id" = String, Path, description = "Document UUID")
    ),
    responses(
        (status = 200, description = "Review result (status: completed/partial_failed/pending/failed)", body = ReviewResultResponse),
        (status = 404, description = "No review record found", body = ErrorResponse)
    )
)]
pub async fn get_review_result(
    State(state): State<AppState>,
    Extension(context): Extension<InternalRequestContext>,
    Path(doc_id): Path<String>,
) -> Result<Json<ReviewResultResponse>, (StatusCode, Json<ErrorResponse>)> {
    let key = document_key(&context, &doc_id)?;
    // 1. 检查内存中已完成的结果（不移除，允许多次查询）
    {
        let results = state.review_results.lock().await;
if let Some(output) = results.get(&key) {
            let usage = state.review_usages.lock().await.get(&key).cloned();
            return Ok(Json(ReviewResultResponse {
                status: output.execution_summary.status.as_str().to_string(),
                result: Some(ReviewResponse {
                    document_id: doc_id,
                    findings: output.findings.clone(),
                    routing_summary: output.routing_summary.clone(),
                    execution_summary: output.execution_summary.clone(),
                    graph_snapshot: output.graph_snapshot.clone(),
                }),
                usage,
                error: None,
            }));
        }
    }

    // 2. 检查失败信息
    {
        let errors = state.review_errors.lock().await;
        if let Some(msg) = errors.get(&key) {
            return Ok(Json(ReviewResultResponse {
                status: "failed".to_string(),
                result: None,
                usage: None,
                error: Some(msg.clone()),
            }));
        }
    }

    // 3. 检查是否仍在进行中
    {
        let buses = state.review_event_buses.lock().await;
        if buses.contains_key(&key) {
            return Ok(Json(ReviewResultResponse {
                status: "pending".to_string(),
                result: None,
                usage: None,
                error: None,
            }));
        }
    }

    // 4. 磁盘 fallback — 重启后内存为空：
    //    已完成结果(_result.json) → 恢复；中断状态(_review_state.json) → 明确失败。
    {
        let Some(findings_dir) = tenant_output_path(&context.tenant_id, "findings") else {
            return Err(document_not_found());
        };
        if let Some(recovered) = disk_recovery(&context.tenant_id, &doc_id, &findings_dir) {
            println!("[DISK] review recovered from disk: doc_id={}", doc_id);
            return Ok(Json(recovered));
        }
    }

    Err(document_not_found())
}

/// 磁盘兜底恢复（纯函数，便于单测）：
/// 1. `{doc_id}_result.json` 存在且归属正确 → 返回已完成结果；
/// 2. `{doc_id}_review_state.json` 存在：
///    - running → 引擎重启导致中断 → failed + 中断文案
///    - failed → 透传原始失败原因
/// 3. 均无 → None（调用方返回 404）。
fn disk_recovery(
    tenant_id: &str,
    doc_id: &str,
    findings_dir: &std::path::Path,
) -> Option<ReviewResultResponse> {
    // 1. 已完成结果优先（完成写盘后、删状态文件前崩溃的窗口）
    let result_path = findings_dir.join(format!("{}_result.json", doc_id));
    if let Ok(json) = std::fs::read_to_string(&result_path)
        && let Ok(result) = serde_json::from_str::<ReviewResultResponse>(&json)
    {
        let belongs_to_document = result
            .result
            .as_ref()
            .map(|review| review.document_id == doc_id)
            .unwrap_or(true);
        if belongs_to_document {
            return Some(result);
        }
    }

    // 2. 中断/失败状态文件
    let state = crate::api::review_state::read(findings_dir, doc_id)?;
    let (status, error) = if state.is_running() {
        (
            "failed".to_string(),
            Some(crate::api::review_state::INTERRUPTED_ERROR_MSG.to_string()),
        )
    } else {
        ("failed".to_string(), state.error)
    };
    Some(ReviewResultResponse {
        status,
        result: None,
        usage: None,
        error,
    })
}

/// POST /api/v1/documents/:id/chat
#[utoipa::path(
    post,
    path = "/api/v1/documents/{id}/chat",
    params(
        ("id" = String, Path, description = "Document UUID")
    ),
    request_body = ChatRequest,
    responses(
        (status = 200, description = "Chat response", body = ChatResponse),
        (status = 404, description = "Document not found", body = ErrorResponse),
        (status = 500, description = "Chat execution failure", body = ErrorResponse)
    )
)]
pub async fn chat_with_document(
    State(state): State<AppState>,
    Extension(context): Extension<InternalRequestContext>,
    Path(doc_id): Path<String>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, Json<ErrorResponse>)> {
    let key = document_key(&context, &doc_id)?;
    let doc = load_document(&state, &key).await?;
    let llm: Arc<dyn LlmClient> =
        Arc::from(create_llm_client().map_err(|e| server_error("创建 Chat LLM 客户端失败", e))?);

    let embed_client = {
        let ec = state.embed_client.lock().unwrap();
        ec.clone()
    };

    let mut chat_tools = ToolRegistry::new();
    eprintln!("[handlers] ── 创建 ChatAgent 对话工具集 ──");
    if let Some(ref ec) = embed_client {
        chat_tools.register(Box::new(SearchDocumentTool::new(
            doc.doc_index.clone(),
            ec.clone(),
        )));
        // 本地知识库检索（对话也能引用已入库的法规/案例原文）
        chat_tools.register(Box::new(SearchKnowledgeBaseTool::new(ec.clone())));
    }
    chat_tools.register(Box::new(ReadSectionTool::new(
        doc.review_chunk_map.clone(),
        doc.chunk_order.clone(),
    )));
    if let Some(ref ds) = state.dashscope_search {
        chat_tools.register(Box::new(SearchKnowledgeTool::with_dashscope(ds.clone())));
    }
    chat_tools.register(Box::new(AnswerUserTool));
    eprintln!(
        "[handlers] ── ChatAgent 工具集注册完成: 共 {} 个工具 ──",
        chat_tools.len()
    );

    let chat_config = ChatAgentConfig::default();
    let chat_agent = ChatAgent::new(
        chat_config,
        llm,
        chat_tools,
        Some(doc.doc_index.clone()),
        embed_client,
        Some(doc.review_chunk_map.clone()),
    )
    .map_err(|e| server_error("创建 ChatAgent 失败", e))?;

    // DTO history → ChatMessage
    let mut chat_vault = (*doc.redaction_vault).clone();
    let selection = req.selection.map(|mut selection| {
        selection.text = chat_vault.redact(&selection.text);
        selection
    });
    let user_input = chat_vault.redact(&req.user_input);
    let history = req.history.map(|h| {
        h.into_iter()
            .map(|m| match m.role.as_str() {
                "system" => ChatMessage::System {
                    content: chat_vault.redact(&m.content.unwrap_or_default()),
                },
                "assistant" => ChatMessage::Assistant {
                    content: m.content.map(|content| chat_vault.redact(&content)),
                    tool_calls: None,
                },
                _ => ChatMessage::User {
                    content: chat_vault.redact(&m.content.unwrap_or_default()),
                },
            })
            .collect()
    });

    let mut response = chat_agent
        .chat(selection, &user_input, history)
        .await
        .map_err(|e| server_error("对话执行失败", e))?;
    restore_chat_response(&mut response, &chat_vault);

    Ok(Json(response))
}

/// POST /api/v1/documents/:id/chat/stream
///
/// SSE streaming endpoint for ChatAgent.
#[utoipa::path(
    post,
    path = "/api/v1/documents/{id}/chat/stream",
    params(
        ("id" = String, Path, description = "Document UUID")
    ),
    request_body = ChatRequest,
    responses(
        (status = 200, description = "SSE stream of chat events (text/event-stream)")
    )
)]
pub async fn chat_with_document_stream(
    State(state): State<AppState>,
    Extension(context): Extension<InternalRequestContext>,
    Path(doc_id): Path<String>,
    Json(req): Json<ChatRequest>,
) -> Result<
    axum::response::Sse<
        impl futures_core::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
    >,
    (StatusCode, Json<ErrorResponse>),
> {
    use axum::response::sse::Event;

    let key = document_key(&context, &doc_id)?;
    let doc = load_document(&state, &key).await?;

    // All setup + streaming in a single async_stream block
    // (each async_stream::stream! creates a unique type — can't have early returns)
    let stream = async_stream::stream! {
        // ── Setup (inside stream to avoid type mismatch) ──
        let doc = doc.clone();

        let llm: Arc<dyn LlmClient> = match create_llm_client() {
            Ok(client) => Arc::from(client),
            Err(e) => {
                yield Ok(Event::default()
                    .event("error")
                    .data(format!(r#"{{"message":"{}"}}"#, e)));
                return;
            }
        };

        let embed_client = {
            let ec = state.embed_client.lock().unwrap();
            ec.clone()
        };

        let mut chat_tools = ToolRegistry::new();
        eprintln!("[handlers] ── 创建 ChatAgent 对话工具集 (stream) ──");
        if let Some(ref ec) = embed_client {
            chat_tools.register(Box::new(SearchDocumentTool::new(
                doc.doc_index.clone(),
                ec.clone(),
            )));
            // 本地知识库检索（对话也能引用已入库的法规/案例原文）
            chat_tools.register(Box::new(SearchKnowledgeBaseTool::new(ec.clone())));
        }
        chat_tools.register(Box::new(ReadSectionTool::new(
            doc.review_chunk_map.clone(),
            doc.chunk_order.clone(),
        )));
        if let Some(ref ds) = state.dashscope_search {
            chat_tools.register(Box::new(SearchKnowledgeTool::with_dashscope(ds.clone())));
        }
        chat_tools.register(Box::new(AnswerUserTool));
        eprintln!(
            "[handlers] ── ChatAgent 工具集注册完成 (stream): 共 {} 个工具 ──",
            chat_tools.len()
        );

        let chat_config = ChatAgentConfig::default();
        let chat_agent = match ChatAgent::new(
            chat_config,
            llm,
            chat_tools,
            Some(doc.doc_index.clone()),
            embed_client,
            Some(doc.review_chunk_map.clone()),
        ) {
            Ok(agent) => agent,
            Err(e) => {
                yield Ok(Event::default()
                    .event("error")
                    .data(format!(r#"{{"message":"创建 ChatAgent 失败: {}"}}"#, e)));
                return;
            }
        };

        let mut chat_vault = (*doc.redaction_vault).clone();
        let selection = req.selection.map(|mut selection| {
            selection.text = chat_vault.redact(&selection.text);
            selection
        });
        let user_input = chat_vault.redact(&req.user_input);
        let history = req.history.map(|h| {
            h.into_iter().map(|m| match m.role.as_str() {
                "system" => ChatMessage::System {
                    content: chat_vault.redact(&m.content.unwrap_or_default()),
                },
                "assistant" => ChatMessage::Assistant {
                    content: m.content.map(|content| chat_vault.redact(&content)),
                    tool_calls: None,
                },
                _ => ChatMessage::User {
                    content: chat_vault.redact(&m.content.unwrap_or_default()),
                },
            }).collect()
        });

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ChatStreamEvent>();

        // Spawn ChatAgent in background
        tokio::spawn(async move {
            let _ = chat_agent.chat_stream(selection, &user_input, history, tx).await;
        });

        // ── Relay events from agent ──
        while let Some(event) = rx.recv().await {
            let (event_type, data) = match &event {
                ChatStreamEvent::Thinking { message } =>
                    ("thinking", format!(r#"{{"message":"{}"}}"#, message)),
                ChatStreamEvent::ToolCall { name, args } =>
                    ("tool_call", format!(r#"{{"name":"{}","args":"{}"}}"#, name, args)),
                ChatStreamEvent::Answer(resp) => {
                    let mut restored = resp.clone();
                    restore_chat_response(&mut restored, &chat_vault);
                    ("answer", serde_json::to_string(&restored).unwrap_or_default())
                },
                ChatStreamEvent::Done(resp) => {
                    let mut restored = resp.clone();
                    restore_chat_response(&mut restored, &chat_vault);
                    ("done", serde_json::to_string(&restored).unwrap_or_default())
                },
                ChatStreamEvent::Error(msg) =>
                    ("error", format!(r#"{{"message":"{}"}}"#, msg)),
            };
            let is_terminal = matches!(event, ChatStreamEvent::Done(_) | ChatStreamEvent::Error(_));
            yield Ok(Event::default().event(event_type).data(data));
            if is_terminal {
                break;
            }
        }
    };

    Ok(axum::response::Sse::new(stream))
}

/// POST /api/v1/documents/:id/search
#[utoipa::path(
    post,
    path = "/api/v1/documents/{id}/search",
    params(
        ("id" = String, Path, description = "Document UUID")
    ),
    request_body = SearchRequest,
    responses(
        (status = 200, description = "Search results", body = SearchResponse),
        (status = 404, description = "Document not found", body = ErrorResponse),
        (status = 500, description = "Search encoding failure", body = ErrorResponse)
    )
)]
pub async fn search_document(
    State(state): State<AppState>,
    Extension(context): Extension<InternalRequestContext>,
    Path(doc_id): Path<String>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, (StatusCode, Json<ErrorResponse>)> {
    let key = document_key(&context, &doc_id)?;
    let doc = load_document(&state, &key).await?;

    if req.queries.is_empty() {
        return Ok(Json(SearchResponse {
            results: Vec::new(),
        }));
    }

    let embed_client = {
        let ec = state.embed_client.lock().unwrap();
        ec.clone()
    };

    let mut search_vault = (*doc.redaction_vault).clone();
    let redacted_queries: Vec<String> = req
        .queries
        .iter()
        .map(|query| search_vault.redact(query))
        .collect();
    let query_texts: Vec<&str> = redacted_queries.iter().map(|s| s.as_str()).collect();
    let query_embs = if let Some(ref ec) = embed_client {
        ec.encode_queries(&query_texts)
            .map_err(|e| server_error("查询编码失败", e))?
    } else {
        return Err(server_error_fmt("嵌入客户端未初始化"));
    };

    let top_k = req.top_k.unwrap_or(5);
    let mut results = Vec::new();
    for (i, query) in req.queries.iter().enumerate() {
        let hits = doc.doc_index.search(&query_embs[i], top_k);
        let hit_dtos: Vec<SearchHitDto> = hits
            .iter()
            .map(|h| SearchHitDto {
                chunk_id: h.chunk_id.clone(),
                title: search_vault.restore(&h.title),
                score: h.score,
                snippet: search_vault.restore(&h.snippet.chars().take(200).collect::<String>()),
                page_start: h.page_start,
            })
            .collect();
        results.push(SearchResultGroup {
            query: query.clone(),
            hits: hit_dtos,
        });
    }

    Ok(Json(SearchResponse { results }))
}

// ─── Block BBox 查询 ─────────────────────────────────────────────

/// 请求参数：ids 为逗号分隔的 block_id 列表
#[derive(Debug, Deserialize, IntoParams, ToSchema)]
pub struct BlockQuery {
    pub ids: String,
}

/// BBox 坐标 DTO
#[derive(Debug, Serialize, ToSchema)]
pub struct BBoxDto {
    pub x0: f64,
    pub top: f64,
    pub x1: f64,
    pub bottom: f64,
}

/// 单个 block 的 BBox 响应
#[derive(Debug, Serialize, ToSchema)]
pub struct BlockBBoxResponse {
    pub block_id: String,
    /// 所在页码 (0-based)
    pub page: usize,
    /// 包围盒坐标（PDF points）
    pub bbox: BBoxDto,
    /// 原始 PDF 页面宽度 (pt)，用于前端 scale = renderedWidth / pageWidth
    pub page_width: f64,
}

/// GET /api/v1/documents/:id/blocks?ids=b_5_2,b_5_3
///
/// 返回指定 block_id 的 BBox 坐标，用于前端 bbox-based PDF 精确高亮。
#[utoipa::path(
    get,
    path = "/api/v1/documents/{id}/blocks",
    params(
        ("id" = String, Path, description = "Document UUID"),
        BlockQuery
    ),
    responses(
        (status = 200, description = "Block bounding boxes", body = Vec<BlockBBoxResponse>),
        (status = 404, description = "Document not found", body = ErrorResponse)
    )
)]
pub async fn get_block_bboxes(
    State(state): State<AppState>,
    Extension(context): Extension<InternalRequestContext>,
    Path(doc_id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<BlockQuery>,
) -> Result<Json<Vec<BlockBBoxResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let key = document_key(&context, &doc_id)?;
    let doc = load_document(&state, &key).await?;
    let requested_ids: Vec<&str> = params.ids.split(',').map(|s| s.trim()).collect();
    println!(
        "[BLOCKS] 查询 block BBox: doc={}, ids={:?}",
        doc_id, requested_ids
    );
    let mut results: Vec<BlockBBoxResponse> = Vec::new();

    for page in &doc.raw_doc.pages {
        for block in &page.blocks {
            if requested_ids.contains(&block.id.as_str()) {
                results.push(BlockBBoxResponse {
                    block_id: block.id.clone(),
                    page: page.page_index,
                    bbox: BBoxDto {
                        x0: block.bbox.x0,
                        top: block.bbox.top,
                        x1: block.bbox.x1,
                        bottom: block.bbox.bottom,
                    },
                    page_width: page.width,
                });
            }
        }
    }

    println!(
        "[BLOCKS] 返回 {} 条 BBox 坐标 (请求 {} 个 block)",
        results.len(),
        requested_ids.len()
    );
    Ok(Json(results))
}

// ─── Helper ────────────────────────────────────────────────────────

fn collect_all_block_ids(section: &Section) -> Vec<&str> {
    let mut ids: Vec<&str> = section.block_ids.iter().map(|s| s.as_str()).collect();
    for child in &section.children {
        ids.extend(collect_all_block_ids(child));
    }
    ids
}

fn bad_request(msg: &str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: "BAD_REQUEST".to_string(),
            detail: msg.to_string(),
        }),
    )
}

fn server_error(msg: &str, e: impl std::fmt::Display) -> (StatusCode, Json<ErrorResponse>) {
    let detail = format!("{}: {}", msg, e);
    eprintln!("[ERROR] {}", detail);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: msg.to_string(),
            detail,
        }),
    )
}

fn server_error_fmt(msg: &str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: msg.to_string(),
            detail: msg.to_string(),
        }),
    )
}

fn not_found(msg: &str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "RESOURCE_NOT_FOUND".to_string(),
            detail: msg.to_string(),
        }),
    )
}

fn document_not_found() -> (StatusCode, Json<ErrorResponse>) {
    not_found("resource not found")
}

// ─── Metrics Helpers ────────────────────────────────────────────────────

/// 解析租户命名空间下的 metrics runs 根目录（`output/runs/{tenant_id}`）。
///
/// tenant_id 必须通过 `is_valid_tenant_id` 校验，且最终路径不得逃逸该命名空间，
/// 防止目录穿越。`relative` 为空时返回命名空间根目录本身。
fn metric_runs_path(tenant_id: &str, relative: &str) -> Option<std::path::PathBuf> {
    if !is_valid_tenant_id(tenant_id) {
        return None;
    }
    let root = std::path::PathBuf::from(crate::paths::data_path_str("output/runs"));
    let namespace = root.join(tenant_id);
    let candidate = if relative.is_empty() {
        namespace.clone()
    } else {
        namespace.join(relative)
    };
    candidate.starts_with(&namespace).then_some(candidate)
}

/// 递归扫描 output/runs/{tenant_id}/ 下所有 .json 文件，返回 (相对文件夹路径, 文件路径)。
fn list_run_files(tenant_id: &str) -> Vec<(Option<String>, std::path::PathBuf)> {
    let Some(base) = metric_runs_path(tenant_id, "") else {
        return Vec::new();
    };
    let mut files = Vec::new();
    let _ = scan_dir(&base.to_string_lossy(), None, &mut files);
    files
}

fn scan_dir(
    dir: &str,
    experiment_group: Option<String>,
    out: &mut Vec<(Option<String>, std::path::PathBuf)>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let group_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
                .to_string();
            let _ = scan_dir(&path.to_string_lossy(), Some(group_name), out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
            out.push((experiment_group.clone(), path));
        }
    }
    Ok(())
}

/// 根据 run_id 查找文件路径（递归搜索子目录）。
fn find_run_path(tenant_id: &str, run_id: &str) -> Option<std::path::PathBuf> {
    list_run_files(tenant_id)
        .into_iter()
        .map(|(_, path)| path)
        .find(|path| path.file_stem().and_then(|s| s.to_str()) == Some(run_id))
}

/// 列出所有实验组名称。
fn list_experiment_groups(tenant_id: &str) -> Vec<String> {
    let Some(base) = metric_runs_path(tenant_id, "") else {
        return Vec::new();
    };
    let mut groups = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.flatten() {
            if entry.path().is_dir()
                && let Some(name) = entry.file_name().to_str()
            {
                groups.push(name.to_string());
            }
        }
    }
    groups.sort();
    groups
}

// ─── Metrics API ────────────────────────────────────────────────────────

/// 单次实验的摘要（从完整 RunMetrics JSON 中提取关键字段）。
#[derive(Debug, Serialize)]
pub struct MetricRunSummary {
    run_id: String,
    title: Option<String>,
    notes: Option<String>,
    experiment_group: Option<String>,
    timestamp: String,
    tags: Vec<String>,
    description: String,
    document_name: String,
    total_secs: f64,
    llm_calls: usize,
    tokens_input: u64,
    tokens_output: u64,
    cost_cny: f64,
    total_findings: usize,
    high_findings: usize,
    coordinator_enabled: bool,
    llm_model: String,
    embed_engine: String,
}

#[derive(Debug, Serialize)]
pub struct MetricRunListResponse {
    runs: Vec<MetricRunSummary>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTagsRequest {
    tags: Vec<String>,
}

/// GET /api/v1/metrics/runs — 列出所有实验的摘要。
pub async fn list_metric_runs(
    Extension(context): Extension<InternalRequestContext>,
) -> (StatusCode, Json<serde_json::Value>) {
    if !is_valid_tenant_id(&context.tenant_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error":"租户不存在"})),
        );
    }
    let mut summaries: Vec<MetricRunSummary> = Vec::new();

    for (experiment_group, path) in list_run_files(&context.tenant_id) {
        if let Ok(content) = std::fs::read_to_string(&path)
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&content)
        {
            let meta = &val["meta"];
            let latency = &val["latency"];
            let llm = &val["llm_efficiency"];
            let quality = &val["review_quality"];
            let _resources = &val["resources"];

            summaries.push(MetricRunSummary {
                run_id: meta["run_id"].as_str().unwrap_or("?").to_string(),
                title: meta["title"].as_str().map(|s| s.to_string()),
                notes: meta["notes"].as_str().map(|s| s.to_string()),
                timestamp: meta["timestamp"].as_str().unwrap_or("?").to_string(),
                tags: meta["tags"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                description: meta["description"].as_str().unwrap_or("").to_string(),
                document_name: meta["document"]["name"].as_str().unwrap_or("?").to_string(),
                total_secs: latency["total_wall_clock_secs"].as_f64().unwrap_or(0.0),
                llm_calls: llm["totals"]["llm_calls"].as_u64().unwrap_or(0) as usize,
                tokens_input: llm["totals"]["tokens_input"].as_u64().unwrap_or(0),
                tokens_output: llm["totals"]["tokens_output"].as_u64().unwrap_or(0),
                cost_cny: llm["totals"]["cost_cny"].as_f64().unwrap_or(0.0),
                total_findings: quality["findings"]["after_dedup"].as_u64().unwrap_or(0) as usize,
                high_findings: quality["findings"]["by_severity"]["high"]
                    .as_u64()
                    .unwrap_or(0) as usize,
                coordinator_enabled: meta["config"]["coordinator_enabled"]
                    .as_bool()
                    .unwrap_or(false),
                llm_model: meta["config"]["llm_model"]
                    .as_str()
                    .unwrap_or("?")
                    .to_string(),
                embed_engine: meta["config"]["embed_engine"]
                    .as_str()
                    .unwrap_or("?")
                    .to_string(),
                experiment_group: experiment_group.clone(),
            });
        }
    }

    // 按时间倒序
    summaries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    (
        StatusCode::OK,
        Json(serde_json::json!({ "runs": summaries })),
    )
}

/// GET /api/v1/metrics/runs/:run_id — 获取单次实验的完整指标。
pub async fn get_metric_run(
    Extension(context): Extension<InternalRequestContext>,
    Path(run_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if !is_valid_tenant_id(&context.tenant_id) {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"租户不存在"})));
    }
    let path = match find_run_path(&context.tenant_id, &run_id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error":"不存在"})),
            );
        }
    };
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(val) => (StatusCode::OK, Json(val)),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("JSON 解析失败: {}", e) })),
            ),
        },
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("实验 {} 不存在", run_id) })),
        ),
    }
}

/// PATCH /api/v1/metrics/runs/:run_id/tags — 更新实验标签。
pub async fn update_metric_tags(
    Extension(context): Extension<InternalRequestContext>,
    Path(run_id): Path<String>,
    Json(body): Json<UpdateTagsRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if !is_valid_tenant_id(&context.tenant_id) {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"租户不存在"})));
    }
    let path = match find_run_path(&context.tenant_id, &run_id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error":"不存在"})),
            );
        }
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error":"读取失败"})),
            );
        }
    };
    let mut val: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("JSON 解析失败: {}", e) })),
            );
        }
    };
    val["meta"]["tags"] = serde_json::json!(body.tags);
    if let Err(e) = std::fs::write(
        &path,
        serde_json::to_string_pretty(&val).unwrap_or_default(),
    ) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("写入失败: {}", e) })),
        );
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({ "ok": true, "run_id": run_id })),
    )
}

/// PATCH /api/v1/metrics/runs/:run_id/title — 更新实验标题。
pub async fn update_metric_title(
    Extension(context): Extension<InternalRequestContext>,
    Path(run_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    if !is_valid_tenant_id(&context.tenant_id) {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"租户不存在"})));
    }
    let Some(path) = metric_runs_path(&context.tenant_id, &format!("{}.json", run_id)) else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"不存在"})));
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error":"不存在"})),
            );
        }
    };
    let mut val: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error":format!("{}",e)})),
            );
        }
    };
    val["meta"]["title"] = body
        .get("title")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    if let Err(e) = std::fs::write(
        &path,
        serde_json::to_string_pretty(&val).unwrap_or_default(),
    ) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error":format!("{}",e)})),
        );
    }
    (StatusCode::OK, Json(serde_json::json!({"ok":true})))
}

/// PATCH /api/v1/metrics/runs/:run_id/notes — 更新实验备注。
pub async fn update_metric_notes(
    Extension(context): Extension<InternalRequestContext>,
    Path(run_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    if !is_valid_tenant_id(&context.tenant_id) {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"租户不存在"})));
    }
    let Some(path) = metric_runs_path(&context.tenant_id, &format!("{}.json", run_id)) else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"不存在"})));
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error":"不存在"})),
            );
        }
    };
    let mut val: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error":format!("{}",e)})),
            );
        }
    };
    val["meta"]["notes"] = body
        .get("notes")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    if let Err(e) = std::fs::write(
        &path,
        serde_json::to_string_pretty(&val).unwrap_or_default(),
    ) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error":format!("{}",e)})),
        );
    }
    (StatusCode::OK, Json(serde_json::json!({"ok":true})))
}

/// PATCH /api/v1/metrics/runs/:run_id/experiment-group — 移动实验到指定实验组。
pub async fn move_metric_experiment_group(
    Extension(context): Extension<InternalRequestContext>,
    Path(run_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    if !is_valid_tenant_id(&context.tenant_id) {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"租户不存在"})));
    }
    let old_path = match find_run_path(&context.tenant_id, &run_id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error":"不存在"})),
            );
        }
    };
    let group = body
        .get("experiment_group")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let Some(base) = metric_runs_path(&context.tenant_id, "") else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"租户不存在"})));
    };
    let new_dir = match group.as_deref() {
        Some(g) if !g.is_empty() => match metric_runs_path(&context.tenant_id, g) {
            Some(d) => d,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error":"非法实验组名"})),
                );
            }
        },
        _ => base.clone(),
    };
    let _ = std::fs::create_dir_all(&new_dir);
    let fname = old_path.file_name().unwrap();
    let new_path = new_dir.join(fname);
    if let Err(e) = std::fs::rename(&old_path, &new_path) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error":format!("{}",e)})),
        );
    }
    // Update experiment_group field in JSON
    if let Ok(content) = std::fs::read_to_string(&new_path)
        && let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&content)
    {
        val["meta"]["experiment_group"] = group
            .map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::Null);
        let _ = std::fs::write(
            &new_path,
            serde_json::to_string_pretty(&val).unwrap_or_default(),
        );
    }
    (StatusCode::OK, Json(serde_json::json!({"ok":true})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chunk::ChunkType;

    const TEST_TENANT_ID: &str = "1";

    fn test_context() -> InternalRequestContext {
        InternalRequestContext {
            tenant_id: TEST_TENANT_ID.to_string(),
            user_id: "tester".to_string(),
            request_id: "req-1".to_string(),
            timestamp: 0,
            body_sha256: String::new(),
        }
    }

    fn make_test_chunk() -> Chunk {
        Chunk {
            chunk_id: "ch_001".to_string(),
            chunk_type: ChunkType::Leaf,
            section_path: vec!["测试章节".to_string()],
            text: "测试条款".to_string(),
            page_start: 0,
            page_end: 0,
            source_block_ids: Vec::new(),
            bbox_refs: Vec::new(),
        }
    }

    fn make_test_document(doc_id: &str) -> Arc<DocumentState> {
        let chunk = make_test_chunk();
        let chunk_map = Arc::new(HashMap::from([(chunk.chunk_id.clone(), chunk.clone())]));
        Arc::new(DocumentState {
            tenant_id: TEST_TENANT_ID.to_string(),
            id: doc_id.to_string(),
            filename: "test.pdf".to_string(),
            stem: "test".to_string(),
            raw_doc: RawDocument {
                document_id: doc_id.to_string(),
                source_path: "test.pdf".to_string(),
                pages: Vec::new(),
            },
            sections: Vec::new(),
            chunks: vec![chunk.clone()],
            review_chunks: vec![chunk],
            chunk_map: chunk_map.clone(),
            review_chunk_map: chunk_map,
            chunk_order: Arc::new(vec!["ch_001".to_string()]),
            doc_index: Arc::new(DocumentVectorIndex::new(Vec::new(), Vec::new())),
            redaction_vault: Arc::new(RedactionVault::default()),
            desensitization_summary: DesensitizationSummary::default(),
        })
    }

    async fn make_test_state(doc_id: &str) -> AppState {
        let state = AppState {
            review_execution_limiter: Arc::new(
                crate::agents::execution_control::GlobalExecutionLimiter::new(
                    crate::agents::execution_control::ExecutionLimits::default(),
                ),
            ),
            documents: Arc::new(TokioRwLock::new(HashMap::new())),
            embed_client: Arc::new(StdMutex::new(None)),
            dashscope_search: None,
            search_backend: "dashscope".to_string(),
            embed_engine: "remote".to_string(),
            review_event_buses: Arc::new(TokioMutex::new(HashMap::new())),
            review_results: Arc::new(TokioMutex::new(HashMap::new())),
            review_usages: Arc::new(TokioMutex::new(HashMap::new())),
            review_errors: Arc::new(TokioMutex::new(HashMap::new())),
            active_reviews: Arc::new(TokioMutex::new(HashSet::new())),
        };
        state
            .documents
            .write()
            .await
            .insert(
                DocumentKey::new(TEST_TENANT_ID, doc_id),
                make_test_document(doc_id),
            );
        state
    }

    /// 按 process_document 的目录约定把一份 DocumentState 落地到临时目录。
    fn write_document_artifacts(root: &std::path::Path, ds: &DocumentState) {
        let tenant = root.join(ds.tenant_id.clone());
        for sub in ["raw_json", "sections", "chunks", "documents", "embeddings"] {
            std::fs::create_dir_all(tenant.join(sub)).unwrap();
        }
        std::fs::write(
            tenant.join("raw_json").join(format!("{}_raw.json", ds.stem)),
            serde_json::to_string(&ds.raw_doc).unwrap(),
        )
        .unwrap();
        std::fs::write(
            tenant.join("sections").join(format!("{}_sections.json", ds.stem)),
            serde_json::to_string(&ds.sections).unwrap(),
        )
        .unwrap();
        std::fs::write(
            tenant.join("chunks").join(format!("{}_chunks.json", ds.stem)),
            serde_json::to_string(&ds.chunks).unwrap(),
        )
        .unwrap();
        let manifest = DocumentManifest {
            document_id: ds.id.clone(),
            filename: ds.filename.clone(),
            stem: ds.stem.clone(),
            desensitization_summary: ds.desensitization_summary.clone(),
            review_chunks: ds.review_chunks.clone(),
            redaction_vault: (*ds.redaction_vault).clone(),
        };
        std::fs::write(
            tenant.join("documents").join(format!("{}_manifest.json", ds.stem)),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();
        crate::services::embedding_service::save_index(
            &ds.doc_index,
            tenant.join("embeddings").to_string_lossy().as_ref(),
            &ds.stem,
        )
        .unwrap();
    }

    /// 重启恢复闭环：一份文档写盘后必须能原样重建内存态（filename/脱敏副本/向量/章节不丢）。
    #[test]
    fn rebuild_document_state_round_trips() {
        let mut chunk = make_test_chunk();
        chunk.text = "投标人须在东莞设有常驻服务机构".to_string();
        let index = DocumentVectorIndex::new(
            vec![crate::domain::vector_index::ChunkMeta {
                chunk_id: chunk.chunk_id.clone(),
                section_path: chunk.section_path.clone(),
                embed_text: chunk.text.clone(),
                text_len: chunk.text.chars().count(),
                page_start: chunk.page_start,
                page_end: chunk.page_end,
            }],
            vec![vec![1.0, 0.0]],
        );
        let ds = DocumentState {
            tenant_id: "3".to_string(),
            id: "doc-reload-1".to_string(),
            filename: "MAOMING_mutated.pdf".to_string(),
            stem: "docreload1".to_string(),
            raw_doc: RawDocument {
                document_id: "doc-reload-1".to_string(),
                source_path: "src.pdf".to_string(),
                pages: Vec::new(),
            },
            sections: vec![Section {
                level: 1,
                title: "第一章".to_string(),
                pattern: "heading".to_string(),
                page_start: 0,
                page_end: 0,
                block_ids: vec!["b_1".to_string()],
                body_text: chunk.text.clone(),
                children: Vec::new(),
                body_page_start: 0,
                body_page_end: 0,
            }],
            chunks: vec![chunk.clone()],
            review_chunks: vec![chunk.clone()],
            chunk_map: Arc::new(HashMap::new()),
            review_chunk_map: Arc::new(HashMap::new()),
            chunk_order: Arc::new(Vec::new()),
            doc_index: Arc::new(index),
            redaction_vault: Arc::new(RedactionVault::new(DesensitizationMode::Low)),
            desensitization_summary: DesensitizationSummary::default(),
        };

        let root = std::env::temp_dir().join(format!("ai-bid-reload-{}", Uuid::new_v4()));
        write_document_artifacts(&root, &ds);

        let rebuilt =
            rebuild_document_state(&root, "3", "docreload1").expect("应能从磁盘重建文档");
        assert_eq!(rebuilt.id, "doc-reload-1");
        assert_eq!(rebuilt.filename, "MAOMING_mutated.pdf");
        assert_eq!(rebuilt.chunks.len(), 1);
        assert_eq!(rebuilt.review_chunks.len(), 1);
        assert_eq!(rebuilt.chunks[0].text, "投标人须在东莞设有常驻服务机构");
        assert_eq!(rebuilt.doc_index.len(), 1);
        assert_eq!(rebuilt.chunk_order.len(), 1);
        assert_eq!(rebuilt.sections.len(), 1);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 缺 manifest（或任何一份必要文件）时优雅跳过，返回 None 而不是 panic。
    #[test]
    fn rebuild_document_state_missing_manifest_is_none() {
        let root = std::env::temp_dir().join(format!("ai-bid-reload-missing-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("3").join("documents")).unwrap();
        assert!(rebuild_document_state(&root, "3", "nosuchdoc").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// P2 开关解析：只有 1/true/on 开启，其余（含未设置、空串、乱写）一律关闭，
    /// 保证 A/B 基线组（control）不受脏环境变量影响。
    #[test]
    fn parse_transcript_compression_accepts_only_truthy() {
        for v in [Some("1"), Some("true"), Some("on")] {
            assert!(parse_transcript_compression(v), "{v:?} 应视为开启");
        }
        for v in [None, Some(""), Some("0"), Some("false"), Some("off"), Some("yes"), Some("TRUE")] {
            assert!(!parse_transcript_compression(v), "{v:?} 应视为关闭");
        }
    }

    #[tokio::test]
    async fn invalid_review_request_does_not_reserve_document() {
        let doc_id = "doc_invalid_request";
        let state = make_test_state(doc_id).await;

        let response = review_document(
            State(state.clone()),
            Extension(test_context()),
            Path(doc_id.to_string()),
            Json(ReviewRequest {
                chunk_ids: Vec::new(),
                max_clauses: Some(0),
                enabled_agents: None,
            }),
        )
        .await;

        assert_eq!(
            response.expect_err("非法请求应返回错误").0,
            StatusCode::BAD_REQUEST
        );
        assert!(
            !state
                .active_reviews
                .lock()
                .await
                .contains(&DocumentKey::new(TEST_TENANT_ID, doc_id)),
            "非法请求不得占用审核锁"
        );
    }

    #[tokio::test]
    async fn invalid_agent_name_is_rejected_before_review_starts() {
        let doc_id = "doc_invalid_agent";
        let state = make_test_state(doc_id).await;

        let response = review_document(
            State(state.clone()),
            Extension(test_context()),
            Path(doc_id.to_string()),
            Json(ReviewRequest {
                chunk_ids: Vec::new(),
                max_clauses: None,
                enabled_agents: Some(vec!["FactCheck".to_string(), "UnknownAgent".to_string()]),
            }),
        )
        .await;

        let (status, Json(error)) = response.expect_err("非法 Agent 名称应返回错误");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(error.detail, "非法 Agent 名称: UnknownAgent");
        assert!(
            !state
                .active_reviews
                .lock()
                .await
                .contains(&DocumentKey::new(TEST_TENANT_ID, doc_id)),
            "非法 Agent 名称不得占用审核锁"
        );
    }

    #[tokio::test]
    async fn invalid_request_takes_precedence_over_active_review_conflict() {
        let doc_id = "doc_invalid_while_active";
        let state = make_test_state(doc_id).await;
        state
            .active_reviews
            .lock()
            .await
            .insert(DocumentKey::new(TEST_TENANT_ID, doc_id));

        let response = review_document(
            State(state),
            Extension(test_context()),
            Path(doc_id.to_string()),
            Json(ReviewRequest {
                chunk_ids: Vec::new(),
                max_clauses: Some(0),
                enabled_agents: None,
            }),
        )
        .await;

        assert_eq!(
            response.expect_err("非法请求应优先返回参数错误").0,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn review_result_preserves_partial_failed_status() {
        let doc_id = "doc_partial_result";
        let state = make_test_state(doc_id).await;
        state.review_results.lock().await.insert(
            DocumentKey::new(TEST_TENANT_ID, doc_id),
            CoordinatorOutput {
                findings: Vec::new(),
                routing_summary: crate::agents::types::RoutingSummary {
                    total_clauses: 1,
                    agent_clause_counts: HashMap::new(),
                    high_risk_count: 0,
                    legal_verify_count: 0,
                    blind_spot_findings: 0,
                },
                graph_snapshot: None,
                execution_summary: crate::agents::types::ExecutionSummary {
                    status: crate::agents::types::ReviewExecutionStatus::PartialFailed,
                    successful_agents: 1,
                    failed_agents: vec![crate::agents::types::AgentExecutionFailure {
                        agent_id: "missing-agent".to_string(),
                        message: "Agent 定义未找到".to_string(),
                    }],
                    failed_clauses: Vec::new(),
                    failed_stages: Vec::new(),
                    budget: None,
                },
            },
        );

        let Json(response) = get_review_result(
            State(state),
            Extension(test_context()),
            Path(doc_id.to_string()),
        )
        .await
        .expect("部分失败结果应可查询");

        assert_eq!(response.status, "partial_failed");
        assert_eq!(
            response
                .result
                .expect("部分失败应保留成功结果")
                .execution_summary
                .failed_agents
                .len(),
            1
        );
    }

    // ── 重启中断检测（disk_recovery 纯函数）──────────────────────────

    #[test]
    fn disk_recovery_returns_interrupted_failed_for_running_state_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::api::review_state::write_running(
            dir.path(),
            "doc-interrupted",
            || "2026-08-27T00:00:00Z".to_string(),
        )
        .expect("write running state");

        let recovered = disk_recovery("1", "doc-interrupted", dir.path())
            .expect("running 状态文件应被识别为中断");

        assert_eq!(recovered.status, "failed");
        assert!(recovered.result.is_none(), "中断审核不得返回结果");
        assert_eq!(
            recovered.error.as_deref(),
            Some(crate::api::review_state::INTERRUPTED_ERROR_MSG),
            "应返回明确的中断文案"
        );
    }

    #[test]
    fn disk_recovery_returns_original_error_for_failed_state_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::api::review_state::write_failed(
            dir.path(),
            "doc-failed-before-restart",
            "审核引擎执行失败: LLM 超时",
            || "2026-08-27T00:00:00Z".to_string(),
        )
        .expect("write failed state");

        let recovered = disk_recovery("1", "doc-failed-before-restart", dir.path())
            .expect("failed 状态文件应被识别");

        assert_eq!(recovered.status, "failed");
        assert_eq!(
            recovered.error.as_deref(),
            Some("审核引擎执行失败: LLM 超时"),
            "应透传原始失败原因"
        );
    }

    #[test]
    fn disk_recovery_returns_none_without_any_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(
            disk_recovery("1", "doc-nothing", dir.path()).is_none(),
            "无结果文件也无状态文件时应返回 None（404）"
        );
    }

    #[test]
    fn disk_recovery_prefers_completed_result_over_state_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let completed: ReviewResultResponse = ReviewResultResponse {
            status: "completed".to_string(),
            result: Some(ReviewResponse {
                document_id: "doc-done".to_string(),
                findings: Vec::new(),
                routing_summary: crate::agents::types::RoutingSummary {
                    total_clauses: 1,
                    agent_clause_counts: HashMap::new(),
                    high_risk_count: 0,
                    legal_verify_count: 0,
                    blind_spot_findings: 0,
                },
                execution_summary: crate::agents::types::ExecutionSummary::default(),
                graph_snapshot: None,
            }),
            error: None,
            usage: None,
        };
        std::fs::write(
            dir.path().join("doc-done_result.json"),
            serde_json::to_string(&completed).expect("serialize result"),
        )
        .expect("write result.json");
        // 陈旧 running 状态文件（模拟完成写盘后、删状态文件前崩溃）
        crate::api::review_state::write_running(
            dir.path(),
            "doc-done",
            || "2026-08-27T00:00:00Z".to_string(),
        )
        .expect("write stale running state");

        let recovered = disk_recovery("1", "doc-done", dir.path())
            .expect("completed 结果优先于状态文件");
        assert_eq!(recovered.status, "completed", "已完成结果必须优先");
    }
}

/// GET /api/v1/metrics/experiment-groups — 列出所有实验组。
pub async fn list_metric_experiment_groups(
    Extension(context): Extension<InternalRequestContext>,
) -> (StatusCode, Json<serde_json::Value>) {
    if !is_valid_tenant_id(&context.tenant_id) {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"租户不存在"})));
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({"experiment_groups": list_experiment_groups(&context.tenant_id)})),
    )
}

/// DELETE /api/v1/metrics/runs/:run_id — 删除实验记录。
pub async fn delete_metric_run(
    Extension(context): Extension<InternalRequestContext>,
    Path(run_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if !is_valid_tenant_id(&context.tenant_id) {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"租户不存在"})));
    }
    let path = match find_run_path(&context.tenant_id, &run_id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error":"不存在"})),
            );
        }
    };
    match std::fs::remove_file(&path) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok":true}))),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error":format!("{}",e)})),
        ),
    }
}

// ─── Block 匹配辅助函数 ──────────────────────────────────────────────────

/// 判定「可靠匹配」所需的最小 bigram 重叠率。
const MIN_OVERLAP: f64 = 0.15;

/// 判定「可靠匹配」所需的最小 bigram 命中数。
///
/// 至少命中 2 个 bigram，且命中率不低于 [`MIN_OVERLAP`]（向上取整）。
/// 这样短 quote（bigram 少）会被要求更高的命中率——例如 4 字符 quote 仅 3 个
/// bigram，命中 1 个的命中率 0.33 虽越过 0.15 门槛，但单点命中不足以视为可靠。
fn min_hits_required(n_bigrams: usize) -> usize {
    ((n_bigrams as f64 * MIN_OVERLAP).ceil() as usize).max(2)
}

/// 判断一个相邻二字组是否参与匹配：两个字符都必须是「有内容」的字符
/// （字母 / 汉字 / 数字），跳过空白与标点（含中文全角标点 ，。；：等）。
///
/// 注意不能只用 `is_ascii_punctuation()`——它不过滤中文全角标点；
/// `is_alphanumeric()` 对汉字与数字都返回 true，对全角/半角标点返回 false，
/// 正好满足「保留文字与数字、跳过标点」的需求。
fn bigram_is_meaningful(a: char, b: char) -> bool {
    a.is_alphanumeric() && b.is_alphanumeric()
}

/// 在 `chunk_text` 中寻找与 `source_quote` 的最佳匹配窗口位置。
///
/// 使用滑动窗口 + bigram 重叠率计算匹配分数。
/// 对中文文本，bigram（相邻二字组）能捕获字符顺序，比字符集重叠
/// 更具区分度，避免"投标人"与"招标投标"的误匹配。
///
/// 重叠率 = source_quote 的 bigram 在窗口中的命中数 / source_quote 的 bigram 总数。
/// 若最佳命中数低于 [`min_hits_required`]，返回 `None`，表示匹配不可靠。
fn find_quote_position(source_quote: &str, chunk_text: &str) -> Option<(usize, usize)> {
    const MIN_QUOTE_CHARS: usize = 4;

    let sq: Vec<char> = source_quote.chars().collect();
    let ct: Vec<char> = chunk_text.chars().collect();

    if sq.len() < MIN_QUOTE_CHARS || ct.is_empty() {
        return None;
    }

    // 从 source_quote 构建 bigram 集合（相邻二字组，跳过含空白/标点的）
    let sq_bigrams: Vec<(char, char)> = sq
        .windows(2)
        .filter(|w| bigram_is_meaningful(w[0], w[1]))
        .map(|w| (w[0], w[1]))
        .collect();

    if sq_bigrams.is_empty() {
        return None;
    }

    let min_hits = min_hits_required(sq_bigrams.len());

    // 从 chunk_text 构建所有位置的 bigram 集合（用于快速查找）
    let ct_bigram_set: std::collections::HashSet<(char, char)> = ct
        .windows(2)
        .filter(|w| bigram_is_meaningful(w[0], w[1]))
        .map(|w| (w[0], w[1]))
        .collect();

    // 计算全局 bigram 命中数（用于判断 source_quote 是否与 chunk 整体相关）
    let global_hits: usize = sq_bigrams
        .iter()
        .filter(|bg| ct_bigram_set.contains(bg))
        .count();

    if global_hits < min_hits {
        return None;
    }

    // 滑动窗口精确定位：在 chunk_text 上滑动，
    // 找到 bigram 命中密度最高的窗口
    let window_len = sq.len().min(ct.len());
    let step = (window_len / 4).max(1);

    let mut best_start = 0usize;
    let mut best_end = window_len;
    let mut best_score = 0usize;

    for start in (0..=ct.len().saturating_sub(window_len)).step_by(step) {
        let end = start + window_len;
        // 窗口内的 bigram 命中数
        let window_bigrams: std::collections::HashSet<(char, char)> = ct[start..end]
            .windows(2)
            .filter(|w| bigram_is_meaningful(w[0], w[1]))
            .map(|w| (w[0], w[1]))
            .collect();

        let hits: usize = sq_bigrams
            .iter()
            .filter(|bg| window_bigrams.contains(bg))
            .count();
        if hits > best_score {
            best_score = hits;
            best_start = start;
            best_end = end;
        }
    }

    if best_score < min_hits {
        return None;
    }

    Some((best_start, best_end))
}

/// 基于 `source_quote` 在 chunk text 中的匹配位置，从 `valid_blocks` 中
/// 选取最近的 `max_blocks` 个 block。
///
/// `valid_blocks` 为 `(block_id, 估计字符偏移)` 列表，偏移是每个 block 在
/// chunk.text 中的估计位置（调用处按真实文本长度累加得到）。函数据此计算
/// 每个 block 与匹配窗口的距离并排序，取最近的 `max_blocks` 个。
///
/// 若 `source_quote` 在 chunk.text 中匹配不可靠（命中数低于阈值），
/// 返回空 Vec，让前端降级为文本定位而不是错误的高亮 block。
fn select_blocks_by_source_quote(
    valid_blocks: &[(String, usize)],
    source_quote: &str,
    chunk_text: &str,
    max_blocks: usize,
) -> Vec<String> {
    let n = valid_blocks.len();
    if n == 0 {
        return Vec::new();
    }

    // 尝试定位 source_quote 在 chunk_text 中的匹配窗口
    let (match_start, _match_end) = match find_quote_position(source_quote, chunk_text) {
        Some(pos) => pos,
        None => {
            // 匹配不可靠 — 返回空，让前端走文本定位
            return Vec::new();
        }
    };

    // 按每个 block 的估计偏移与匹配位置的距离排序
    let mut scored: Vec<(usize, &String)> = valid_blocks
        .iter()
        .map(|(bid, offset)| (offset.abs_diff(match_start), bid))
        .collect();

    // 按距离升序排列，距离最近的在前
    scored.sort_by_key(|(d, _)| *d);

    scored
        .into_iter()
        .take(max_blocks)
        .map(|(_, bid)| bid.clone())
        .collect()
}

/// 在页面的词序列（阅读顺序）中定位 `source_quote`，返回命中词的逐行紧致包围盒。
///
/// 与 `select_blocks_by_source_quote` 共用同一套 bigram 定位（`find_quote_position`），
/// 区别是匹配粒度在「词」而非「段落块」，因此返回的矩形紧贴原句、不会盖住整段。
/// 每个返回值对应一个视觉行（同行命中词合并为一个矩形）。
///
/// 匹配不可靠时返回空 `Vec`，前端回落到 block 级高亮 + 文本层收敛。
fn match_words_to_quote(words: &[(String, BBox)], source_quote: &str) -> Vec<BBox> {
    if words.is_empty() || source_quote.trim().is_empty() {
        return Vec::new();
    }

    // 1. 按阅读顺序「无分隔」拼接词文本（中文词间本无空格），并记录每个词的字符区间。
    //    字符偏移与 `find_quote_position` 的 char 语义保持一致（UTF-8 按 char 而非字节）。
    let mut word_text = String::new();
    let mut ranges: Vec<(usize, usize, usize)> = Vec::with_capacity(words.len());
    for (i, (text, bbox)) in words.iter().enumerate() {
        let start = word_text.chars().count();
        word_text.push_str(text);
        let end = word_text.chars().count();
        if end > start && (bbox.bottom - bbox.top) > 0.0 {
            ranges.push((i, start, end));
        }
    }

    // 2. 去掉 source_quote 空白，与无分隔拼接的 word_text 对齐
    //   （英文短语 / LLM 输出可能带空格）。
    let compact_quote: String = source_quote.chars().filter(|c| !c.is_whitespace()).collect();
    if compact_quote.is_empty() {
        return Vec::new();
    }

    // 3. 定位匹配范围：逐字引用是最常见情形，先做精确子串匹配（字节偏移 → 字符偏移）；
    //    引用带标点 / OCR 误差导致不逐字相等时，回落到 bigram 滑动窗口。
    let (match_start, match_end): (usize, usize) = match word_text.find(&compact_quote) {
        Some(byte_start) => {
            let char_start = word_text[..byte_start].chars().count();
            (char_start, char_start + compact_quote.chars().count())
        }
        None => match find_quote_position(&compact_quote, &word_text) {
            Some(pos) => pos,
            None => return Vec::new(),
        },
    };

    // 3. 找出与匹配窗口相交的词
    let mut hit_idx: Vec<usize> = Vec::new();
    for &(i, start, end) in &ranges {
        if start < match_end && match_start < end {
            hit_idx.push(i);
        }
    }
    if hit_idx.is_empty() {
        return Vec::new();
    }

    // 4. 按视觉行分组（top 接近视为同行），每行合并为一个紧致矩形。
    //    行分组阈值参照 compute_blocks：line_height * 1.2。
    let hit_boxes: Vec<&BBox> = hit_idx.iter().map(|&i| &words[i].1).collect();
    let mut heights: Vec<f64> = hit_boxes.iter().map(|b| b.bottom - b.top).collect();
    heights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let line_height = heights[heights.len() / 2];

    let mut sorted: Vec<&BBox> = hit_boxes.clone();
    sorted.sort_by(|a, b| {
        a.top
            .partial_cmp(&b.top)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal))
    });

    let tol = (line_height * 1.2).max(6.0);
    let mut lines: Vec<BBox> = Vec::new();
    for b in sorted {
        if let Some(last) = lines.last_mut() {
            if b.top - last.top < tol {
                last.x0 = last.x0.min(b.x0);
                last.top = last.top.min(b.top);
                last.x1 = last.x1.max(b.x1);
                last.bottom = last.bottom.max(b.bottom);
                continue;
            }
        }
        lines.push(b.clone());
    }
    lines
}

#[cfg(test)]
mod block_matching_tests {
    use super::*;

    /// 构造一个模拟 chunk.text：N 个 block，每个 block 的文本唯一，以 "\n" 拼接。
    fn make_chunk_text(block_texts: &[&str]) -> String {
        block_texts.join("\n")
    }

    /// 计算每个 block 在 chunk.text 中的字符中心偏移。
    ///
    /// 与调用处一致：偏移按 block 文本长度累加（此处额外计入 "\n" 分隔符，
    /// 使偏移与 make_chunk_text 生成的 chunk.text 字符位置精确对应），
    /// 每个 block 用其「中心」偏移代表其位置。
    fn block_centers(block_texts: &[&str]) -> Vec<usize> {
        let mut acc = 0usize;
        let mut centers = Vec::with_capacity(block_texts.len());
        for t in block_texts {
            let len = t.chars().count();
            centers.push(acc + len / 2);
            acc += len + 1; // +1 对应 "\n" 分隔符
        }
        centers
    }

    /// 将 block id 与字符中心偏移配对成 `(block_id, offset)` 列表。
    fn make_valid_blocks(prefix: &str, block_texts: &[&str]) -> Vec<(String, usize)> {
        block_centers(block_texts)
            .into_iter()
            .enumerate()
            .map(|(i, off)| (format!("{}_{}", prefix, i), off))
            .collect()
    }

    #[test]
    fn find_quote_position_strong_match() {
        let chunk = "第一条 投标人资格要求。投标人须为中华人民共和国境内注册的企业法人。";
        let quote = "投标人须为中华人民共和国境内注册";
        let pos = find_quote_position(quote, chunk);
        assert!(pos.is_some(), "强匹配应返回位置");
    }

    #[test]
    fn find_quote_position_no_match() {
        let chunk = "第一条 项目概况与招标范围。本项目位于北京市朝阳区。";
        let quote = "投标人须具有独立法人资格";
        let pos = find_quote_position(quote, chunk);
        assert!(pos.is_none(), "无重叠应返回 None");
    }

    #[test]
    fn find_quote_position_short_quote_returns_none() {
        let chunk = "第一章 总则";
        let quote = "第";
        let pos = find_quote_position(quote, chunk);
        assert!(pos.is_none(), "过短的 quote（<4 字符）应返回 None");
    }

    #[test]
    fn find_quote_position_single_bigram_hit_not_reliable() {
        // 4 字符 quote 仅 3 个 bigram；只命中 1 个时命中率 0.33 虽越过旧阈值 0.15，
        // 但单点命中不足以视为可靠，现在应返回 None。
        let chunk = "本项目采用公开投标方式。";
        let quote = "投标人须";
        let pos = find_quote_position(quote, chunk);
        assert!(pos.is_none(), "仅 1 个 bigram 命中不应判定为可靠");
    }

    #[test]
    fn select_blocks_returns_empty_when_match_unreliable() {
        // chunk.text 与 source_quote 无交集
        let valid_blocks: Vec<(String, usize)> = (0..10)
            .map(|i| (format!("b_1_{}", i), i))
            .collect();
        let chunk_text =
            "第一章 总则。本办法适用于所有政府采购项目的招标投标活动。".repeat(5);
        let source_quote = "投标人须为本省注册企业且具有独立法人资格";

        let result =
            select_blocks_by_source_quote(&valid_blocks, source_quote, &chunk_text, 5);
        assert!(
            result.is_empty(),
            "不可靠匹配应返回空，让前端走文本定位"
        );
    }

    #[test]
    fn select_blocks_prefers_latter_half_when_evidence_there() {
        // 模拟 10 个 block 的大 chunk，证据位于后半段
        let block_texts: Vec<&str> = vec![
            "第一条 总则。本办法依据《中华人民共和国招标投标法》制定。",
            "第二条 适用范围。本办法适用于所有政府采购项目。",
            "第三条 基本原则。招标投标活动应遵循公开、公平、公正原则。",
            "第四条 采购人职责。采购人应对采购需求的合法性负责。",
            "第五条 代理机构。采购代理机构应具备相应的资格条件。",
            "第六条 招标文件。招标文件不得包含歧视性条款。",
            "第七条 投标人资格。投标人须为中华人民共和国境内注册的企业法人。",
            "第八条 联合体投标。两个以上法人可组成联合体参与投标。",
            "第九条 投标保证金。投标保证金不得超过项目估算价的2%。",
            "第十条 开标程序。开标应在招标文件确定的提交投标文件截止时间公开进行。",
        ];
        let chunk_text = make_chunk_text(&block_texts);
        let valid_blocks = make_valid_blocks("b_1", &block_texts);

        // 证据在第九条（block_texts[8]，index 8）：投标保证金不得超过项目估算价的2%
        let source_quote = "投标保证金不得超过项目估算价的2%";

        let result =
            select_blocks_by_source_quote(&valid_blocks, source_quote, &chunk_text, 3);

        assert!(!result.is_empty(), "应找到匹配的 block");
        // 第九条的 block 是 b_1_8（index 8），应在结果中排在前面
        assert!(
            result.contains(&"b_1_8".to_string()),
            "结果应包含证据所在 block b_1_8（第九条），实际: {:?}",
            result
        );
    }

    #[test]
    fn select_blocks_falls_back_to_empty_for_very_different_texts() {
        let valid_blocks: Vec<(String, usize)> = (0..6)
            .map(|i| (format!("b_2_{}", i), i))
            .collect();
        let chunk_text = "项目名称：XX市污水处理厂建设工程。建设地点：XX市南郊。工期：365天。";
        let source_quote = "投标人须具备有效的安全生产许可证且在有效期内";

        let result =
            select_blocks_by_source_quote(&valid_blocks, source_quote, chunk_text, 5);
        assert!(
            result.is_empty(),
            "完全不相关的 source_quote 应返回空 block_ids"
        );
    }

    #[test]
    fn select_blocks_respects_max_blocks_limit() {
        let block_texts: Vec<String> = (0..20)
            .map(|i| format!("第{}条 条款内容文本占位。", i + 1))
            .collect();
        let block_refs: Vec<&str> = block_texts.iter().map(|s| s.as_str()).collect();
        let chunk_text = make_chunk_text(&block_refs);
        let valid_blocks = make_valid_blocks("b_3", &block_refs);

        let source_quote = "第15条 条款内容文本占位";

        let result =
            select_blocks_by_source_quote(&valid_blocks, source_quote, &chunk_text, 5);
        assert!(
            result.len() <= 5,
            "返回的 block 数不应超过 max_blocks=5，实际: {}",
            result.len()
        );
        assert!(
            result.contains(&"b_3_14".to_string()),
            "应包含证据所在 block b_3_14（第15条，index 14），实际: {:?}",
            result
        );
    }

    #[test]
    fn select_blocks_handles_non_uniform_block_lengths() {
        // 非均匀场景：前 9 个 block 极短，最后一个 block 极长（800+ 字符），
        // 证据落在长 block 的中段。若按 index 比例估算偏移，长 block 会被
        // 误估到 chunk 末尾，导致选中错误的短 block；按真实文本长度累加的
        // 中心偏移则能正确选中长 block（index 9）。
        let mut block_texts: Vec<String> = (0..9)
            .map(|i| format!("第{}条 短条款。", i + 1))
            .collect();
        let long_block = format!(
            "第十条 详细说明。{}投标保证金不得超过项目估算价的2%。{}",
            "内容".repeat(200),
            "内容".repeat(200),
        );
        block_texts.push(long_block);

        let block_refs: Vec<&str> = block_texts.iter().map(|s| s.as_str()).collect();
        let chunk_text = make_chunk_text(&block_refs);
        let valid_blocks = make_valid_blocks("b_nu", &block_refs);

        let source_quote = "投标保证金不得超过项目估算价的2%";
        let result =
            select_blocks_by_source_quote(&valid_blocks, source_quote, &chunk_text, 3);

        assert!(
            result.contains(&"b_nu_9".to_string()),
            "应按真实偏移选中长 block b_nu_9（第十条，index 9），实际: {:?}",
            result
        );
    }

    // ─── match_words_to_quote 测试 ────────────────────────────────────────

    /// 构造一行词：每个词按「字数 ×10pt」给宽、高 12pt，行首 x=10。
    /// 用于验证词级高亮返回的矩形紧贴命中的词、不吞掉相邻词。
    fn words_on_line(top: f64, texts: &[&str]) -> Vec<(String, BBox)> {
        let mut x = 10.0f64;
        texts
            .iter()
            .map(|t| {
                let w = t.chars().count() as f64 * 10.0;
                let b = BBox {
                    x0: x,
                    top,
                    x1: x + w,
                    bottom: top + 12.0,
                };
                x += w;
                (t.to_string(), b)
            })
            .collect()
    }

    #[test]
    fn match_words_to_quote_single_line_tight_box() {
        let words = words_on_line(
            100.0,
            &["本", "项目", "要求", "投标人", "须", "在本市", "注册", "三年"],
        );
        let quote = "投标人须在本市注册";
        let rects = match_words_to_quote(&words, quote);
        assert_eq!(rects.len(), 1, "单行命中应合并为 1 个矩形，实际: {:?}", rects);
        let r = &rects[0];
        assert!(
            (r.x0 - words[3].1.x0).abs() < 1e-6,
            "x0 应紧贴首个命中词（投标人），实际: {:.3} vs 期望 {:.3}",
            r.x0,
            words[3].1.x0
        );
        assert!(
            (r.x1 - words[6].1.x1).abs() < 1e-6,
            "x1 应紧贴末个命中词（注册），不得吞掉「三年」"
        );
        assert!((r.top - 100.0).abs() < 1e-9 && (r.bottom - 112.0).abs() < 1e-9);
    }

    #[test]
    fn match_words_to_quote_two_lines_two_boxes() {
        let mut words = words_on_line(100.0, &["投标人", "须", "为中华"]);
        words.extend(words_on_line(
            130.0,
            &["人民", "共和国", "境内", "注册", "的企业"],
        ));
        let quote = "投标人须为中华人民共和国境内注册";
        let rects = match_words_to_quote(&words, quote);
        assert_eq!(rects.len(), 2, "跨两行应得到 2 个矩形，实际: {:?}", rects);
        assert!((rects[0].top - 100.0).abs() < 1e-9, "第一行 top 应为 100");
        assert!((rects[1].top - 130.0).abs() < 1e-9, "第二行 top 应为 130");
        // 第二行 x1 应停在「注册」，不吞掉「的企业」
        assert!(
            (rects[1].x1 - words[6].1.x1).abs() < 1e-6,
            "第二行 x1 应紧贴注册，实际: {:.3} vs 期望 {:.3}",
            rects[1].x1,
            words[6].1.x1
        );
    }

    #[test]
    fn match_words_to_quote_unreliable_returns_empty() {
        let words = words_on_line(100.0, &["本", "项目", "位于", "北京", "市", "朝阳"]);
        let quote = "投标人须具有独立法人资格";
        assert!(
            match_words_to_quote(&words, quote).is_empty(),
            "无重叠 quote 应返回空，前端回落 block 级高亮"
        );
    }

    #[test]
    fn match_words_to_quote_empty_inputs_return_empty() {
        let words = words_on_line(100.0, &["本", "项目"]);
        assert!(match_words_to_quote(&words, "").is_empty());
        assert!(match_words_to_quote(&[], "投标人").is_empty());
    }

    #[test]
    fn match_words_to_quote_english_with_spaces() {
        let words = words_on_line(50.0, &["the", "quick", "brown", "fox", "jumps"]);
        let quote = "the quick brown";
        let rects = match_words_to_quote(&words, quote);
        assert_eq!(rects.len(), 1, "英文带空格 quote 应命中单行，实际: {:?}", rects);
        let r = &rects[0];
        assert!((r.x0 - words[0].1.x0).abs() < 1e-6, "x0 应贴 the");
        assert!((r.x1 - words[2].1.x1).abs() < 1e-6, "x1 应贴 brown，不吞掉 fox/jumps");
    }
}
