package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.common.BaseContext;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.mapper.AuditIssueMapper;
import com.ithsd.smart_tender.mapper.AuditReportMapper;
import com.ithsd.smart_tender.mapper.AuditTaskMapper;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.entity.AuditTask;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.model.vo.ReportVO;
import com.ithsd.smart_tender.tenant.fixture.TenantQueryAssertions;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.ArgumentCaptor;
import org.mockito.InjectMocks;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.*;

/**
 * 跨租户隔离测试 — ReportServiceImpl。
 *
 * <p>验证租户 A 的用户无法生成/读取租户 B 的审核报告。</p>
 */
@ExtendWith(MockitoExtension.class)
class ReportServiceTenantIsolationTest {

    private static final Long TENANT_A = 2001L;
    private static final Long TENANT_B = 2002L;
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
    private ReportServiceImpl service;

    @BeforeEach
    void setUp() {
        BaseContext.setCurrentId(USER_A);
    }

    @AfterEach
    void tearDown() {
        TenantContext.clear();
        BaseContext.removeCurrentId();
    }

    private void givenUserInTenantA() {
        TenantContext.set(new TenantRequestContext(USER_A, TENANT_A, "OWNER", 1L, "report-test-a"));
    }

    // ── generateReport ─────────────────────────────────────────

    @Test
    void generateReport_shouldRejectCrossTenantAuditId() {
        givenUserInTenantA();

        // resolveAuditId 中的三次查询都返回 null（租户 A 查不到租户 B 的数据）
        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        assertThatThrownBy(() -> service.generateReport(String.valueOf(AUDIT_TASK_ID)))
                .isInstanceOf(RuntimeException.class)
                .hasMessageContaining("审核任务不存在");

        // resolveAuditId 尝试了 bidId 匹配和 auditId 匹配，都返回 null
        ArgumentCaptor<LambdaQueryWrapper<AuditTask>> captor = ArgumentCaptor.forClass(LambdaQueryWrapper.class);
        verify(auditTaskMapper, atLeast(1)).selectOne(captor.capture());
        captor.getAllValues().forEach(w -> TenantQueryAssertions.assertTenantScoped(w, TENANT_A));
        // 不应生成报告或查询 issues
        verify(auditIssueMapper, never()).selectList(any(LambdaQueryWrapper.class));
        verify(auditReportMapper, never()).insert(any());
    }

    @Test
    void generateReport_shouldAllowSameTenantAuditId() {
        givenUserInTenantA();

        AuditTask task = AuditTask.builder()
                .id(AUDIT_TASK_ID).tenantId(TENANT_A).bidId(5001L)
                .taskStatus(2).build();

        // resolveAuditId: by audit_task.id 匹配（第二个 selectOne 调用）
        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class)))
                .thenReturn(null)   // 第一次：bidId 匹配失败
                .thenReturn(task);  // 第二次：auditId 匹配成功
        // generateReport 内部的 selectOne 验证
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class)))
                .thenReturn(Tender.builder().id(5001L).tenantId(TENANT_A)
                        .bidName("测试项目").build());
        when(auditIssueMapper.selectList(any(LambdaQueryWrapper.class))).thenReturn(java.util.List.of());
        when(auditReportMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        ReportVO report = service.generateReport(String.valueOf(AUDIT_TASK_ID));

        assertThat(report).isNotNull();
        verify(auditReportMapper).insert(any());
    }

    // ── getReportContent ───────────────────────────────────────

    @Test
    void getReportContent_shouldThrowForCrossTenantAuditId() {
        givenUserInTenantA();

        // resolveAuditId 尝试 bidId 和 auditId 查找，均为其他租户 → 抛异常
        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        assertThatThrownBy(() -> service.getReportContent(String.valueOf(AUDIT_TASK_ID)))
                .isInstanceOf(RuntimeException.class)
                .hasMessageContaining("审核任务不存在");

        // resolveAuditId 尝试了 bidId 匹配和 auditId 匹配，都返回 null
        ArgumentCaptor<LambdaQueryWrapper<AuditTask>> captor = ArgumentCaptor.forClass(LambdaQueryWrapper.class);
        verify(auditTaskMapper, atLeast(1)).selectOne(captor.capture());
        captor.getAllValues().forEach(w -> TenantQueryAssertions.assertTenantScoped(w, TENANT_A));
        verify(auditReportMapper, never()).selectOne(any(LambdaQueryWrapper.class));
    }

    @Test
    void getReportContent_shouldReturnContentForSameTenantAuditId() {
        givenUserInTenantA();

        AuditTask task = AuditTask.builder()
                .id(AUDIT_TASK_ID).tenantId(TENANT_A).bidId(5001L)
                .taskStatus(2).build();

        // resolveAuditId → 两次查询
        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class)))
                .thenReturn(null)   // bidId 匹配
                .thenReturn(task)   // auditId 匹配
                .thenReturn(task);  // getReportContent 内部再次验证

        var report = com.ithsd.smart_tender.model.entity.AuditReport.builder()
                .auditId(AUDIT_TASK_ID).docContent("# 报告内容").build();
        when(auditReportMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(report);

        String content = service.getReportContent(String.valueOf(AUDIT_TASK_ID));

        assertThat(content).isEqualTo("# 报告内容");
    }

    // ── TenantContext 缺失 ─────────────────────────────────────

    @Test
    void generateReport_shouldThrowWhenNoTenantContext() {
        assertThatThrownBy(() -> service.generateReport(String.valueOf(AUDIT_TASK_ID)))
                .isInstanceOf(TenantAuthException.class)
                .matches(ex -> ((TenantAuthException) ex).getErrorCode().equals("TENANT_REQUIRED"));
    }

    @Test
    void getReportContent_shouldThrowWhenNoTenantContext() {
        assertThatThrownBy(() -> service.getReportContent(String.valueOf(AUDIT_TASK_ID)))
                .isInstanceOf(TenantAuthException.class)
                .matches(ex -> ((TenantAuthException) ex).getErrorCode().equals("TENANT_REQUIRED"));
    }
}
