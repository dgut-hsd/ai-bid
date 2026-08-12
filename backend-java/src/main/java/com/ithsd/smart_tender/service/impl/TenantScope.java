package com.ithsd.smart_tender.service.impl;

import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantRequestContext;

import java.util.UUID;

/**
 * Current-tenant boundary for the Project/Tender/AuditTask MVP.
 *
 * <p>Resource services must obtain the tenant from the request context and
 * must never accept a tenant id supplied by a DTO.</p>
 */
public final class TenantScope {

    private TenantScope() {
    }

    public static Long requiredTenantId() {
        TenantRequestContext context = TenantContext.get();
        if (context == null || context.tenantId() == null) {
            throw new TenantAuthException(
                    400, "TENANT_REQUIRED", "租户上下文缺失", requestId(context));
        }
        return context.tenantId();
    }

    public static TenantAuthException resourceNotFound() {
        TenantRequestContext context = TenantContext.get();
        return new TenantAuthException(
                404, "RESOURCE_NOT_FOUND", "资源不存在", requestId(context));
    }

    private static String requestId(TenantRequestContext context) {
        return context == null || context.requestId() == null || context.requestId().isBlank()
                ? UUID.randomUUID().toString()
                : context.requestId();
    }
}
