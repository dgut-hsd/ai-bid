package com.ithsd.smart_tender.controller;

import com.ithsd.smart_tender.model.dto.CreatePlatformTenantRequest;
import com.ithsd.smart_tender.model.dto.TransferTenantOwnerRequest;
import com.ithsd.smart_tender.model.result.Result;
import com.ithsd.smart_tender.model.vo.EnterpriseUserVO;
import com.ithsd.smart_tender.model.vo.PlatformTenantPageVO;
import com.ithsd.smart_tender.model.vo.PlatformTenantVO;
import com.ithsd.smart_tender.service.PlatformTenantService;
import jakarta.validation.Valid;
import lombok.RequiredArgsConstructor;
import java.util.List;
import org.springframework.web.bind.annotation.DeleteMapping;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;

/** 系统管理模块：平台管理员（系统管理者）管理所有企业。 */
@RestController
@RequestMapping("/api/platform/tenants")
@RequiredArgsConstructor
public class PlatformTenantController {

    private final PlatformTenantService platformTenantService;

    @GetMapping
    public Result<PlatformTenantPageVO> listTenants(
            @RequestParam(defaultValue = "1") int page,
            @RequestParam(defaultValue = "20") int size,
            @RequestParam(required = false) String keyword,
            @RequestParam(required = false) String status
    ) {
        return Result.success(platformTenantService.listTenants(page, size, keyword, status));
    }

    @PostMapping
    public Result<PlatformTenantVO> createTenant(
            @Valid @RequestBody CreatePlatformTenantRequest request
    ) {
        return Result.success(platformTenantService.createTenant(request));
    }

    @GetMapping("/{tenantId}")
    public Result<PlatformTenantVO> getTenant(@PathVariable Long tenantId) {
        return Result.success(platformTenantService.getTenant(tenantId));
    }

    @GetMapping("/{tenantId}/members")
    public Result<List<EnterpriseUserVO>> listMembers(@PathVariable Long tenantId) {
        return Result.success(platformTenantService.listTenantMembers(tenantId));
    }

    @PostMapping("/{tenantId}/transfer-owner")
    public Result<PlatformTenantVO> transferOwner(
            @PathVariable Long tenantId,
            @Valid @RequestBody TransferTenantOwnerRequest request
    ) {
        return Result.success(platformTenantService.transferOwner(tenantId, request.getTargetUserId()));
    }

    @PostMapping("/{tenantId}/disable")
    public Result<PlatformTenantVO> disableTenant(@PathVariable Long tenantId) {
        return Result.success(platformTenantService.disableTenant(tenantId));
    }

    @PostMapping("/{tenantId}/restore")
    public Result<PlatformTenantVO> restoreTenant(@PathVariable Long tenantId) {
        return Result.success(platformTenantService.restoreTenant(tenantId));
    }

    @DeleteMapping("/{tenantId}")
    public Result<PlatformTenantVO> deleteTenant(@PathVariable Long tenantId) {
        return Result.success(platformTenantService.deleteTenant(tenantId));
    }
}