package com.ithsd.smart_tender.model.dto;

import com.fasterxml.jackson.annotation.JsonProperty;
import lombok.Data;

import java.io.Serializable;

/** 企业管理员修改用户信息：账号（username）和/或姓名（real_name），至少提供一个。 */
@Data
public class EnterpriseUpdateUserRequest implements Serializable {

    /** 可选；非空时更新账号（3~50 个字符，需全局唯一）。 */
    @JsonProperty("username")
    private String username;

    /** 可选；非空时更新姓名（最长 50 个字符）。 */
    @JsonProperty("real_name")
    private String realName;
}