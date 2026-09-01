package com.ithsd.smart_tender.service.impl;

import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantJwtClaims;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.common.util.JwtTokenService;
import com.ithsd.smart_tender.mapper.TenantMapper;
import com.ithsd.smart_tender.mapper.TenantMemberMapper;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.dto.TenantSwitchDTO;
import com.ithsd.smart_tender.model.dto.UserLoginDTO;
import com.ithsd.smart_tender.model.entity.Tenant;
import com.ithsd.smart_tender.model.entity.TenantMember;
import com.ithsd.smart_tender.model.entity.User;
import com.ithsd.smart_tender.model.vo.TenantSessionStateVO;
import com.ithsd.smart_tender.model.vo.TenantSummaryVO;
import com.ithsd.smart_tender.model.vo.UserInfoVO;
import com.ithsd.smart_tender.model.vo.UserLoginVO;
import com.ithsd.smart_tender.service.TenantAuthService;
import com.ithsd.smart_tender.service.TenantAuthorizationService;
import com.ithsd.smart_tender.service.TenantSessionStore;
import com.ithsd.smart_tender.service.UserService;
import lombok.RequiredArgsConstructor;
import org.springframework.stereotype.Service;
import org.springframework.util.StringUtils;

import java.time.Duration;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.UUID;

@Service
@RequiredArgsConstructor
public class TenantAuthServiceImpl implements TenantAuthService {

    private static final String ACTIVE = "ACTIVE";

    private final UserService userService;
    private final UserMapper userMapper;
    private final TenantMapper tenantMapper;
    private final TenantMemberMapper tenantMemberMapper;
    private final TenantSessionStore tenantSessionStore;
    private final JwtTokenService jwtTokenService;

    @Override
    public UserLoginVO login(UserLoginDTO request) {
        String requestId = newRequestId();
        User user;
        try {
            user = userService.login(request);
        } catch (RuntimeException ex) {
            throw error(401, "AUTH_INVALID", "账号或密码错误", requestId);
        }
        ensureActiveUser(user, requestId);

        List<TenantAccess> activeTenants = activeTenantsFor(user.getId());
        TenantSessionStateVO previous = currentSession(user.getId()).orElse(null);
        TenantAccess current = chooseCurrentTenant(activeTenants, previous);
        long version = nextVersion(previous);
        TenantSessionStateVO session = sessionState(user.getId(), current, version, UUID.randomUUID().toString());
        tenantSessionStore.save(session, sessionTtl());
        return sessionResponse(user, activeTenants, current, session);
    }

    @Override
    public UserLoginVO refresh(String authorization, String requestId) {
        // 容忍 access token 已过期（签名仍有效），凭 Redis 会话续期。
        // 会话 TTL 即续期窗口上限：会话一旦过期，任何旧 token 都无法再续期。
        TenantJwtClaims claims = parsePermissiveClaims(authorization, requestId);
        TenantSessionStateVO previous = requireCurrentSession(claims.userId(), requestId);
        User user = activeUser(claims.userId(), requestId);

        // 会话身份必须一致才允许续期，防止用「已切租户 / 已重新登录」的旧 token 无限续期。
        if (!java.util.Objects.equals(claims.sessionId(), previous.getSessionId())) {
            throw error(401, "AUTH_INVALID", "会话已失效", requestId);
        }
        if (!java.util.Objects.equals(claims.tenantId(), previous.getCurrentTenantId())) {
            throw error(401, "TENANT_SESSION_STALE", "租户会话已失效", requestId);
        }

        List<TenantAccess> activeTenants = activeTenantsFor(user.getId());
        TenantAccess current = findTenant(activeTenants, previous.getCurrentTenantId());
        if (previous.getCurrentTenantId() != null && current == null) {
            throw error(401, "TENANT_SESSION_STALE", "租户会话已失效", requestId);
        }

        // session 身份（sessionId + version）保持不变，仅重签 token 并续期会话
        long version = previous.getSessionVersion() == null ? 1L : previous.getSessionVersion();
        TenantSessionStateVO next = sessionState(user.getId(), current, version, previous.getSessionId());
        tenantSessionStore.save(next, sessionTtl());
        return sessionResponse(user, activeTenants, current, next);
    }

    @Override
    public UserLoginVO switchTenant(String authorization, TenantSwitchDTO request, String requestId) {
        TenantRequestContext context = authenticate(authorization, requestId);
        if (request == null || request.getTenantId() == null) {
            throw error(400, "REQUEST_INVALID", "tenant_id is required", requestId);
        }

        TenantMember member = tenantMemberMapper.findByUserAndTenantId(
                context.userId(), request.getTenantId());
        Tenant tenant = tenantMapper.selectById(request.getTenantId());
        if (member == null || !ACTIVE.equals(member.getStatus())
                || tenant == null || !ACTIVE.equals(tenant.getStatus())) {
            throw error(404, "TENANT_NOT_FOUND", "租户不存在", requestId);
        }

        User user = activeUser(context.userId(), requestId);
        List<TenantAccess> activeTenants = activeTenantsFor(user.getId());
        TenantAccess current = findTenant(activeTenants, tenant.getId());
        if (current == null) {
            throw error(404, "TENANT_NOT_FOUND", "租户不存在", requestId);
        }
        TenantSessionStateVO previous = requireCurrentSession(user.getId(), requestId);
        TenantSessionStateVO next = sessionState(user.getId(), current, nextVersion(previous), UUID.randomUUID().toString());
        tenantSessionStore.save(next, sessionTtl());
        return sessionResponse(user, activeTenants, current, next);
    }

    @Override
    public void logout(String authorization, String requestId) {
        TenantRequestContext context = authenticate(authorization, requestId);
        tenantSessionStore.deleteByUserId(context.userId());
    }

    @Override
    public TenantRequestContext authenticate(String authorization, String requestId) {
        String normalizedRequestId = normalizeRequestId(requestId);
        String token = bearerToken(authorization, normalizedRequestId);
        TenantJwtClaims claims;
        try {
            claims = jwtTokenService.parse(token);
        } catch (RuntimeException ex) {
            throw error(401, "AUTH_INVALID", "token 无效", normalizedRequestId);
        }

        TenantSessionStateVO session = requireCurrentSession(claims.userId(), normalizedRequestId);
        if (!sameSession(claims, session)) {
            throw error(401, "TENANT_SESSION_STALE", "租户会话已失效", normalizedRequestId);
        }

        User user = activeUser(claims.userId(), normalizedRequestId);
        boolean platformAdmin = isPlatformAdmin(user);
        if (claims.tenantId() == null) {
            return new TenantRequestContext(
                    user.getId(), null, null, claims.sessionVersion(), normalizedRequestId, platformAdmin);
        }

        Tenant tenant = tenantMapper.selectById(claims.tenantId());
        TenantMember member = tenantMemberMapper.findByUserAndTenantId(
                claims.userId(), claims.tenantId());
        if (tenant == null || !ACTIVE.equals(tenant.getStatus())
                || member == null || !ACTIVE.equals(member.getStatus())) {
            throw error(401, "TENANT_SESSION_STALE", "租户会话已失效", normalizedRequestId);
        }
        return new TenantRequestContext(
                user.getId(), tenant.getId(), member.getRole(), claims.sessionVersion(), normalizedRequestId,
                platformAdmin);
    }

    private User activeUser(Long userId, String requestId) {
        User user = userMapper.selectById(userId);
        ensureActiveUser(user, requestId);
        return user;
    }

    private void ensureActiveUser(User user, String requestId) {
        if (user == null || user.getId() == null || !Integer.valueOf(1).equals(user.getStatus())) {
            throw error(401, "AUTH_INVALID", "用户会话已失效", requestId);
        }
    }

    private Optional<TenantSessionStateVO> currentSession(Long userId) {
        Optional<TenantSessionStateVO> session = tenantSessionStore.findByUserId(userId);
        return session == null ? Optional.empty() : session;
    }

    private TenantSessionStateVO requireCurrentSession(Long userId, String requestId) {
        return currentSession(userId)
                .orElseThrow(() -> error(401, "AUTH_INVALID", "会话不存在", requestId));
    }

    private List<TenantAccess> activeTenantsFor(Long userId) {
        List<TenantMember> members = tenantMemberMapper.findByUserId(userId);
        if (members == null || members.isEmpty()) {
            return List.of();
        }
        return members.stream()
                .filter(member -> member != null && ACTIVE.equals(member.getStatus()))
                .map(member -> access(member, tenantMapper.selectById(member.getTenantId())))
                .filter(access -> access != null && ACTIVE.equals(access.tenant().getStatus()))
                .toList();
    }

    private TenantAccess access(TenantMember member, Tenant tenant) {
        if (tenant == null || member.getTenantId() == null || !member.getTenantId().equals(tenant.getId())) {
            return null;
        }
        List<String> permissions = TenantAuthorizationService.permissionsFor(member.getRole());
        return new TenantAccess(tenant, member, permissions);
    }

    private TenantAccess chooseCurrentTenant(List<TenantAccess> tenants, TenantSessionStateVO previous) {
        if (previous != null && previous.getCurrentTenantId() != null) {
            TenantAccess previousTenant = findTenant(tenants, previous.getCurrentTenantId());
            if (previousTenant != null) {
                return previousTenant;
            }
        }
        return tenants.isEmpty() ? null : tenants.get(0);
    }

    private TenantAccess findTenant(List<TenantAccess> tenants, Long tenantId) {
        if (tenantId == null) {
            return null;
        }
        return tenants.stream()
                .filter(access -> tenantId.equals(access.tenant().getId()))
                .findFirst()
                .orElse(null);
    }

    private TenantSessionStateVO sessionState(Long userId, TenantAccess current, long version, String sessionId) {
        return TenantSessionStateVO.builder()
                .userId(userId)
                .currentTenantId(current == null ? null : current.tenant().getId())
                .role(current == null ? "" : current.member().getRole())
                .permissions(current == null ? List.of() : current.permissions())
                .sessionVersion(version)
                .sessionId(sessionId)
                .build();
    }

    /** 续期窗口上限：access token 可有 24h，但会话只需在 7 天内续期一次即可维持登录。 */
    private Duration sessionTtl() {
        return Duration.ofMillis(jwtTokenService.getSessionTtlMillis());
    }

    /** 解析 refresh 用的 bearer token，容忍「签名有效、仅 exp 过期」；签名/格式错误仍抛 401。 */
    private TenantJwtClaims parsePermissiveClaims(String authorization, String requestId) {
        String normalizedRequestId = normalizeRequestId(requestId);
        String token = bearerToken(authorization, normalizedRequestId);
        try {
            return jwtTokenService.parsePermissive(token);
        } catch (RuntimeException ex) {
            throw error(401, "AUTH_INVALID", "token 无效", normalizedRequestId);
        }
    }

    private UserLoginVO sessionResponse(
            User user,
            List<TenantAccess> activeTenants,
            TenantAccess current,
            TenantSessionStateVO session
    ) {
        String token = jwtTokenService.issue(
                user.getId(),
                session.getCurrentTenantId(),
                session.getRole(),
                session.getPermissions(),
                session.getSessionVersion(),
                session.getSessionId()
        );
        List<TenantSummaryVO> summaries = activeTenants.stream()
                .map(access -> summary(access, current != null && access.tenant().getId().equals(current.tenant().getId())))
                .toList();
        return UserLoginVO.builder()
                .token(token)
                .tokenType("Bearer")
                .expiresIn(jwtTokenService.getExpiresInSeconds())
                .sessionVersion(session.getSessionVersion())
                .userInfo(UserInfoVO.builder()
                        .id(user.getId())
                        .username(user.getUsername())
                        .realName(user.getRealName())
                        .isPlatformAdmin(isPlatformAdmin(user))
                        .build())
                .currentTenant(current == null ? null : summary(current, true))
                .tenants(summaries)
                .build();
    }

    private TenantSummaryVO summary(TenantAccess access, boolean current) {
        return TenantSummaryVO.builder()
                .tenantId(access.tenant().getId())
                .tenantCode(access.tenant().getTenantCode())
                .name(access.tenant().getName())
                .status(access.tenant().getStatus())
                .role(access.member().getRole())
                .permissions(access.permissions())
                .current(current)
                .build();
    }

    private static long nextVersion(TenantSessionStateVO previous) {
        if (previous == null || previous.getSessionVersion() == null || previous.getSessionVersion() < 1) {
            return 1L;
        }
        return previous.getSessionVersion() + 1L;
    }

    private static boolean sameSession(TenantJwtClaims claims, TenantSessionStateVO session) {
        if (session.getSessionVersion() == null || claims.sessionVersion() != session.getSessionVersion()) {
            return false;
        }
        if (!java.util.Objects.equals(claims.tenantId(), session.getCurrentTenantId())) {
            return false;
        }
        return session.getSessionId() == null || java.util.Objects.equals(claims.sessionId(), session.getSessionId());
    }

    private static String bearerToken(String authorization, String requestId) {
        if (!StringUtils.hasText(authorization)) {
            throw error(401, "AUTH_REQUIRED", "缺少 Bearer token", requestId);
        }
        if (!authorization.startsWith("Bearer ")) {
            throw error(401, "AUTH_INVALID", "Bearer token 格式无效", requestId);
        }
        String token = authorization.substring("Bearer ".length()).trim();
        if (token.isEmpty()) {
            throw error(401, "AUTH_INVALID", "Bearer token 格式无效", requestId);
        }
        return token;
    }

    private static String normalizeRequestId(String requestId) {
        if (StringUtils.hasText(requestId) && requestId.length() <= 64) {
            return requestId;
        }
        return newRequestId();
    }

    private static String newRequestId() {
        return UUID.randomUUID().toString();
    }

    private static TenantAuthException error(int status, String code, String message, String requestId) {
        return new TenantAuthException(status, code, message, normalizeRequestId(requestId), Map.of());
    }

    private static boolean isPlatformAdmin(User user) {
        return user != null && Boolean.TRUE.equals(user.getIsPlatformAdmin());
    }

    private record TenantAccess(Tenant tenant, TenantMember member, List<String> permissions) {
    }
}
