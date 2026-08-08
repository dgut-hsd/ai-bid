package com.ithsd.smart_tender.model.vo;

import com.fasterxml.jackson.annotation.JsonProperty;
import lombok.AllArgsConstructor;
import lombok.Builder;
import lombok.Data;
import lombok.NoArgsConstructor;
import java.io.Serializable;

@Data
@Builder
@NoArgsConstructor
@AllArgsConstructor
public class UserLoginVO implements Serializable {
    private String token;
    @JsonProperty("token_type")
    private String tokenType;
    @JsonProperty("expires_in")
    private Long expiresIn;
    @JsonProperty("session_version")
    private Long sessionVersion;
    @JsonProperty("user_info")
    private UserInfoVO userInfo;
    @JsonProperty("current_tenant")
    private TenantSummaryVO currentTenant;
    private java.util.List<TenantSummaryVO> tenants;
}
