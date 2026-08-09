package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.mapper.AuditTaskMapper;
import com.ithsd.smart_tender.mapper.TraceEventBlockMapper;
import com.ithsd.smart_tender.mapper.TraceEventMapper;
import com.ithsd.smart_tender.mapper.TraceSessionMapper;
import com.ithsd.smart_tender.model.entity.AuditTask;
import com.ithsd.smart_tender.model.entity.TraceSession;
import com.ithsd.smart_tender.model.vo.TraceSessionDetailVO;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.InjectMocks;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;

import java.util.List;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.*;

/**
 * 跨租户隔离测试 — TraceServiceImpl。
 *
 * <p>验证租户 A 的用户无法通过已知 taskId/sessionId 查看租户 B 的追溯会话。</p>
 */
@ExtendWith(MockitoExtension.class)
class TraceServiceTenantIsolationTest {

    private static final Long TENANT_A = 2001L;
    private static final Long TENANT_B = 2002L;
    private static final Long USER_A = 1001L;
    private static final String TASK_ID = "task_123";
    private static final String SESSION_ID = "session-uuid-abc";

    @Mock
    private TraceSessionMapper sessionMapper;
    @Mock
    private TraceEventMapper eventMapper;
    @Mock
    private TraceEventBlockMapper blockMapper;
    @Mock
    private AuditTaskMapper auditTaskMapper;

    @InjectMocks
    private TraceServiceImpl service;

    @AfterEach
    void tearDown() {
        TenantContext.clear();
    }

    private void givenUserInTenantA() {
        TenantContext.set(new TenantRequestContext(USER_A, TENANT_A, "OWNER", 1L, "trace-test-a"));
    }

    // ── listByTaskId ───────────────────────────────────────────

    @Test
    void listByTaskId_shouldReturnEmptyForCrossTenantTask() {
        givenUserInTenantA();

        // 租户 A 的审核任务中不存在 taskId → 返回空
        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        var result = service.listByTaskId(TASK_ID, null, null, 1, 20);

        assertThat(result.getTotal()).isEqualTo(0L);
        verify(auditTaskMapper).selectOne(any(LambdaQueryWrapper.class));
        // 不应查询 trace session
        verify(sessionMapper, never()).selectPage(any(), any(LambdaQueryWrapper.class));
    }

    @Test
    void listByTaskId_shouldReturnSessionsForSameTenantTask() {
        givenUserInTenantA();

        AuditTask task = AuditTask.builder()
                .id(8001L).tenantId(TENANT_A).taskId(TASK_ID).build();
        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(task);

        @SuppressWarnings("unchecked")
        var mockPage = mock(com.baomidou.mybatisplus.extension.plugins.pagination.Page.class);
        when(mockPage.getRecords()).thenReturn(List.of());
        when(mockPage.getTotal()).thenReturn(0L);
        when(sessionMapper.selectPage(any(), any(LambdaQueryWrapper.class))).thenReturn(mockPage);

        var result = service.listByTaskId(TASK_ID, null, null, 1, 20);

        assertThat(result).isNotNull();
        verify(sessionMapper).selectPage(any(), any(LambdaQueryWrapper.class));
    }

    // ── getSessionDetail ───────────────────────────────────────

    @Test
    void getSessionDetail_shouldReturnNullForCrossTenantSession() {
        givenUserInTenantA();

        // Session 存在但关联的 task 属于其他租户
        TraceSession session = new TraceSession();
        session.setId(SESSION_ID);
        session.setTaskId(TASK_ID);
        when(sessionMapper.selectById(SESSION_ID)).thenReturn(session);

        // 租户 A 查不到该 task
        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        TraceSessionDetailVO result = service.getSessionDetail(SESSION_ID);

        assertThat(result).isNull();
        verify(sessionMapper).selectById(SESSION_ID);
        verify(auditTaskMapper).selectOne(any(LambdaQueryWrapper.class));
        // 不应查询事件
        verify(eventMapper, never()).selectList(any(LambdaQueryWrapper.class));
    }

    @Test
    void getSessionDetail_shouldReturnDetailForSameTenantSession() {
        givenUserInTenantA();

        TraceSession session = new TraceSession();
        session.setId(SESSION_ID);
        session.setTaskId(TASK_ID);
        when(sessionMapper.selectById(SESSION_ID)).thenReturn(session);

        AuditTask task = AuditTask.builder()
                .id(8001L).tenantId(TENANT_A).taskId(TASK_ID).build();
        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(task);

        when(eventMapper.selectList(any(LambdaQueryWrapper.class))).thenReturn(List.of());

        TraceSessionDetailVO result = service.getSessionDetail(SESSION_ID);

        assertThat(result).isNotNull();
        verify(eventMapper).selectList(any(LambdaQueryWrapper.class));
    }

    // ── TenantContext 缺失 ─────────────────────────────────────

    @Test
    void listByTaskId_shouldThrowWhenNoTenantContext() {
        assertThatThrownBy(() -> service.listByTaskId(TASK_ID, null, null, 1, 20))
                .isInstanceOf(TenantAuthException.class)
                .matches(ex -> ((TenantAuthException) ex).getErrorCode().equals("TENANT_REQUIRED"));
    }

    @Test
    void getSessionDetail_shouldThrowWhenNoTenantContext() {
        // 需要 mock session 存在，否则 selectById 返回 null 就直接返回了
        TraceSession session = new TraceSession();
        session.setId(SESSION_ID);
        session.setTaskId(TASK_ID);
        when(sessionMapper.selectById(SESSION_ID)).thenReturn(session);

        assertThatThrownBy(() -> service.getSessionDetail(SESSION_ID))
                .isInstanceOf(TenantAuthException.class)
                .matches(ex -> ((TenantAuthException) ex).getErrorCode().equals("TENANT_REQUIRED"));
    }
}
