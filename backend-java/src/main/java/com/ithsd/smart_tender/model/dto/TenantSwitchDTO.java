package com.ithsd.smart_tender.model.dto;

import com.fasterxml.jackson.annotation.JsonProperty;
import lombok.Data;

import java.io.Serializable;

@Data
public class TenantSwitchDTO implements Serializable {
    @JsonProperty("tenant_id")
    private Long tenantId;
}
