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
use sha2::{Digest, Sha256};
use tower_http::cors::{Any, CorsLayer};
use utoipa::OpenApi;

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

type HmacSha256 = Hmac<Sha256>;

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

    let context = match verify_internal_request(
        &parts.method,
        &parts.uri,
        &parts.headers,
        &body,
        &config,
    ) {
        Ok(context) => context,
        Err(status) => return status.into_response(),
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
) -> Result<InternalRequestContext, StatusCode> {
    let tenant_id = required_header(headers, INTERNAL_TENANT_HEADER)?;
    let user_id = required_header(headers, INTERNAL_USER_HEADER)?;
    let request_id = required_header(headers, INTERNAL_REQUEST_HEADER)?;
    let timestamp_value = required_header(headers, INTERNAL_TIMESTAMP_HEADER)?;
    let signature = required_header(headers, INTERNAL_SIGNATURE_HEADER)?;

    let timestamp = timestamp_value
        .parse::<i64>()
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    let now = unix_timestamp();
    if now.abs_diff(timestamp) > MAX_CLOCK_SKEW_SECONDS as u64 {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let body_sha256 = sha256_hex(body);

    let signature_hex = signature
        .strip_prefix("v1=")
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let signature_bytes = decode_hex(signature_hex).ok_or(StatusCode::UNAUTHORIZED)?;
    let secret = config
        .secret
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let canonical_request = canonical_request(
        method,
        uri,
        &timestamp_value,
        &tenant_id,
        &user_id,
        &request_id,
        &body_sha256,
    );
    let mut mac = HmacSha256::new_from_slice(secret.as_slice())
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    mac.update(canonical_request.as_bytes());
    mac.verify_slice(&signature_bytes)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let mut replayed_requests = config
        .replayed_requests
        .lock()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    replayed_requests.retain(|_, seen_at| now.saturating_sub(*seen_at) <= REPLAY_RETENTION_SECONDS);
    if replayed_requests
        .insert((tenant_id.clone(), request_id.clone()), now)
        .is_some()
    {
        return Err(StatusCode::UNAUTHORIZED);
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
    // 罗盘（实验指标仪表板）是随 server 启动的本地调试工具，浏览器直接访问无法做 HMAC 签名；
    // 其余 /api/v1/* 仍受内部认证保护（供 Java 后端调用）。
    let is_api = path == "/api/v1" || path.starts_with("/api/v1/");
    let is_metrics = path.starts_with("/api/v1/metrics");
    is_api && !is_metrics
}

fn required_header(
    headers: &axum::http::HeaderMap,
    name: &str,
) -> Result<String, StatusCode> {
    let value = headers
        .get(name)
        .ok_or(StatusCode::UNAUTHORIZED)?
        .to_str()
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    if value.is_empty() || value.chars().any(|ch| ch == '\r' || ch == '\n') {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(value.to_string())
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
    if value.len() != 64 || !value.is_ascii() {
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
        // 知识库向量删除（Java 删除标准库文件时联动调用）
        .route(
            "/knowledge/document/:document_id",
            delete(knowledge_handlers::delete_knowledge_document),
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
    use axum::http::{HeaderName, HeaderValue, Method};
    use tower::util::ServiceExt;

    async fn context_handler(request: Request<Body>) -> StatusCode {
        if request.extensions().get::<InternalRequestContext>().is_some() {
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

    #[tokio::test]
    async fn accepts_valid_request_and_exposes_verified_context() {
        let secret = "test-secret";
        let request = signed_request(
            Method::POST,
            "/api/v1/test",
            b"{}",
            unix_timestamp(),
            "tenant-a",
            "user-a",
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
            "tenant-a",
            "user-a",
            "request-a",
            secret,
        );
        request.headers_mut().insert(
            HeaderName::from_static(INTERNAL_TENANT_HEADER),
            HeaderValue::from_static("tenant-b"),
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
            "tenant-a",
            "user-a",
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
    async fn rejects_replayed_request_id_for_same_tenant() {
        let secret = "test-secret";
        let config = InternalAuthConfig::test_secret(secret);
        let first = signed_request(
            Method::POST,
            "/api/v1/test",
            b"{}",
            unix_timestamp(),
            "tenant-a",
            "user-a",
            "request-a",
            secret,
        );
        let second = signed_request(
            Method::POST,
            "/api/v1/test",
            b"{}",
            unix_timestamp(),
            "tenant-a",
            "user-a",
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
