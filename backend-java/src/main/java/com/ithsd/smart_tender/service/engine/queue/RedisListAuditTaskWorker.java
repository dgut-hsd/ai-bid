package com.ithsd.smart_tender.service.engine.queue;

import com.ithsd.smart_tender.service.AuditEngineService;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.boot.autoconfigure.condition.ConditionalOnProperty;
import org.springframework.data.redis.core.StringRedisTemplate;
import org.springframework.scheduling.annotation.Scheduled;
import org.springframework.stereotype.Component;

import java.util.concurrent.TimeUnit;

@Component
@ConditionalOnProperty(name = "audit.queue.mode", havingValue = "redis-list")
public class RedisListAuditTaskWorker {
    private static final Logger log = LoggerFactory.getLogger(RedisListAuditTaskWorker.class);
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
                    log.error("discarding invalid audit task envelope from redis list", ex);
                    return;
                }
                log.info("received audit task from redis list, taskId={}, tenantId={}"
                        , envelope.taskId(), envelope.tenantId());
                try {
                    auditEngineService.start(envelope);
                } catch (Exception e) {
                    log.error("failed to process audit task, taskId={}, tenantId={}"
                            , envelope.taskId(), envelope.tenantId(), e);
                    // Simple retry logic could be implemented here if needed, 
                    // but per requirement we stick to simple list for now.
                    // If strict reliability is needed, we should push to DLQ or retry queue.
                }
            }
        } catch (Exception ex) {
            log.error("error polling redis list queue", ex);
        }
    }
}
