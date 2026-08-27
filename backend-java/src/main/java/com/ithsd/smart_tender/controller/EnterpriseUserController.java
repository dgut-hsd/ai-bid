package com.ithsd.smart_tender.controller;

import com.ithsd.smart_tender.model.dto.EnterpriseCreateUserRequest;
import com.ithsd.smart_tender.model.dto.EnterpriseResetPasswordRequest;
import com.ithsd.smart_tender.model.dto.EnterpriseUpdateMemberRequest;
import com.ithsd.smart_tender.model.dto.EnterpriseUpdateUserRequest;
import com.ithsd.smart_tender.model.result.Result;
import com.ithsd.smart_tender.model.vo.EnterpriseUserVO;
import com.ithsd.smart_tender.service.EnterpriseUserService;
import jakarta.validation.Valid;
import lombok.RequiredArgsConstructor;
import org.springframework.web.bind.annotation.DeleteMapping;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PatchMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.PutMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;

import java.util.List;

/** 企业管理模块：企业 OWNER 管理本企业用户（仅当前租户 OWNER 可访问）。 */
@RestController
@RequestMapping("/api/enterprise/users")
@RequiredArgsConstructor
public class EnterpriseUserController {

    private final EnterpriseUserService enterpriseUserService;

    @GetMapping
    public Result<List<EnterpriseUserVO>> listUsers() {
        return Result.success(enterpriseUserService.listUsers());
    }

    @PostMapping
    public Result<EnterpriseUserVO> createUser(@Valid @RequestBody EnterpriseCreateUserRequest request) {
        return Result.success(enterpriseUserService.createUser(request));
    }

    @PutMapping("/{userId}")
    public Result<Void> updateUser(
            @PathVariable Long userId,
            @RequestBody EnterpriseUpdateUserRequest request
    ) {
        enterpriseUserService.updateUser(userId, request);
        return Result.success();
    }

    @PatchMapping("/{userId}")
    public Result<EnterpriseUserVO> updateMember(
            @PathVariable Long userId,
            @RequestBody EnterpriseUpdateMemberRequest request
    ) {
        return Result.success(enterpriseUserService.updateMember(userId, request));
    }

    @PostMapping("/{userId}/password")
    public Result<Void> resetPassword(
            @PathVariable Long userId,
            @Valid @RequestBody EnterpriseResetPasswordRequest request
    ) {
        enterpriseUserService.resetPassword(userId, request.getPassword());
        return Result.success();
    }

    @DeleteMapping("/{userId}")
    public Result<Void> removeMember(@PathVariable Long userId) {
        enterpriseUserService.removeMember(userId);
        return Result.success();
    }
}