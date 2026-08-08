use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::header;
use axum::response::Html;
use axum::routing::{delete, get, patch, post};
use tower_http::cors::{Any, CorsLayer};
use utoipa::OpenApi;

use super::handlers::{self, AppState};
use super::knowledge_handlers;
use crate::agents::types::{
    BlockRef, ChatResponse, Citation, CoordinatorOutput, GraphSnapshot, KnowledgeRef, RiskFinding,
    RiskSeverity, RiskTier, RoutingSummary, SuggestedAgent, TextSelection,
};

// ─── OpenAPI 文档 ──────────────────────────────────────────────────────

/// ai-bid Rust 引擎 API 文档。
///
/// 智能标书审核系统的 AI 引擎，提供文档解析、向量嵌入、Multi-Agent 审核、
/// RAG 对话、语义搜索等能力。
#[derive(OpenApi)]
#[openapi(
    paths(
        handlers::health,
        handlers::process_document,
        handlers::get_document,
        handlers::review_document,
        handlers::stream_review_events,
        handlers::get_review_result,
        handlers::chat_with_document,
        handlers::chat_with_document_stream,
        handlers::search_document,
        handlers::get_block_bboxes,
    ),
    components(
        schemas(
            // Request DTOs
            handlers::ReviewRequest,
            handlers::ChatRequest,
            handlers::ChatMessageDto,
            handlers::SearchRequest,
            handlers::BlockQuery,
            // Response DTOs
            handlers::ProcessResponse,
            handlers::DocumentInfo,
            handlers::ReviewAccepted,
            handlers::ReviewResponse,
            handlers::ReviewResultResponse,
            handlers::SearchResponse,
            handlers::SearchResultGroup,
            handlers::SearchHitDto,
            handlers::ErrorResponse,
            handlers::BBoxDto,
            handlers::BlockBBoxResponse,
            // Core domain types (from agents::types)
            RiskFinding,
            RoutingSummary,
            GraphSnapshot,
            CoordinatorOutput,
            RiskSeverity,
            RiskTier,
            Citation,
            SuggestedAgent,
            ChatResponse,
            BlockRef,
            KnowledgeRef,
            TextSelection,
            crate::agents::types::BBox,
        )
    ),
    tags(
        (name = "health", description = "健康检查"),
        (name = "documents", description = "文档管理 — 上传、解析、查询"),
        (name = "review", description = "智能审核 — 异步 Multi-Agent 审核（SSE 实时进度）"),
        (name = "chat", description = "智能对话 — 基于 RAG 的文档问答"),
        (name = "search", description = "语义搜索 — 向量相似度检索"),
        (name = "blocks", description = "辅助接口 — PDF 坐标定位"),
    )
)]
pub struct ApiDoc;

// ─── Swagger UI (CDN, 无需额外依赖) ─────────────────────────────────────

/// GET /swagger-ui — 内联 Swagger UI HTML（CDN 加载）。
async fn swagger_ui() -> Html<&'static str> {
    Html(SWAGGER_UI_HTML)
}

const SWAGGER_UI_HTML: &str = r##"<!DOCTYPE html>
<html lang="zh-CN">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>ai-bid Rust 引擎 — API 文档</title>
    <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui.css">
    <style>
        html { box-sizing: border-box; overflow-y: scroll; }
        *, *:before, *:after { box-sizing: inherit; }
        body { margin: 0; background: #fafafa; }
        .topbar { display: none; }
    </style>
</head>
<body>
<div id="swagger-ui"></div>
<script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui-bundle.js" crossorigin></script>
<script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui-standalone-preset.js" crossorigin></script>
<script>
    window.onload = function() {
        SwaggerUIBundle({
            url: "/api-docs/openapi.json",
            dom_id: "#swagger-ui",
            deepLinking: true,
            presets: [SwaggerUIBundle.presets.apis, SwaggerUIStandalonePreset],
            plugins: [SwaggerUIBundle.plugins.DownloadUrl],
            layout: "StandaloneLayout"
        });
    };
</script>
</body>
</html>"##;

/// GET /api-docs/openapi.json — 返回 OpenAPI 3.0 JSON 规格。
async fn openapi_json() -> (
    axum::http::StatusCode,
    [(header::HeaderName, &'static str); 1],
    String,
) {
    let spec = ApiDoc::openapi();
    let json = serde_json::to_string_pretty(&spec).unwrap_or_else(|_| "{}".to_string());
    (
        axum::http::StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        json,
    )
}

/// GET /metrics — 实验指标仪表板。
async fn metrics_dashboard() -> Html<&'static str> {
    Html(include_str!("metrics_dashboard.html"))
}

// ─── Router 构建 ───────────────────────────────────────────────────────

/// 构建完整的 API Router。
pub fn build(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api = Router::new()
        .route("/documents", post(handlers::process_document))
        .route("/documents/:id", get(handlers::get_document))
        .route("/documents/:id/review", post(handlers::review_document))
        .route("/documents/:id/chat", post(handlers::chat_with_document))
        .route(
            "/documents/:id/chat/stream",
            post(handlers::chat_with_document_stream),
        )
        .route("/documents/:id/search", post(handlers::search_document))
        .route("/documents/:id/blocks", get(handlers::get_block_bboxes))
        // 知识库导入（法规/标准库，入库组）
        .route(
            "/knowledge/ingest",
            post(knowledge_handlers::ingest_knowledge),
        )
        // 知识库检索（检索组）
        .route(
            "/knowledge/search",
            post(knowledge_handlers::search_knowledge),
        )
        // SSE 实时推送 + 异步审查结果
        .route(
            "/review/:doc_id/stream",
            get(handlers::stream_review_events),
        )
        .route("/review/:doc_id/result", get(handlers::get_review_result))
        // Metrics API
        .route("/metrics/runs", get(handlers::list_metric_runs))
        .route("/metrics/runs/:run_id", get(handlers::get_metric_run))
        .route("/metrics/runs/:run_id", delete(handlers::delete_metric_run))
        .route(
            "/metrics/runs/:run_id/tags",
            patch(handlers::update_metric_tags),
        )
        .route(
            "/metrics/runs/:run_id/title",
            patch(handlers::update_metric_title),
        )
        .route(
            "/metrics/runs/:run_id/notes",
            patch(handlers::update_metric_notes),
        )
        .route(
            "/metrics/runs/:run_id/experiment-group",
            patch(handlers::move_metric_experiment_group),
        )
        .route(
            "/metrics/experiment-groups",
            get(handlers::list_metric_experiment_groups),
        );

    Router::new()
        .route("/health", get(handlers::health))
        // Swagger UI + OpenAPI JSON
        .route("/swagger-ui", get(swagger_ui))
        .route("/api-docs/openapi.json", get(openapi_json))
        // Metrics Dashboard
        .route("/metrics", get(metrics_dashboard))
        .nest("/api/v1", api)
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024)) // 50MB, 对齐 Java 的 max-file-size
        .layer(cors)
        .with_state(state)
}
