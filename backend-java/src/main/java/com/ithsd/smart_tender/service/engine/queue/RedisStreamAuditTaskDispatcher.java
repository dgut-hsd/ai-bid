package com.ithsd.smart_tender.service.engine.queue;

import com.ithsd.smart_tender.common.TenantContext;
import org.springframework.boot.autoconfigure.condition.ConditionalOnProperty;
import org.springframework.data.redis.connection.stream.MapRecord;
import org.springframework.data.redis.core.StringRedisTemplate;
import org.springframework.stereotype.Component;

import java.util.Map;

@Component
@ConditionalOnProperty(prefix = "audit.queue", name = "mode", havingValue = "redis-stream")
public class RedisStreamAuditTaskDispatcher implements AuditTaskDispatcher {
    private final StringRedisTemplate redisTemplate;
    private final AuditQueueProperties queueProperties;

    public RedisStreamAuditTaskDispatcher(StringRedisTemplate redisTemplate, AuditQueueProperties queueProperties) {
        this.redisTemplate = redisTemplate;
        this.queueProperties = queueProperties;
    }

    @Override
    public void dispatch(String taskId) {
        // 把租户上下文一并入队，worker 线程才能重建 TenantContext 发起 Rust 内部签名请求。
        Map<String, String> fields = QueuedAuditTask.fields(taskId, TenantContext.snapshot());
        fields.put("retry", "0");
        MapRecord<String, String, String> record = MapRecord.create(queueProperties.getStreamKey(), fields);
        redisTemplate.opsForStream().add(record);
    }
}
