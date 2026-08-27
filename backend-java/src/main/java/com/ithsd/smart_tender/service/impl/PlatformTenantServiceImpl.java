package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.baomidou.mybatisplus.extension.plugins.pagination.Page;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.common.util.PasswordService;
import com.ithsd.smart_tender.mapper.TenantMapper;
import com.ithsd.smart_tender.mapper.TenantMemberMapper;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.dto.CreatePlatformTenantRequest;
import com.ithsd.smart_tender.model.entity.Tenant;
import com.ithsd.smart_tender.model.entity.TenantMember;
import com.ithsd.smart_tender.model.entity.User;
import com.ithsd.smart_tender.model.vo.EnterpriseUserVO;
import com.ithsd.smart_tender.model.vo.PlatformTenantPageVO;
import com.ithsd.smart_tender.model.vo.PlatformTenantVO;
import com.ithsd.smart_tender.service.PlatformTenantService;
import com.ithsd.smart_tender.service.TenantAuthorizationService;
import lombok.RequiredArgsConstructor;
import org.springframework.dao.DuplicateKeyException;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;

import java.time.LocalDateTime;
import java.time.ZoneOffset;
import java.util.List;
import java.util.Map;
import java.util.UUID;

@Service
@RequiredArgsConstructor
public class PlatformTenantServiceImpl implements PlatformTenantService {

    private static final String ACTIVE = "ACTIVE";
    private static final String DISABLED = "DISABLED";
    private static final String DELETED = "DELETED";
    private static final String REMOVED = "REMOVED";
    private static final String OWNER = "OWNER";
    private static final String ADMIN = "ADMIN";

    private final TenantMapper tenantMapper;
    private final TenantMemberMapper tenantMemberMapper;
    private final UserMapper userMapper;
    private final TenantAuthorizationService authorization;
    private final PasswordService passwordService;

    @Override
    public PlatformTenantPageVO listTenants(int page, int size, String keyword, String status) {
        TenantRequestContext context = authorization.requirePlatformAdmin();
        if (page < 1 || size < 1 || size > 100) {
            throw error(400, "REQUEST_INVALID", "Invalid page or size", context.requestId());
        }

        LambdaQueryWrapper<Tenant> wrapper = new LambdaQueryWrapper<>();
        if (keyword != null && !keyword.isBlank()) {
            wrapper.and(w -> w.like(Tenant::getName, keyword.trim())
                    .or().like(Tenant::getTenantCode, keyword.trim()));
        }
        if (status != null && !status.isBlank()) {
            wrapper.eq(Tenant::getStatus, status.trim().toUpperCase());
        }
        wrapper.orderByDesc(Tenant::getCreatedAt);

        Page<Tenant> result = tenantMapper.selectPage(new Page<>(page, size), wrapper);
        List<PlatformTenantVO> items = result.getRecords().stream()
                .map(this::toVO)
                .toList();
        return PlatformTenantPageVO.builder()
                .page(page)
                .size(size)
                .total(result.getTotal())
                .items(items)
                .build();
    }

    @Override
    @Transactional(rollbackFor = Exception.class)
    public PlatformTenantVO createTenant(CreatePlatformTenantRequest request) {
        TenantRequestContext context = authorization.requirePlatformAdmin();
        if (request == null || request.getName() == null || request.getName().isBlank()) {
            throw error(400, "REQUEST_INVALID", "企业名称必填", context.requestId());
        }

        String tenantCode = normalizeTenantCode(request.getTenantCode(), context.requestId());
        String planCode = request.getPlanCode() == null || request.getPlanCode().isBlank()
                ? "STANDARD"
                : request.getPlanCode().trim().toUpperCase();
        User owner = resolveOrCreateOwner(request, context);

        LocalDateTime now = LocalDateTime.now(ZoneOffset.UTC);
        Tenant tenant = Tenant.builder()
                .tenantCode(tenantCode)
                .name(request.getName().trim())
                .status(ACTIVE)
                .ownerUserId(owner.getId())
                .planCode(planCode)
                .settingsJson(null)
                .version(0L)
                .createdAt(now)
                .updatedAt(now)
                .build();

        try {
            tenantMapper.insert(tenant);
        } catch (DuplicateKeyException ex) {
            throw error(409, "TENANT_STATE_INVALID", "企业编码已存在", context.requestId());
        }

        TenantMember ownerMember = TenantMember.builder()
                .tenantId(tenant.getId())
                .userId(owner.getId())
                .role(OWNER)
                .status(ACTIVE)
                .joinedAt(now)
                .build();
        tenantMemberMapper.insert(ownerMember);

        return toVO(tenant, owner);
    }

    @Override
    public PlatformTenantVO getTenant(Long tenantId) {
        TenantRequestContext context = authorization.requirePlatformAdmin();
        Tenant tenant = requireTenant(tenantId, context.requestId());
        return toVO(tenant);
    }

    @Override
    public List<EnterpriseUserVO> listTenantMembers(Long tenantId) {
        TenantRequestContext context = authorization.requirePlatformAdmin();
        requireTenant(tenantId, context.requestId());
        List<TenantMember> members = tenantMemberMapper.findByTenantId(tenantId);
        if (members == null || members.isEmpty()) {
            return List.of();
        }
        return members.stream()
                .filter(member -> member != null && !REMOVED.equals(member.getStatus()))
                .map(this::toMemberVO)
                .toList();
    }

    private EnterpriseUserVO toMemberVO(TenantMember member) {
        User user = member.getUserId() == null ? null : userMapper.selectById(member.getUserId());
        return EnterpriseUserVO.builder()
                .userId(member.getUserId())
                .username(user == null ? null : user.getUsername())
                .realName(user == null ? null : user.getRealName())
                .role(member.getRole())
                .status(member.getStatus())
                .memberId(member.getId())
                .createdAt(user == null ? null : user.getCreateTime())
                .build();
    }

    @Override
    @Transactional(rollbackFor = Exception.class)
    public PlatformTenantVO transferOwner(Long tenantId, Long targetUserId) {
        TenantRequestContext context = authorization.requirePlatformAdmin();
        Tenant tenant = requireTenant(tenantId, context.requestId());
        TenantMember target = tenantMemberMapper.findByUserAndTenantId(targetUserId, tenantId);
        if (target == null || !ACTIVE.equals(target.getStatus())) {
            throw error(404, "TENANT_MEMBER_NOT_FOUND", "目标成员不存在或非活跃", context.requestId());
        }

        // 旧 OWNER（可能多个历史残留）降级为 ADMIN
        List<TenantMember> members = tenantMemberMapper.findByTenantId(tenantId);
        for (TenantMember member : members) {
            if (member != null && OWNER.equalsIgnoreCase(member.getRole())
                    && !member.getId().equals(target.getId())) {
                member.setRole(ADMIN);
                tenantMemberMapper.updateById(member);
            }
        }

        if (!OWNER.equalsIgnoreCase(target.getRole())) {
            target.setRole(OWNER);
            tenantMemberMapper.updateById(target);
        }

        tenant.setOwnerUserId(targetUserId);
        tenant.setVersion(tenant.getVersion() == null ? 1L : tenant.getVersion() + 1L);
        tenant.setUpdatedAt(LocalDateTime.now(ZoneOffset.UTC));
        tenantMapper.updateById(tenant);

        // 所有权/角色变化后，相关用户会话需重新校验。
        return toVO(tenant);
    }

    @Override
    public PlatformTenantVO disableTenant(Long tenantId) {
        TenantRequestContext context = authorization.requirePlatformAdmin();
        Tenant tenant = requireTenant(tenantId, context.requestId());
        if (DISABLED.equals(tenant.getStatus())) {
            return toVO(tenant);
        }
        if (!ACTIVE.equals(tenant.getStatus())) {
            throw error(409, "TENANT_STATE_INVALID", "当前状态不能停用", context.requestId());
        }
        tenant.setStatus(DISABLED);
        tenant.setVersion(tenant.getVersion() == null ? 1L : tenant.getVersion() + 1L);
        tenant.setUpdatedAt(LocalDateTime.now(ZoneOffset.UTC));
        tenantMapper.updateById(tenant);
        return toVO(tenant);
    }

    @Override
    public PlatformTenantVO restoreTenant(Long tenantId) {
        TenantRequestContext context = authorization.requirePlatformAdmin();
        Tenant tenant = requireTenant(tenantId, context.requestId());
        if (ACTIVE.equals(tenant.getStatus())) {
            return toVO(tenant);
        }
        if (!DISABLED.equals(tenant.getStatus())) {
            throw error(409, "TENANT_STATE_INVALID", "当前状态不能恢复", context.requestId());
        }
        tenant.setStatus(ACTIVE);
        tenant.setVersion(tenant.getVersion() == null ? 1L : tenant.getVersion() + 1L);
        tenant.setUpdatedAt(LocalDateTime.now(ZoneOffset.UTC));
        tenantMapper.updateById(tenant);
        return toVO(tenant);
    }

    @Override
    public PlatformTenantVO deleteTenant(Long tenantId) {
        TenantRequestContext context = authorization.requirePlatformAdmin();
        Tenant tenant = requireTenant(tenantId, context.requestId());
        if (DELETED.equals(tenant.getStatus())) {
            return toVO(tenant);
        }
        if (DISABLED.equals(tenant.getStatus()) || ACTIVE.equals(tenant.getStatus())) {
            tenant.setStatus(DELETED);
            tenant.setDeletedAt(LocalDateTime.now(ZoneOffset.UTC));
            tenant.setUpdatedAt(tenant.getDeletedAt());
            tenantMapper.updateById(tenant);
            return toVO(tenant);
        }
        throw error(409, "TENANT_STATE_INVALID", "当前状态不能删除", context.requestId());
    }

    private Tenant requireTenant(Long tenantId, String requestId) {
        Tenant tenant = tenantMapper.selectById(tenantId);
        if (tenant == null || DELETED.equals(tenant.getStatus())) {
            throw new TenantAuthException(404, "TENANT_NOT_FOUND", "企业不存在", requestId);
        }
        return tenant;
    }

    private User resolveOrCreateOwner(CreatePlatformTenantRequest request, TenantRequestContext context) {
        if (request.getOwnerUserId() != null) {
            User user = userMapper.selectById(request.getOwnerUserId());
            if (user == null || Integer.valueOf(0).equals(user.getStatus())) {
                throw error(404, "TENANT_MEMBER_NOT_FOUND", "指定 OWNER 用户不存在", context.requestId());
            }
            return user;
        }

        if (request.getOwnerUsername() == null || request.getOwnerUsername().isBlank()
                || request.getOwnerPassword() == null || request.getOwnerPassword().isBlank()) {
            throw error(400, "REQUEST_INVALID", "需提供 owner_username 和 owner_password（或 owner_user_id）",
                    context.requestId());
        }
        String username = request.getOwnerUsername().trim();
        Long exists = userMapper.selectCount(
                new LambdaQueryWrapper<User>().eq(User::getUsername, username));
        if (exists != null && exists > 0) {
            throw error(409, "TENANT_MEMBER_EXISTS", "OWNER 账号已存在", context.requestId());
        }
        LocalDateTime now = LocalDateTime.now(ZoneOffset.UTC);
        User user = User.builder()
                .username(username)
                .password(passwordService.encode(request.getOwnerPassword()))
                .realName(request.getOwnerRealName() == null ? username : request.getOwnerRealName().trim())
                .status(1)
                .isPlatformAdmin(false)
                .createTime(now)
                .updateTime(now)
                .build();
        userMapper.insert(user);
        return user;
    }

    private PlatformTenantVO toVO(Tenant tenant) {
        User owner = tenant.getOwnerUserId() == null ? null : userMapper.selectById(tenant.getOwnerUserId());
        long memberCount = tenantMemberMapper.countActiveByTenantId(tenant.getId());
        return PlatformTenantVO.builder()
                .tenantId(tenant.getId())
                .tenantCode(tenant.getTenantCode())
                .name(tenant.getName())
                .status(tenant.getStatus())
                .planCode(tenant.getPlanCode())
                .ownerUserId(tenant.getOwnerUserId())
                .ownerUsername(owner == null ? null : owner.getUsername())
                .ownerRealName(owner == null ? null : owner.getRealName())
                .memberCount(memberCount)
                .version(tenant.getVersion())
                .createdAt(tenant.getCreatedAt())
                .updatedAt(tenant.getUpdatedAt())
                .build();
    }

    private PlatformTenantVO toVO(Tenant tenant, User owner) {
        long memberCount = tenantMemberMapper.countActiveByTenantId(tenant.getId());
        return PlatformTenantVO.builder()
                .tenantId(tenant.getId())
                .tenantCode(tenant.getTenantCode())
                .name(tenant.getName())
                .status(tenant.getStatus())
                .planCode(tenant.getPlanCode())
                .ownerUserId(tenant.getOwnerUserId())
                .ownerUsername(owner == null ? null : owner.getUsername())
                .ownerRealName(owner == null ? null : owner.getRealName())
                .memberCount(memberCount)
                .version(tenant.getVersion())
                .createdAt(tenant.getCreatedAt())
                .updatedAt(tenant.getUpdatedAt())
                .build();
    }

    private String normalizeTenantCode(String requested, String requestId) {
        if (requested == null || requested.isBlank()) {
            return "tenant-" + UUID.randomUUID().toString().replace("-", "").substring(0, 12);
        }
        String code = requested.trim();
        if (!code.matches("^[a-z0-9][a-z0-9_-]{2,63}$")) {
            throw error(400, "REQUEST_INVALID", "企业编码格式不合法", requestId);
        }
        return code;
    }

    private static TenantAuthException error(int status, String code, String message, String requestId) {
        return new TenantAuthException(status, code, message, requestId, Map.of());
    }
}