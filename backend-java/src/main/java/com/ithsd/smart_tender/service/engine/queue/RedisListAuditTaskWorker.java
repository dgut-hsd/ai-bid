package com.ithsd.smart_tender.service.engine.queue;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.ithsd.smart_tender.service.AuditEngineService;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.boot.autoconfigure.condition.ConditionalOnProperty;
import org.springframework.data.redis.core.StringRedisTemplate;
import org.springframework.scheduling.annotation.Scheduled;
import org.springframework.stereotype.Component;

import java.util.LinkedHashMap;
import java.util.Map;
import java.util.concurrent.TimeUnit;

@Component
@ConditionalOnProperty(name = "audit.queue.mode", havingValue = "redis-list")
public class RedisListAuditTaskWorker {
    private static final Logger log = LoggerFactory.getLogger(RedisListAuditTaskWorker.class);
    private static final ObjectMapper DLQ_MAPPER = new ObjectMapper();
    private final StringRedisTemplate redisTemplate;
    private final AuditQueueProperties queueProperties;
    private final AuditEngineService auditEngineService;

    public RedisListAuditTaskWorker(StringRedisTemplate redisTemplate, AuditQueueProperties queueProperties, AuditEngineService auditEngineService) {
        this.redisTemplate = redisTemplate;
        this.queueProperties = queueProperties;
        this.auditEngineService = auditEngineService;
    }

    @Scheduled(fixedDelay = 100) // Poll frequently
    public void poll() {
        try {
            String key = queueProperties.getStreamKey();
            // Using blocking pop with timeout to reduce idle CPU usage, but need to be careful with connection timeout
            // If blockMs is configured, use it, otherwise default to 1 second
            long timeout = queueProperties.getBlockMs() != null ? queueProperties.getBlockMs() : 1000;
            
            // LPOP is non-blocking, BLPOP is blocking. Let's use leftPop with timeout which is BLPOP
            String payload = redisTemplate.opsForList().leftPop(key, timeout, TimeUnit.MILLISECONDS);

            if (payload != null) {
                AuditTaskEnvelope envelope;
                try {
                    envelope = AuditTaskEnvelope.fromJson(payload);
                } catch (IllegalArgumentException ex) {
                    // leftPop 已破坏性取出，旧格式/无法解析的在途任务必须路由到 DLQ，不能静默丢弃
                    routeToDlq(payload, "unparseable/legacy payload: " + ex.getMessage());
                    return;
                }
                log.info("received audit task from redis list, taskId={}, tenantId={}"
                        , envelope.taskId(), envelope.tenantId());
                try {
                    auditEngineService.start(envelope);
                } catch (Exception e) {
                    log.error("failed to process audit task, taskId={}, tenantId={}"
                            , envelope.taskId(), envelope.tenantId(), e);
                    routeToDlq(payload, "start failure: " + e.getMessage());
                }
            }
        } catch (Exception ex) {
            log.error("error polling redis list queue", ex);
        }
    }

    private void routeToDlq(String rawPayload, String reason) {
        try {
            Map<String, Object> dead = new LinkedHashMap<>();
            dead.put("reason", reason == null ? "unknown" : reason);
            dead.put("raw_payload", rawPayload);
            dead.put("enqueued_at", System.currentTimeMillis());
            String deadJson = DLQ_MAPPER.writeValueAsString(dead);
            redisTemplate.opsForList().rightPush(queueProperties.getDlqListKey(), deadJson);
            log.warn("routed audit task to DLQ list, key={}, reason={}",
                    queueProperties.getDlqListKey(), reason);
        } catch (Exception ex) {
            log.error("failed to route audit task to DLQ list, key={}",
                    queueProperties.getDlqListKey(), ex);
        }
    }
}
