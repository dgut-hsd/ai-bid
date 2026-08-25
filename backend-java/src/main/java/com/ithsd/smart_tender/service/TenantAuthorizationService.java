package com.ithsd.smart_tender.service;

import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import org.springframework.stereotype.Component;

import java.util.List;
import java.util.Map;
import java.util.UUID;

/**
 * Small authorization boundary for the MVP tenant and knowledge APIs.
 * The request context is authoritative; request parameters are only resource
 * selectors and never establish tenant ownership.
 */
@Component
public class TenantAuthorizationService {

    private static final Map<String, List<String>> ROLE_PERMISSIONS = Map.of(
            "OWNER", List.of(
                    "tenant.read", "tenant.settings.write", "tenant.members.invite",
                    "tenant.members.remove", "tenant.members.role.write", "tenant.owner.transfer",
                    "tender.write", "audit.start", "audit.report.read", "knowledge.write", "tenant.delete"
            ),
            "MEMBER", List.of("tenant.read", "tender.write", "audit.start", "audit.report.read", "knowledge.write")
    );

    public TenantRequestContext requireAuthenticated() {
        TenantRequestContext context = TenantContext.get();
        if (context == null || context.userId() == null) {
            throw error(401, "AUTH_REQUIRED", "Authentication is required", null);
        }
        return context;
    }

    public TenantRequestContext requireCurrentTenant() {
        TenantRequestContext context = requireAuthenticated();
        if (context.tenantId() == null) {
            throw error(400, "TENANT_REQUIRED", "A current tenant is required", context.requestId());
        }
        return context;
    }

    public TenantRequestContext requireTenant(Long requestedTenantId) {
        TenantRequestContext context = requireAuthenticated();
        if (requestedTenantId == null
                || context.tenantId() == null
                || !requestedTenantId.equals(context.tenantId())) {
            throw error(404, "TENANT_NOT_FOUND", "Tenant not found", context.requestId());
        }
        return context;
    }

    public TenantRequestContext requireTenant(Long requestedTenantId, String permission) {
        TenantRequestContext context = requireTenant(requestedTenantId);
        if (!hasPermission(context.role(), permission)) {
            throw new TenantAuthException(
                    403,
                    "TENANT_ROLE_FORBIDDEN",
                    "The current role cannot perform this operation",
                    requestId(context),
                    Map.of("required_permission", permission)
            );
        }
        return context;
    }

    public static List<String> permissionsFor(String role) {
        return ROLE_PERMISSIONS.getOrDefault(role, List.of());
    }

    private static boolean hasPermission(String role, String permission) {
        return permissionsFor(role).contains(permission);
    }

    private static String requestId(TenantRequestContext context) {
        return context == null || context.requestId() == null || context.requestId().isBlank()
                ? UUID.randomUUID().toString()
                : context.requestId();
    }

    private static TenantAuthException error(int status, String code, String message, String requestId) {
        return new TenantAuthException(
                status,
                code,
                message,
                requestId == null || requestId.isBlank() ? UUID.randomUUID().toString() : requestId
        );
    }
}
