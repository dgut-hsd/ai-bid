package com.ithsd.smart_tender.model.vo;

import com.fasterxml.jackson.annotation.JsonProperty;
import lombok.AllArgsConstructor;
import lombok.Builder;
import lombok.Data;
import lombok.NoArgsConstructor;

import java.io.Serializable;
import java.util.List;

/** Redis-backed authoritative session state; the JWT is only its signed snapshot. */
@Data
@Builder
@NoArgsConstructor
@AllArgsConstructor
public class TenantSessionStateVO implements Serializable {
    @JsonProperty("user_id")
    private Long userId;
    @JsonProperty("current_tenant_id")
    private Long currentTenantId;
    private String role;
    private List<String> permissions;
    @JsonProperty("session_version")
    private Long sessionVersion;
    @JsonProperty("session_id")
    private String sessionId;
}
