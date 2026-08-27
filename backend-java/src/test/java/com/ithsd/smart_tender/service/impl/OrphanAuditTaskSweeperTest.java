package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.mapper.AuditTaskMapper;
import com.ithsd.smart_tender.model.entity.AuditTask;
import com.ithsd.smart_tender.model.enums.AuditTaskStatusEnum;
import com.ithsd.smart_tender.service.AuditEngineService;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.ArgumentCaptor;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;

import java.time.LocalDateTime;
import java.util.List;
import java.util.concurrent.atomic.AtomicReference;

import static org.junit.jupiter.api.Assertions.*;
import static org.mockito.ArgumentMatchers.*;
import static org.mockito.Mockito.*;

/**
 * F3：孤儿任务守护（TDD 先行）。
 */
@ExtendWith(MockitoExtension.class)
class OrphanAuditTaskSweeperTest {

    @Mock
    private AuditTaskMapper auditTaskMapper;

    @Mock
    private AuditEngineService auditEngineService;

    @Mock
    private RunningTaskRegistry runningTaskRegistry;

    private OrphanAuditTaskSweeper sweeper;

    private static final long STALE_AFTER_MS = 180_000;

    @BeforeEach
    void setUp() {
        // 单测无 Spring 上下文，需手动初始化 MyBatis-Plus 表元数据，
        // 否则 LambdaQueryWrapper 的列名解析会抛"can not find lambda cache"。
        try {
            com.baomidou.mybatisplus.core.MybatisConfiguration config =
                    new com.baomidou.mybatisplus.core.MybatisConfiguration();
            com.baomidou.mybatisplus.core.metadata.TableInfoHelper.initTableInfo(
                    new org.apache.ibatis.builder.MapperBuilderAssistant(config, "audit_task"),
                    AuditTask.class);
        } catch (RuntimeException ignored) {
            // 已初始化（幂等）
        }
        sweeper = new OrphanAuditTaskSweeper(
                auditTaskMapper, auditEngineService, runningTaskRegistry,
                true, STALE_AFTER_MS);
    }

    @AfterEach
    void tearDown() {
        TenantContext.clear();
    }

    private AuditTask task(String taskId, Integer status, LocalDateTime updatedAt) {
        return AuditTask.builder()
                .id(1L)
                .taskId(taskId)
                .tenantId(1L)
                .bidId(4L)
                .taskStatus(status)
                .auditUserId(1L)
                .updatedAt(updatedAt)
                .build();
    }

    @Test
    void stateLostTask_getsMarkedFailed() {
        AuditTask stale = task("task-stale", AuditTaskStatusEnum.PROCESSING.getCode(),
                LocalDateTime.now().minusSeconds(600));
        when(auditTaskMapper.selectList(any())).thenReturn(List.of(stale));
        when(runningTaskRegistry.contains("1:task-stale")).thenReturn(false);
        when(auditEngineService.recover("task-stale"))
                .thenReturn(AuditEngineService.RecoverOutcome.STATE_LOST);

        sweeper.sweep();

        verify(auditEngineService).failOrphan("task-stale");
    }

    @Test
    void settledTask_notMarkedFailed() {
        AuditTask stale = task("task-settled", AuditTaskStatusEnum.PROCESSING.getCode(),
                LocalDateTime.now().minusSeconds(600));
        when(auditTaskMapper.selectList(any())).thenReturn(List.of(stale));
        when(runningTaskRegistry.contains("1:task-settled")).thenReturn(false);
        when(auditEngineService.recover("task-settled"))
                .thenReturn(AuditEngineService.RecoverOutcome.COMPLETED);

        sweeper.sweep();

        verify(auditEngineService, never()).failOrphan(any());
    }

    @Test
    void stillRunningTask_isLeftAlone() {
        AuditTask stale = task("task-running", AuditTaskStatusEnum.PROCESSING.getCode(),
                LocalDateTime.now().minusSeconds(600));
        when(auditTaskMapper.selectList(any())).thenReturn(List.of(stale));
        when(runningTaskRegistry.contains("1:task-running")).thenReturn(false);
        when(auditEngineService.recover("task-running"))
                .thenReturn(AuditEngineService.RecoverOutcome.STILL_RUNNING);

        sweeper.sweep();

        verify(auditEngineService, never()).failOrphan(any());
    }

    @Test
    void unreachableTask_isLeftAloneForNextSweep() {
        AuditTask stale = task("task-unreachable", AuditTaskStatusEnum.PROCESSING.getCode(),
                LocalDateTime.now().minusSeconds(600));
        when(auditTaskMapper.selectList(any())).thenReturn(List.of(stale));
        when(runningTaskRegistry.contains("1:task-unreachable")).thenReturn(false);
        when(auditEngineService.recover("task-unreachable"))
                .thenReturn(AuditEngineService.RecoverOutcome.UNREACHABLE);

        sweeper.sweep();

        verify(auditEngineService, never()).failOrphan(any());
    }

    @Test
    void taskWithLiveThread_isSkipped() {
        AuditTask stale = task("task-alive", AuditTaskStatusEnum.PROCESSING.getCode(),
                LocalDateTime.now().minusSeconds(600));
        when(auditTaskMapper.selectList(any())).thenReturn(List.of(stale));
        when(runningTaskRegistry.contains("1:task-alive")).thenReturn(true);

        sweeper.sweep();

        verify(auditEngineService, never()).recover(any());
        verify(auditEngineService, never()).failOrphan(any());
    }

    @Test
    void freshTask_isNotSwept() {
        // mapper 的查询过滤由 MyBatis-Plus wrapper 完成；守护不额外判断新鲜度，
        // 这里验证 wrapper 确实带上了「状态 IN (0,1)」与「updated_at < cutoff」条件。
        when(auditTaskMapper.selectList(any())).thenReturn(List.of());

        sweeper.sweep();

        ArgumentCaptor<LambdaQueryWrapper<AuditTask>> captor =
                ArgumentCaptor.forClass(LambdaQueryWrapper.class);
        verify(auditTaskMapper).selectList(captor.capture());
        String sqlSegment = captor.getValue().getSqlSegment();
        assertTrue(sqlSegment.contains("task_status"),
                "查询应包含任务状态过滤: " + sqlSegment);
        assertTrue(sqlSegment.contains("updated_at"),
                "查询应包含 updated_at 陈旧过滤: " + sqlSegment);
    }

    @Test
    void disabledSweeper_doesNothing() {
        sweeper = new OrphanAuditTaskSweeper(
                auditTaskMapper, auditEngineService, runningTaskRegistry,
                false, STALE_AFTER_MS);

        sweeper.sweep();

        verifyNoInteractions(auditTaskMapper);
    }

    @Test
    void tenantContextIsSetDuringRecover() {
        AuditTask stale = task("task-tenant", AuditTaskStatusEnum.PROCESSING.getCode(),
                LocalDateTime.now().minusSeconds(600));
        when(auditTaskMapper.selectList(any())).thenReturn(List.of(stale));
        when(runningTaskRegistry.contains("1:task-tenant")).thenReturn(false);

        AtomicReference<Long> tenantSeen = new AtomicReference<>();
        when(auditEngineService.recover("task-tenant")).thenAnswer(inv -> {
            tenantSeen.set(TenantContext.get() == null ? null : TenantContext.get().tenantId());
            return AuditEngineService.RecoverOutcome.STILL_RUNNING;
        });

        sweeper.sweep();

        assertEquals(1L, tenantSeen.get(), "recover 执行时必须有租户上下文");
        assertNull(TenantContext.get(), "守护结束后必须清理租户上下文");
    }

    @Test
    void pendingStaleTask_isMarkedFailedDirectly() {
        AuditTask stalePending = task("task-pending", AuditTaskStatusEnum.PENDING.getCode(),
                LocalDateTime.now().minusSeconds(600));
        when(auditTaskMapper.selectList(any())).thenReturn(List.of(stalePending));
        when(runningTaskRegistry.contains("1:task-pending")).thenReturn(false);

        sweeper.sweep();

        verify(auditEngineService, never()).recover(any());
        verify(auditEngineService).failOrphan("task-pending");
    }
}
