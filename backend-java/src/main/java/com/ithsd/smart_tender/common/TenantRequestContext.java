package com.ithsd.smart_tender.common;

/** Immutable request-scoped identity and tenant authority snapshot. */
public record TenantRequestContext(
        Long userId,
        Long tenantId,
        String role,
        long sessionVersion,
        String requestId,
        boolean platformAdmin
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

    /**
     * Compatibility constructor. Tenant-scoped callers and legacy tests that do
     * not carry a platform scope default to {@code platformAdmin = false}.
     */
    public TenantRequestContext(
            Long userId,
            Long tenantId,
            String role,
            long sessionVersion,
            String requestId
    ) {
        this(userId, tenantId, role, sessionVersion, requestId, false);
    }
}