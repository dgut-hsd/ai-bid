use std::collections::BTreeMap;

use serde_json::{json, Value};

pub const TENANT_A_ID: &str = "20001";
pub const TENANT_B_ID: &str = "20002";
pub const ACTOR_USER_ID: &str = "10001";
pub const REQUEST_ID: &str = "8b9c0e7f-9d8b-4f86-b77d-9a3d4c2f5001";
pub const OCCURRED_AT: &str = "2026-08-06T13:01:00.123Z";

pub const REQUIRED_INTERNAL_HEADERS: [&str; 5] = [
    "X-Tenant-Id",
    "X-User-Id",
    "X-Request-Id",
    "X-Internal-Timestamp",
    "X-Internal-Signature",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tenant {
    pub id: String,
    pub code: String,
    pub role: String,
    pub permissions: Vec<String>,
}

impl Tenant {
    pub fn new(id: &str, code: &str, role: &str, permissions: &[&str]) -> Self {
        Self {
            id: id.to_string(),
            code: code.to_string(),
            role: role.to_string(),
            permissions: permissions.iter().map(|value| (*value).to_string()).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resource {
    pub id: String,
    pub kind: String,
    pub tenant_id: String,
    pub parent_id: Option<String>,
    pub storage_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub tenant_id: String,
    pub query: String,
    pub page: u32,
    pub size: u32,
    pub total: u64,
    pub items: Vec<Resource>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadPreview {
    pub resource_id: String,
    pub tenant_id: String,
    pub download_path: String,
    pub preview_path: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SseEvent {
    pub schema_version: u8,
    pub event_id: String,
    pub event: String,
    pub tenant_id: String,
    pub task_id: String,
    pub occurred_at: String,
    pub data: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueMessage {
    pub schema_version: u8,
    pub tenant_id: String,
    pub task_id: String,
    pub actor_user_id: String,
    pub request_id: String,
}

impl QueueMessage {
    pub fn as_json(&self) -> Value {
        json!({
            "schema_version": self.schema_version,
            "tenant_id": self.tenant_id,
            "task_id": self.task_id,
            "actor_user_id": self.actor_user_id,
            "request_id": self.request_id,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InternalRequest {
    pub method: String,
    pub path_with_query: String,
    pub body_sha256: String,
    pub timestamp: String,
    pub tenant_id: String,
    pub user_id: String,
    pub request_id: String,
    pub signature: String,
    pub replay_key: String,
    pub canonical_request: String,
    pub headers: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct TenantFixture {
    pub actor_user_id: String,
    pub tenant_a: Tenant,
    pub tenant_b: Tenant,
    pub resources: Vec<Resource>,
    pub pages: Vec<Page>,
    pub download_previews: Vec<DownloadPreview>,
    pub events: Vec<SseEvent>,
    pub queue_messages: Vec<QueueMessage>,
    pub internal_requests: Vec<InternalRequest>,
}

impl TenantFixture {
    pub fn builder() -> TenantFixtureBuilder {
        TenantFixtureBuilder::default()
    }

    pub fn resource_for(&self, tenant_id: &str, resource_id: &str) -> &Resource {
        self.resources
            .iter()
            .find(|resource| resource.tenant_id == tenant_id && resource.id == resource_id)
            .expect("fixture resource must exist")
    }

    pub fn page_for(&self, tenant_id: &str) -> &Page {
        self.pages
            .iter()
            .find(|page| page.tenant_id == tenant_id)
            .expect("fixture page must exist")
    }

    pub fn download_preview_for(&self, tenant_id: &str) -> &DownloadPreview {
        self.download_previews
            .iter()
            .find(|case| case.tenant_id == tenant_id)
            .expect("fixture download/preview case must exist")
    }

    pub fn event_for(&self, tenant_id: &str) -> &SseEvent {
        self.events
            .iter()
            .find(|event| event.tenant_id == tenant_id)
            .expect("fixture SSE event must exist")
    }

    pub fn queue_message_for(&self, tenant_id: &str) -> &QueueMessage {
        self.queue_messages
            .iter()
            .find(|message| message.tenant_id == tenant_id)
            .expect("fixture queue message must exist")
    }

    pub fn internal_request_for(&self, tenant_id: &str) -> &InternalRequest {
        self.internal_requests
            .iter()
            .find(|request| request.tenant_id == tenant_id)
            .expect("fixture internal request must exist")
    }
}

#[derive(Clone, Debug)]
pub struct TenantFixtureBuilder {
    actor_user_id: String,
    tenant_a: Tenant,
    tenant_b: Tenant,
}

impl Default for TenantFixtureBuilder {
    fn default() -> Self {
        Self {
            actor_user_id: ACTOR_USER_ID.to_string(),
            tenant_a: Tenant::new(
                TENANT_A_ID,
                "acme-bid",
                "ADMIN",
                &["tenant.read", "tender.write", "audit.start", "audit.report.read"],
            ),
            tenant_b: Tenant::new(
                TENANT_B_ID,
                "solo-10001",
                "OWNER",
                &["tenant.read", "tenant.delete", "audit.report.read"],
            ),
        }
    }
}

impl TenantFixtureBuilder {
    pub fn actor_user_id(mut self, actor_user_id: &str) -> Self {
        self.actor_user_id = actor_user_id.to_string();
        self
    }

    pub fn tenant_a(mut self, tenant: Tenant) -> Self {
        self.tenant_a = tenant;
        self
    }

    pub fn tenant_b(mut self, tenant: Tenant) -> Self {
        self.tenant_b = tenant;
        self
    }

    pub fn build(self) -> TenantFixture {
        let project_a = resource(&self.tenant_a, "project-01", "project", None);
        let document_a = resource(
            &self.tenant_a,
            "document-01",
            "document",
            Some(project_a.id.clone()),
        );
        let project_b = resource(&self.tenant_b, "project-01", "project", None);
        let document_b = resource(
            &self.tenant_b,
            "document-01",
            "document",
            Some(project_b.id.clone()),
        );

        let pages = vec![
            Page {
                tenant_id: self.tenant_a.id.clone(),
                query: "smart campus".to_string(),
                page: 1,
                size: 20,
                total: 1,
                items: vec![document_a.clone()],
            },
            Page {
                tenant_id: self.tenant_b.id.clone(),
                query: "smart campus".to_string(),
                page: 1,
                size: 20,
                total: 1,
                items: vec![document_b.clone()],
            },
        ];
        let download_previews = vec![
            download_preview(&document_a),
            download_preview(&document_b),
        ];
        let events = vec![
            event(&self.tenant_a, "task-01", "1002"),
            event(&self.tenant_b, "task-01", "1002"),
        ];
        let queue_messages = vec![
            queue_message(&self.tenant_a, &self.actor_user_id, "task-01"),
            queue_message(&self.tenant_b, &self.actor_user_id, "task-01"),
        ];
        let internal_requests = vec![
            internal_request(&self.tenant_a, &self.actor_user_id),
            internal_request(&self.tenant_b, &self.actor_user_id),
        ];

        TenantFixture {
            actor_user_id: self.actor_user_id,
            tenant_a: self.tenant_a,
            tenant_b: self.tenant_b,
            resources: vec![project_a, document_a, project_b, document_b],
            pages,
            download_previews,
            events,
            queue_messages,
            internal_requests,
        }
    }
}

fn resource(tenant: &Tenant, id: &str, kind: &str, parent_id: Option<String>) -> Resource {
    Resource {
        id: id.to_string(),
        kind: kind.to_string(),
        tenant_id: tenant.id.clone(),
        parent_id,
        storage_path: format!(
            "storage/tenants/{}/tenders/2026-08-06/{}.pdf",
            tenant.id, id
        ),
    }
}

fn download_preview(resource: &Resource) -> DownloadPreview {
    DownloadPreview {
        resource_id: resource.id.clone(),
        tenant_id: resource.tenant_id.clone(),
        download_path: format!("/api/resources/{}/download", resource.id),
        preview_path: format!("/api/resources/{}/preview", resource.id),
    }
}

fn event(tenant: &Tenant, task_id: &str, event_id: &str) -> SseEvent {
    SseEvent {
        schema_version: 1,
        event_id: event_id.to_string(),
        event: "progress".to_string(),
        tenant_id: tenant.id.clone(),
        task_id: task_id.to_string(),
        occurred_at: OCCURRED_AT.to_string(),
        data: json!({
            "stage": "legal",
            "percent": 42,
            "message": "reviewing",
        }),
    }
}

fn queue_message(tenant: &Tenant, actor_user_id: &str, task_id: &str) -> QueueMessage {
    QueueMessage {
        schema_version: 1,
        tenant_id: tenant.id.clone(),
        task_id: task_id.to_string(),
        actor_user_id: actor_user_id.to_string(),
        request_id: REQUEST_ID.to_string(),
    }
}

fn internal_request(tenant: &Tenant, actor_user_id: &str) -> InternalRequest {
    let method = "POST".to_string();
    let path_with_query = "/api/v1/review/document-01/stream?mode=full".to_string();
    let timestamp = "1786019460".to_string();
    let body_sha256 = "a".repeat(64);
    let request_id = REQUEST_ID.to_string();
    let signature = format!("v1={}", "b".repeat(64));
    let canonical_request = [
        "v1",
        method.as_str(),
        path_with_query.as_str(),
        timestamp.as_str(),
        tenant.id.as_str(),
        actor_user_id,
        request_id.as_str(),
        body_sha256.as_str(),
    ]
    .join("\n");
    let mut headers = BTreeMap::new();
    headers.insert("X-Tenant-Id".to_string(), tenant.id.clone());
    headers.insert("X-User-Id".to_string(), actor_user_id.to_string());
    headers.insert("X-Request-Id".to_string(), request_id.clone());
    headers.insert("X-Internal-Timestamp".to_string(), timestamp.clone());
    headers.insert("X-Internal-Signature".to_string(), signature.clone());

    InternalRequest {
        method,
        path_with_query,
        body_sha256,
        timestamp,
        tenant_id: tenant.id.clone(),
        user_id: actor_user_id.to_string(),
        request_id: request_id.clone(),
        signature,
        replay_key: format!("{}:{}", tenant.id, request_id),
        canonical_request,
        headers,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NegativeCase {
    pub name: &'static str,
    pub surface: &'static str,
    pub activation: &'static str,
    pub expected_error_code: &'static str,
}

pub const NEGATIVE_CASES: &[NegativeCase] = &[
    NegativeCase {
        name: "cross_tenant_id_lookup_is_not_visible",
        surface: "resource lookup by tenantId and resourceId",
        activation: "T3",
        expected_error_code: "RESOURCE_NOT_FOUND",
    },
    NegativeCase {
        name: "cross_tenant_page_and_search_do_not_mix_items",
        surface: "pagination and search",
        activation: "T3",
        expected_error_code: "RESOURCE_NOT_FOUND",
    },
    NegativeCase {
        name: "cross_tenant_parent_child_lookup_is_not_visible",
        surface: "parent-child resource traversal",
        activation: "T3",
        expected_error_code: "RESOURCE_NOT_FOUND",
    },
    NegativeCase {
        name: "cross_tenant_download_is_not_visible",
        surface: "download",
        activation: "T4",
        expected_error_code: "RESOURCE_NOT_FOUND",
    },
    NegativeCase {
        name: "cross_tenant_preview_is_not_visible",
        surface: "preview and normalized storage path",
        activation: "T4",
        expected_error_code: "RESOURCE_NOT_FOUND",
    },
    NegativeCase {
        name: "cross_tenant_sse_replay_is_not_visible",
        surface: "SSE subscription and Last-Event-ID replay",
        activation: "T5",
        expected_error_code: "RESOURCE_NOT_FOUND",
    },
    NegativeCase {
        name: "queue_message_without_matching_tenant_context_is_rejected",
        surface: "queue and outbox envelope",
        activation: "T6",
        expected_error_code: "TENANT_CONTEXT_INVALID",
    },
    NegativeCase {
        name: "rust_header_tenant_mismatch_is_rejected",
        surface: "Java-to-Rust internal request",
        activation: "T7",
        expected_error_code: "INTERNAL_TENANT_MISMATCH",
    },
    NegativeCase {
        name: "replayed_request_id_is_scoped_by_tenant",
        surface: "Rust replay cache",
        activation: "T7",
        expected_error_code: "INTERNAL_REQUEST_REPLAYED",
    },
    NegativeCase {
        name: "all_surfaces_keep_the_same_negative_contract",
        surface: "cross-surface regression sweep",
        activation: "T8",
        expected_error_code: "RESOURCE_NOT_FOUND",
    },
];
