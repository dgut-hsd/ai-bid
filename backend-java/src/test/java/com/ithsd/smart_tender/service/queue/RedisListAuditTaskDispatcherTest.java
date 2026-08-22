package com.ithsd.smart_tender.service.queue;

import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.service.engine.queue.AuditQueueProperties;
import com.ithsd.smart_tender.service.engine.queue.RedisListAuditTaskDispatcher;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.ArgumentCaptor;
import org.mockito.InjectMocks;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;
import org.springframework.data.redis.core.ListOperations;
import org.springframework.data.redis.core.StringRedisTemplate;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.mockito.ArgumentMatchers.eq;
import static org.mockito.Mockito.*;

@ExtendWith(MockitoExtension.class)
class RedisListAuditTaskDispatcherTest {

    @Mock
    private StringRedisTemplate redisTemplate;

    @Mock
    private ListOperations<String, String> listOperations;

    @Mock
    private AuditQueueProperties queueProperties;

    @InjectMocks
    private RedisListAuditTaskDispatcher dispatcher;

    @AfterEach
    void tearDown() {
        TenantContext.clear();
    }

    @Test
    void dispatch_serializesTenantContext() {
        when(redisTemplate.opsForList()).thenReturn(listOperations);
        when(queueProperties.getStreamKey()).thenReturn("queue:audit:tasks");
        TenantContext.set(new TenantRequestContext(1001L, 2001L, "OWNER", 1L, "dispatch-test"));

        dispatcher.dispatch("task_123");

        ArgumentCaptor<String> payloadCaptor = ArgumentCaptor.forClass(String.class);
        verify(listOperations).rightPush(eq("queue:audit:tasks"), payloadCaptor.capture());
        assertThat(payloadCaptor.getValue())
                .contains("\"taskId\":\"task_123\"")
                .contains("\"tenantId\":\"2001\"")
                .contains("\"userId\":\"1001\"");
    }

    @Test
    void dispatch_refusesWithoutTenantContext() {
        when(queueProperties.getStreamKey()).thenReturn("queue:audit:tasks");

        assertThatThrownBy(() -> dispatcher.dispatch("task_123"))
                .isInstanceOf(IllegalArgumentException.class);

        verify(listOperations, never()).rightPush(anyString(), anyString());
    }
}
