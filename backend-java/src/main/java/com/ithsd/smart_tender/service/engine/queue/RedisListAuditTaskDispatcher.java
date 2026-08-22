package com.ithsd.smart_tender.service.engine.queue;

import com.ithsd.smart_tender.common.TenantContext;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.boot.autoconfigure.condition.ConditionalOnProperty;
import org.springframework.data.redis.core.StringRedisTemplate;
import org.springframework.stereotype.Component;

@Component
@ConditionalOnProperty(name = "audit.queue.mode", havingValue = "redis-list")
public class RedisListAuditTaskDispatcher implements AuditTaskDispatcher {
    private static final Logger log = LoggerFactory.getLogger(RedisListAuditTaskDispatcher.class);
    private final StringRedisTemplate redisTemplate;
    private final AuditQueueProperties queueProperties;

    public RedisListAuditTaskDispatcher(StringRedisTemplate redisTemplate, AuditQueueProperties queueProperties) {
        this.redisTemplate = redisTemplate;
        this.queueProperties = queueProperties;
    }

    @Override
    public void dispatch(String taskId) {
        try {
            String key = queueProperties.getStreamKey(); // reusing streamKey config as list key
            // 把租户上下文一并入队，worker 线程才能重建 TenantContext 发起 Rust 内部签名请求。
            String payload = QueuedAuditTask.encode(taskId, TenantContext.snapshot());
            redisTemplate.opsForList().rightPush(key, payload);
            log.info("audit task dispatched to redis list, taskId={}, key={}", taskId, key);
        } catch (Exception ex) {
            log.error("failed to dispatch audit task to redis list, taskId={}", taskId, ex);
            throw ex;
        }
    }
}
