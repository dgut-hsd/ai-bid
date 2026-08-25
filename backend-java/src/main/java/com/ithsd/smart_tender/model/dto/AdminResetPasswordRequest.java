package com.ithsd.smart_tender.model.dto;

import jakarta.validation.constraints.NotBlank;
import jakarta.validation.constraints.Size;
import lombok.Data;

import java.io.Serializable;

/** 管理员重置用户密码的请求。 */
@Data
public class AdminResetPasswordRequest implements Serializable {

    @NotBlank
    @Size(min = 6, max = 100)
    private String password;
}