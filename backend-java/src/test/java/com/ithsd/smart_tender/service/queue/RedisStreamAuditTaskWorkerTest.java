package com.ithsd.smart_tender.service.queue;

import com.ithsd.smart_tender.service.AuditEngineService;
import com.ithsd.smart_tender.service.engine.queue.AuditQueueProperties;
import com.ithsd.smart_tender.service.engine.queue.AuditTaskEnvelope;
import com.ithsd.smart_tender.service.engine.queue.RedisStreamAuditTaskWorker;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.ArgumentCaptor;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;
import org.springframework.data.redis.connection.stream.MapRecord;
import org.springframework.data.redis.core.StreamOperations;
import org.springframework.data.redis.core.StringRedisTemplate;
import org.springframework.test.util.ReflectionTestUtils;

import java.util.LinkedHashMap;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.doThrow;
import static org.mockito.Mockito.lenient;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

@ExtendWith(MockitoExtension.class)
class RedisStreamAuditTaskWorkerTest {

    @Mock
    private StringRedisTemplate redisTemplate;

    @Mock
    private StreamOperations<String, Object, Object> streamOperations;

    @Mock
    private AuditQueueProperties queueProperties;

    @Mock
    private AuditEngineService auditEngineService;

    private RedisStreamAuditTaskWorker worker;

    @BeforeEach
    void setUp() {
        when(queueProperties.getConsumerNamePrefix()).thenReturn("worker");
        when(queueProperties.getStreamKey()).thenReturn("queue:audit:tasks");
        when(queueProperties.getConsumerGroup()).thenReturn("audit-task-workers");
        lenient().when(queueProperties.getDlqStreamKey()).thenReturn("queue:audit:tasks:dlq");
        when(queueProperties.getMaxRetry()).thenReturn(3);
        when(redisTemplate.opsForStream()).thenReturn(streamOperations);
        worker = new RedisStreamAuditTaskWorker(redisTemplate, auditEngineService, queueProperties);
    }

    @Test
    void retry_preservesCompleteEnvelopeIdentityIncludingRole() {
        AuditTaskEnvelope envelope = envelope();
        doThrow(new RuntimeException("temporary failure")).when(auditEngineService).start(envelope);

        ReflectionTestUtils.invokeMethod(worker, "processRecord", record(envelope, 0));

        ArgumentCaptor<MapRecord> added = ArgumentCaptor.forClass(MapRecord.class);
        verify(streamOperations).add(added.capture());
        Map<String, String> fields = streamFields(added.getValue());
        assertThat(fields)
                .containsEntry("schema_version", "1")
                .containsEntry("tenant_id", "20001")
                .containsEntry("task_id", "task_123")
                .containsEntry("actor_user_id", "10001")
                .containsEntry("role", "OWNER")
                .containsEntry("session_version", "7")
                .containsEntry("request_id", "request-a")
                .containsEntry("retry", "1");
    }

    @Test
    void dlq_preservesCompleteEnvelopeIdentityIncludingRole() {
        AuditTaskEnvelope envelope = envelope();
        when(queueProperties.getMaxRetry()).thenReturn(0);
        doThrow(new RuntimeException("permanent failure")).when(auditEngineService).start(envelope);

        ReflectionTestUtils.invokeMethod(worker, "processRecord", record(envelope, 0));

        ArgumentCaptor<MapRecord> added = ArgumentCaptor.forClass(MapRecord.class);
        verify(streamOperations).add(added.capture());
        Map<String, String> fields = streamFields(added.getValue());
        assertThat(fields)
                .containsEntry("schema_version", "1")
                .containsEntry("tenant_id", "20001")
                .containsEntry("task_id", "task_123")
                .containsEntry("actor_user_id", "10001")
                .containsEntry("role", "OWNER")
                .containsEntry("session_version", "7")
                .containsEntry("request_id", "request-a")
                .containsEntry("retry", "0")
                .containsEntry("reason", "permanent failure");
    }

    private static AuditTaskEnvelope envelope() {
        return new AuditTaskEnvelope(1, 20001L, "task_123", 10001L, "OWNER", 7L, "request-a");
    }

    private static MapRecord<String, String, String> record(AuditTaskEnvelope envelope, int retry) {
        Map<String, String> fields = new LinkedHashMap<>(envelope.toRedisFields());
        fields.put("retry", String.valueOf(retry));
        return MapRecord.create("queue:audit:tasks", fields);
    }

    @SuppressWarnings("unchecked")
    private static Map<String, String> streamFields(MapRecord<?, ?, ?> record) {
        return (Map<String, String>) record.getValue();
    }
}
