package com.ithsd.smart_tender.service.engine.queue;

import org.springframework.boot.autoconfigure.condition.ConditionalOnProperty;
import org.springframework.data.redis.connection.stream.MapRecord;
import org.springframework.data.redis.core.StringRedisTemplate;
import org.springframework.stereotype.Component;

import java.util.Map;
import java.util.LinkedHashMap;

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
    public void dispatch(AuditTaskEnvelope envelope) {
        Map<String, String> fields = new LinkedHashMap<>(envelope.toRedisFields());
        fields.put("retry", "0");
        MapRecord<String, String, String> record = MapRecord.create(queueProperties.getStreamKey(), fields);
        redisTemplate.opsForStream().add(record);
    }
}
