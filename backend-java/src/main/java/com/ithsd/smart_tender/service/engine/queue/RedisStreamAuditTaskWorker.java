package com.ithsd.smart_tender.service.engine.queue;

import com.ithsd.smart_tender.service.AuditEngineService;
import jakarta.annotation.PostConstruct;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.boot.autoconfigure.condition.ConditionalOnProperty;
import org.springframework.data.redis.RedisSystemException;
import org.springframework.data.redis.connection.stream.Consumer;
import org.springframework.data.redis.connection.stream.MapRecord;
import org.springframework.data.redis.connection.stream.ReadOffset;
import org.springframework.data.redis.connection.stream.StreamOffset;
import org.springframework.data.redis.connection.stream.StreamReadOptions;
import org.springframework.data.redis.core.StringRedisTemplate;
import org.springframework.scheduling.annotation.Scheduled;
import org.springframework.stereotype.Component;
import org.springframework.util.StringUtils;

import java.time.Duration;
import java.util.List;
import java.util.Map;
import java.util.UUID;

@Component
@ConditionalOnProperty(prefix = "audit.queue", name = "mode", havingValue = "redis-stream")
@SuppressWarnings("unchecked")
public class RedisStreamAuditTaskWorker {
    private static final Logger log = LoggerFactory.getLogger(RedisStreamAuditTaskWorker.class);
    private final StringRedisTemplate redisTemplate;
    private final AuditEngineService auditEngineService;
    private final AuditQueueProperties queueProperties;
    private final String consumerName;

    public RedisStreamAuditTaskWorker(StringRedisTemplate redisTemplate, AuditEngineService auditEngineService, AuditQueueProperties queueProperties) {
        this.redisTemplate = redisTemplate;
        this.auditEngineService = auditEngineService;
        this.queueProperties = queueProperties;
        this.consumerName = queueProperties.getConsumerNamePrefix() + "-" + UUID.randomUUID();
    }

    @PostConstruct
    public void initGroup() {
        ensureGroup();
    }

    @Scheduled(fixedDelayString = "${audit.queue.poll-delay-ms:500}")
    public void poll() {
        try {
            List<MapRecord<String, Object, Object>> records = redisTemplate.opsForStream().read(
                    Consumer.from(queueProperties.getConsumerGroup(), consumerName),
                    StreamReadOptions.empty()
                            .block(Duration.ofMillis(Math.max(1, queueProperties.getBlockMs())))
                            .count(Math.max(1, queueProperties.getBatchSize())),
                    StreamOffset.create(queueProperties.getStreamKey(), ReadOffset.lastConsumed())
            );
            if (records == null || records.isEmpty()) {
                return;
            }
            for (MapRecord<String, Object, Object> record : records) {
                processRecord(record);
            }
        } catch (RuntimeException ex) {
            log.warn("poll redis stream failed, stream={}, group={}", queueProperties.getStreamKey(), queueProperties.getConsumerGroup(), ex);
        }
    }

    private void processRecord(MapRecord<String, Object, Object> record) {
        int retry = parseRetry(value(record, "retry"));
        AuditTaskEnvelope envelope;
        try {
            envelope = AuditTaskEnvelope.fromRedisFields(record.getValue());
        } catch (IllegalArgumentException ex) {
            log.warn("discarding invalid audit task envelope from redis stream, messageId={}"
                    , record.getId().getValue(), ex);
            ack(record.getId().getValue());
            return;
        }
        try {
            auditEngineService.start(envelope);
            ack(record.getId().getValue());
        } catch (RuntimeException ex) {
            handleFailure(record.getId().getValue(), envelope, retry, ex);
        }
    }

    private void handleFailure(String messageId, AuditTaskEnvelope envelope, int retry, RuntimeException ex) {
        int nextRetry = retry + 1;
        if (nextRetry > Math.max(0, queueProperties.getMaxRetry())) {
            Map<String, String> fields = new java.util.LinkedHashMap<>(envelope.toRedisFields());
            fields.put("retry", String.valueOf(retry));
            fields.put("reason", ex.getMessage() == null ? "unknown" : ex.getMessage());
            redisTemplate.opsForStream().add(MapRecord.<String, String, String>create(queueProperties.getDlqStreamKey(), Map.of(
                    "schema_version", fields.get("schema_version"),
                    "tenant_id", fields.get("tenant_id"),
                    "task_id", fields.get("task_id"),
                    "actor_user_id", fields.get("actor_user_id"),
                    "session_version", fields.get("session_version"),
                    "request_id", fields.get("request_id"),
                    "retry", fields.get("retry"),
                    "reason", fields.get("reason")
            )));
            ack(messageId);
            return;
        }
        Map<String, String> fields = new java.util.LinkedHashMap<>(envelope.toRedisFields());
        fields.put("retry", String.valueOf(nextRetry));
        redisTemplate.opsForStream().add(MapRecord.<String, String, String>create(queueProperties.getStreamKey(), Map.of(
                "schema_version", fields.get("schema_version"),
                "tenant_id", fields.get("tenant_id"),
                "task_id", fields.get("task_id"),
                "actor_user_id", fields.get("actor_user_id"),
                "session_version", fields.get("session_version"),
                "request_id", fields.get("request_id"),
                "retry", fields.get("retry")
        )));
        ack(messageId);
    }

    private void ack(String messageId) {
        redisTemplate.opsForStream().acknowledge(
                queueProperties.getStreamKey(),
                queueProperties.getConsumerGroup(),
                messageId
        );
    }

    private int parseRetry(String value) {
        if (!StringUtils.hasText(value)) {
            return 0;
        }
        try {
            return Integer.parseInt(value);
        } catch (NumberFormatException ex) {
            return 0;
        }
    }

    private String value(MapRecord<String, Object, Object> record, String key) {
        Object value = record.getValue().get(key);
        return value == null ? null : String.valueOf(value);
    }

    private void ensureGroup() {
        try {
            redisTemplate.opsForStream().createGroup(queueProperties.getStreamKey(), ReadOffset.latest(), queueProperties.getConsumerGroup());
            return;
        } catch (RedisSystemException ex) {
            String message = ex.getMessage();
            if (message != null && message.contains("BUSYGROUP")) {
                return;
            }
            if (message == null || !message.contains("requires the key to exist")) {
                throw ex;
            }
        }
        redisTemplate.opsForStream().add(MapRecord.<String, String, String>create(queueProperties.getStreamKey(), Map.of(
                "schema_version", "1",
                "tenant_id", "0",
                "task_id", "_init_",
                "actor_user_id", "0",
                "session_version", "0",
                "request_id", "_init_",
                "retry", "0"
        )));
        try {
            redisTemplate.opsForStream().createGroup(queueProperties.getStreamKey(), ReadOffset.latest(), queueProperties.getConsumerGroup());
        } catch (RedisSystemException ex) {
            String message = ex.getMessage();
            if (message == null || !message.contains("BUSYGROUP")) {
                throw ex;
            }
        }
    }
}
