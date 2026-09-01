package com.ithsd.smart_tender.service.impl;

import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantJwtClaims;
import com.ithsd.smart_tender.common.util.JwtTokenService;
import com.ithsd.smart_tender.common.util.JwtUtil;
import com.ithsd.smart_tender.config.JwtProperties;
import com.ithsd.smart_tender.mapper.TenantMapper;
import com.ithsd.smart_tender.mapper.TenantMemberMapper;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.dto.UserLoginDTO;
import com.ithsd.smart_tender.model.dto.TenantSwitchDTO;
import com.ithsd.smart_tender.model.entity.Tenant;
import com.ithsd.smart_tender.model.entity.TenantMember;
import com.ithsd.smart_tender.model.entity.User;
import com.ithsd.smart_tender.model.vo.UserLoginVO;
import com.ithsd.smart_tender.model.vo.TenantSessionStateVO;
import com.ithsd.smart_tender.service.TenantAuthService;
import com.ithsd.smart_tender.service.TenantSessionStore;
import com.ithsd.smart_tender.service.UserService;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;

import java.time.LocalDateTime;
import java.util.List;
import java.util.Map;
import java.util.Optional;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

@ExtendWith(MockitoExtension.class)
class TenantAuthServiceImplTest {

    @Mock
    private UserService userService;
    @Mock
    private UserMapper userMapper;
    @Mock
    private TenantMapper tenantMapper;
    @Mock
    private TenantMemberMapper tenantMemberMapper;
    @Mock
    private TenantSessionStore tenantSessionStore;

    private TenantAuthService tenantAuthService;
    private JwtTokenService jwtTokenService;

    @BeforeEach
    void setUp() {
        JwtProperties properties = new JwtProperties();
        properties.setSecret("test-secret-key-for-tenant-authentication");
        properties.setTtlMillis(86_400_000L);
        jwtTokenService = new JwtTokenService(properties);
        tenantAuthService = new TenantAuthServiceImpl(
                userService,
                userMapper,
                tenantMapper,
                tenantMemberMapper,
                tenantSessionStore,
                jwtTokenService
        );
    }

    @Test
    void login_shouldCreateCurrentTenantSessionAndReturnContractSnapshot() {
        UserLoginDTO request = new UserLoginDTO();
        request.setUsername("alice");
        request.setPassword("password");

        User user = User.builder()
                .id(10001L)
                .username("alice")
                .realName("Alice")
                .phone("13800138000")
                .status(1)
                .build();
        Tenant tenant = Tenant.builder()
                .id(20001L)
                .tenantCode("acme-bid")
                .name("Acme 招标团队")
                .status("ACTIVE")
                .ownerUserId(user.getId())
                .createdAt(LocalDateTime.now())
                .updatedAt(LocalDateTime.now())
                .build();
        TenantMember member = TenantMember.builder()
                .id(30001L)
                .tenantId(tenant.getId())
                .userId(user.getId())
                .role("OWNER")
                .status("ACTIVE")
                .joinedAt(LocalDateTime.now())
                .build();

        when(userService.login(request)).thenReturn(user);
        when(tenantMemberMapper.findByUserId(user.getId())).thenReturn(List.of(member));
        when(tenantMapper.selectById(tenant.getId())).thenReturn(tenant);
        when(tenantSessionStore.findByUserId(user.getId())).thenReturn(Optional.empty());

        UserLoginVO response = tenantAuthService.login(request);

        assertThat(response.getToken()).isNotBlank();
        assertThat(response.getTokenType()).isEqualTo("Bearer");
        assertThat(response.getExpiresIn()).isEqualTo(86_400L);
        assertThat(response.getSessionVersion()).isEqualTo(1L);
        assertThat(response.getUserInfo().getId()).isEqualTo(user.getId());
        assertThat(response.getUserInfo().getUsername()).isEqualTo("alice");
        assertThat(response.getCurrentTenant().getTenantId()).isEqualTo(tenant.getId());
        assertThat(response.getCurrentTenant().getRole()).isEqualTo("OWNER");
        assertThat(response.getCurrentTenant().getPermissions())
                .contains("tenant.read", "tenant.settings.write", "tender.write");
        assertThat(response.getTenants()).hasSize(1);
        assertThat(response.getTenants().get(0).isCurrent()).isTrue();

        TenantJwtClaims claims = jwtTokenService.parse(response.getToken());
        assertThat(claims.userId()).isEqualTo(user.getId());
        assertThat(claims.tenantId()).isEqualTo(tenant.getId());
        assertThat(claims.role()).isEqualTo("OWNER");
        assertThat(claims.sessionVersion()).isEqualTo(1L);

        var savedSession = org.mockito.ArgumentCaptor.forClass(TenantSessionStateVO.class);
        verify(tenantSessionStore).save(savedSession.capture(), any());
        assertThat(savedSession.getValue().getUserId()).isEqualTo(user.getId());
        assertThat(savedSession.getValue().getCurrentTenantId()).isEqualTo(tenant.getId());
        assertThat(savedSession.getValue().getSessionVersion()).isEqualTo(1L);
        assertThat(savedSession.getValue().getRole()).isEqualTo("OWNER");
    }

    @Test
    void refresh_shouldRevalidateStateAndInvalidatePreviousVersion() {
        User user = activeUser();
        Tenant tenant = tenant(20001L, "acme-bid", "ACTIVE");
        TenantMember member = member(30001L, tenant.getId(), user.getId(), "ADMIN", "ACTIVE");
        TenantSessionStateVO previous = session(user.getId(), tenant.getId(), 7L, "session-old", "ADMIN");
        String oldToken = jwtTokenService.issue(
                user.getId(), tenant.getId(), "ADMIN", List.of("tenant.read"), 7L, "session-old");

        when(tenantSessionStore.findByUserId(user.getId())).thenReturn(Optional.of(previous));
        when(userMapper.selectById(user.getId())).thenReturn(user);
        when(tenantMapper.selectById(tenant.getId())).thenReturn(tenant);
        when(tenantMemberMapper.findByUserAndTenantId(user.getId(), tenant.getId())).thenReturn(member);
        when(tenantMemberMapper.findByUserId(user.getId())).thenReturn(List.of(member));

        UserLoginVO response = tenantAuthService.refresh("Bearer " + oldToken, "request-refresh");

        assertThat(response.getSessionVersion()).isEqualTo(8L);
        TenantJwtClaims refreshedClaims = jwtTokenService.parse(response.getToken());
        assertThat(refreshedClaims.sessionVersion()).isEqualTo(8L);
        assertThat(refreshedClaims.tenantId()).isEqualTo(tenant.getId());

        var savedSession = org.mockito.ArgumentCaptor.forClass(TenantSessionStateVO.class);
        verify(tenantSessionStore).save(savedSession.capture(), any());
        assertThat(savedSession.getValue().getSessionVersion()).isEqualTo(8L);
        assertThat(savedSession.getValue().getSessionId()).isNotEqualTo("session-old");
    }

    @Test
    void switchTenant_shouldOnlyAllowActiveMembershipAndRotateSession() {
        User user = activeUser();
        Tenant firstTenant = tenant(20001L, "first", "ACTIVE");
        Tenant secondTenant = tenant(20002L, "second", "ACTIVE");
        TenantMember firstMember = member(30001L, firstTenant.getId(), user.getId(), "ADMIN", "ACTIVE");
        TenantMember secondMember = member(30002L, secondTenant.getId(), user.getId(), "MEMBER", "ACTIVE");
        TenantSessionStateVO previous = session(user.getId(), firstTenant.getId(), 3L, "session-first", "ADMIN");
        String oldToken = jwtTokenService.issue(
                user.getId(), firstTenant.getId(), "ADMIN", List.of("tenant.read"), 3L, "session-first");

        when(tenantSessionStore.findByUserId(user.getId())).thenReturn(Optional.of(previous));
        when(userMapper.selectById(user.getId())).thenReturn(user);
        when(tenantMapper.selectById(firstTenant.getId())).thenReturn(firstTenant);
        when(tenantMapper.selectById(secondTenant.getId())).thenReturn(secondTenant);
        when(tenantMemberMapper.findByUserAndTenantId(user.getId(), firstTenant.getId())).thenReturn(firstMember);
        when(tenantMemberMapper.findByUserAndTenantId(user.getId(), secondTenant.getId())).thenReturn(secondMember);
        when(tenantMemberMapper.findByUserId(user.getId())).thenReturn(List.of(firstMember, secondMember));

        TenantSwitchDTO request = new TenantSwitchDTO();
        request.setTenantId(secondTenant.getId());
        UserLoginVO response = tenantAuthService.switchTenant(
                "Bearer " + oldToken, request, "request-switch");

        assertThat(response.getSessionVersion()).isEqualTo(4L);
        assertThat(response.getCurrentTenant().getTenantId()).isEqualTo(secondTenant.getId());
        assertThat(response.getCurrentTenant().getRole()).isEqualTo("MEMBER");
        TenantJwtClaims switchedClaims = jwtTokenService.parse(response.getToken());
        assertThat(switchedClaims.tenantId()).isEqualTo(secondTenant.getId());
        assertThat(switchedClaims.sessionVersion()).isEqualTo(4L);

        var savedSession = org.mockito.ArgumentCaptor.forClass(TenantSessionStateVO.class);
        verify(tenantSessionStore).save(savedSession.capture(), any());
        when(tenantSessionStore.findByUserId(user.getId())).thenReturn(Optional.of(savedSession.getValue()));
        assertThatThrownBy(() -> tenantAuthService.authenticate("Bearer " + oldToken, "request-old"))
                .isInstanceOf(TenantAuthException.class)
                .satisfies(error -> assertThat(((TenantAuthException) error).getErrorCode())
                        .isEqualTo("TENANT_SESSION_STALE"));
    }

    @Test
    void switchTenant_shouldHideDisabledTargetTenant() {
        User user = activeUser();
        Tenant currentTenant = tenant(20001L, "first", "ACTIVE");
        Tenant disabledTenant = tenant(20002L, "disabled", "DISABLED");
        TenantMember currentMember = member(30001L, currentTenant.getId(), user.getId(), "ADMIN", "ACTIVE");
        TenantMember disabledMember = member(30002L, disabledTenant.getId(), user.getId(), "MEMBER", "ACTIVE");
        TenantSessionStateVO previous = session(user.getId(), currentTenant.getId(), 1L, "session", "ADMIN");
        String token = jwtTokenService.issue(
                user.getId(), currentTenant.getId(), "ADMIN", List.of("tenant.read"), 1L, "session");

        when(tenantSessionStore.findByUserId(user.getId())).thenReturn(Optional.of(previous));
        when(userMapper.selectById(user.getId())).thenReturn(user);
        when(tenantMapper.selectById(currentTenant.getId())).thenReturn(currentTenant);
        when(tenantMapper.selectById(disabledTenant.getId())).thenReturn(disabledTenant);
        when(tenantMemberMapper.findByUserAndTenantId(user.getId(), currentTenant.getId())).thenReturn(currentMember);
        when(tenantMemberMapper.findByUserAndTenantId(user.getId(), disabledTenant.getId())).thenReturn(disabledMember);

        TenantSwitchDTO request = new TenantSwitchDTO();
        request.setTenantId(disabledTenant.getId());

        assertThatThrownBy(() -> tenantAuthService.switchTenant("Bearer " + token, request, "request-switch"))
                .isInstanceOf(TenantAuthException.class)
                .satisfies(error -> {
                    TenantAuthException authError = (TenantAuthException) error;
                    assertThat(authError.getStatus()).isEqualTo(404);
                    assertThat(authError.getErrorCode()).isEqualTo("TENANT_NOT_FOUND");
                });
    }

    @Test
    void login_withoutActiveTenant_shouldReturnEmptyTenantSnapshot() {
        User user = activeUser();
        Tenant disabledTenant = tenant(20001L, "disabled", "DISABLED");
        TenantMember disabledMember = member(30001L, disabledTenant.getId(), user.getId(), "OWNER", "ACTIVE");
        UserLoginDTO request = new UserLoginDTO();
        request.setUsername("alice");
        request.setPassword("password");

        when(userService.login(request)).thenReturn(user);
        when(tenantMemberMapper.findByUserId(user.getId())).thenReturn(List.of(disabledMember));
        when(tenantMapper.selectById(disabledTenant.getId())).thenReturn(disabledTenant);
        when(tenantSessionStore.findByUserId(user.getId())).thenReturn(Optional.empty());

        UserLoginVO response = tenantAuthService.login(request);

        assertThat(response.getCurrentTenant()).isNull();
        assertThat(response.getTenants()).isEmpty();
        assertThat(response.getSessionVersion()).isEqualTo(1L);
        TenantJwtClaims claims = jwtTokenService.parse(response.getToken());
        assertThat(claims.tenantId()).isNull();
        assertThat(claims.role()).isEmpty();
    }

    @Test
    void authenticate_shouldRejectRedisSessionVersionMismatch() {
        User user = activeUser();
        Tenant tenant = tenant(20001L, "acme", "ACTIVE");
        TenantMember member = member(30001L, tenant.getId(), user.getId(), "ADMIN", "ACTIVE");
        TenantSessionStateVO current = session(user.getId(), tenant.getId(), 9L, "current", "ADMIN");
        String staleToken = jwtTokenService.issue(
                user.getId(), tenant.getId(), "ADMIN", List.of("tenant.read"), 8L, "stale");
        when(tenantSessionStore.findByUserId(user.getId())).thenReturn(Optional.of(current));

        assertThatThrownBy(() -> tenantAuthService.authenticate("Bearer " + staleToken, "request-stale"))
                .isInstanceOf(TenantAuthException.class)
                .satisfies(error -> {
                    TenantAuthException authError = (TenantAuthException) error;
                    assertThat(authError.getStatus()).isEqualTo(401);
                    assertThat(authError.getErrorCode()).isEqualTo("TENANT_SESSION_STALE");
                });
    }

    @Test
    void authenticate_shouldRejectTokenWithMissingTenantClaim() {
        String token = JwtUtil.createJWT(
                "test-secret-key-for-tenant-authentication",
                86_400_000L,
                Map.of("sub", "10001", "user_id", "10001", "role", "ADMIN", "session_version", 1L));

        assertThatThrownBy(() -> tenantAuthService.authenticate("Bearer " + token, "request-invalid"))
                .isInstanceOf(TenantAuthException.class)
                .satisfies(error -> assertThat(((TenantAuthException) error).getErrorCode())
                        .isEqualTo("AUTH_INVALID"));
    }

    @Test
    void authenticate_shouldRejectInactiveMembershipAsStaleSession() {
        User user = activeUser();
        Tenant tenant = tenant(20001L, "acme", "ACTIVE");
        TenantMember suspended = member(30001L, tenant.getId(), user.getId(), "ADMIN", "SUSPENDED");
        TenantSessionStateVO current = session(user.getId(), tenant.getId(), 1L, "current", "ADMIN");
        String token = jwtTokenService.issue(
                user.getId(), tenant.getId(), "ADMIN", List.of("tenant.read"), 1L, "current");
        when(tenantSessionStore.findByUserId(user.getId())).thenReturn(Optional.of(current));
        when(userMapper.selectById(user.getId())).thenReturn(user);
        when(tenantMapper.selectById(tenant.getId())).thenReturn(tenant);
        when(tenantMemberMapper.findByUserAndTenantId(user.getId(), tenant.getId())).thenReturn(suspended);

        assertThatThrownBy(() -> tenantAuthService.authenticate("Bearer " + token, "request-suspended"))
                .isInstanceOf(TenantAuthException.class)
                .satisfies(error -> assertThat(((TenantAuthException) error).getErrorCode())
                        .isEqualTo("TENANT_SESSION_STALE"));
    }

    @Test
    void logout_shouldDeleteOnlyTheCurrentUserSession() {
        User user = activeUser();
        Tenant tenant = tenant(20001L, "acme", "ACTIVE");
        TenantMember member = member(30001L, tenant.getId(), user.getId(), "ADMIN", "ACTIVE");
        TenantSessionStateVO current = session(user.getId(), tenant.getId(), 1L, "current", "ADMIN");
        String token = jwtTokenService.issue(
                user.getId(), tenant.getId(), "ADMIN", List.of("tenant.read"), 1L, "current");
        when(tenantSessionStore.findByUserId(user.getId())).thenReturn(Optional.of(current));
        when(userMapper.selectById(user.getId())).thenReturn(user);
        when(tenantMapper.selectById(tenant.getId())).thenReturn(tenant);
        when(tenantMemberMapper.findByUserAndTenantId(user.getId(), tenant.getId())).thenReturn(member);

        tenantAuthService.logout("Bearer " + token, "request-logout");

        verify(tenantSessionStore).deleteByUserId(user.getId());
    }

    private User activeUser() {
        return User.builder().id(10001L).username("alice").realName("Alice").status(1).build();
    }

    private Tenant tenant(Long id, String code, String status) {
        return Tenant.builder()
                .id(id)
                .tenantCode(code)
                .name(code)
                .status(status)
                .ownerUserId(10001L)
                .build();
    }

    private TenantMember member(Long id, Long tenantId, Long userId, String role, String status) {
        return TenantMember.builder()
                .id(id)
                .tenantId(tenantId)
                .userId(userId)
                .role(role)
                .status(status)
                .build();
    }

    private TenantSessionStateVO session(
            Long userId,
            Long tenantId,
            Long version,
            String sessionId,
            String role
    ) {
        return TenantSessionStateVO.builder()
                .userId(userId)
                .currentTenantId(tenantId)
                .sessionVersion(version)
                .sessionId(sessionId)
                .role(role)
                .permissions(List.of("tenant.read"))
                .build();
    }
}
