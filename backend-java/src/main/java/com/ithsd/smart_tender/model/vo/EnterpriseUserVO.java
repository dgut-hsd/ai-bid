package com.ithsd.smart_tender.model.vo;

import com.fasterxml.jackson.annotation.JsonProperty;
import lombok.AllArgsConstructor;
import lombok.Builder;
import lombok.Data;
import lombok.NoArgsConstructor;

import java.io.Serializable;
import java.time.LocalDateTime;

/** 企业管理后台用户列表项。 */
@Data
@Builder
@NoArgsConstructor
@AllArgsConstructor
public class EnterpriseUserVO implements Serializable {

    @JsonProperty("user_id")
    private Long userId;
    private String username;
    @JsonProperty("real_name")
    private String realName;
    /** 租户角色：OWNER / ADMIN / MEMBER */
    private String role;
    /** 成员状态：ACTIVE / SUSPENDED */
    private String status;
    @JsonProperty("member_id")
    private Long memberId;
    @JsonProperty("created_at")
    private LocalDateTime createdAt;
}