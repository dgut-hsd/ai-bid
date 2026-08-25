package com.ithsd.smart_tender.model.dto;

import jakarta.validation.constraints.NotBlank;
import jakarta.validation.constraints.Size;
import lombok.Data;

import java.io.Serializable;

/** 管理员创建用户（建号即加入当前企业租户）的请求。 */
@Data
public class AdminCreateUserRequest implements Serializable {

    /** 登录账号，对应 sys_user.username，全局唯一 */
    @NotBlank
    @Size(min = 3, max = 50)
    private String username;

    @NotBlank
    @Size(min = 6, max = 100)
    private String password;

    @NotBlank
    @Size(min = 1, max = 50)
    private String realName;

    /** 角色：OWNER 或 MEMBER */
    @NotBlank
    private String role;
}