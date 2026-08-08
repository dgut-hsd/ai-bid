package com.ithsd.smart_tender.model.vo;

import com.fasterxml.jackson.annotation.JsonProperty;
import lombok.AllArgsConstructor;
import lombok.Builder;
import lombok.Data;
import lombok.NoArgsConstructor;

import java.io.Serializable;
import java.time.LocalDateTime;
import java.util.List;
import java.util.Map;

@Data
@Builder
@NoArgsConstructor
@AllArgsConstructor
public class TenantVO implements Serializable {

    @JsonProperty("tenant_id")
    private Long tenantId;
    @JsonProperty("tenant_code")
    private String tenantCode;
    private String name;
    private String status;
    @JsonProperty("owner_user_id")
    private Long ownerUserId;
    @JsonProperty("plan_code")
    private String planCode;
    private Map<String, Object> settings;
    private Long version;
    @JsonProperty("created_at")
    private LocalDateTime createdAt;
    @JsonProperty("updated_at")
    private LocalDateTime updatedAt;
    private String role;
    private List<String> permissions;
    @JsonProperty("is_current")
    private boolean current;
}
