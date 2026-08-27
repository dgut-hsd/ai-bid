package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.common.util.PasswordService;
import com.ithsd.smart_tender.mapper.TenantMemberMapper;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.dto.EnterpriseCreateUserRequest;
import com.ithsd.smart_tender.model.dto.EnterpriseUpdateMemberRequest;
import com.ithsd.smart_tender.model.dto.EnterpriseUpdateUserRequest;
import com.ithsd.smart_tender.model.entity.TenantMember;
import com.ithsd.smart_tender.model.entity.User;
import com.ithsd.smart_tender.model.vo.EnterpriseUserVO;
import com.ithsd.smart_tender.service.EnterpriseUserService;
import com.ithsd.smart_tender.service.TenantAuthorizationService;
import com.ithsd.smart_tender.service.TenantSessionStore;
import lombok.RequiredArgsConstructor;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;

import java.time.LocalDateTime;
import java.time.ZoneOffset;
import java.util.List;
import java.util.Map;
import java.util.Set;

@Service
@RequiredArgsConstructor
public class EnterpriseUserServiceImpl implements EnterpriseUserService {

    private static final String ACTIVE = "ACTIVE";
    private static final String SUSPENDED = "SUSPENDED";
    private static final String REMOVED = "REMOVED";
    private static final String OWNER = "OWNER";

    private static final Set<String> CREATE_ROLES = Set.of("ADMIN", "MEMBER");

    private final UserMapper userMapper;
    private final TenantMemberMapper tenantMemberMapper;
    private final TenantAuthorizationService authorization;
    private final TenantSessionStore tenantSessionStore;
    private final PasswordService passwordService;

    /** 只有企业 OWNER 能进入企业管理并操作用户。 */
    private TenantRequestContext requireOwner() {
        TenantRequestContext context = authorization.requireCurrentTenant();
        if (!OWNER.equalsIgnoreCase(context.role())) {
            throw error(403, "TENANT_ROLE_FORBIDDEN",
                    "只有 OWNER 可以执行此操作", context.requestId(),
                    Map.of("required_permission", "tenant.members.role.write"));
        }
        return context;
    }

    @Override
    public List<EnterpriseUserVO> listUsers() {
        TenantRequestContext context = requireOwner();
        List<TenantMember> members = tenantMemberMapper.findByTenantId(context.tenantId());
        if (members == null || members.isEmpty()) {
            return List.of();
        }
        return members.stream()
                .filter(member -> member != null && !REMOVED.equals(member.getStatus()))
                .map(this::toVO)
                .toList();
    }

    @Override
    @Transactional(rollbackFor = Exception.class)
    public EnterpriseUserVO createUser(EnterpriseCreateUserRequest request) {
        TenantRequestContext context = requireOwner();
        String username = request.getUsername().trim();
        String role = normalizeCreateRole(request.getRole(), context);

        Long exists = userMapper.selectCount(
                new LambdaQueryWrapper<User>().eq(User::getUsername, username));
        if (exists != null && exists > 0) {
            throw error(409, "TENANT_MEMBER_EXISTS", "用户名已存在", context.requestId());
        }

        LocalDateTime now = LocalDateTime.now(ZoneOffset.UTC);
        User user = User.builder()
                .username(username)
                .password(passwordService.encode(request.getPassword()))
                .realName(request.getRealName().trim())
                .status(1)
                .isPlatformAdmin(false)
                .createTime(now)
                .updateTime(now)
                .build();
        userMapper.insert(user);

        TenantMember member = TenantMember.builder()
                .tenantId(context.tenantId())
                .userId(user.getId())
                .role(role)
                .status(ACTIVE)
                .joinedAt(now)
                .build();
        tenantMemberMapper.insert(member);

        return toVO(member, user);
    }

    @Override
    @Transactional(rollbackFor = Exception.class)
    public void updateUser(Long userId, EnterpriseUpdateUserRequest request) {
        TenantRequestContext context = requireOwner();
        User user = requireMemberUser(userId, context);

        String newUsername = request.getUsername() == null ? null : request.getUsername().trim();
        String newRealName = request.getRealName() == null ? null : request.getRealName().trim();

        boolean noUsername = newUsername == null || newUsername.isEmpty();
        boolean noRealName = newRealName == null || newRealName.isEmpty();
        if (noUsername && noRealName) {
            throw error(400, "REQUEST_INVALID", "至少提供一个要修改的字段", context.requestId());
        }

        boolean usernameChanged = false;
        if (!noUsername) {
            if (newUsername.length() < 3 || newUsername.length() > 50) {
                throw error(400, "REQUEST_INVALID", "账号长度需 3~50 个字符", context.requestId());
            }
            if (!newUsername.equals(user.getUsername())) {
                Long exists = userMapper.selectCount(
                        new LambdaQueryWrapper<User>()
                                .eq(User::getUsername, newUsername)
                                .ne(User::getId, userId));
                if (exists != null && exists > 0) {
                    throw error(409, "TENANT_MEMBER_EXISTS", "用户名已存在", context.requestId());
                }
                user.setUsername(newUsername);
                usernameChanged = true;
            }
        }

        if (!noRealName) {
            if (newRealName.length() > 50) {
                throw error(400, "REQUEST_INVALID", "姓名不能超过 50 个字符", context.requestId());
            }
            user.setRealName(newRealName);
        }

        user.setUpdateTime(LocalDateTime.now(ZoneOffset.UTC));
        userMapper.updateById(user);

        if (usernameChanged) {
            tenantSessionStore.deleteByUserId(userId);
        }
    }

    @Override
    @Transactional(rollbackFor = Exception.class)
    public EnterpriseUserVO updateMember(Long userId, EnterpriseUpdateMemberRequest request) {
        TenantRequestContext context = requireOwner();
        TenantMember member = requireMember(userId, context);
        if (OWNER.equalsIgnoreCase(member.getRole())) {
            throw error(403, "TENANT_OWNER_REQUIRED",
                    "不能修改 OWNER 的角色或状态", context.requestId());
        }

        String role = null;
        if (request.getRole() != null && !request.getRole().isBlank()) {
            role = normalizeCreateRole(request.getRole(), context);
        }
        String status = null;
        if (request.getStatus() != null && !request.getStatus().isBlank()) {
            status = request.getStatus().trim().toUpperCase();
            if (!ACTIVE.equals(status) && !SUSPENDED.equals(status)) {
                throw error(400, "REQUEST_INVALID", "status 只能为 ACTIVE 或 SUSPENDED", context.requestId());
            }
        }
        if (role == null && status == null) {
            throw error(400, "REQUEST_INVALID", "至少提供一个要修改的字段", context.requestId());
        }

        if (role != null) {
            member.setRole(role);
        }
        if (status != null) {
            member.setStatus(status);
        }
        tenantMemberMapper.updateById(member);

        // 角色/状态变化后，旧会话需要在下次请求时重新校验成员状态。
        tenantSessionStore.deleteByUserId(userId);
        return toVO(member, userMapper.selectById(userId));
    }

    @Override
    public void resetPassword(Long userId, String newPassword) {
        TenantRequestContext context = requireOwner();
        User user = requireMemberUser(userId, context);
        user.setPassword(passwordService.encode(newPassword));
        user.setUpdateTime(LocalDateTime.now(ZoneOffset.UTC));
        userMapper.updateById(user);
        tenantSessionStore.deleteByUserId(user.getId());
    }

    @Override
    @Transactional(rollbackFor = Exception.class)
    public void removeMember(Long userId) {
        TenantRequestContext context = requireOwner();
        if (userId.equals(context.userId())) {
            throw error(409, "TENANT_STATE_INVALID", "不能移除自己", context.requestId());
        }

        TenantMember member = requireMember(userId, context);
        if (OWNER.equalsIgnoreCase(member.getRole()) && isLastOwner(context.tenantId(), member.getId())) {
            throw error(409, "TENANT_LAST_OWNER", "不能移除最后一个 OWNER", context.requestId());
        }

        member.setStatus(REMOVED);
        tenantMemberMapper.updateById(member);

        // 移出企业只影响本企业成员关系，不停用全局账号，也不影响其在其他企业的身份。
        tenantSessionStore.deleteByUserId(userId);
    }

    private TenantMember requireMember(Long userId, TenantRequestContext context) {
        TenantMember member = tenantMemberMapper.findByUserAndTenantId(userId, context.tenantId());
        if (member == null || REMOVED.equals(member.getStatus())) {
            throw error(404, "TENANT_MEMBER_NOT_FOUND", "成员不存在", context.requestId());
        }
        return member;
    }

    private User requireMemberUser(Long userId, TenantRequestContext context) {
        TenantMember member = requireMember(userId, context);
        User user = userMapper.selectById(userId);
        if (user == null) {
            throw error(404, "TENANT_MEMBER_NOT_FOUND", "用户不存在", context.requestId());
        }
        return user;
    }

    private boolean isLastOwner(Long tenantId, Long targetMemberId) {
        long ownerCount = tenantMemberMapper.findByTenantId(tenantId).stream()
                .filter(member -> member != null
                        && !member.getId().equals(targetMemberId)
                        && OWNER.equalsIgnoreCase(member.getRole())
                        && ACTIVE.equals(member.getStatus()))
                .count();
        return ownerCount == 0;
    }

    private String normalizeCreateRole(String role, TenantRequestContext context) {
        if (role == null || role.isBlank()) {
            throw error(400, "REQUEST_INVALID", "role is required", context.requestId());
        }
        String normalized = role.trim().toUpperCase();
        if (!CREATE_ROLES.contains(normalized)) {
            throw error(400, "REQUEST_INVALID",
                    "role 只能为 ADMIN / MEMBER", context.requestId());
        }
        return normalized;
    }

    private EnterpriseUserVO toVO(TenantMember member) {
        User user = member.getUserId() == null ? null : userMapper.selectById(member.getUserId());
        return toVO(member, user);
    }

    private EnterpriseUserVO toVO(TenantMember member, User user) {
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

    private static TenantAuthException error(
            int status, String code, String message, String requestId, Map<String, Object> details) {
        return new TenantAuthException(status, code, message, requestId, details);
    }

    private static TenantAuthException error(int status, String code, String message, String requestId) {
        return error(status, code, message, requestId, Map.of());
    }
}