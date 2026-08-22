//! HTTP 请求处理函数 — 薄胶水层。
//!
//! 不写业务逻辑，只负责：
//! 1. 解析 HTTP 请求（JSON / multipart）
//! 2. 调用现有核心函数（services / agents）
//! 3. 格式化 HTTP 响应（JSON + 状态码）

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use axum::Json;
use axum::extract::{Multipart, Path, State};
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
    ReviewClause, TextSelection,
};
use crate::domain::chunk::{Chunk, ChunkingConfig};
use crate::domain::raw_document::RawDocument;
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

// ─── 应用状态 ───────────────────────────────────────────────────────

/// 服务全局共享状态。
#[derive(Clone)]
pub struct AppState {
    /// 文档缓存：document_id → 已处理文档
    pub documents: Arc<TokioRwLock<HashMap<String, Arc<DocumentState>>>>,
    /// 嵌入客户端（BGE-M3，启动时加载一次）
    pub embed_client: Arc<StdMutex<Option<Arc<EmbeddingClient>>>>,
    /// DashScope 联网搜索
    pub dashscope_search: Option<Arc<DashScopeSearchBackend>>,
    /// 搜索后端类型（dashscope / searxng）
    pub search_backend: String,
    /// 嵌入引擎类型（local / remote）
    pub embed_engine: String,
    /// SSE 实时推送通道：doc_id → ReviewEventBus
    pub review_event_buses: Arc<TokioMutex<HashMap<String, Arc<ReviewEventBus>>>>,
    /// 异步审查结果缓存：doc_id → CoordinatorOutput
    pub review_results: Arc<TokioMutex<HashMap<String, CoordinatorOutput>>>,
    /// 异步审查失败信息：doc_id → 错误消息
    pub review_errors: Arc<TokioMutex<HashMap<String, String>>>,
    /// 正在执行的审核任务：doc_id（用于并发控制，防止重复提交）
    pub active_reviews: Arc<TokioMutex<HashSet<String>>>,
}

use std::sync::Mutex as StdMutex;

/// 单个文档的处理状态。
pub struct DocumentState {
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
            documents: Arc::new(TokioRwLock::new(HashMap::new())),
            embed_client: Arc::new(StdMutex::new(embed_client)),
            dashscope_search,
            search_backend,
            embed_engine,
            review_event_buses: Arc::new(TokioMutex::new(HashMap::new())),
            review_results: Arc::new(TokioMutex::new(HashMap::new())),
            review_errors: Arc::new(TokioMutex::new(HashMap::new())),
            active_reviews: Arc::new(TokioMutex::new(HashSet::new())),
        })
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot: Option<crate::agents::types::GraphSnapshot>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ReviewResultResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ReviewResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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
    mut multipart: Multipart,
) -> Result<Json<ProcessResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    let tmp_dir = data_path_str("tmp");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| server_error("创建临时目录失败", e))?;
    let stem = Uuid::new_v4().to_string();
    let ext = std::path::Path::new(&filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("pdf");
    let tmp_path = format!("{}/{}.{}", tmp_dir, stem, ext);
    std::fs::write(&tmp_path, &file_data).map_err(|e| server_error("写入临时文件失败", e))?;

    // DOCX → PDF 转换（对齐 CLI 行为）
    let pdf_path = if ext == "docx" || ext == "doc" {
        println!("[STAGE] DOCX → PDF 转换...");
        convert_docx_to_pdf(&tmp_path, &tmp_dir).map_err(|e| server_error("DOCX 转 PDF 失败", e))?
    } else {
        std::path::PathBuf::from(&tmp_path)
    };

    // 阶段 1: PDF → RawDocument（Rust 主路径 + Python 兜底）
    println!("[STAGE] PDF 文本提取...");
    let pdf_path_str = pdf_path.to_str().unwrap_or(&tmp_path).to_string();
    let raw_doc: RawDocument = match extract_pdf_to_raw_json(&pdf_path_str) {
        Ok(doc) => {
            println!("Rust pdfplumber 解析成功");
            doc
        }
        Err(e) => {
            println!("Rust pdfplumber 失败: {}", e);
            println!("切换到 Python pdfplumber 兜底提取...");
            let fallback_json = format!("{}/{}_python_fallback_raw.json", tmp_dir, stem);
            extract_with_python(&pdf_path_str, &fallback_json)
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

    // 构建磁盘输出用的 stem：{原始文件名}_{uuid前8位}
    let file_stem = std::path::Path::new(&filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("document");
    let disk_stem = format!("{}_{}", file_stem, &stem[..8.min(stem.len())]);

    // ── 写盘：raw_json ──
    {
        let dir = data_path_str("output/raw_json");
        let _ = std::fs::create_dir_all(&dir);
        let path = format!("{}/{}_raw.json", dir, disk_stem);
        if let Ok(json) = serde_json::to_string_pretty(&raw_doc) {
            let _ = std::fs::write(&path, json);
            println!("[DISK] raw_json → {}", path);
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
        let dir = data_path_str("output/sections");
        let _ = std::fs::create_dir_all(&dir);
        let path = format!("{}/{}_sections.json", dir, disk_stem);
        if let Ok(json) = serde_json::to_string_pretty(&all_sections) {
            let _ = std::fs::write(&path, json);
            println!("[DISK] sections → {}", path);
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
        let dir = data_path_str("output/chunks");
        let _ = std::fs::create_dir_all(&dir);
        let path = format!("{}/{}_chunks.json", dir, disk_stem);
        if let Ok(json) = serde_json::to_string_pretty(&chunks) {
            let _ = std::fs::write(&path, json);
            println!("[DISK] chunks → {}", path);
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
        let dir = data_path_str("output/embeddings");
        if let Err(e) = crate::services::embedding_service::save_index(&doc_index, &dir, &disk_stem)
        {
            eprintln!("[DISK] embeddings 写入失败: {}", e);
        } else {
            println!("[DISK] embeddings → {}/{}_embedding_index/", dir, disk_stem);
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

    let doc_id = stem.clone();
    let doc_state = Arc::new(DocumentState {
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
        .insert(doc_id.clone(), doc_state);

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
    Path(doc_id): Path<String>,
) -> Result<Json<DocumentInfo>, (StatusCode, Json<ErrorResponse>)> {
    let docs = state.documents.read().await;
    let doc = docs
        .get(&doc_id)
        .ok_or_else(|| not_found(&format!("文档不存在: {}", doc_id)))?;
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
        (status = 404, description = "Document not found", body = ErrorResponse),
        (status = 409, description = "Review already in progress", body = ReviewAccepted)
    )
)]
#[axum::debug_handler]
pub async fn review_document(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    Json(req): Json<ReviewRequest>,
) -> Result<(StatusCode, Json<ReviewAccepted>), (StatusCode, Json<ErrorResponse>)> {
    let docs = state.documents.read().await;
    let doc = docs
        .get(&doc_id)
        .ok_or_else(|| not_found(&format!("文档不存在: {}", doc_id)))?
        .clone();
    drop(docs);

    println!(
        "[REQ] 启动异步审核: doc_id={}, filename={}",
        doc_id, doc.filename
    );

    // 并发控制：检查是否已有进行中的审核（用 active_reviews 标记而非 bus 存在性）
    {
        let mut active = state.active_reviews.lock().await;
        if active.contains(&doc_id) {
            return Ok((
                StatusCode::CONFLICT,
                Json(ReviewAccepted {
                    status: "conflict".to_string(),
                    document_id: doc_id,
                    message: "该文档已有进行中的审核任务".to_string(),
                }),
            ));
        }
        active.insert(doc_id.clone());
    }

    // 创建或获取 ReviewEventBus（SSE 客户端可能已提前连接）
    let review_events = {
        let mut buses = state.review_event_buses.lock().await;
        buses
            .entry(doc_id.clone())
            .or_insert_with(|| Arc::new(ReviewEventBus::new(review_event_capacity())))
            .clone()
    };

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

    println!(
        "[REQ] 审核条款数: {}, 启用 Agent: {:?}",
        review_clauses.len(),
        req.enabled_agents
    );

    // 提取后台任务所需数据（脱离 doc 引用）
    let enabled_agents = req.enabled_agents.clone();
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

    // 后台执行管线
    let state_for_task = state.clone();
    let doc_id_for_task = doc_id.clone();
    tokio::spawn(async move {
        run_review_pipeline(
            state_for_task,
            doc_id_for_task,
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
    doc_id: String,
    review_clauses: Vec<ReviewClause>,
    enabled_agents: Option<Vec<String>>,
    chunk_map: Arc<HashMap<String, Chunk>>,
    review_chunk_map: Arc<HashMap<String, Chunk>>,
    doc_index: Arc<DocumentVectorIndex>,
    chunk_order: Arc<Vec<String>>,
    redaction_vault: Arc<RedactionVault>,
    dashscope_search: Option<Arc<DashScopeSearchBackend>>,
    search_backend: String,
    embed_client_for_tools: Option<Arc<EmbeddingClient>>,
    review_events: Arc<ReviewEventBus>,
) {
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
    if let Some(ref agent_names) = enabled_agents {
        coord_config.enabled_agents = agent_names
            .iter()
            .filter_map(|s| AgentId::parse(s))
            .collect();
    }

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
        .with_review_events(review_events.clone())
        .with_metrics(metrics.clone()),
    );

    println!("[STAGE] Multi-Agent 审核中 (async)...");
    match coordinator.review(&review_clauses).await {
        Ok(mut output) => {
            let duration_secs = start_time.elapsed().as_secs_f64();
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
                    let source_quote = finding.source_quote.clone();
                    let valid_blocks: Vec<String> = chunk
                        .source_block_ids
                        .iter()
                        .filter(|bid| {
                            chunk.bbox_refs.iter().any(|r| {
                                let is_same = &r.block_id == *bid;
                                let is_placeholder =
                                    r.bbox.x0 == 0.0 && r.bbox.x1 == 400.0
                                        && (r.bbox.bottom - r.bbox.top) <= 20.1;
                                is_same && !is_placeholder
                            })
                        })
                        .cloned()
                        .collect();

                    // 如果经过滤后为空（全是占位 bbox），则不退化为文本匹配，
                    // 保持空数组让前端走文本高亮路径。
                    // 如果仍有过多有效 block（如大 section），取最多前 5 个。
                    let max_blocks = 5usize;
                    finding.block_ids = if valid_blocks.len() > max_blocks {
                        // 优选与 source_quote 文本相关的 block
                        let truncated: Vec<String> = valid_blocks
                            .into_iter()
                            .take(max_blocks)
                            .collect();
                        truncated
                    } else {
                        valid_blocks
                    };

                    let _ = source_quote; // 预留后续按文本相关性排序
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
                let disk_stem = {
                    let docs = state.documents.read().await;
                    if let Some(doc) = docs.get(&doc_id) {
                        let file_stem = std::path::Path::new(&doc.filename)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("document");
                        format!("{}_{}", file_stem, &doc_id[..8.min(doc_id.len())])
                    } else {
                        format!("doc_{}", &doc_id[..8.min(doc_id.len())])
                    }
                };
                let dir = data_path_str("output/findings");
                let _ = std::fs::create_dir_all(&dir);
                let findings_path = format!("{}/{}_findings.json", dir, disk_stem);
                if let Ok(json) = serde_json::to_string_pretty(&output.findings) {
                    let _ = std::fs::write(&findings_path, json);
                    println!("[DISK] findings → {}", findings_path);
                }
                let summary_path = format!("{}/{}_routing_summary.json", dir, disk_stem);
                if let Ok(json) = serde_json::to_string_pretty(&output.routing_summary) {
                    let _ = std::fs::write(&summary_path, json);
                }
                if let Some(ref snap) = output.graph_snapshot {
                    let snap_path = format!("{}/{}_graph_snapshot.json", dir, disk_stem);
                    if let Ok(json) = serde_json::to_string_pretty(snap) {
                        let _ = std::fs::write(&snap_path, json);
                    }
                }
            }

            // 存入 review_results 供 GET /result 查询
            {
                let mut results = state.review_results.lock().await;
                results.insert(doc_id.clone(), output.clone());
            }

            // 写盘: {doc_id}_result.json — 重启后磁盘 fallback
            {
                let dir = data_path_str("output/findings");
                let _ = std::fs::create_dir_all(&dir);
                let result_path = format!("{}/{}_result.json", dir, doc_id);
                let persisted = ReviewResultResponse {
                    status: "completed".to_string(),
                    result: Some(ReviewResponse {
                        document_id: doc_id.clone(),
                        findings: output.findings.clone(),
                        routing_summary: output.routing_summary.clone(),
                        graph_snapshot: output.graph_snapshot.clone(),
                    }),
                    error: None,
                };
                if let Ok(json) = serde_json::to_string_pretty(&persisted) {
                    let _ = std::fs::write(&result_path, json);
                    println!("[DISK] result → {}", result_path);
                }
            }

            // ── 指标：写盘 ──
            {
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

                let run_id = chrono::Local::now().format("%Y%m%dT%H%M%S").to_string();
                let meta = crate::metrics::RunMeta {
                    run_id: run_id.clone(),
                    title: None,
                    notes: None,
                    experiment_group: None,
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
                    },
                };
                let run_metrics = collector.finalize(meta);

                let runs_dir = data_path_str("output/runs");
                let _ = std::fs::create_dir_all(&runs_dir);
                let run_path = format!("{}/{}.json", runs_dir, run_id);
                if let Ok(json) = serde_json::to_string_pretty(&run_metrics) {
                    let _ = std::fs::write(&run_path, json);
                    println!("[METRICS] → {}", run_path);
                }
            }

            // 发送 Done 事件
            review_events.emit(&crate::agents::review_event::ReviewEvent::Done {
                total_findings: output.findings.len(),
                high_risk: high_risk_count,
                session_id: doc_id.clone(),
                duration_secs,
            });
        }
        Err(e) => {
            let msg = format!("审核引擎执行失败: {}", e);
            eprintln!("[ERROR] async review failed: doc_id={}, {}", doc_id, msg);

            // 存入 review_errors
            {
                let mut errors = state.review_errors.lock().await;
                errors.insert(doc_id.clone(), msg.clone());
            }

            // 发送 Error 事件
            review_events.emit(&crate::agents::review_event::ReviewEvent::Error {
                message: msg,
                session_id: doc_id.clone(),
            });
        }
    }

    // 延迟清理 ReviewEventBus 和 active_reviews
    // （给 SSE 客户端时间接收 Done/Error 事件）
    let cleanup_doc_id = doc_id.clone();
    let cleanup_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let mut buses = cleanup_state.review_event_buses.lock().await;
        buses.remove(&cleanup_doc_id);
        let mut active = cleanup_state.active_reviews.lock().await;
        active.remove(&cleanup_doc_id);
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
    Path(doc_id): Path<String>,
) -> axum::response::Sse<
    impl futures_core::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
> {
    use axum::response::sse::Event;

    // 创建或获取 ReviewEventBus（如果 POST /review 尚未创建）
    let review_events = {
        let mut buses = state.review_event_buses.lock().await;
        buses
            .entry(doc_id.clone())
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

    axum::response::Sse::new(stream)
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
        (status = 200, description = "Review result (status: completed/pending/failed)", body = ReviewResultResponse),
        (status = 404, description = "No review record found", body = ErrorResponse)
    )
)]
pub async fn get_review_result(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
) -> Result<Json<ReviewResultResponse>, (StatusCode, Json<ErrorResponse>)> {
    // 1. 检查内存中已完成的结果（不移除，允许多次查询）
    {
        let results = state.review_results.lock().await;
        if let Some(output) = results.get(&doc_id) {
            return Ok(Json(ReviewResultResponse {
                status: "completed".to_string(),
                result: Some(ReviewResponse {
                    document_id: doc_id,
                    findings: output.findings.clone(),
                    routing_summary: output.routing_summary.clone(),
                    graph_snapshot: output.graph_snapshot.clone(),
                }),
                error: None,
            }));
        }
    }

    // 2. 检查失败信息
    {
        let errors = state.review_errors.lock().await;
        if let Some(msg) = errors.get(&doc_id) {
            return Ok(Json(ReviewResultResponse {
                status: "failed".to_string(),
                result: None,
                error: Some(msg.clone()),
            }));
        }
    }

    // 3. 检查是否仍在进行中
    {
        let buses = state.review_event_buses.lock().await;
        if buses.contains_key(&doc_id) {
            return Ok(Json(ReviewResultResponse {
                status: "pending".to_string(),
                result: None,
                error: None,
            }));
        }
    }

    // 4. 磁盘 fallback — 重启后内存为空，从 JSON 文件恢复
    {
        let dir = data_path_str("output/findings");
        let result_path = format!("{}/{}_result.json", dir, doc_id);
        if let Ok(json) = std::fs::read_to_string(&result_path)
            && let Ok(result) = serde_json::from_str::<ReviewResultResponse>(&json)
        {
            println!("[DISK] result loaded from disk: {}", result_path);
            return Ok(Json(result));
        }
    }

    Err(not_found(&format!("审查结果不存在: {}", doc_id)))
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
    Path(doc_id): Path<String>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, Json<ErrorResponse>)> {
    let docs = state.documents.read().await;
    let doc = docs
        .get(&doc_id)
        .ok_or_else(|| not_found(&format!("文档不存在: {}", doc_id)))?
        .clone();
    drop(docs);

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
    Path(doc_id): Path<String>,
    Json(req): Json<ChatRequest>,
) -> axum::response::Sse<
    impl futures_core::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
> {
    use axum::response::sse::Event;

    // All setup + streaming in a single async_stream block
    // (each async_stream::stream! creates a unique type — can't have early returns)
    let stream = async_stream::stream! {
        // ── Setup (inside stream to avoid type mismatch) ──
        let docs = state.documents.read().await;
        let doc = match docs.get(&doc_id) {
            Some(d) => d.clone(),
            None => {
                yield Ok(Event::default()
                    .event("error")
                    .data(r#"{"message":"文档不存在"}"#));
                return;
            }
        };
        drop(docs);

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

    axum::response::Sse::new(stream)
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
    Path(doc_id): Path<String>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, (StatusCode, Json<ErrorResponse>)> {
    let docs = state.documents.read().await;
    let doc = docs
        .get(&doc_id)
        .ok_or_else(|| not_found(&format!("文档不存在: {}", doc_id)))?
        .clone();
    drop(docs);

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
    Path(doc_id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<BlockQuery>,
) -> Result<Json<Vec<BlockBBoxResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let docs = state.documents.read().await;
    let doc = docs
        .get(&doc_id)
        .ok_or_else(|| not_found(&format!("文档不存在: {}", doc_id)))?;

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
            error: "NOT_FOUND".to_string(),
            detail: msg.to_string(),
        }),
    )
}

// ─── Metrics Helpers ────────────────────────────────────────────────────

/// 递归扫描 output/runs/ 下所有 .json 文件，返回 (相对文件夹路径, 文件路径)。
fn list_run_files() -> Vec<(Option<String>, std::path::PathBuf)> {
    let base = crate::paths::data_path_str("output/runs");
    let mut files = Vec::new();
    let _ = scan_dir(&base, None, &mut files);
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
fn find_run_path(run_id: &str) -> Option<std::path::PathBuf> {
    list_run_files()
        .into_iter()
        .map(|(_, path)| path)
        .find(|path| path.file_stem().and_then(|s| s.to_str()) == Some(run_id))
}

/// 列出所有实验组名称。
fn list_experiment_groups() -> Vec<String> {
    let base = crate::paths::data_path_str("output/runs");
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
pub async fn list_metric_runs() -> (StatusCode, Json<serde_json::Value>) {
    let mut summaries: Vec<MetricRunSummary> = Vec::new();

    for (experiment_group, path) in list_run_files() {
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
pub async fn get_metric_run(Path(run_id): Path<String>) -> (StatusCode, Json<serde_json::Value>) {
    let path = match find_run_path(&run_id) {
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
    Path(run_id): Path<String>,
    Json(body): Json<UpdateTagsRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let path = match find_run_path(&run_id) {
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
    Path(run_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    let path = format!(
        "{}/{}.json",
        crate::paths::data_path_str("output/runs"),
        run_id
    );
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
    Path(run_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    let path = format!(
        "{}/{}.json",
        crate::paths::data_path_str("output/runs"),
        run_id
    );
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
    Path(run_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    let old_path = match find_run_path(&run_id) {
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
    let base = crate::paths::data_path_str("output/runs");
    let new_dir = if let Some(ref g) = group {
        if g.is_empty() {
            base.clone()
        } else {
            format!("{}/{}", base, g)
        }
    } else {
        base.clone()
    };
    let _ = std::fs::create_dir_all(&new_dir);
    let fname = old_path.file_name().unwrap();
    let new_path = std::path::Path::new(&new_dir).join(fname);
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

/// GET /api/v1/metrics/experiment-groups — 列出所有实验组。
pub async fn list_metric_experiment_groups() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::OK,
        Json(serde_json::json!({"experiment_groups": list_experiment_groups()})),
    )
}

/// DELETE /api/v1/metrics/runs/:run_id — 删除实验记录。
pub async fn delete_metric_run(
    Path(run_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let path = match find_run_path(&run_id) {
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
