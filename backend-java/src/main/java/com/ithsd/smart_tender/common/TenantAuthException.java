package com.ithsd.smart_tender.common;

import java.util.Map;

/** Stable error used by the authentication and tenant-session boundary. */
public class TenantAuthException extends RuntimeException {

    private final int status;
    private final String errorCode;
    private final String requestId;
    private final Map<String, Object> details;

    public TenantAuthException(int status, String errorCode, String message, String requestId) {
        this(status, errorCode, message, requestId, Map.of());
    }

    public TenantAuthException(
            int status,
            String errorCode,
            String message,
            String requestId,
            Map<String, Object> details
    ) {
        super(message);
        this.status = status;
        this.errorCode = errorCode;
        this.requestId = requestId;
        this.details = details == null ? Map.of() : Map.copyOf(details);
    }

    public int getStatus() {
        return status;
    }

    public String getErrorCode() {
        return errorCode;
    }

    public String getRequestId() {
        return requestId;
    }

    public Map<String, Object> getDetails() {
        return details;
    }
}
