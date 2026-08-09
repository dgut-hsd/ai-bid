package com.ithsd.smart_tender.model.dto;

import com.fasterxml.jackson.annotation.JsonProperty;
import jakarta.validation.constraints.NotBlank;
import jakarta.validation.constraints.Pattern;
import jakarta.validation.constraints.Size;
import lombok.AllArgsConstructor;
import lombok.Data;
import lombok.NoArgsConstructor;

import java.util.Map;

@Data
@NoArgsConstructor
@AllArgsConstructor
public class CreateTenantRequest {

    @NotBlank
    @Size(max = 128)
    private String name;

    @JsonProperty("tenant_code")
    @Pattern(regexp = "^[a-z0-9][a-z0-9_-]{2,63}$")
    private String tenantCode;

    @JsonProperty("plan_code")
    private String planCode;

    private Map<String, Object> settings;

    /** Accepted for compatibility but deliberately ignored for authorization. */
    @JsonProperty("tenant_id")
    private Long tenantId;
}
