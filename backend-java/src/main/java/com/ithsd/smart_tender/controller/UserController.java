package com.ithsd.smart_tender.controller;

import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.model.dto.ChangePasswordRequest;
import com.ithsd.smart_tender.model.dto.TenantSwitchDTO;
import com.ithsd.smart_tender.model.dto.UserLoginDTO;
import com.ithsd.smart_tender.model.result.Result;
import com.ithsd.smart_tender.model.vo.UserLoginVO;
import com.ithsd.smart_tender.service.TenantAuthService;
import com.ithsd.smart_tender.service.UserService;
import jakarta.validation.Valid;
import lombok.RequiredArgsConstructor;
import org.springframework.util.StringUtils;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RequestHeader;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;

import java.util.UUID;

@RestController
@RequestMapping("/api/auth")
@RequiredArgsConstructor
public class UserController {

    private final TenantAuthService tenantAuthService;
    private final UserService userService;

    @PostMapping("/login")
    public Result<UserLoginVO> login(@RequestBody UserLoginDTO userLoginDTO) {
        return Result.success(tenantAuthService.login(userLoginDTO));
    }

    @PostMapping("/logout")
    public Result<Void> logout(
            @RequestHeader(value = "Authorization", required = false) String authorization,
            @RequestHeader(value = "X-Request-Id", required = false) String requestId
    ) {
        tenantAuthService.logout(authorization, resolveRequestId(requestId));
        return Result.success();
    }

    @PostMapping("/refresh")
    public Result<UserLoginVO> refresh(
            @RequestHeader(value = "Authorization", required = false) String authorization,
            @RequestHeader(value = "X-Request-Id", required = false) String requestId
    ) {
        return Result.success(tenantAuthService.refresh(authorization, resolveRequestId(requestId)));
    }

    @PostMapping("/switch-tenant")
    public Result<UserLoginVO> switchTenant(
            @RequestHeader(value = "Authorization", required = false) String authorization,
            @RequestHeader(value = "X-Request-Id", required = false) String requestId,
            @RequestBody(required = false) TenantSwitchDTO request
    ) {
        return Result.success(tenantAuthService.switchTenant(
                authorization, request, resolveRequestId(requestId)));
    }

    @PostMapping("/change-password")
    public Result<Void> changePassword(@Valid @RequestBody ChangePasswordRequest request) {
        TenantRequestContext context = TenantContext.get();
        if (context == null || context.userId() == null) {
            throw new TenantAuthException(401, "AUTH_REQUIRED", "请先登录", resolveRequestId(null));
        }
        userService.changePassword(context.userId(), request.getOldPassword(), request.getNewPassword());
        return Result.success();
    }

    private String resolveRequestId(String requestId) {
        if (StringUtils.hasText(requestId) && requestId.length() <= 64) {
            return requestId;
        }
        TenantRequestContext context = TenantContext.get();
        return context == null ? UUID.randomUUID().toString() : context.requestId();
    }
}
