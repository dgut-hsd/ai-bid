package com.ithsd.smart_tender.controller;

import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.model.dto.TenantSwitchDTO;
import com.ithsd.smart_tender.model.dto.UserLoginDTO;
import com.ithsd.smart_tender.model.dto.UserRegisterDTO;
import com.ithsd.smart_tender.model.result.Result;
import com.ithsd.smart_tender.model.vo.UserLoginVO;
import com.ithsd.smart_tender.service.TenantAuthService;
import com.ithsd.smart_tender.service.UserService;
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

    private final UserService userService;
    private final TenantAuthService tenantAuthService;

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

    @PostMapping("/register")
    public Result<Void> register(@RequestBody UserRegisterDTO userRegisterDTO) {
        userService.register(userRegisterDTO);
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
