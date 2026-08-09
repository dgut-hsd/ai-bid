package com.ithsd.smart_tender.service.impl;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.mapper.TenantMapper;
import com.ithsd.smart_tender.mapper.TenantMemberMapper;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.dto.CreateTenantRequest;
import com.ithsd.smart_tender.model.entity.Tenant;
import com.ithsd.smart_tender.model.entity.TenantMember;
import com.ithsd.smart_tender.model.entity.User;
import com.ithsd.smart_tender.model.vo.TenantListVO;
import com.ithsd.smart_tender.model.vo.TenantMemberPageVO;
import com.ithsd.smart_tender.model.vo.TenantMemberVO;
import com.ithsd.smart_tender.model.vo.TenantSummaryVO;
import com.ithsd.smart_tender.model.vo.TenantVO;
import com.ithsd.smart_tender.service.TenantAuthorizationService;
import com.ithsd.smart_tender.service.TenantService;
import lombok.RequiredArgsConstructor;
import org.springframework.dao.DuplicateKeyException;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;

import java.time.LocalDateTime;
import java.time.ZoneOffset;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.UUID;

@Service
@RequiredArgsConstructor
public class TenantServiceImpl implements TenantService {

    private static final String ACTIVE = "ACTIVE";
    private static final String OWNER = "OWNER";

    private final TenantMapper tenantMapper;
    private final TenantMemberMapper tenantMemberMapper;
    private final UserMapper userMapper;
    private final TenantAuthorizationService authorization;
    private final ObjectMapper objectMapper;

    @Override
    public TenantListVO listTenants() {
        TenantRequestContext context = authorization.requireAuthenticated();
        List<TenantSummaryVO> items = new ArrayList<>();
        List<TenantMember> members = tenantMemberMapper.findByUserId(context.userId());
        if (members != null) {
            for (TenantMember member : members) {
                if (member == null || member.getTenantId() == null || !ACTIVE.equals(member.getStatus())) {
                    continue;
                }
                Tenant tenant = tenantMapper.selectById(member.getTenantId());
                if (tenant == null || !ACTIVE.equals(tenant.getStatus())) {
                    continue;
                }
                items.add(toSummary(tenant, member, context.tenantId()));
            }
        }
        return TenantListVO.builder()
                .currentTenantId(context.tenantId())
                .items(items)
                .build();
    }

    @Override
    @Transactional(rollbackFor = Exception.class)
    public TenantVO createTenant(CreateTenantRequest request) {
        TenantRequestContext context = authorization.requireAuthenticated();
        if (request == null || request.getName() == null || request.getName().isBlank()) {
            throw error(400, "REQUEST_INVALID", "Tenant name is required", context.requestId());
        }

        String name = request.getName().trim();
        if (name.length() > 128) {
            throw error(400, "REQUEST_INVALID", "Tenant name is too long", context.requestId());
        }
        String tenantCode = normalizeTenantCode(request.getTenantCode());
        String planCode = request.getPlanCode() == null || request.getPlanCode().isBlank()
                ? "STANDARD"
                : request.getPlanCode().trim().toUpperCase();
        LocalDateTime now = LocalDateTime.now(ZoneOffset.UTC);
        Tenant tenant = Tenant.builder()
                .tenantCode(tenantCode)
                .name(name)
                .status(ACTIVE)
                .ownerUserId(context.userId())
                .planCode(planCode)
                .settingsJson(writeSettings(request.getSettings(), context.requestId()))
                .version(0L)
                .createdAt(now)
                .updatedAt(now)
                .build();

        try {
            tenantMapper.insert(tenant);
            TenantMember owner = TenantMember.builder()
                    .tenantId(tenant.getId())
                    .userId(context.userId())
                    .role(OWNER)
                    .status(ACTIVE)
                    .joinedAt(now)
                    .invitedBy(null)
                    .build();
            tenantMemberMapper.insert(owner);
            return toDetail(tenant, owner, context.tenantId());
        } catch (DuplicateKeyException ex) {
            throw error(409, "TENANT_STATE_INVALID", "Tenant code already exists", context.requestId());
        }
    }

    @Override
    public TenantSummaryVO currentTenant() {
        TenantRequestContext context = authorization.requireCurrentTenant();
        Tenant tenant = tenantMapper.selectById(context.tenantId());
        TenantMember member = tenantMemberMapper.findByUserAndTenantId(context.userId(), context.tenantId());
        if (tenant == null || member == null || !ACTIVE.equals(tenant.getStatus())
                || !ACTIVE.equals(member.getStatus())) {
            throw error(404, "TENANT_NOT_FOUND", "Tenant not found", context.requestId());
        }
        return toSummary(tenant, member, context.tenantId());
    }

    @Override
    public TenantMemberPageVO listMembers(Long tenantId, int page, int size) {
        TenantRequestContext context = authorization.requireTenant(tenantId, "tenant.read");
        if (page < 1 || size < 1 || size > 100) {
            throw error(400, "REQUEST_INVALID", "Invalid page or size", context.requestId());
        }
        Tenant tenant = tenantMapper.selectById(tenantId);
        if (tenant == null || !ACTIVE.equals(tenant.getStatus())) {
            throw error(404, "TENANT_NOT_FOUND", "Tenant not found", context.requestId());
        }
        List<TenantMember> all = tenantMemberMapper.findByTenantId(tenantId);
        if (all == null) {
            all = List.of();
        }
        int from = Math.min((page - 1) * size, all.size());
        int to = Math.min(from + size, all.size());
        List<TenantMemberVO> items = all.subList(from, to).stream()
                .map(this::toMember)
                .toList();
        return TenantMemberPageVO.builder()
                .page(page)
                .size(size)
                .total(all.size())
                .items(items)
                .build();
    }

    private TenantMemberVO toMember(TenantMember member) {
        User user = member.getUserId() == null ? null : userMapper.selectById(member.getUserId());
        return TenantMemberVO.builder()
                .memberId(member.getId())
                .tenantId(member.getTenantId())
                .userId(member.getUserId())
                .username(user == null ? null : user.getUsername())
                .realName(user == null ? null : user.getRealName())
                .role(member.getRole())
                .permissions(TenantAuthorizationService.permissionsFor(member.getRole()))
                .status(member.getStatus())
                .joinedAt(member.getJoinedAt())
                .lastSeenAt(member.getLastSeenAt())
                .build();
    }

    private TenantSummaryVO toSummary(Tenant tenant, TenantMember member, Long currentTenantId) {
        return TenantSummaryVO.builder()
                .tenantId(tenant.getId())
                .tenantCode(tenant.getTenantCode())
                .name(tenant.getName())
                .status(tenant.getStatus())
                .role(member == null ? null : member.getRole())
                .permissions(TenantAuthorizationService.permissionsFor(member == null ? null : member.getRole()))
                .current(tenant.getId() != null && tenant.getId().equals(currentTenantId))
                .build();
    }

    private TenantVO toDetail(Tenant tenant, TenantMember member, Long currentTenantId) {
        return TenantVO.builder()
                .tenantId(tenant.getId())
                .tenantCode(tenant.getTenantCode())
                .name(tenant.getName())
                .status(tenant.getStatus())
                .ownerUserId(tenant.getOwnerUserId())
                .planCode(tenant.getPlanCode())
                .settings(readSettings(tenant.getSettingsJson()))
                .version(tenant.getVersion())
                .createdAt(tenant.getCreatedAt())
                .updatedAt(tenant.getUpdatedAt())
                .role(member == null ? OWNER : member.getRole())
                .permissions(TenantAuthorizationService.permissionsFor(member == null ? OWNER : member.getRole()))
                .current(tenant.getId() != null && tenant.getId().equals(currentTenantId))
                .build();
    }

    private String normalizeTenantCode(String requested) {
        if (requested == null || requested.isBlank()) {
            return "tenant-" + UUID.randomUUID().toString().replace("-", "").substring(0, 12);
        }
        String code = requested.trim();
        if (!code.matches("^[a-z0-9][a-z0-9_-]{2,63}$")) {
            throw error(400, "REQUEST_INVALID", "Invalid tenant_code", requestId());
        }
        return code;
    }

    private String writeSettings(Map<String, Object> settings, String requestId) {
        if (settings == null || settings.isEmpty()) {
            return null;
        }
        try {
            return objectMapper.writeValueAsString(settings);
        } catch (JsonProcessingException ex) {
            throw error(400, "REQUEST_INVALID", "Invalid settings", requestId);
        }
    }

    private Map<String, Object> readSettings(String settingsJson) {
        if (settingsJson == null || settingsJson.isBlank()) {
            return Collections.emptyMap();
        }
        try {
            return objectMapper.readValue(settingsJson, new TypeReference<>() {
            });
        } catch (JsonProcessingException ex) {
            return Collections.emptyMap();
        }
    }

    private String requestId() {
        TenantRequestContext context = com.ithsd.smart_tender.common.TenantContext.get();
        return context == null ? UUID.randomUUID().toString() : context.requestId();
    }

    private static TenantAuthException error(int status, String code, String message, String requestId) {
        return new TenantAuthException(status, code, message, requestId);
    }
}
