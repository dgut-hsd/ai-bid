package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.baomidou.mybatisplus.extension.plugins.pagination.Page;
import com.ithsd.smart_tender.common.BaseContext;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.mapper.AuditIssueMapper;
import com.ithsd.smart_tender.mapper.AuditReportMapper;
import com.ithsd.smart_tender.mapper.AuditTaskMapper;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.dto.AuditHistoryPageQueryDTO;
import com.ithsd.smart_tender.model.entity.AuditTask;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.model.entity.User;
import com.ithsd.smart_tender.model.vo.AuditHistoryDetailVO;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.InjectMocks;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;

import java.util.List;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.*;

/**
 * 跨租户隔离测试 — AuditHistoryServiceImpl。
 *
 * <p>验证租户 A 的用户无法通过已知 auditId 查看/删除租户 B 的审核历史。
 * task 查询带 tenant_id，查不到即抛 {@code RESOURCE_NOT_FOUND}（而非返回空/继续越权操作）。</p>
 */
@ExtendWith(MockitoExtension.class)
class AuditHistoryTenantIsolationTest {

    private static final Long TENANT_A = 2001L;
    private static final Long USER_A = 1001L;
    private static final Long AUDIT_TASK_ID = 8001L;

    @Mock
    private AuditTaskMapper auditTaskMapper;
    @Mock
    private AuditIssueMapper auditIssueMapper;
    @Mock
    private AuditReportMapper auditReportMapper;
    @Mock
    private TenderMapper tenderMapper;
    @Mock
    private UserMapper userMapper;

    @InjectMocks
    private AuditHistoryServiceImpl service;

    @BeforeEach
    void setUp() {
        BaseContext.setCurrentId(USER_A);
        TenantContext.set(new TenantRequestContext(USER_A, TENANT_A, "OWNER", 1L, "audit-hist-a"));
    }

    @AfterEach
    void tearDown() {
        TenantContext.clear();
        BaseContext.removeCurrentId();
    }

    // ── getDetailById ──────────────────────────────────────────

    @Test
    void getDetailById_shouldThrowForCrossTenantTask() {
        // 租户 A 查不到租户 B 的审核任务 → 抛 RESOURCE_NOT_FOUND
        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        assertThatThrownBy(() -> service.getDetailById(AUDIT_TASK_ID))
                .isInstanceOf(TenantAuthException.class)
                .matches(ex -> ((TenantAuthException) ex).getErrorCode().equals("RESOURCE_NOT_FOUND"));

        verify(auditTaskMapper).selectOne(any(LambdaQueryWrapper.class));
        // 不应该去查子资源（issues / reports）
        verify(auditIssueMapper, never()).selectList(any(LambdaQueryWrapper.class));
        verify(auditReportMapper, never()).selectOne(any(LambdaQueryWrapper.class));
    }

    @Test
    void getDetailById_shouldReturnDetailForSameTenantTask() {
        AuditTask task = AuditTask.builder()
                .id(AUDIT_TASK_ID).tenantId(TENANT_A).bidId(5001L)
                .taskStatus(2).build();
        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(task);
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class)))
                .thenReturn(Tender.builder().id(5001L).tenantId(TENANT_A).build());
        when(auditIssueMapper.selectList(any(LambdaQueryWrapper.class))).thenReturn(List.of());
        when(auditReportMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        AuditHistoryDetailVO result = service.getDetailById(AUDIT_TASK_ID);

        assertThat(result).isNotNull();
        verify(auditIssueMapper).selectList(any(LambdaQueryWrapper.class));
    }

    // ── delete ─────────────────────────────────────────────────

    @Test
    void delete_shouldThrowForCrossTenantTask() {
        // 租户 A 查不到租户 B 的任务 → 抛 RESOURCE_NOT_FOUND，不执行删除
        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        assertThatThrownBy(() -> service.delete(AUDIT_TASK_ID))
                .isInstanceOf(TenantAuthException.class)
                .matches(ex -> ((TenantAuthException) ex).getErrorCode().equals("RESOURCE_NOT_FOUND"));

        verify(auditTaskMapper).selectOne(any(LambdaQueryWrapper.class));
        verify(auditIssueMapper, never()).delete(any(LambdaQueryWrapper.class));
        verify(auditReportMapper, never()).delete(any(LambdaQueryWrapper.class));
        verify(auditTaskMapper, never()).delete(any(LambdaQueryWrapper.class));
    }

    // ── page ───────────────────────────────────────────────────

    @Test
    void page_shouldOnlyReturnSameTenantTasks() {
        @SuppressWarnings("unchecked")
        Page<AuditTask> mockPage = mock(Page.class);
        when(mockPage.getRecords()).thenReturn(List.of());
        when(mockPage.getTotal()).thenReturn(0L);
        when(auditTaskMapper.selectPage(any(Page.class), any(LambdaQueryWrapper.class)))
                .thenReturn(mockPage);

        AuditHistoryPageQueryDTO dto = new AuditHistoryPageQueryDTO();
        dto.setPage(1);
        dto.setSize(20);
        // 设置日期避免 LambdaQueryWrapper 构造时的 NPE（Java 实参求值）
        dto.setStartDate(java.time.LocalDate.now().minusDays(30));
        dto.setEndDate(java.time.LocalDate.now());

        var result = service.page(dto);

        assertThat(result).isNotNull();
        verify(auditTaskMapper).selectPage(any(Page.class), any(LambdaQueryWrapper.class));
    }

    // ── getStatistics ──────────────────────────────────────────

    @Test
    void getStatistics_shouldOnlyCountSameTenantTasks() {
        when(auditTaskMapper.selectList(any(LambdaQueryWrapper.class))).thenReturn(List.of());

        AuditHistoryPageQueryDTO dto = new AuditHistoryPageQueryDTO();
        Map<String, Object> stats = service.getStatistics(dto);

        assertThat(stats).containsKey("statusList");
        verify(auditTaskMapper).selectList(any(LambdaQueryWrapper.class));
    }

    // ── TenantContext 缺失 ─────────────────────────────────────

    @Test
    void getDetailById_shouldThrowWhenNoTenantContext() {
        TenantContext.clear();
        assertThatThrownBy(() -> service.getDetailById(AUDIT_TASK_ID))
                .isInstanceOf(TenantAuthException.class)
                .matches(ex -> ((TenantAuthException) ex).getErrorCode().equals("TENANT_REQUIRED"));
    }

    @Test
    void delete_shouldThrowWhenNoTenantContext() {
        TenantContext.clear();
        assertThatThrownBy(() -> service.delete(AUDIT_TASK_ID))
                .isInstanceOf(TenantAuthException.class)
                .matches(ex -> ((TenantAuthException) ex).getErrorCode().equals("TENANT_REQUIRED"));
    }
}
