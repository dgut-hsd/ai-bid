mod support;

use support::tenant_assertions::{
    assert_cross_tenant, assert_internal_request, assert_queue_message, assert_same_tenant,
    assert_error_code, assert_required_headers, assert_signature_shape, assert_sse_event,
    assert_tenant_path, assert_tenant_scoped,
};
use support::tenant_fixture::{
    Tenant, TenantFixture, NEGATIVE_CASES, TENANT_A_ID, TENANT_B_ID,
};

#[test]
fn fixture_separates_same_resource_ids_by_tenant() {
    let fixture = TenantFixture::builder().build();
    assert_eq!(fixture.tenant_a.id, TENANT_A_ID);
    assert_eq!(fixture.tenant_b.id, TENANT_B_ID);
    let document_a = fixture.resource_for(TENANT_A_ID, "document-01");
    let document_b = fixture.resource_for(TENANT_B_ID, "document-01");

    assert_eq!(document_a.id, document_b.id);
    assert_ne!(document_a.tenant_id, document_b.tenant_id);
    assert_cross_tenant(TENANT_A_ID, document_b);
}

#[test]
fn fixture_builder_allows_custom_context() {
    let fixture = TenantFixture::builder()
        .actor_user_id("10002")
        .tenant_a(Tenant::new("30001", "tenant-a", "VIEWER", &["tenant.read"]))
        .tenant_b(Tenant::new("30002", "tenant-b", "MEMBER", &["tenant.read"]))
        .build();

    assert_eq!(fixture.actor_user_id, "10002");
    assert_eq!(fixture.tenant_a.id, "30001");
    assert_eq!(fixture.tenant_b.id, "30002");
    assert_eq!(fixture.queue_message_for("30001").actor_user_id, "10002");
}

#[test]
fn fixture_keeps_pagination_and_search_results_tenant_scoped() {
    let fixture = TenantFixture::builder().build();
    let page_a = fixture.page_for(TENANT_A_ID);
    let page_b = fixture.page_for(TENANT_B_ID);

    assert_eq!(page_a.page, 1);
    assert_eq!(page_a.size, 20);
    assert_eq!(page_a.query, "smart campus");
    assert_tenant_scoped(TENANT_A_ID, &page_a.items);
    assert_tenant_scoped(TENANT_B_ID, &page_b.items);
}

#[test]
fn fixture_keeps_parent_and_child_resources_in_the_same_tenant() {
    let fixture = TenantFixture::builder().build();
    let project_a = fixture.resource_for(TENANT_A_ID, "project-01");
    let document_a = fixture.resource_for(TENANT_A_ID, "document-01");
    let project_b = fixture.resource_for(TENANT_B_ID, "project-01");
    let document_b = fixture.resource_for(TENANT_B_ID, "document-01");

    assert_same_tenant(project_a, document_a);
    assert_same_tenant(project_b, document_b);
    assert_cross_tenant(TENANT_A_ID, project_b);
}

#[test]
fn fixture_tenant_prefixes_download_preview_and_storage_paths() {
    let fixture = TenantFixture::builder().build();
    let preview_a = fixture.download_preview_for(TENANT_A_ID);
    let preview_b = fixture.download_preview_for(TENANT_B_ID);
    let document_a = fixture.resource_for(TENANT_A_ID, "document-01");
    let document_b = fixture.resource_for(TENANT_B_ID, "document-01");

    assert_eq!(preview_a.resource_id, preview_b.resource_id);
    assert_eq!(preview_a.tenant_id, TENANT_A_ID);
    assert_eq!(preview_b.tenant_id, TENANT_B_ID);
    assert_tenant_path(TENANT_A_ID, &document_a.storage_path);
    assert_tenant_path(TENANT_B_ID, &document_b.storage_path);
    assert!(!preview_a.download_path.is_empty());
    assert!(!preview_a.preview_path.is_empty());
}

#[test]
fn fixture_propagates_tenant_context_through_sse_and_queue() {
    let fixture = TenantFixture::builder().build();
    let event_a = fixture.event_for(TENANT_A_ID);
    let event_b = fixture.event_for(TENANT_B_ID);
    let queue_a = fixture.queue_message_for(TENANT_A_ID);

    assert_sse_event(TENANT_A_ID, event_a);
    assert_sse_event(TENANT_B_ID, event_b);
    assert_queue_message(TENANT_A_ID, &fixture.actor_user_id, queue_a);
    assert_eq!(event_a.event_id, event_b.event_id);
    assert_eq!(event_a.task_id, event_b.task_id);
    assert_eq!(queue_a.request_id, fixture.queue_message_for(TENANT_B_ID).request_id);
}

#[test]
fn fixture_models_internal_headers_canonical_lines_and_replay_scope() {
    let fixture = TenantFixture::builder().build();
    let request_a = fixture.internal_request_for(TENANT_A_ID);
    let request_b = fixture.internal_request_for(TENANT_B_ID);

    assert_internal_request(TENANT_A_ID, request_a);
    assert_internal_request(TENANT_B_ID, request_b);
    assert_required_headers(&request_a.headers);
    assert_signature_shape(&request_a.signature);
    assert_ne!(request_a.replay_key, request_b.replay_key);
    assert_ne!(request_a.canonical_request, request_b.canonical_request);
    assert_eq!(
        request_a.canonical_request.lines().count(),
        8,
        "canonical request must keep the frozen eight-line order"
    );
}

#[test]
fn negative_matrix_marks_t3_to_t8_activation_points() {
    assert_eq!(NEGATIVE_CASES.len(), 10);
    for case in NEGATIVE_CASES {
        assert!(!case.name.is_empty());
        assert!(!case.surface.is_empty());
        assert!(matches!(case.activation, "T3" | "T4" | "T5" | "T6" | "T7" | "T8"));
        assert!(!case.expected_error_code.is_empty());
    }
    assert!(NEGATIVE_CASES.iter().any(|case| case.expected_error_code == "RESOURCE_NOT_FOUND"));
    assert!(NEGATIVE_CASES.iter().any(|case| case.expected_error_code == "TENANT_CONTEXT_INVALID"));
    assert!(NEGATIVE_CASES.iter().any(|case| case.expected_error_code == "INTERNAL_TENANT_MISMATCH"));
    let error = serde_json::json!({
        "error_code": "RESOURCE_NOT_FOUND",
        "request_id": "request-a"
    });
    assert_error_code(&error, "RESOURCE_NOT_FOUND");
}
