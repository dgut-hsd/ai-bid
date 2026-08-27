package com.ithsd.smart_tender.model.dto;

import com.fasterxml.jackson.annotation.JsonProperty;
import jakarta.validation.constraints.NotBlank;
import jakarta.validation.constraints.Size;
import lombok.Data;

import java.io.Serializable;

/** 企业 OWNER 创建用户（建号并加入当前企业）的请求。 */
@Data
public class EnterpriseCreateUserRequest implements Serializable {

    /** 登录账号，对应 sys_user.username，全局唯一 */
    @NotBlank
    @Size(min = 3, max = 50)
    private String username;

    @NotBlank
    @Size(min = 6, max = 100)
    private String password;

    @JsonProperty("real_name")
    @NotBlank
    @Size(min = 1, max = 50)
    @JsonProperty("real_name")
    private String realName;

    /** 角色：ADMIN / MEMBER（不允许 OWNER，OWNER 只能平台分配/转移） */
    @NotBlank
    private String role;
}