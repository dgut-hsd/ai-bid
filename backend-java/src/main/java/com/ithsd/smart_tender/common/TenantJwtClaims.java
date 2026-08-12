package com.ithsd.smart_tender.common;

import java.util.List;

/** Immutable, typed claims accepted from a tenant session JWT. */
public record TenantJwtClaims(
        Long userId,
        Long tenantId,
        String role,
        long sessionVersion,
        List<String> permissions,
        String sessionId
) {
    public TenantJwtClaims {
        permissions = permissions == null ? List.of() : List.copyOf(permissions);
    }
}
