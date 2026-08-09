package com.ithsd.smart_tender.common;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.ithsd.smart_tender.model.result.Result;
import com.ithsd.smart_tender.service.TenantAuthService;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.stereotype.Component;
import org.springframework.web.method.HandlerMethod;
import org.springframework.web.servlet.HandlerInterceptor;
import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpServletResponse;

import java.io.IOException;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.UUID;

@Component
public class JwtTokenAdminInterceptor implements HandlerInterceptor {

    public static final String REQUEST_ID_ATTRIBUTE = JwtTokenAdminInterceptor.class.getName() + ".requestId";

    private final TenantAuthService tenantAuthService;
    private final ObjectMapper objectMapper;

    @Autowired
    public JwtTokenAdminInterceptor(TenantAuthService tenantAuthService, ObjectMapper objectMapper) {
        this.tenantAuthService = tenantAuthService;
        this.objectMapper = objectMapper;
    }

    public JwtTokenAdminInterceptor(TenantAuthService tenantAuthService) {
        this(tenantAuthService, new ObjectMapper());
    }

    @Override
    public boolean preHandle(HttpServletRequest request, HttpServletResponse response, Object handler) throws Exception {
        if (!(handler instanceof HandlerMethod)) {
            return true;
        }

        String requestId = requestId(request.getHeader("X-Request-Id"));
        request.setAttribute(REQUEST_ID_ATTRIBUTE, requestId);
        response.setHeader("X-Request-Id", requestId);

        try {
            TenantRequestContext context = tenantAuthService.authenticate(
                    request.getHeader("Authorization"), requestId);
            TenantContext.set(context);
            return true;
        } catch (TenantAuthException ex) {
            clearContext();
            writeError(response, ex);
            return false;
        } catch (RuntimeException ex) {
            clearContext();
            writeError(response, new TenantAuthException(
                    401, "AUTH_INVALID", "token 无效", requestId));
            return false;
        }
    }

    @Override
    public void afterCompletion(HttpServletRequest request, HttpServletResponse response, Object handler, Exception ex) throws Exception {
        clearContext();
    }

    private void writeError(HttpServletResponse response, TenantAuthException ex) {
        response.setStatus(ex.getStatus());
        response.setCharacterEncoding("UTF-8");
        response.setContentType("application/json;charset=UTF-8");

        Map<String, Object> errorData = new LinkedHashMap<>();
        errorData.put("error_code", ex.getErrorCode());
        errorData.put("request_id", ex.getRequestId());
        if (!ex.getDetails().isEmpty()) {
            errorData.put("details", ex.getDetails());
        }
        Result<Map<String, Object>> result = Result.error(ex.getStatus(), ex.getMessage());
        result.setData(errorData);
        try {
            objectMapper.writeValue(response.getWriter(), result);
        } catch (IOException ignored) {
            // The status code remains available if a client disconnects while writing.
        }
    }

    private static String requestId(String candidate) {
        return candidate != null && !candidate.isBlank() && candidate.length() <= 64
                ? candidate
                : UUID.randomUUID().toString();
    }

    private static void clearContext() {
        TenantContext.clear();
    }
}
