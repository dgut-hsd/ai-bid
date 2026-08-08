package com.ithsd.smart_tender.tenant.fixture;

import java.util.List;

/**
 * Negative contract cases to activate as implementation slices arrive.
 */
public final class TenantSecurityMatrix {

    public enum Activation {
        T3_RESOURCE_AND_PARENT_CHILD,
        T4_DOWNLOAD_AND_PREVIEW,
        T5_SSE_REPLAY,
        T6_QUEUE_AND_OUTBOX,
        T7_RUST_INTERNAL_HEADERS,
        T8_REGRESSION_SWEEP
    }

    public record NegativeCase(
            String name,
            String surface,
            Activation activation,
            String expectedErrorCode
    ) {
    }

    private TenantSecurityMatrix() {
    }

    public static List<NegativeCase> cases() {
        return List.of(
                new NegativeCase(
                        "cross_tenant_id_lookup_is_not_visible",
                        "resource lookup by tenantId and resourceId",
                        Activation.T3_RESOURCE_AND_PARENT_CHILD,
                        "RESOURCE_NOT_FOUND"
                ),
                new NegativeCase(
                        "cross_tenant_page_and_search_do_not_mix_items",
                        "pagination and search",
                        Activation.T3_RESOURCE_AND_PARENT_CHILD,
                        "RESOURCE_NOT_FOUND"
                ),
                new NegativeCase(
                        "cross_tenant_parent_child_lookup_is_not_visible",
                        "parent-child resource traversal",
                        Activation.T3_RESOURCE_AND_PARENT_CHILD,
                        "RESOURCE_NOT_FOUND"
                ),
                new NegativeCase(
                        "cross_tenant_download_is_not_visible",
                        "download",
                        Activation.T4_DOWNLOAD_AND_PREVIEW,
                        "RESOURCE_NOT_FOUND"
                ),
                new NegativeCase(
                        "cross_tenant_preview_is_not_visible",
                        "preview and normalized storage path",
                        Activation.T4_DOWNLOAD_AND_PREVIEW,
                        "RESOURCE_NOT_FOUND"
                ),
                new NegativeCase(
                        "cross_tenant_sse_replay_is_not_visible",
                        "SSE subscription and Last-Event-ID replay",
                        Activation.T5_SSE_REPLAY,
                        "RESOURCE_NOT_FOUND"
                ),
                new NegativeCase(
                        "queue_message_without_matching_tenant_context_is_rejected",
                        "queue and outbox envelope",
                        Activation.T6_QUEUE_AND_OUTBOX,
                        "TENANT_CONTEXT_INVALID"
                ),
                new NegativeCase(
                        "rust_header_tenant_mismatch_is_rejected",
                        "Java-to-Rust internal request",
                        Activation.T7_RUST_INTERNAL_HEADERS,
                        "INTERNAL_TENANT_MISMATCH"
                ),
                new NegativeCase(
                        "replayed_request_id_is_scoped_by_tenant",
                        "Rust replay cache",
                        Activation.T7_RUST_INTERNAL_HEADERS,
                        "INTERNAL_REQUEST_REPLAYED"
                ),
                new NegativeCase(
                        "all_surfaces_keep_the_same_negative_contract",
                        "cross-surface regression sweep",
                        Activation.T8_REGRESSION_SWEEP,
                        "RESOURCE_NOT_FOUND"
                )
        );
    }
}
