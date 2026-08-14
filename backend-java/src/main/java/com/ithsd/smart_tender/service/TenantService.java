package com.ithsd.smart_tender.service;

import com.ithsd.smart_tender.model.dto.CreateTenantRequest;
import com.ithsd.smart_tender.model.vo.TenantListVO;
import com.ithsd.smart_tender.model.vo.TenantMemberPageVO;
import com.ithsd.smart_tender.model.vo.TenantSummaryVO;
import com.ithsd.smart_tender.model.vo.TenantVO;

public interface TenantService {

    TenantListVO listTenants();

    TenantVO createTenant(CreateTenantRequest request);

    TenantSummaryVO currentTenant();

    TenantMemberPageVO listMembers(Long tenantId, int page, int size);
}
