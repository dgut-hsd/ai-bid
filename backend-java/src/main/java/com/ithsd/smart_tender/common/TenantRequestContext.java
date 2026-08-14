package com.ithsd.smart_tender.common;

/** Immutable request-scoped identity and tenant authority snapshot. */
public record TenantRequestContext(
        Long userId,
        Long tenantId,
        String role,
        long sessionVersion,
        String requestId
) {
    public TenantRequestContext {
        if (userId == null || userId <= 0) {
            throw new IllegalArgumentException("userId must be positive");
        }
        if (sessionVersion <= 0) {
            throw new IllegalArgumentException("sessionVersion must be positive");
        }
        if (requestId == null || requestId.isBlank()) {
            throw new IllegalArgumentException("requestId is required");
        }
    }
}
