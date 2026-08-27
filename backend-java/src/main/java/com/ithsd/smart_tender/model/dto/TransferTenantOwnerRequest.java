package com.ithsd.smart_tender.model.dto;

import com.fasterxml.jackson.annotation.JsonProperty;
import jakarta.validation.constraints.NotNull;
import lombok.Data;

import java.io.Serializable;

/** 平台管理员转移企业所有权的请求。 */
@Data
public class TransferTenantOwnerRequest implements Serializable {

    /** 新 OWNER 的全局用户 ID，必须是该企业的 ACTIVE 成员。 */
    @JsonProperty("target_user_id")
    @NotNull
    private Long targetUserId;
}