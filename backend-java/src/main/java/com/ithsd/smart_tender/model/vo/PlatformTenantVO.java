package com.ithsd.smart_tender.model.vo;

import com.fasterxml.jackson.annotation.JsonProperty;
import lombok.AllArgsConstructor;
import lombok.Builder;
import lombok.Data;
import lombok.NoArgsConstructor;

import java.io.Serializable;
import java.time.LocalDateTime;

/** 平台管理员视角的企业（租户）信息。 */
@Data
@Builder
@NoArgsConstructor
@AllArgsConstructor
public class PlatformTenantVO implements Serializable {

    @JsonProperty("tenant_id")
    private Long tenantId;
    @JsonProperty("tenant_code")
    private String tenantCode;
    private String name;
    private String status;
    @JsonProperty("plan_code")
    private String planCode;
    @JsonProperty("owner_user_id")
    private Long ownerUserId;
    @JsonProperty("owner_username")
    private String ownerUsername;
    @JsonProperty("owner_real_name")
    private String ownerRealName;
    @JsonProperty("member_count")
    private Long memberCount;
    private Long version;
    @JsonProperty("created_at")
    private LocalDateTime createdAt;
    @JsonProperty("updated_at")
    private LocalDateTime updatedAt;
}