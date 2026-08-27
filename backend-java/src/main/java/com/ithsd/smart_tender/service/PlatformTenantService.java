package com.ithsd.smart_tender.service;

import com.ithsd.smart_tender.model.dto.CreatePlatformTenantRequest;
import com.ithsd.smart_tender.model.vo.EnterpriseUserVO;
import com.ithsd.smart_tender.model.vo.PlatformTenantPageVO;
import com.ithsd.smart_tender.model.vo.PlatformTenantVO;

import java.util.List;

/** 平台管理员（系统管理者）对企业（租户）的管理能力，不绑定当前租户。 */
public interface PlatformTenantService {

    PlatformTenantPageVO listTenants(int page, int size, String keyword, String status);

    PlatformTenantVO createTenant(CreatePlatformTenantRequest request);

    PlatformTenantVO getTenant(Long tenantId);

    /** 查看某个企业（租户）的成员列表，供平台管理员转移 OWNER 前选择目标用户。 */
    List<EnterpriseUserVO> listTenantMembers(Long tenantId);

    PlatformTenantVO transferOwner(Long tenantId, Long targetUserId);

    PlatformTenantVO disableTenant(Long tenantId);

    PlatformTenantVO restoreTenant(Long tenantId);

    PlatformTenantVO deleteTenant(Long tenantId);
}