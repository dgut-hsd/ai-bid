//! 知识库（法规/标准库）导入接口
//!
//! POST /api/v1/knowledge/ingest —— 供 Java 上传后异步触发，也支持 curl 手动导入。

use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;

// 注意：handlers.rs 中 bad_request / server_error 均为私有函数，不可跨模块 use，
// 此处按 handlers.rs 的 ErrorResponse { error, detail } 结构自行复制同构实现。
use crate::api::handlers::{AppState, ErrorResponse};
use crate::services::knowledge_ingest_service::{ingest_bytes, IngestResult};

fn bad_request(msg: &str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: msg.to_string(),
            detail: msg.to_string(),
        }),
    )
}

fn server_error(msg: &str, e: impl std::fmt::Display) -> (StatusCode, Json<ErrorResponse>) {
    eprintln!("[ERROR] {}: {:#}", msg, e);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: format!("{}: {:#}", msg, e),
            detail: format!("{}: {:#}", msg, e),
        }),
    )
}

/// 入库响应
#[derive(Debug, Serialize)]
pub struct IngestResponse {
    pub document_id: String,
    pub document_name: String,
    pub chunk_count: usize,
    pub dimension: u64,
    pub collection: String,
    pub elapsed_ms: u64,
    pub message: String,
}

impl IngestResponse {
    fn from_result(r: &IngestResult) -> Self {
        Self {
            document_id: r.document_id.clone(),
            document_name: r.document_name.clone(),
            chunk_count: r.chunk_count,
            dimension: r.dimension,
            collection: r.collection.clone(),
            elapsed_ms: r.elapsed_ms,
            message: "入库成功".to_string(),
        }
    }
}

/// multipart 表单字段（从 multipart 流解析后的中间结构）
#[derive(Debug, Default)]
struct IngestForm {
    file_bytes: Vec<u8>,
    filename: String,
    category: String,
    applicable_scope: String,
    document_name: Option<String>,
}

impl IngestForm {
    /// 应用默认值并校验（纯函数，可单测）：
    /// - 缺文件（file_bytes 为空）→ Err
    /// - category / applicable_scope 为空 → 使用默认值
    /// - filename 为空 → 使用默认文件名
    fn finalize(mut self) -> Result<Self, String> {
        if self.file_bytes.is_empty() {
            return Err("上传文件为空".to_string());
        }
        if self.filename.is_empty() {
            self.filename = "regulation.pdf".to_string();
        }
        if self.category.is_empty() {
            self.category = "regulation".to_string();
        }
        if self.applicable_scope.is_empty() {
            self.applicable_scope = "general".to_string();
        }
        Ok(self)
    }
}

/// POST /api/v1/knowledge/ingest
/// multipart 字段：file（必填）、category（必填）、applicable_scope（必填）、document_name（可选）
pub async fn ingest_knowledge(
    State(_state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<IngestResponse>, (StatusCode, Json<ErrorResponse>)> {
    let mut form = IngestForm::default();
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or_default().to_string();
        if let Some(file_name) = field.file_name().map(str::to_string) {
            form.filename = file_name;
            if let Ok(data) = field.bytes().await {
                form.file_bytes = data.to_vec();
            }
        } else if let Ok(value) = field.text().await {
            match name.as_str() {
                "category" => form.category = value,
                "applicable_scope" => form.applicable_scope = value,
                "document_name" => form.document_name = Some(value),
                _ => {}
            }
        }
    }
    let form = form.finalize().map_err(|msg| bad_request(&msg))?;
    let display_name = form.document_name.unwrap_or_else(|| form.filename.clone());

    // 同步模式：直接 await 入库（契约 7.1 注明"同步模式，Java 侧建议异步触发"）
    let result = ingest_bytes(
        form.file_bytes,
        &display_name,
        &form.category,
        &form.applicable_scope,
    )
    .await
    .map_err(|e| server_error("入库失败", e))?;

    Ok(Json(IngestResponse::from_result(&result)))
}

// ─── 知识库搜索 ──────────────────────────────────────────────

/// 搜索请求
#[derive(Debug, serde::Deserialize)]
pub struct KnowledgeSearchRequest {
    pub query: String,
    #[serde(default = "default_top_k")]
    pub top_k: u64,
    pub category: Option<String>,
    pub applicable_scope: Option<String>,
}

fn default_top_k() -> u64 { 10 }

/// 搜索结果中的单条证据
#[derive(Debug, serde::Serialize)]
pub struct KnowledgeEvidence {
    pub document_id: String,
    pub document_name: String,
    pub chunk_id: String,
    pub relevance_score: f32,
    pub text: String,
    pub category: String,
    pub section_path: Vec<String>,
    pub page_start: usize,
    pub page_end: usize,
}

/// 搜索响应
#[derive(Debug, serde::Serialize)]
pub struct KnowledgeSearchResponse {
    pub evidences: Vec<KnowledgeEvidence>,
    pub total_candidates: usize,
    pub query_ms: u64,
}

/// POST /api/v1/knowledge/search
///
/// 语义搜索知识库（法规/案例/负面清单/范本）。
/// Agent 组通过此接口获取带原文引用的法规依据。
pub async fn search_knowledge(
    State(_state): State<AppState>,
    Json(req): Json<KnowledgeSearchRequest>,
) -> Result<Json<KnowledgeSearchResponse>, (StatusCode, Json<ErrorResponse>)> {
    if req.query.trim().is_empty() {
        return Err(bad_request("query 不能为空"));
    }
    let top_k = req.top_k.min(50);

    let t0 = std::time::Instant::now();

    // 1. 查询向量化（复用 EmbeddingClient）
    let embed_client = crate::services::embedding_api_client::EmbeddingApiClient::from_env()
        .map_err(|e| server_error("Embedding API 初始化失败", e))?;
    let query_embeddings = embed_client
        .encode_batch(&[req.query.clone()])
        .map_err(|e| server_error("查询向量化失败", e))?;
    let query_vec = query_embeddings.into_iter().next().unwrap_or_default();

    // 2. 搜索 Qdrant 知识库
    let store = crate::services::qdrant_store::QdrantStore::from_env()
        .map_err(|e| server_error("Qdrant 连接失败", e))?;
    let results = store
        .search(query_vec, top_k, req.category.clone(), req.applicable_scope.clone())
        .await
        .map_err(|e| server_error("知识库搜索失败", e))?;

    // 3. 格式化为 EvidenceSet
    let evidences: Vec<KnowledgeEvidence> = results
        .into_iter()
        .map(|(score, payload)| KnowledgeEvidence {
            document_id: payload.document_id,
            document_name: payload.document_name,
            chunk_id: payload.chunk_id,
            relevance_score: score,
            text: payload.embed_text,
            category: payload.category,
            section_path: payload.section_path,
            page_start: payload.page_start,
            page_end: payload.page_end,
        })
        .collect();

    let total = evidences.len();
    Ok(Json(KnowledgeSearchResponse {
        evidences,
        total_candidates: total,
        query_ms: t0.elapsed().as_millis() as u64,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_finalize_defaults() {
        // category / applicable_scope 为空时使用默认值
        let form = IngestForm {
            file_bytes: vec![1, 2, 3],
            filename: "某办法.pdf".into(),
            category: String::new(),
            applicable_scope: String::new(),
            document_name: None,
        };
        let form = form.finalize().unwrap();
        assert_eq!(form.category, "regulation");
        assert_eq!(form.applicable_scope, "general");
        assert_eq!(form.filename, "某办法.pdf");
    }

    #[test]
    fn test_finalize_missing_file() {
        // 缺文件（file_bytes 为空）时返回错误
        let form = IngestForm::default();
        let err = form.finalize().unwrap_err();
        assert!(err.contains("上传文件为空"));
    }

    #[test]
    fn test_finalize_default_filename() {
        let form = IngestForm {
            file_bytes: vec![1],
            filename: String::new(),
            category: "regulation".into(),
            applicable_scope: "general".into(),
            document_name: None,
        };
        let form = form.finalize().unwrap();
        assert_eq!(form.filename, "regulation.pdf");
    }

    #[test]
    fn test_bad_request_response_structure() {
        let (status, Json(body)) = bad_request("缺少文件");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body.error, "缺少文件");
        assert_eq!(body.detail, "缺少文件");
    }

    #[test]
    fn test_server_error_response_structure() {
        let (status, Json(body)) = server_error("入库失败", anyhow::anyhow!("boom"));
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body.error.contains("入库失败"));
        assert!(body.error.contains("boom"));
        assert_eq!(body.detail, body.error);
    }

    #[test]
    fn test_ingest_response_from_result() {
        let r = IngestResult {
            document_id: "doc-1".into(),
            document_name: "某办法.pdf".into(),
            category: "regulation".into(),
            applicable_scope: "engineering".into(),
            chunk_count: 12,
            dimension: 1024,
            collection: "legal_kb".into(),
            elapsed_ms: 88,
        };
        let resp = IngestResponse::from_result(&r);
        assert_eq!(resp.document_id, "doc-1");
        assert_eq!(resp.chunk_count, 12);
        assert_eq!(resp.dimension, 1024);
        assert_eq!(resp.collection, "legal_kb");
        assert_eq!(resp.message, "入库成功");
        // 序列化后字段完整
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("document_id").is_some());
        assert!(json.get("message").is_some());
    }
}
