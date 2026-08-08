use std::collections::BTreeMap;

use serde_json::Value;

use super::tenant_fixture::{
    InternalRequest, QueueMessage, Resource, SseEvent, REQUIRED_INTERNAL_HEADERS,
};

pub fn assert_same_tenant(parent: &Resource, child: &Resource) {
    assert_eq!(parent.tenant_id, child.tenant_id);
    assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
}

pub fn assert_tenant_scoped(expected_tenant_id: &str, resources: &[Resource]) {
    assert!(
        resources
            .iter()
            .all(|resource| resource.tenant_id == expected_tenant_id),
        "resource collection crossed tenant boundary: expected {expected_tenant_id}, got {:?}",
        resources
            .iter()
            .map(|resource| resource.tenant_id.as_str())
            .collect::<Vec<_>>()
    );
}

pub fn assert_cross_tenant(requester_tenant_id: &str, target: &Resource) {
    assert_ne!(requester_tenant_id, target.tenant_id);
}

pub fn assert_tenant_path(tenant_id: &str, path: &str) {
    assert!(
        path.contains(&format!("/{tenant_id}/")),
        "path is not tenant-prefixed: {path}"
    );
}

pub fn assert_sse_event(tenant_id: &str, event: &SseEvent) {
    assert_eq!(event.schema_version, 1);
    assert!(!event.event_id.is_empty());
    assert!(!event.event.is_empty());
    assert_eq!(event.tenant_id, tenant_id);
    assert!(!event.task_id.is_empty());
    assert!(event.occurred_at.ends_with('Z'));
    assert!(event.data.is_object());
}

pub fn assert_queue_message(
    tenant_id: &str,
    actor_user_id: &str,
    message: &QueueMessage,
) {
    assert_eq!(message.schema_version, 1);
    assert_eq!(message.tenant_id, tenant_id);
    assert_eq!(message.actor_user_id, actor_user_id);
    assert!(!message.task_id.is_empty());
    assert!(!message.request_id.is_empty());

    let json = message.as_json();
    for field in ["schema_version", "tenant_id", "task_id", "actor_user_id", "request_id"] {
        assert!(json.get(field).is_some(), "queue field missing: {field}");
    }
}

pub fn assert_internal_request(tenant_id: &str, request: &InternalRequest) {
    assert_eq!(request.tenant_id, tenant_id);
    assert!(!request.user_id.is_empty());
    assert!(!request.request_id.is_empty());
    assert_eq!(request.body_sha256.len(), 64);
    assert!(request.body_sha256.chars().all(is_lower_hex));
    assert!(!request.canonical_request.ends_with('\n'));
    assert!(request.canonical_request.contains(&format!("\n{tenant_id}\n")));
    assert_eq!(
        request.replay_key,
        format!("{}:{}", request.tenant_id, request.request_id)
    );
    for header in REQUIRED_INTERNAL_HEADERS {
        assert!(
            request.headers.contains_key(header),
            "required internal header missing: {header}"
        );
    }
    assert_eq!(request.headers.get("X-Tenant-Id"), Some(&request.tenant_id));
    assert_eq!(request.headers.get("X-User-Id"), Some(&request.user_id));
    assert_eq!(request.headers.get("X-Request-Id"), Some(&request.request_id));
    assert_eq!(
        request.headers.get("X-Internal-Timestamp"),
        Some(&request.timestamp)
    );
    assert_eq!(
        request.headers.get("X-Internal-Signature"),
        Some(&request.signature)
    );
    assert_signature_shape(&request.signature);
}

pub fn assert_signature_shape(signature: &str) {
    assert!(signature.starts_with("v1="));
    let digest = &signature[3..];
    assert_eq!(digest.len(), 64);
    assert!(digest.chars().all(is_lower_hex));
}

pub fn assert_required_headers(headers: &BTreeMap<String, String>) {
    for header in REQUIRED_INTERNAL_HEADERS {
        assert!(headers.contains_key(header), "missing header: {header}");
    }
}

fn is_lower_hex(value: char) -> bool {
    value.is_ascii_digit() || ('a'..='f').contains(&value)
}

pub fn assert_error_code(data: &Value, expected: &str) {
    assert_eq!(data.get("error_code").and_then(Value::as_str), Some(expected));
}
