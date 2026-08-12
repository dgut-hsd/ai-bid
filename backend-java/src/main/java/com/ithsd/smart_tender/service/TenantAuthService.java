package com.ithsd.smart_tender.service;

import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.model.dto.TenantSwitchDTO;
import com.ithsd.smart_tender.model.dto.UserLoginDTO;
import com.ithsd.smart_tender.model.vo.UserLoginVO;

public interface TenantAuthService {
    UserLoginVO login(UserLoginDTO request);

    UserLoginVO refresh(String authorization, String requestId);

    UserLoginVO switchTenant(String authorization, TenantSwitchDTO request, String requestId);

    void logout(String authorization, String requestId);

    TenantRequestContext authenticate(String authorization, String requestId);
}
