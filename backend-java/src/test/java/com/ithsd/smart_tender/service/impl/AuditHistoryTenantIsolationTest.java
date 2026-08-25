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
import com.ithsd.smart_tender.model.entity.AuditTask;
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
import static org.mockito.Mockito.never;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

@ExtendWith(MockitoExtension.class)
class AuditHistoryTenantIsolationTest {

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
    private AuditHistoryServiceImpl historyService;

    @BeforeEach
    void setUp() {
        TenantContext.set(new TenantRequestContext(10001L, TENANT_A, "ADMIN", 1L, "history-test"));
    }

    @AfterEach
    void tearDown() {
        TenantContext.clear();
    }

    @Test
    void detail_hidesCrossTenantTaskWithTenantScopedParentLookup() {
        assertThatThrownBy(() -> historyService.getDetailById(77L))
                .isInstanceOf(TenantAuthException.class)
                .satisfies(error -> {
                    TenantAuthException tenantError = (TenantAuthException) error;
                    assertThat(tenantError.getStatus()).isEqualTo(404);
                    assertThat(tenantError.getErrorCode()).isEqualTo("RESOURCE_NOT_FOUND");
                });

        ArgumentCaptor<QueryWrapper<AuditTask>> taskQuery =
                ArgumentCaptor.forClass(QueryWrapper.class);
        verify(auditTaskMapper).selectOne(taskQuery.capture());
        assertThat(taskQuery.getValue().getSqlSegment()).contains("tenant_id");
        assertThat(taskQuery.getValue().getParamNameValuePairs()).containsValue(TENANT_A);
        verify(auditTaskMapper, never()).selectById(77L);
    }
}
