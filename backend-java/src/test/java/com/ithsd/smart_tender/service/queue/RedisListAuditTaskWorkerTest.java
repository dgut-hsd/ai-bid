package com.ithsd.smart_tender.service.queue;

import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantContextSnapshot;
import com.ithsd.smart_tender.service.AuditEngineService;
import com.ithsd.smart_tender.service.engine.queue.AuditQueueProperties;
import com.ithsd.smart_tender.service.engine.queue.QueuedAuditTask;
import com.ithsd.smart_tender.service.engine.queue.RedisListAuditTaskWorker;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.InjectMocks;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;
import org.springframework.data.redis.core.ListOperations;
import org.springframework.data.redis.core.StringRedisTemplate;

import java.util.concurrent.TimeUnit;

import static org.assertj.core.api.Assertions.assertThat;
import static org.mockito.ArgumentMatchers.anyString;
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

    @AfterEach
    void tearDown() {
        TenantContext.clear();
    }

    @Test
    void poll_TaskAvailable_rebuildsTenantContext() {
        when(redisTemplate.opsForList()).thenReturn(listOperations);
        when(queueProperties.getStreamKey()).thenReturn("queue:audit:tasks");
        when(queueProperties.getBlockMs()).thenReturn(1000);
        String payload = QueuedAuditTask.encode("task_123",
                new TenantContextSnapshot(1001L, 2001L, "OWNER", 1L, "worker-test"));
        when(listOperations.leftPop(eq("queue:audit:tasks"), eq(1000L), eq(TimeUnit.MILLISECONDS)))
                .thenReturn(payload);

        doAnswer(inv -> {
            assertThat(TenantContext.get()).isNotNull();
            assertThat(TenantContext.get().tenantId()).isEqualTo(2001L);
            assertThat(TenantContext.get().userId()).isEqualTo(1001L);
            return null;
        }).when(auditEngineService).start("task_123");

        worker.poll();

        verify(auditEngineService).start("task_123");
        // worker 用后即清，避免污染 @Scheduled 线程
        assertThat(TenantContext.get()).isNull();
    }

    @Test
    void poll_NoTask() {
        when(redisTemplate.opsForList()).thenReturn(listOperations);
        when(queueProperties.getStreamKey()).thenReturn("queue:audit:tasks");
        when(queueProperties.getBlockMs()).thenReturn(1000);
        when(listOperations.leftPop(eq("queue:audit:tasks"), eq(1000L), eq(TimeUnit.MILLISECONDS))).thenReturn(null);

        worker.poll();

        verify(auditEngineService, never()).start(anyString());
    }
}
