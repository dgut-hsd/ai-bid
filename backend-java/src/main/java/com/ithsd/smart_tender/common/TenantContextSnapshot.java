package com.ithsd.smart_tender.common;

/** Explicit value object for crossing a thread or async boundary later. */
public record TenantContextSnapshot(
        Long userId,
        Long tenantId,
        String role,
        long sessionVersion,
        String requestId
) {
    public TenantRequestContext toContext() {
        return new TenantRequestContext(userId, tenantId, role, sessionVersion, requestId);
    }

    public static TenantContextSnapshot from(TenantRequestContext context) {
        if (context == null) {
            return null;
        }
        return new TenantContextSnapshot(
                context.userId(),
                context.tenantId(),
                context.role(),
                context.sessionVersion(),
                context.requestId()
        );
    }
}
