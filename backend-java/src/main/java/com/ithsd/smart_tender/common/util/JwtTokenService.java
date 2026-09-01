package com.ithsd.smart_tender.common.util;

import com.ithsd.smart_tender.common.TenantJwtClaims;
import com.ithsd.smart_tender.config.JwtProperties;
import io.jsonwebtoken.Claims;
import org.springframework.stereotype.Component;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;

/** Creates and validates the claims required by the tenant session contract. */
@Component
public class JwtTokenService {

    private final JwtProperties properties;

    public JwtTokenService(JwtProperties properties) {
        this.properties = properties;
    }

    public String issue(
            Long userId,
            Long tenantId,
            String role,
            List<String> permissions,
            long sessionVersion,
            String sessionId
    ) {
        requireConfigured();
        Map<String, Object> claims = new LinkedHashMap<>();
        String userIdValue = String.valueOf(userId);
        claims.put("sub", userIdValue);
        claims.put("user_id", userIdValue);
        claims.put("tenant_id", tenantId == null ? "" : String.valueOf(tenantId));
        claims.put("role", role == null ? "" : role);
        claims.put("permissions", permissions == null ? List.of() : List.copyOf(permissions));
        claims.put("session_version", sessionVersion);
        claims.put("session_id", sessionId == null ? UUID.randomUUID().toString() : sessionId);
        return JwtUtil.createJWT(properties.getSecret(), properties.getTtlMillis(), claims);
    }

    public TenantJwtClaims parse(String token) {
        requireConfigured();
        return toClaims(JwtUtil.parseJWT(properties.getSecret(), token));
    }

    /**
     * 容忍「签名有效、仅 exp 过期」的 token（签名错误、格式错误仍会抛异常）。
     * 仅供 refresh 在 Redis 会话存活窗口内续期使用，续期窗口上限由会话 TTL 决定。
     */
    public TenantJwtClaims parsePermissive(String token) {
        requireConfigured();
        return toClaims(JwtUtil.parseJWTPermissive(properties.getSecret(), token));
    }

    private TenantJwtClaims toClaims(Claims claims) {
        String userIdValue = textClaim(claims, "user_id");
        if (userIdValue == null && properties.isAcceptLegacyUserId()) {
            userIdValue = textClaim(claims, "userId");
        }
        Long userId = requiredLong(userIdValue, "user_id");

        if (!claims.containsKey("tenant_id")) {
            throw new IllegalArgumentException("missing JWT claim: tenant_id");
        }
        String tenantValue = textClaim(claims, "tenant_id");
        Long tenantId = tenantValue == null || tenantValue.isBlank() ? null : requiredLong(tenantValue, "tenant_id");
        String role = requiredText(claims, "role");
        long sessionVersion = requiredLongValue(claims.get("session_version"), "session_version");
        String sessionId = textClaim(claims, "session_id");

        List<String> permissions = new ArrayList<>();
        Object rawPermissions = claims.get("permissions");
        if (rawPermissions instanceof Iterable<?> iterable) {
            for (Object permission : iterable) {
                if (permission != null && !permission.toString().isBlank()) {
                    permissions.add(permission.toString());
                }
            }
        }
        return new TenantJwtClaims(userId, tenantId, role, sessionVersion, permissions, sessionId);
    }

    public long getTtlMillis() {
        return properties.getTtlMillis();
    }

    public long getSessionTtlMillis() {
        return properties.getSessionTtlMillis();
    }

    public long getExpiresInSeconds() {
        return Math.max(1L, (properties.getTtlMillis() + 999L) / 1000L);
    }

    private void requireConfigured() {
        if (properties.getSecret() == null || properties.getSecret().isBlank()) {
            throw new IllegalStateException("security.jwt.secret must be configured");
        }
        if (properties.getTtlMillis() <= 0) {
            throw new IllegalStateException("security.jwt.ttl-millis must be positive");
        }
    }

    private static String requiredText(Claims claims, String name) {
        String value = textClaim(claims, name);
        if (value == null) {
            throw new IllegalArgumentException("missing JWT claim: " + name);
        }
        return value;
    }

    private static String textClaim(Claims claims, String name) {
        Object value = claims.get(name);
        return value == null ? null : value.toString();
    }

    private static Long requiredLong(String value, String name) {
        if (value == null || value.isBlank()) {
            throw new IllegalArgumentException("missing JWT claim: " + name);
        }
        try {
            return Long.valueOf(value);
        } catch (NumberFormatException ex) {
            throw new IllegalArgumentException("invalid JWT claim: " + name, ex);
        }
    }

    private static long requiredLongValue(Object value, String name) {
        if (value == null) {
            throw new IllegalArgumentException("missing JWT claim: " + name);
        }
        try {
            return value instanceof Number number ? number.longValue() : Long.parseLong(value.toString());
        } catch (NumberFormatException ex) {
            throw new IllegalArgumentException("invalid JWT claim: " + name, ex);
        }
    }
}
