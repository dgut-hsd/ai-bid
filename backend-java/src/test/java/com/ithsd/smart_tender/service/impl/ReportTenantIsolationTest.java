package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.QueryWrapper;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.mapper.AuditIssueMapper;
import com.ithsd.smart_tender.mapper.AuditReportMapper;
import com.ithsd.smart_tender.mapper.AuditTaskMapper;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.entity.AuditReport;
import com.ithsd.smart_tender.model.entity.AuditTask;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.model.vo.ReportVO;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.ArgumentCaptor;
import org.mockito.InjectMocks;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;
import org.springframework.util.ReflectionUtils;

import java.lang.reflect.Field;
import java.util.List;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.atLeastOnce;
import static org.mockito.Mockito.doAnswer;
import static org.mockito.Mockito.lenient;
import static org.mockito.Mockito.never;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

@ExtendWith(MockitoExtension.class)
class ReportTenantIsolationTest {

    private static final Long TENANT_A = 20001L;

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
    private ReportServiceImpl reportService;

    @BeforeEach
    void setUp() {
        TenantContext.set(new TenantRequestContext(10001L, TENANT_A, "ADMIN", 1L, "report-test"));
    }

    @AfterEach
    void tearDown() {
        TenantContext.clear();
    }

    @Test
    void generateReport_scopesEveryResourceAndPersistsCurrentTenant() {
        AuditTask task = AuditTask.builder()
                .id(77L)
                .taskId("task-a")
                .bidId(88L)
                .tenantId(TENANT_A)
                .taskStatus(2)
                .build();
        Tender tender = Tender.builder()
                .id(88L)
                .projectId(99L)
                .tenantId(TENANT_A)
                .bidName("Tenant A tender")
                .build();

        lenient().when(auditTaskMapper.selectOne(any())).thenReturn(task);
        lenient().when(tenderMapper.selectOne(any())).thenReturn(tender);
        lenient().when(auditReportMapper.selectOne(any())).thenReturn(null);
        lenient().when(auditIssueMapper.selectList(any())).thenReturn(List.of());
        doAnswer(invocation -> 1).when(auditReportMapper).insert(any(AuditReport.class));

        ReportVO result = reportService.generateReport("task-a");

        assertThat(result).isNotNull();
        ArgumentCaptor<AuditReport> report = ArgumentCaptor.forClass(AuditReport.class);
        verify(auditReportMapper).insert(report.capture());
        assertThat(tenantIdOf(report.getValue())).isEqualTo(TENANT_A);

        ArgumentCaptor<QueryWrapper<AuditTask>> taskQueries =
                ArgumentCaptor.forClass(QueryWrapper.class);
        verify(auditTaskMapper, atLeastOnce()).selectOne(taskQueries.capture());
        assertThat(taskQueries.getAllValues())
                .allSatisfy(query -> assertTenantPredicate(query));

        ArgumentCaptor<QueryWrapper<AuditReport>> reportQuery =
                ArgumentCaptor.forClass(QueryWrapper.class);
        verify(auditReportMapper).selectOne(reportQuery.capture());
        assertTenantPredicate(reportQuery.getValue());
    }

    @Test
    void getReportContent_hidesCrossTenantTaskBeforeReadingReport() {
        when(auditTaskMapper.selectOne(any())).thenReturn(null);

        assertThatThrownBy(() -> reportService.getReportContent("task-from-b"))
                .isInstanceOf(TenantAuthException.class)
                .satisfies(error -> {
                    TenantAuthException tenantError = (TenantAuthException) error;
                    assertThat(tenantError.getStatus()).isEqualTo(404);
                    assertThat(tenantError.getErrorCode()).isEqualTo("RESOURCE_NOT_FOUND");
                });

        ArgumentCaptor<QueryWrapper<AuditTask>> taskQuery =
                ArgumentCaptor.forClass(QueryWrapper.class);
        verify(auditTaskMapper).selectOne(taskQuery.capture());
        assertTenantPredicate(taskQuery.getValue());
        verify(auditReportMapper, never()).selectOne(any());
    }

    private static void assertTenantPredicate(QueryWrapper<?> wrapper) {
        assertThat(wrapper.getSqlSegment()).contains("tenant_id");
        assertThat(wrapper.getParamNameValuePairs()).containsValue(TENANT_A);
    }

    private static Long tenantIdOf(Object entity) {
        Field field = ReflectionUtils.findField(entity.getClass(), "tenantId");
        if (field == null) {
            return null;
        }
        field.setAccessible(true);
        try {
            return (Long) field.get(entity);
        } catch (IllegalAccessException e) {
            throw new AssertionError(e);
        }
    }
}
