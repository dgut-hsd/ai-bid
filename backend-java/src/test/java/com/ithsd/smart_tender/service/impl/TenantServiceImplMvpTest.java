package com.ithsd.smart_tender.service.impl;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.mapper.TenantMapper;
import com.ithsd.smart_tender.mapper.TenantMemberMapper;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.dto.CreateTenantRequest;
import com.ithsd.smart_tender.model.entity.Tenant;
import com.ithsd.smart_tender.model.entity.TenantMember;
import com.ithsd.smart_tender.model.vo.TenantVO;
import com.ithsd.smart_tender.service.TenantAuthorizationService;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.ArgumentCaptor;
import org.mockito.InjectMocks;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.doAnswer;
import static org.mockito.Mockito.doThrow;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

@ExtendWith(MockitoExtension.class)
class TenantServiceImplMvpTest {

    @Mock
    private TenantMapper tenantMapper;
    @Mock
    private TenantMemberMapper tenantMemberMapper;
    @Mock
    private UserMapper userMapper;
    @Mock
    private TenantAuthorizationService authorization;
    @Mock
    private ObjectMapper objectMapper;

    @InjectMocks
    private TenantServiceImpl tenantService;

    @Test
    void createTenantUsesContextUserAndInsertsOwnerAtomically() {
        TenantRequestContext context = new TenantRequestContext(1001L, null, null, 1L, "request-1");
        when(authorization.requireAuthenticated()).thenReturn(context);
        doAnswer(invocation -> {
            Tenant tenant = invocation.getArgument(0);
            tenant.setId(2001L);
            return 1;
        }).when(tenantMapper).insert(any(Tenant.class));
        doAnswer(invocation -> {
            TenantMember member = invocation.getArgument(0);
            member.setId(3001L);
            return 1;
        }).when(tenantMemberMapper).insert(any(TenantMember.class));

        CreateTenantRequest request = new CreateTenantRequest();
        request.setName("Acme");
        request.setTenantCode("acme-bid");
        request.setTenantId(9999L);

        TenantVO result = tenantService.createTenant(request);

        assertThat(result.getTenantId()).isEqualTo(2001L);
        assertThat(result.getOwnerUserId()).isEqualTo(1001L);
        assertThat(result.getRole()).isEqualTo("OWNER");

        ArgumentCaptor<Tenant> tenantCaptor = ArgumentCaptor.forClass(Tenant.class);
        verify(tenantMapper).insert(tenantCaptor.capture());
        assertThat(tenantCaptor.getValue().getOwnerUserId()).isEqualTo(1001L);
        assertThat(tenantCaptor.getValue().getTenantCode()).isEqualTo("acme-bid");

        ArgumentCaptor<TenantMember> memberCaptor = ArgumentCaptor.forClass(TenantMember.class);
        verify(tenantMemberMapper).insert(memberCaptor.capture());
        assertThat(memberCaptor.getValue().getTenantId()).isEqualTo(2001L);
        assertThat(memberCaptor.getValue().getUserId()).isEqualTo(1001L);
        assertThat(memberCaptor.getValue().getRole()).isEqualTo("OWNER");
        assertThat(memberCaptor.getValue().getStatus()).isEqualTo("ACTIVE");
    }

    @Test
    void ownerInsertFailurePropagatesSoTransactionRollsBack() {
        TenantRequestContext context = new TenantRequestContext(1001L, null, null, 1L, "request-2");
        when(authorization.requireAuthenticated()).thenReturn(context);
        doAnswer(invocation -> {
            Tenant tenant = invocation.getArgument(0);
            tenant.setId(2001L);
            return 1;
        }).when(tenantMapper).insert(any(Tenant.class));
        doThrow(new IllegalStateException("owner insert failed"))
                .when(tenantMemberMapper).insert(any(TenantMember.class));

        CreateTenantRequest request = new CreateTenantRequest();
        request.setName("Acme");

        assertThatThrownBy(() -> tenantService.createTenant(request))
                .isInstanceOf(IllegalStateException.class)
                .hasMessage("owner insert failed");
        verify(tenantMapper).insert(any(Tenant.class));
        verify(tenantMemberMapper).insert(any(TenantMember.class));
    }
}
