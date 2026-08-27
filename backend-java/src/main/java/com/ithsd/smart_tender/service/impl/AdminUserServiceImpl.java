package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.common.util.PasswordService;
import com.ithsd.smart_tender.mapper.TenantMemberMapper;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.dto.AdminCreateUserRequest;
import com.ithsd.smart_tender.model.dto.AdminUpdateUserRequest;
import com.ithsd.smart_tender.model.entity.TenantMember;
import com.ithsd.smart_tender.model.entity.User;
import com.ithsd.smart_tender.model.vo.AdminUserVO;
import com.ithsd.smart_tender.service.AdminUserService;
import com.ithsd.smart_tender.service.TenantAuthorizationService;
import com.ithsd.smart_tender.service.TenantSessionStore;
import lombok.RequiredArgsConstructor;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;

import java.time.LocalDateTime;
import java.time.ZoneOffset;
import java.util.List;
import java.util.Map;

@Service
@RequiredArgsConstructor
public class AdminUserServiceImpl implements AdminUserService {

    private static final String ACTIVE = "ACTIVE";
    private static final String REMOVED = "REMOVED";
    private static final String OWNER = "OWNER";
    private static final String MEMBER = "MEMBER";

    private final UserMapper userMapper;
    private final TenantMemberMapper tenantMemberMapper;
    private final TenantAuthorizationService authorization;
    private final TenantSessionStore tenantSessionStore;
    private final PasswordService passwordService;

    /** 只有企业 OWNER 能进入系统管理并操作用户。 */
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
    public List<AdminUserVO> listUsers() {
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
    public AdminUserVO createUser(AdminCreateUserRequest request) {
        TenantRequestContext context = requireOwner();
        String username = request.getUsername().trim();
        String role = normalizeRole(request.getRole(), context.requestId());

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
    public void updateUser(Long userId, AdminUpdateUserRequest request) {
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
            // 账号变更后使旧会话失效，用户需用新账号重新登录
            tenantSessionStore.deleteByUserId(userId);
        }
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

        TenantMember member = tenantMemberMapper.findByUserAndTenantId(userId, context.tenantId());
        if (member == null || REMOVED.equals(member.getStatus())) {
            throw error(404, "TENANT_MEMBER_NOT_FOUND", "成员不存在", context.requestId());
        }
        if (OWNER.equalsIgnoreCase(member.getRole()) && isLastOwner(context.tenantId(), member.getId())) {
            throw error(409, "TENANT_LAST_OWNER", "不能移除最后一个 OWNER", context.requestId());
        }

        member.setStatus(REMOVED);
        tenantMemberMapper.updateById(member);

        User user = userMapper.selectById(userId);
        if (user != null) {
            user.setStatus(0);
            userMapper.updateById(user);
        }
        tenantSessionStore.deleteByUserId(userId);
    }

    private User requireMemberUser(Long userId, TenantRequestContext context) {
        TenantMember member = tenantMemberMapper.findByUserAndTenantId(userId, context.tenantId());
        if (member == null || REMOVED.equals(member.getStatus())) {
            throw error(404, "TENANT_MEMBER_NOT_FOUND", "成员不存在", context.requestId());
        }
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

    private String normalizeRole(String role, String requestId) {
        if (role == null || role.isBlank()) {
            throw error(400, "REQUEST_INVALID", "role is required", requestId);
        }
        String normalized = role.trim().toUpperCase();
        if (OWNER.equals(normalized) || MEMBER.equals(normalized)) {
            return normalized;
        }
        throw error(400, "REQUEST_INVALID", "role must be OWNER or MEMBER", requestId);
    }

    private AdminUserVO toVO(TenantMember member) {
        User user = member.getUserId() == null ? null : userMapper.selectById(member.getUserId());
        return toVO(member, user);
    }

    private AdminUserVO toVO(TenantMember member, User user) {
        return AdminUserVO.builder()
                .userId(member.getUserId())
                .username(user == null ? null : user.getUsername())
                .realName(user == null ? null : user.getRealName())
                .role(member.getRole())
                .status(user == null || Integer.valueOf(1).equals(user.getStatus()) ? ACTIVE : "DISABLED")
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