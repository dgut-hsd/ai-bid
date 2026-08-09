package com.ithsd.smart_tender.controller;

import com.ithsd.smart_tender.model.dto.CreateTenantRequest;
import com.ithsd.smart_tender.model.result.Result;
import com.ithsd.smart_tender.model.vo.TenantListVO;
import com.ithsd.smart_tender.model.vo.TenantMemberPageVO;
import com.ithsd.smart_tender.model.vo.TenantSummaryVO;
import com.ithsd.smart_tender.model.vo.TenantVO;
import com.ithsd.smart_tender.service.TenantService;
import jakarta.validation.Valid;
import lombok.RequiredArgsConstructor;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;

@RestController
@RequestMapping("/api/tenants")
@RequiredArgsConstructor
public class TenantController {

    private final TenantService tenantService;

    @GetMapping
    public Result<TenantListVO> listTenants() {
        return Result.success(tenantService.listTenants());
    }

    @PostMapping
    public Result<TenantVO> createTenant(@Valid @RequestBody CreateTenantRequest request) {
        return Result.success(tenantService.createTenant(request));
    }

    @GetMapping("/current")
    public Result<TenantSummaryVO> currentTenant() {
        return Result.success(tenantService.currentTenant());
    }

    @GetMapping("/{tenantId}/members")
    public Result<TenantMemberPageVO> listMembers(
            @PathVariable Long tenantId,
            @RequestParam(defaultValue = "1") int page,
            @RequestParam(defaultValue = "20") int size
    ) {
        return Result.success(tenantService.listMembers(tenantId, page, size));
    }
}
