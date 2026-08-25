package com.ithsd.smart_tender.controller;

import com.ithsd.smart_tender.common.GlobalExceptionHandler;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.model.dto.TenantSwitchDTO;
import com.ithsd.smart_tender.model.vo.UserLoginVO;
import com.ithsd.smart_tender.service.TenantAuthService;
import com.ithsd.smart_tender.service.UserService;
import org.junit.jupiter.api.Test;
import org.springframework.http.MediaType;
import org.springframework.test.web.servlet.MockMvc;
import org.springframework.test.web.servlet.setup.MockMvcBuilders;

import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.mock;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.post;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.jsonPath;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.status;

class UserControllerTest {

    private final TenantAuthService tenantAuthService = mock(TenantAuthService.class);
    private final UserService userService = mock(UserService.class);
    private final MockMvc mvc = MockMvcBuilders
            .standaloneSetup(new UserController(tenantAuthService, userService))
            .setControllerAdvice(new GlobalExceptionHandler())
            .build();

    @Test
    void switchTenant_shouldExposeNewAuthSessionAndPassServerRequestId() throws Exception {
        UserLoginVO response = UserLoginVO.builder()
                .token("new-token")
                .tokenType("Bearer")
                .sessionVersion(8L)
                .build();
        when(tenantAuthService.switchTenant(any(), any(TenantSwitchDTO.class), any())).thenReturn(response);

        mvc.perform(post("/api/auth/switch-tenant")
                        .header("Authorization", "Bearer old-token")
                        .header("X-Request-Id", "request-switch")
                        .contentType(MediaType.APPLICATION_JSON)
                        .content("{\"tenant_id\":20002}"))
                .andExpect(status().isOk())
                .andExpect(jsonPath("$.data.token").value("new-token"))
                .andExpect(jsonPath("$.data.session_version").value(8));

        verify(tenantAuthService).switchTenant(
                org.mockito.ArgumentMatchers.eq("Bearer old-token"),
                any(TenantSwitchDTO.class),
                org.mockito.ArgumentMatchers.eq("request-switch"));
    }

    @Test
    void tenantAuthException_shouldSetHttpStatusAndStableErrorBody() throws Exception {
        when(tenantAuthService.switchTenant(any(), any(TenantSwitchDTO.class), any()))
                .thenThrow(new TenantAuthException(
                        401, "TENANT_SESSION_STALE", "租户会话已失效", "request-stale"));

        mvc.perform(post("/api/auth/switch-tenant")
                        .header("Authorization", "Bearer stale-token")
                        .header("X-Request-Id", "request-stale")
                        .contentType(MediaType.APPLICATION_JSON)
                        .content("{\"tenant_id\":20002}"))
                .andExpect(status().isUnauthorized())
                .andExpect(jsonPath("$.code").value(401))
                .andExpect(jsonPath("$.data.error_code").value("TENANT_SESSION_STALE"))
                .andExpect(jsonPath("$.data.request_id").value("request-stale"));
    }
}
