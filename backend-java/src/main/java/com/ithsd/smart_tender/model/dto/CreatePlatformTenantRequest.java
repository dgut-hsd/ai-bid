package com.ithsd.smart_tender.model.dto;

import com.fasterxml.jackson.annotation.JsonProperty;
import jakarta.validation.constraints.NotBlank;
import jakarta.validation.constraints.Pattern;
import jakarta.validation.constraints.Size;
import lombok.Data;

import java.io.Serializable;

/**
 * 平台管理员创建企业请求。
 * 支持两种初始 OWNER 指定方式：
 * <ul>
 *   <li>新建账号：给 owner_username / owner_password / owner_real_name；</li>
 *   <li>绑定已有全局用户：给 owner_user_id（可选，后续支持）。</li>
 * </ul>
 */
@Data
public class CreatePlatformTenantRequest implements Serializable {

    @NotBlank
    @Size(max = 128)
    private String name;

    @JsonProperty("tenant_code")
    @Pattern(regexp = "^[a-z0-9][a-z0-9_-]{2,63}$")
    private String tenantCode;

    @JsonProperty("plan_code")
    private String planCode;

    @JsonProperty("owner_user_id")
    private Long ownerUserId;

    @JsonProperty("owner_username")
    private String ownerUsername;

    @JsonProperty("owner_password")
    private String ownerPassword;

    @JsonProperty("owner_real_name")
    private String ownerRealName;
}