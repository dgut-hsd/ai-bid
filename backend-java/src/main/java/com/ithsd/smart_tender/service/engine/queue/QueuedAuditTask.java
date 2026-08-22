package com.ithsd.smart_tender.service.engine.queue;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.ithsd.smart_tender.common.TenantContextSnapshot;

import java.util.LinkedHashMap;
import java.util.Map;

/**
 * Redis 队列消息载荷。
 *
 * <p>除 taskId 外，把租户上下文一并序列化进队列，使 worker 线程（无请求
 * ThreadLocal）能重建 {@link com.ithsd.smart_tender.common.TenantContext}
 * 后再发起需要内部签名的 Rust 调用。</p>
 */
public final class QueuedAuditTask {

    public static final String TASK_ID = "taskId";
    public static final String USER_ID = "userId";
    public static final String TENANT_ID = "tenantId";
    public static final String ROLE = "role";
    public static final String SESSION_VERSION = "sessionVersion";
    public static final String REQUEST_ID = "requestId";

    private static final ObjectMapper MAPPER = new ObjectMapper();

    private QueuedAuditTask() {
    }

    /** 构建 taskId + 租户上下文字段集合（Redis Stream 用）。 */
    public static Map<String, String> fields(String taskId, TenantContextSnapshot context) {
        if (context == null) {
            throw new IllegalArgumentException("TenantContextSnapshot is required for queued audit task");
        }
        Map<String, String> fields = new LinkedHashMap<>();
        fields.put(TASK_ID, taskId);
        fields.put(USER_ID, String.valueOf(context.userId()));
        fields.put(TENANT_ID, String.valueOf(context.tenantId()));
        fields.put(ROLE, context.role() == null ? "" : context.role());
        fields.put(SESSION_VERSION, String.valueOf(context.sessionVersion()));
        fields.put(REQUEST_ID, context.requestId());
        return fields;
    }

    /** 序列化为 JSON（Redis List 用）。 */
    public static String encode(String taskId, TenantContextSnapshot context) {
        try {
            return MAPPER.writeValueAsString(fields(taskId, context));
        } catch (JsonProcessingException e) {
            throw new IllegalStateException("failed to encode queued audit task", e);
        }
    }

    /** 从 JSON 反序列化（Redis List worker 用）。 */
    public static Decoded decode(String payload) {
        try {
            Map<String, String> fields = MAPPER.readValue(payload, new TypeReference<Map<String, String>>() {
            });
            return decode(fields);
        } catch (JsonProcessingException e) {
            throw new IllegalStateException("failed to decode queued audit task", e);
        }
    }

    /** 从字段集合反序列化（Redis Stream worker 用）。 */
    public static Decoded decode(Map<String, String> fields) {
        Long userId = Long.valueOf(fields.get(USER_ID));
        Long tenantId = Long.valueOf(fields.get(TENANT_ID));
        String role = fields.get(ROLE);
        long sessionVersion = Long.parseLong(fields.get(SESSION_VERSION));
        String requestId = fields.get(REQUEST_ID);
        TenantContextSnapshot context = new TenantContextSnapshot(userId, tenantId, role, sessionVersion, requestId);
        return new Decoded(fields.get(TASK_ID), context);
    }

    /** 解码结果：taskId 与可重建线程上下文的租户快照。 */
    public record Decoded(String taskId, TenantContextSnapshot context) {
    }
}
