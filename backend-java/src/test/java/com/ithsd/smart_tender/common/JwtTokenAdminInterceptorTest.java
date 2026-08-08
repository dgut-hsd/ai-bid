package com.ithsd.smart_tender.common;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.ithsd.smart_tender.service.TenantAuthService;
import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpServletResponse;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;
import org.springframework.mock.web.MockHttpServletRequest;
import org.springframework.mock.web.MockHttpServletResponse;
import org.springframework.web.method.HandlerMethod;

import java.lang.reflect.Method;

import static org.assertj.core.api.Assertions.assertThat;
import static org.mockito.ArgumentMatchers.anyString;
import static org.mockito.Mockito.mock;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

class JwtTokenAdminInterceptorTest {

    private final TenantAuthService authService = mock(TenantAuthService.class);
    private final JwtTokenAdminInterceptor interceptor =
            new JwtTokenAdminInterceptor(authService, new ObjectMapper());

    @AfterEach
    void cleanUp() {
        TenantContext.clear();
        BaseContext.removeCurrentId();
    }

    @Test
    void validRequest_shouldInstallImmutableTenantContextAndCleanAfterCompletion() throws Exception {
        TenantRequestContext context = new TenantRequestContext(10001L, 20001L, "ADMIN", 4L, "request-1");
        when(authService.authenticate("Bearer token", "request-1")).thenReturn(context);
        MockHttpServletRequest request = request("Bearer token", "request-1");
        MockHttpServletResponse response = new MockHttpServletResponse();
        HandlerMethod handler = handlerMethod();

        assertThat(interceptor.preHandle(request, response, handler)).isTrue();
        assertThat(TenantContext.get()).isEqualTo(context);
        assertThat(BaseContext.getCurrentId()).isEqualTo(10001L);
        assertThat(response.getHeader("X-Request-Id")).isEqualTo("request-1");

        interceptor.afterCompletion(request, response, handler, null);

        assertThat(TenantContext.get()).isNull();
        assertThat(BaseContext.getCurrentId()).isNull();
    }

    @Test
    void invalidRequest_shouldReturnContractErrorAndCleanFailurePath() throws Exception {
        when(authService.authenticate(anyString(), anyString()))
                .thenThrow(new TenantAuthException(401, "TENANT_SESSION_STALE", "stale", "request-2"));
        MockHttpServletRequest request = request("Bearer stale-token", "request-2");
        MockHttpServletResponse response = new MockHttpServletResponse();

        assertThat(interceptor.preHandle(request, response, handlerMethod())).isFalse();

        assertThat(response.getStatus()).isEqualTo(401);
        assertThat(response.getContentAsString()).contains("TENANT_SESSION_STALE", "request-2");
        assertThat(TenantContext.get()).isNull();
        assertThat(BaseContext.getCurrentId()).isNull();
        verify(authService).authenticate("Bearer stale-token", "request-2");
    }

    @Test
    void missingRequestId_shouldGenerateAndPropagateRequestId() throws Exception {
        when(authService.authenticate(anyString(), anyString())).thenAnswer(invocation ->
                new TenantRequestContext(10001L, null, null, 1L, invocation.getArgument(1)));
        MockHttpServletRequest request = request("Bearer token", null);
        MockHttpServletResponse response = new MockHttpServletResponse();

        assertThat(interceptor.preHandle(request, response, handlerMethod())).isTrue();

        String generated = response.getHeader("X-Request-Id");
        assertThat(generated).isNotBlank().hasSize(36);
        assertThat(TenantContext.get().requestId()).isEqualTo(generated);

        interceptor.afterCompletion(request, response, handlerMethod(), null);
    }

    private MockHttpServletRequest request(String authorization, String requestId) {
        MockHttpServletRequest request = new MockHttpServletRequest();
        request.setRequestURI("/api/auth/refresh");
        request.addHeader("Authorization", authorization);
        if (requestId != null) {
            request.addHeader("X-Request-Id", requestId);
        }
        return request;
    }

    private HandlerMethod handlerMethod() throws NoSuchMethodException {
        Method method = TestHandler.class.getDeclaredMethod("handle");
        return new HandlerMethod(new TestHandler(), method);
    }

    private static final class TestHandler {
        @SuppressWarnings("unused")
        public void handle() {
        }
    }
}
