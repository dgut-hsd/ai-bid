package com.ithsd.smart_tender.controller;

import com.ithsd.smart_tender.model.dto.AdminCreateUserRequest;
import com.ithsd.smart_tender.model.dto.AdminResetPasswordRequest;
import com.ithsd.smart_tender.model.result.Result;
import com.ithsd.smart_tender.model.vo.AdminUserVO;
import com.ithsd.smart_tender.service.AdminUserService;
import jakarta.validation.Valid;
import lombok.RequiredArgsConstructor;
import org.springframework.web.bind.annotation.DeleteMapping;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;

import java.util.List;

/** 系统管理模块：部门用户管理（仅企业 OWNER 可访问）。 */
@RestController
@RequestMapping("/api/admin/users")
@RequiredArgsConstructor
public class AdminUserController {

    private final AdminUserService adminUserService;

    @GetMapping
    public Result<List<AdminUserVO>> listUsers() {
        return Result.success(adminUserService.listUsers());
    }

    @PostMapping
    public Result<AdminUserVO> createUser(@Valid @RequestBody AdminCreateUserRequest request) {
        return Result.success(adminUserService.createUser(request));
    }

    @PostMapping("/{userId}/password")
    public Result<Void> resetPassword(
            @PathVariable Long userId,
            @Valid @RequestBody AdminResetPasswordRequest request
    ) {
        adminUserService.resetPassword(userId, request.getPassword());
        return Result.success();
    }

    @DeleteMapping("/{userId}")
    public Result<Void> removeMember(@PathVariable Long userId) {
        adminUserService.removeMember(userId);
        return Result.success();
    }
}