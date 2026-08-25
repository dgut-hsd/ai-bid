package com.ithsd.smart_tender.model.vo;

import com.fasterxml.jackson.annotation.JsonProperty;
import lombok.AllArgsConstructor;
import lombok.Builder;
import lombok.Data;
import lombok.NoArgsConstructor;

import java.io.Serializable;
import java.time.LocalDateTime;

/** 管理后台用户列表项。 */
@Data
@Builder
@NoArgsConstructor
@AllArgsConstructor
public class AdminUserVO implements Serializable {

    @JsonProperty("user_id")
    private Long userId;
    private String username;
    @JsonProperty("real_name")
    private String realName;
    /** OWNER 或 MEMBER */
    private String role;
    /** 账号状态：ACTIVE / DISABLED */
    private String status;
    @JsonProperty("member_id")
    private Long memberId;
    @JsonProperty("created_at")
    private LocalDateTime createdAt;
}