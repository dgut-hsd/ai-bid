package com.ithsd.smart_tender.service.engine.queue;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.PropertyNamingStrategies;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;

import java.util.LinkedHashMap;
import java.util.Map;

/**
 * Explicit identity carried with every audit dispatch.
 *
 * <p>A queue consumer must be able to establish tenant context before it
 * touches the task, tender, Rust client, or any child resource. The envelope
 * is therefore the authorization boundary; a task id alone is not a valid
 * audit message.</p>
 */
@JsonInclude(JsonInclude.Include.NON_NULL)
public record AuditTaskEnvelope(
        int schemaVersion,
        Long tenantId,
        String taskId,
        Long actorUserId,
        String role,
        long sessionVersion,
        String requestId
) {

    public static final int CURRENT_SCHEMA_VERSION = 1;

    private static final ObjectMapper OBJECT_MAPPER = new ObjectMapper()
            .setPropertyNamingStrategy(PropertyNamingStrategies.SNAKE_CASE);

    public AuditTaskEnvelope {
        if (schemaVersion != CURRENT_SCHEMA_VERSION) {
            throw new IllegalArgumentException("Unsupported audit envelope schema version");
        }
        if (tenantId == null || tenantId <= 0) {
            throw new IllegalArgumentException("tenantId must be positive");
        }
        if (taskId == null || taskId.isBlank()) {
            throw new IllegalArgumentException("taskId is required");
        }
        if (actorUserId == null || actorUserId <= 0) {
            throw new IllegalArgumentException("actorUserId must be positive");
        }
        if (sessionVersion <= 0) {
            throw new IllegalArgumentException("sessionVersion must be positive");
        }
        if (requestId == null || requestId.isBlank()) {
            throw new IllegalArgumentException("requestId is required");
        }
    }

    public static AuditTaskEnvelope capture(String taskId) {
        TenantRequestContext context = TenantContext.get();
        if (context == null || context.tenantId() == null || context.tenantId() <= 0) {
            throw new IllegalStateException("TenantContext with tenant is required for audit dispatch");
        }
        return new AuditTaskEnvelope(
                CURRENT_SCHEMA_VERSION,
                context.tenantId(),
                taskId,
                context.userId(),
                context.role(),
                context.sessionVersion(),
                context.requestId());
    }

    public TenantRequestContext toContext() {
        return new TenantRequestContext(actorUserId, tenantId, role, sessionVersion, requestId);
    }

    public String toJson() {
        try {
            return OBJECT_MAPPER.writeValueAsString(this);
        } catch (Exception ex) {
            throw new IllegalStateException("Unable to serialize audit task envelope", ex);
        }
    }

    public static AuditTaskEnvelope fromJson(String json) {
        if (json == null || json.isBlank()) {
            throw new IllegalArgumentException("Audit task envelope is empty");
        }
        try {
            return OBJECT_MAPPER.readValue(json, AuditTaskEnvelope.class);
        } catch (Exception ex) {
            throw new IllegalArgumentException("Invalid audit task envelope", ex);
        }
    }

    public Map<String, String> toRedisFields() {
        Map<String, String> fields = new LinkedHashMap<>();
        fields.put("schema_version", String.valueOf(schemaVersion));
        fields.put("tenant_id", String.valueOf(tenantId));
        fields.put("task_id", taskId);
        fields.put("actor_user_id", String.valueOf(actorUserId));
        if (role != null) {
            fields.put("role", role);
        }
        fields.put("session_version", String.valueOf(sessionVersion));
        fields.put("request_id", requestId);
        return fields;
    }

    public static AuditTaskEnvelope fromRedisFields(Map<?, ?> values) {
        if (values == null) {
            throw new IllegalArgumentException("Audit task envelope fields are empty");
        }
        return new AuditTaskEnvelope(
                parseInt(values, "schema_version"),
                parseLong(values, "tenant_id"),
                required(values, "task_id"),
                parseLong(values, "actor_user_id"),
                optional(values, "role"),
                parseLong(values, "session_version"),
                required(values, "request_id"));
    }

    private static String required(Map<?, ?> values, String key) {
        String value = optional(values, key);
        if (value == null || value.isBlank()) {
            throw new IllegalArgumentException("Missing audit envelope field: " + key);
        }
        return value;
    }

    private static String optional(Map<?, ?> values, String key) {
        Object value = values.get(key);
        return value == null ? null : String.valueOf(value);
    }

    private static int parseInt(Map<?, ?> values, String key) {
        try {
            return Integer.parseInt(required(values, key));
        } catch (NumberFormatException ex) {
            throw new IllegalArgumentException("Invalid audit envelope field: " + key, ex);
        }
    }

    private static long parseLong(Map<?, ?> values, String key) {
        try {
            return Long.parseLong(required(values, key));
        } catch (NumberFormatException ex) {
            throw new IllegalArgumentException("Invalid audit envelope field: " + key, ex);
        }
    }
}
