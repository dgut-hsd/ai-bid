package com.ithsd.smart_tender.model.vo;

import com.fasterxml.jackson.annotation.JsonProperty;
import lombok.AllArgsConstructor;
import lombok.Builder;
import lombok.Data;
import lombok.NoArgsConstructor;

import java.io.Serializable;
import java.util.List;

@Data
@Builder
@NoArgsConstructor
@AllArgsConstructor
public class TenantSummaryVO implements Serializable {
    @JsonProperty("tenant_id")
    private Long tenantId;
    @JsonProperty("tenant_code")
    private String tenantCode;
    private String name;
    private String status;
    private String role;
    private List<String> permissions;
    @JsonProperty("is_current")
    private boolean current;

    public boolean isCurrent() {
        return current;
    }
}
