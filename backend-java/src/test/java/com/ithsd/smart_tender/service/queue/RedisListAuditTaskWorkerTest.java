package com.ithsd.smart_tender.service.queue;

import com.ithsd.smart_tender.service.AuditEngineService;
import com.ithsd.smart_tender.service.engine.queue.AuditQueueProperties;
import com.ithsd.smart_tender.service.engine.queue.AuditTaskEnvelope;
import com.ithsd.smart_tender.service.engine.queue.RedisListAuditTaskWorker;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.InjectMocks;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;
import org.springframework.data.redis.core.ListOperations;
import org.springframework.data.redis.core.StringRedisTemplate;

import java.util.concurrent.TimeUnit;

import static org.mockito.ArgumentMatchers.any;
import static org.mockito.ArgumentMatchers.anyLong;
import static org.mockito.ArgumentMatchers.eq;
import static org.mockito.Mockito.*;

@ExtendWith(MockitoExtension.class)
class RedisListAuditTaskWorkerTest {

    @Mock
    private StringRedisTemplate redisTemplate;

    @Mock
    private ListOperations<String, String> listOperations;

    @Mock
    private AuditQueueProperties queueProperties;

    @Mock
    private AuditEngineService auditEngineService;

    @InjectMocks
    private RedisListAuditTaskWorker worker;

    @Test
    void poll_TaskAvailable() {
        when(redisTemplate.opsForList()).thenReturn(listOperations);
        when(queueProperties.getStreamKey()).thenReturn("queue:audit:tasks");
        when(queueProperties.getBlockMs()).thenReturn(1000);
        AuditTaskEnvelope envelope = new AuditTaskEnvelope(
                1, 20001L, "task_123", 10001L, "OWNER", 1L, "request-a");
        when(listOperations.leftPop(eq("queue:audit:tasks"), eq(1000L), eq(TimeUnit.MILLISECONDS)))
                .thenReturn(envelope.toJson());

        worker.poll();

        verify(auditEngineService).start(envelope);
    }

    @Test
    void poll_NoTask() {
        when(redisTemplate.opsForList()).thenReturn(listOperations);
        when(queueProperties.getStreamKey()).thenReturn("queue:audit:tasks");
        when(queueProperties.getBlockMs()).thenReturn(1000);
        when(listOperations.leftPop(eq("queue:audit:tasks"), eq(1000L), eq(TimeUnit.MILLISECONDS))).thenReturn(null);

        worker.poll();

        verify(auditEngineService, never()).start(any(AuditTaskEnvelope.class));
    }
}
