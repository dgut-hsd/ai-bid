use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::DefaultBodyLimit;
use axum::http::{Request, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tower_http::cors::{Any, CorsLayer};
use utoipa::OpenApi;
use uuid::Uuid;

use super::handlers::{self, AppState, InternalRequestContext};
use super::knowledge_handlers;
use crate::agents::types::{
    BlockRef, ChatResponse, Citation, CoordinatorOutput, GraphSnapshot, KnowledgeRef, RiskFinding,
    RiskSeverity, RiskTier, RoutingSummary, SuggestedAgent, TextSelection,
};

const INTERNAL_TENANT_HEADER: &str = "x-tenant-id";
const INTERNAL_USER_HEADER: &str = "x-user-id";
const INTERNAL_REQUEST_HEADER: &str = "x-request-id";
const INTERNAL_TIMESTAMP_HEADER: &str = "x-internal-timestamp";
const INTERNAL_SIGNATURE_HEADER: &str = "x-internal-signature";
const INTERNAL_SIGNATURE_VERSION: &str = "v1";
const INTERNAL_SECRET_ENV: &str = "RUST_API_INTERNAL_SECRET";
const INTERNAL_SECRET_FALLBACK_ENV: &str = "AIBID_INTERNAL_API_SECRET";
const MAX_CLOCK_SKEW_SECONDS: i64 = 300;
const REPLAY_RETENTION_SECONDS: i64 = 600;
const MAX_INTERNAL_BODY_BYTES: usize = 50 * 1024 * 1024;
const MAX_INTERNAL_REQUEST_ID_BYTES: usize = 128;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InternalAuthError {
    Missing,
    Invalid,
    Expired,
    Replayed,
}

impl InternalAuthError {
    fn error_code(self) -> &'static str {
        match self {
            Self::Missing => "INTERNAL_SIGNATURE_MISSING",
            Self::Invalid => "INTERNAL_SIGNATURE_INVALID",
            Self::Expired => "INTERNAL_SIGNATURE_EXPIRED",
            Self::Replayed => "INTERNAL_REQUEST_REPLAYED",
        }
    }
}

#[derive(Debug, Serialize)]
struct InternalErrorData {
    error_code: &'static str,
    request_id: String,
}

#[derive(Debug, Serialize)]
struct InternalErrorBody {
    code: u16,
    msg: &'static str,
    data: InternalErrorData,
    timestamp: i64,
}

#[derive(Clone)]
struct InternalAuthConfig {
    secret: Option<Arc<Vec<u8>>>,
    replayed_requests: Arc<Mutex<HashMap<(String, String), i64>>>,
}

impl InternalAuthConfig {
    fn from_env() -> Self {
        let secret = [INTERNAL_SECRET_ENV, INTERNAL_SECRET_FALLBACK_ENV]
            .into_iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.trim().is_empty());
        Self::from_secret(secret)
    }

    fn from_secret(secret: Option<String>) -> Self {
        Self {
            secret: secret
                .filter(|value| !value.trim().is_empty())
                .map(|value| Arc::new(value.into_bytes())),
            replayed_requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[cfg(test)]
    fn test_secret(secret: &str) -> Self {
        Self::from_secret(Some(secret.to_string()))
    }
}

async fn internal_request_middleware(
    request: Request<Body>,
    next: Next,
    config: InternalAuthConfig,
) -> Response {
    if !is_internal_path(request.uri().path()) {
        return next.run(request).await;
    }

    if config.secret.is_none() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }

    let (mut parts, body) = request.into_parts();
    let body = match to_bytes(body, MAX_INTERNAL_BODY_BYTES).await {
        Ok(body) => body,
        Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };

    let context =
        match verify_internal_request(&parts.method, &parts.uri, &parts.headers, &body, &config) {
            Ok(context) => context,
            Err(error) => return internal_auth_error_response(error, &parts.headers),
        };
    parts.extensions.insert(context);

    next.run(Request::from_parts(parts, Body::from(body))).await
}

fn verify_internal_request(
    method: &axum::http::Method,
    uri: &Uri,
    headers: &axum::http::HeaderMap,
    body: &[u8],
    config: &InternalAuthConfig,
) -> Result<InternalRequestContext, InternalAuthError> {
    let tenant_id = required_header(headers, INTERNAL_TENANT_HEADER)?;
    let user_id = required_header(headers, INTERNAL_USER_HEADER)?;
    let request_id = required_header(headers, INTERNAL_REQUEST_HEADER)?;
    let timestamp_value = required_header(headers, INTERNAL_TIMESTAMP_HEADER)?;
    let signature = required_header(headers, INTERNAL_SIGNATURE_HEADER)?;

    if !handlers::is_valid_tenant_id(&tenant_id)
        || !handlers::is_valid_tenant_id(&user_id)
        || request_id.len() > MAX_INTERNAL_REQUEST_ID_BYTES
    {
        return Err(InternalAuthError::Invalid);
    }

    let timestamp = timestamp_value
        .parse::<i64>()
        .map_err(|_| InternalAuthError::Invalid)?;
    let now = unix_timestamp();
    if now.abs_diff(timestamp) > MAX_CLOCK_SKEW_SECONDS as u64 {
        return Err(InternalAuthError::Expired);
    }

    let body_sha256 = sha256_hex(body);

    let signature_hex = signature
        .strip_prefix("v1=")
        .ok_or(InternalAuthError::Invalid)?;
    let signature_bytes = decode_hex(signature_hex).ok_or(InternalAuthError::Invalid)?;
    let secret = config.secret.as_ref().ok_or(InternalAuthError::Invalid)?;
    let canonical_request = canonical_request(
        method,
        uri,
        &timestamp_value,
        &tenant_id,
        &user_id,
        &request_id,
        &body_sha256,
    );
    let mut mac =
        HmacSha256::new_from_slice(secret.as_slice()).map_err(|_| InternalAuthError::Invalid)?;
    mac.update(canonical_request.as_bytes());
    mac.verify_slice(&signature_bytes)
        .map_err(|_| InternalAuthError::Invalid)?;

    let mut replayed_requests = config
        .replayed_requests
        .lock()
        .map_err(|_| InternalAuthError::Invalid)?;
    replayed_requests.retain(|_, seen_at| now.saturating_sub(*seen_at) <= REPLAY_RETENTION_SECONDS);
    if replayed_requests
        .insert((tenant_id.clone(), request_id.clone()), now)
        .is_some()
    {
        return Err(InternalAuthError::Replayed);
    }

    Ok(InternalRequestContext {
        tenant_id,
        user_id,
        request_id,
        timestamp,
        body_sha256,
    })
}

fn add_internal_auth<S>(router: Router<S>, config: InternalAuthConfig) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.layer(middleware::from_fn(
        move |request: Request<Body>, next: Next| {
            let config = config.clone();
            async move { internal_request_middleware(request, next, config).await }
        },
    ))
}

fn is_internal_path(path: &str) -> bool {
    path == "/api/v1" || path.starts_with("/api/v1/")
}

fn required_header(
    headers: &axum::http::HeaderMap,
    name: &str,
) -> Result<String, InternalAuthError> {
    let value = headers
        .get(name)
        .ok_or(InternalAuthError::Missing)?
        .to_str()
        .map_err(|_| InternalAuthError::Invalid)?;
    if value.is_empty() || value.chars().any(|ch| ch == '\r' || ch == '\n') {
        return Err(InternalAuthError::Invalid);
    }
    Ok(value.to_string())
}

fn internal_auth_error_response(
    error: InternalAuthError,
    headers: &axum::http::HeaderMap,
) -> Response {
    let request_id = headers
        .get(INTERNAL_REQUEST_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= MAX_INTERNAL_REQUEST_ID_BYTES)
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let status = StatusCode::UNAUTHORIZED;
    (
        status,
        axum::Json(InternalErrorBody {
            code: status.as_u16(),
            msg: "internal request authentication failed",
            data: InternalErrorData {
                error_code: error.error_code(),
                request_id,
            },
            timestamp: unix_timestamp_millis(),
        }),
    )
        .into_response()
}

fn canonical_request(
    method: &axum::http::Method,
    uri: &Uri,
    timestamp: &str,
    tenant_id: &str,
    user_id: &str,
    request_id: &str,
    body_sha256: &str,
) -> String {
    let path_and_query = uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    [
        INTERNAL_SIGNATURE_VERSION,
        method.as_str(),
        path_and_query,
        timestamp,
        tenant_id,
        user_id,
        request_id,
        body_sha256,
    ]
    .join("\n")
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn unix_timestamp_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex_encode(&digest)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 0x0f) as usize] as char);
    }
    result
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() != 64
        || !value.is_ascii()
        || value
            .bytes()
            .any(|byte| !(byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| Some((hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?))
        .collect()
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

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
            handlers::ReviewUsage,
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

    let router = Router::new()
        .route("/health", get(handlers::health))
        // Swagger UI + OpenAPI JSON
        .route("/swagger-ui", get(swagger_ui))
        .route("/api-docs/openapi.json", get(openapi_json))
        // Metrics Dashboard
        .route("/metrics", get(metrics_dashboard))
        .nest("/api/v1", api);

    add_internal_auth(router, InternalAuthConfig::from_env())
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024)) // 50MB, 对齐 Java 的 max-file-size
        .layer(cors)
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::raw_document::RawDocument;
    use crate::domain::vector_index::DocumentVectorIndex;
    use crate::services::desensitize_service::{DesensitizationMode, RedactionVault};
    use axum::http::{HeaderName, HeaderValue, Method};
    use std::collections::HashSet;
    use tower::util::ServiceExt;

    async fn context_handler(request: Request<Body>) -> StatusCode {
        if request
            .extensions()
            .get::<InternalRequestContext>()
            .is_some()
        {
            StatusCode::OK
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }

    fn test_router(config: InternalAuthConfig) -> Router {
        let router = Router::new()
            .route("/health", get(handlers::health))
            .route("/api/v1/test", post(context_handler));
        add_internal_auth(router, config)
    }

    fn test_document_state() -> Arc<handlers::DocumentState> {
        Arc::new(handlers::DocumentState {
            tenant_id: "100".to_string(),
            id: "shared-doc".to_string(),
            filename: "shared.pdf".to_string(),
            stem: "shared-doc".to_string(),
            raw_doc: RawDocument {
                document_id: "shared-doc".to_string(),
                source_path: String::new(),
                pages: Vec::new(),
            },
            sections: Vec::new(),
            chunks: Vec::new(),
            review_chunks: Vec::new(),
            chunk_map: Arc::new(HashMap::new()),
            review_chunk_map: Arc::new(HashMap::new()),
            chunk_order: Arc::new(Vec::new()),
            doc_index: Arc::new(DocumentVectorIndex::new(Vec::new(), Vec::new())),
            redaction_vault: Arc::new(RedactionVault::new(DesensitizationMode::Off)),
            desensitization_summary: Default::default(),
        })
    }

    fn document_router(config: InternalAuthConfig) -> Router {
        let state = AppState {
            documents: Arc::new(tokio::sync::RwLock::new(HashMap::from([(
                handlers::DocumentKey::new("100", "shared-doc"),
                test_document_state(),
            )]))),
            embed_client: Arc::new(Mutex::new(None)),
            dashscope_search: None,
            search_backend: "searxng".to_string(),
            embed_engine: "test".to_string(),
            review_event_buses: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            review_results: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            review_errors: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            review_usages: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            active_reviews: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
        };
        add_internal_auth(
            Router::new()
                .route("/api/v1/documents/:id", get(handlers::get_document))
                .route(
                    "/api/v1/documents/:id/review",
                    post(handlers::review_document),
                )
                .route(
                    "/api/v1/documents/:id/chat",
                    post(handlers::chat_with_document),
                )
                .route(
                    "/api/v1/documents/:id/chat/stream",
                    post(handlers::chat_with_document_stream),
                )
                .route(
                    "/api/v1/documents/:id/search",
                    post(handlers::search_document),
                )
                .route(
                    "/api/v1/documents/:id/blocks",
                    get(handlers::get_block_bboxes),
                )
                .route(
                    "/api/v1/review/:doc_id/stream",
                    get(handlers::stream_review_events),
                )
                .route(
                    "/api/v1/review/:doc_id/result",
                    get(handlers::get_review_result),
                )
                .with_state(state),
            config,
        )
    }

    async fn assert_internal_error(
        response: Response,
        expected_status: StatusCode,
        expected_error_code: &str,
        expected_request_id: &str,
    ) {
        assert_eq!(response.status(), expected_status);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        let body = to_bytes(response.into_body(), MAX_INTERNAL_BODY_BYTES)
            .await
            .expect("error body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("JSON error body");
        assert_eq!(json["code"], expected_status.as_u16());
        assert_eq!(json["data"]["error_code"], expected_error_code);
        assert_eq!(json["data"]["request_id"], expected_request_id);
        assert!(json["timestamp"].is_i64() || json["timestamp"].is_u64());
    }

    fn signed_request(
        method: Method,
        path: &str,
        body: &[u8],
        timestamp: i64,
        tenant_id: &str,
        user_id: &str,
        request_id: &str,
        secret: &str,
    ) -> Request<Body> {
        let uri = path.parse::<Uri>().expect("test URI");
        let body_sha256 = sha256_hex(body);
        let path_and_query = uri
            .path_and_query()
            .map(|value| value.as_str())
            .unwrap_or("/");
        let canonical = [
            INTERNAL_SIGNATURE_VERSION,
            method.as_str(),
            path_and_query,
            &timestamp.to_string(),
            tenant_id,
            user_id,
            request_id,
            &body_sha256,
        ]
        .join("\n");
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("test HMAC key");
        mac.update(canonical.as_bytes());
        let signature = format!(
            "{}={}",
            INTERNAL_SIGNATURE_VERSION,
            hex_encode(&mac.finalize().into_bytes())
        );

        Request::builder()
            .method(method)
            .uri(uri)
            .header(INTERNAL_TENANT_HEADER, tenant_id)
            .header(INTERNAL_USER_HEADER, user_id)
            .header(INTERNAL_REQUEST_HEADER, request_id)
            .header(INTERNAL_TIMESTAMP_HEADER, timestamp.to_string())
            .header(INTERNAL_SIGNATURE_HEADER, signature)
            .body(Body::from(body.to_vec()))
            .expect("test request")
    }

    fn signed_json_request(
        method: Method,
        path: &str,
        body: &[u8],
        tenant_id: &str,
        user_id: &str,
        request_id: &str,
        secret: &str,
    ) -> Request<Body> {
        let mut request = signed_request(
            method,
            path,
            body,
            unix_timestamp(),
            tenant_id,
            user_id,
            request_id,
            secret,
        );
        request.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        request
    }

    const JAVA_REQUEST_ID: &str =
        "550e8400-e29b-41d4-a716-446655440000.6ba7b810-9dad-11d1-80b4-00c04fd430c8";

    #[tokio::test]
    async fn accepts_java_style_request_id() {
        assert_eq!(JAVA_REQUEST_ID.len(), 36 + 1 + 36);
        let secret = "test-secret";
        let request = signed_request(
            Method::POST,
            "/api/v1/test",
            b"{}",
            unix_timestamp(),
            "100",
            "200",
            JAVA_REQUEST_ID,
            secret,
        );

        let response = test_router(InternalAuthConfig::test_secret(secret))
            .oneshot(request)
            .await
            .expect("router response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn accepts_valid_request_and_exposes_verified_context() {
        let secret = "test-secret";
        let request = signed_request(
            Method::POST,
            "/api/v1/test",
            b"{}",
            unix_timestamp(),
            "100",
            "200",
            "request-a",
            secret,
        );

        let response = test_router(InternalAuthConfig::test_secret(secret))
            .oneshot(request)
            .await
            .expect("router response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn rejects_tampered_tenant_even_when_other_headers_are_valid() {
        let secret = "test-secret";
        let mut request = signed_request(
            Method::POST,
            "/api/v1/test",
            b"{}",
            unix_timestamp(),
            "100",
            "200",
            "request-a",
            secret,
        );
        request.headers_mut().insert(
            HeaderName::from_static(INTERNAL_TENANT_HEADER),
            HeaderValue::from_static("101"),
        );

        let response = test_router(InternalAuthConfig::test_secret(secret))
            .oneshot(request)
            .await
            .expect("router response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn rejects_expired_timestamp() {
        let secret = "test-secret";
        let request = signed_request(
            Method::POST,
            "/api/v1/test",
            b"{}",
            unix_timestamp() - MAX_CLOCK_SKEW_SECONDS - 1,
            "100",
            "200",
            "request-a",
            secret,
        );

        let response = test_router(InternalAuthConfig::test_secret(secret))
            .oneshot(request)
            .await
            .expect("router response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn returns_contract_json_for_missing_signature() {
        let request_id = "missing-signature";
        let mut request = signed_request(
            Method::POST,
            "/api/v1/test",
            b"{}",
            unix_timestamp(),
            "100",
            "200",
            request_id,
            "test-secret",
        );
        request.headers_mut().remove(INTERNAL_SIGNATURE_HEADER);

        let response = test_router(InternalAuthConfig::test_secret("test-secret"))
            .oneshot(request)
            .await
            .expect("router response");

        assert_internal_error(
            response,
            StatusCode::UNAUTHORIZED,
            "INTERNAL_SIGNATURE_MISSING",
            request_id,
        )
        .await;
    }

    #[tokio::test]
    async fn returns_contract_json_for_invalid_signature() {
        let request_id = JAVA_REQUEST_ID;
        let mut request = signed_request(
            Method::POST,
            "/api/v1/test",
            b"{}",
            unix_timestamp(),
            "100",
            "200",
            request_id,
            "test-secret",
        );
        request.headers_mut().insert(
            HeaderName::from_static(INTERNAL_SIGNATURE_HEADER),
            HeaderValue::from_static(
                "v1=0000000000000000000000000000000000000000000000000000000000000000",
            ),
        );

        let response = test_router(InternalAuthConfig::test_secret("test-secret"))
            .oneshot(request)
            .await
            .expect("router response");

        assert_internal_error(
            response,
            StatusCode::UNAUTHORIZED,
            "INTERNAL_SIGNATURE_INVALID",
            request_id,
        )
        .await;
    }

    #[tokio::test]
    async fn returns_contract_json_for_expired_signature() {
        let request_id = "expired-signature";
        let request = signed_request(
            Method::POST,
            "/api/v1/test",
            b"{}",
            unix_timestamp() - MAX_CLOCK_SKEW_SECONDS - 1,
            "100",
            "200",
            request_id,
            "test-secret",
        );

        let response = test_router(InternalAuthConfig::test_secret("test-secret"))
            .oneshot(request)
            .await
            .expect("router response");

        assert_internal_error(
            response,
            StatusCode::UNAUTHORIZED,
            "INTERNAL_SIGNATURE_EXPIRED",
            request_id,
        )
        .await;
    }

    #[tokio::test]
    async fn rejects_replayed_request_id_for_same_tenant() {
        let secret = "test-secret";
        let config = InternalAuthConfig::test_secret(secret);
        let first = signed_request(
            Method::POST,
            "/api/v1/test",
            b"{}",
            unix_timestamp(),
            "100",
            "200",
            "request-a",
            secret,
        );
        let second = signed_request(
            Method::POST,
            "/api/v1/test",
            b"{}",
            unix_timestamp(),
            "100",
            "200",
            "request-a",
            secret,
        );
        let router = test_router(config);

        let first_response = router
            .clone()
            .oneshot(first)
            .await
            .expect("first router response");
        let second_response = router
            .oneshot(second)
            .await
            .expect("second router response");

        assert_eq!(first_response.status(), StatusCode::OK);
        assert_eq!(second_response.status(), StatusCode::UNAUTHORIZED);
        assert_internal_error(
            second_response,
            StatusCode::UNAUTHORIZED,
            "INTERNAL_REQUEST_REPLAYED",
            "request-a",
        )
        .await;
    }

    #[tokio::test]
    async fn document_id_lookup_is_tenant_scoped() {
        let secret = "test-secret";
        let router = document_router(InternalAuthConfig::test_secret(secret));

        let tenant_a = signed_request(
            Method::GET,
            "/api/v1/documents/shared-doc",
            &[],
            unix_timestamp(),
            "100",
            "200",
            "tenant-a-document",
            secret,
        );
        let tenant_b = signed_request(
            Method::GET,
            "/api/v1/documents/shared-doc",
            &[],
            unix_timestamp(),
            "101",
            "201",
            "tenant-b-document",
            secret,
        );

        let tenant_a_response = router
            .clone()
            .oneshot(tenant_a)
            .await
            .expect("tenant A response");
        let tenant_b_response = router.oneshot(tenant_b).await.expect("tenant B response");

        assert_eq!(tenant_a_response.status(), StatusCode::OK);
        assert_eq!(tenant_b_response.status(), StatusCode::NOT_FOUND);
    }

    async fn assert_resource_not_found(response: Response) {
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        let body = to_bytes(response.into_body(), MAX_INTERNAL_BODY_BYTES)
            .await
            .expect("resource error body");
        let body_text = String::from_utf8(body.to_vec()).expect("UTF-8 error body");
        assert!(!body_text.contains("shared-doc"));
        let json: serde_json::Value = serde_json::from_str(&body_text).expect("JSON error body");
        assert_eq!(json["error"], "RESOURCE_NOT_FOUND");
    }

    #[tokio::test]
    async fn every_document_endpoint_is_tenant_scoped() {
        let secret = "test-secret";
        let router = document_router(InternalAuthConfig::test_secret(secret));

        let tenant_a_get = router
            .clone()
            .oneshot(signed_request(
                Method::GET,
                "/api/v1/documents/shared-doc",
                &[],
                unix_timestamp(),
                "100",
                "200",
                "matrix-a-get",
                secret,
            ))
            .await
            .expect("tenant A get response");
        assert_eq!(tenant_a_get.status(), StatusCode::OK);

        let tenant_a_blocks = router
            .clone()
            .oneshot(signed_request(
                Method::GET,
                "/api/v1/documents/shared-doc/blocks?ids=missing",
                &[],
                unix_timestamp(),
                "100",
                "200",
                "matrix-a-blocks",
                secret,
            ))
            .await
            .expect("tenant A blocks response");
        assert_eq!(tenant_a_blocks.status(), StatusCode::OK);

        let tenant_a_search = router
            .clone()
            .oneshot(signed_json_request(
                Method::POST,
                "/api/v1/documents/shared-doc/search",
                br#"{"queries":[]}"#,
                "100",
                "200",
                "matrix-a-search",
                secret,
            ))
            .await
            .expect("tenant A search response");
        assert_eq!(tenant_a_search.status(), StatusCode::OK);

        let tenant_a_review_stream = router
            .clone()
            .oneshot(signed_request(
                Method::GET,
                "/api/v1/review/shared-doc/stream",
                &[],
                unix_timestamp(),
                "100",
                "200",
                "matrix-a-review-stream",
                secret,
            ))
            .await
            .expect("tenant A review stream response");
        assert_eq!(tenant_a_review_stream.status(), StatusCode::OK);

        let tenant_a_result = router
            .clone()
            .oneshot(signed_request(
                Method::GET,
                "/api/v1/review/shared-doc/result",
                &[],
                unix_timestamp(),
                "100",
                "200",
                "matrix-a-result",
                secret,
            ))
            .await
            .expect("tenant A result response");
        assert_eq!(tenant_a_result.status(), StatusCode::OK);

        let tenant_a_chat_stream = router
            .clone()
            .oneshot(signed_json_request(
                Method::POST,
                "/api/v1/documents/shared-doc/chat/stream",
                br#"{"user_input":"hello"}"#,
                "100",
                "200",
                "matrix-a-chat-stream",
                secret,
            ))
            .await
            .expect("tenant A chat stream response");
        assert_eq!(tenant_a_chat_stream.status(), StatusCode::OK);

        let tenant_a_review = router
            .clone()
            .oneshot(signed_json_request(
                Method::POST,
                "/api/v1/documents/shared-doc/review",
                br#"{"chunk_ids":[]}"#,
                "100",
                "200",
                "matrix-a-review",
                secret,
            ))
            .await
            .expect("tenant A review response");
        assert_eq!(tenant_a_review.status(), StatusCode::ACCEPTED);

        let tenant_b_requests = [
            signed_request(
                Method::GET,
                "/api/v1/documents/shared-doc",
                &[],
                unix_timestamp(),
                "101",
                "201",
                "matrix-b-get",
                secret,
            ),
            signed_request(
                Method::GET,
                "/api/v1/documents/shared-doc/blocks?ids=missing",
                &[],
                unix_timestamp(),
                "101",
                "201",
                "matrix-b-blocks",
                secret,
            ),
            signed_json_request(
                Method::POST,
                "/api/v1/documents/shared-doc/search",
                br#"{"queries":[]}"#,
                "101",
                "201",
                "matrix-b-search",
                secret,
            ),
            signed_request(
                Method::GET,
                "/api/v1/review/shared-doc/stream",
                &[],
                unix_timestamp(),
                "101",
                "201",
                "matrix-b-review-stream",
                secret,
            ),
            signed_request(
                Method::GET,
                "/api/v1/review/shared-doc/result",
                &[],
                unix_timestamp(),
                "101",
                "201",
                "matrix-b-result",
                secret,
            ),
            signed_json_request(
                Method::POST,
                "/api/v1/documents/shared-doc/chat/stream",
                br#"{"user_input":"hello"}"#,
                "101",
                "201",
                "matrix-b-chat-stream",
                secret,
            ),
            signed_json_request(
                Method::POST,
                "/api/v1/documents/shared-doc/chat",
                br#"{"user_input":"hello"}"#,
                "101",
                "201",
                "matrix-b-chat",
                secret,
            ),
            signed_json_request(
                Method::POST,
                "/api/v1/documents/shared-doc/review",
                br#"{"chunk_ids":[]}"#,
                "101",
                "201",
                "matrix-b-review",
                secret,
            ),
        ];

        for request in tenant_b_requests {
            let response = router
                .clone()
                .oneshot(request)
                .await
                .expect("tenant B response");
            assert_resource_not_found(response).await;
        }
    }

    #[tokio::test]
    async fn result_disk_fallback_is_tenant_namespaced() {
        let secret = "test-secret";
        let tenant_a = format!("{}{}", std::process::id(), unix_timestamp_millis());
        let tenant_b = format!("{}1", tenant_a);
        let document_id = format!("fallback-{}", Uuid::new_v4().simple());
        let tenant_root =
            std::path::PathBuf::from(crate::paths::data_path_str("output/tenants")).join(&tenant_a);
        let findings_dir = tenant_root.join("findings");
        std::fs::create_dir_all(&findings_dir).expect("tenant findings directory");
        let result_path = findings_dir.join(format!("{}_result.json", document_id));
        std::fs::write(
            &result_path,
            serde_json::json!({
                "status": "completed",
                "result": {
                    "document_id": document_id.clone(),
                    "findings": [],
                    "routing_summary": {
                        "total_clauses": 0,
                        "agent_clause_counts": {},
                        "high_risk_count": 0,
                        "legal_verify_count": 0,
                        "blind_spot_findings": 0
                    },
                    "graph_snapshot": null
                },
                "error": null
            })
            .to_string(),
        )
        .expect("persisted review result");

        let router = document_router(InternalAuthConfig::test_secret(secret));
        let tenant_a_response = router
            .clone()
            .oneshot(signed_request(
                Method::GET,
                &format!("/api/v1/review/{document_id}/result"),
                &[],
                unix_timestamp(),
                &tenant_a,
                "200",
                "disk-fallback-a",
                secret,
            ))
            .await
            .expect("tenant A disk fallback response");
        assert_eq!(tenant_a_response.status(), StatusCode::OK);

        let tenant_b_response = router
            .oneshot(signed_request(
                Method::GET,
                &format!("/api/v1/review/{document_id}/result"),
                &[],
                unix_timestamp(),
                &tenant_b,
                "201",
                "disk-fallback-b",
                secret,
            ))
            .await
            .expect("tenant B disk fallback response");
        assert_resource_not_found(tenant_b_response).await;

        std::fs::remove_dir_all(tenant_root).expect("remove test tenant namespace");
    }

    #[tokio::test]
    async fn leaves_health_unprotected_and_rejects_internal_requests_without_secret() {
        let health_response = test_router(InternalAuthConfig::from_secret(None))
            .clone()
            .oneshot(
                Request::get("/health")
                    .body(Body::empty())
                    .expect("health request"),
            )
            .await
            .expect("health response");
        assert_eq!(health_response.status(), StatusCode::OK);

        let internal_response = test_router(InternalAuthConfig::from_secret(None))
            .oneshot(
                Request::post("/api/v1/test")
                    .body(Body::empty())
                    .expect("internal request"),
            )
            .await
            .expect("internal response");
        assert_eq!(internal_response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
