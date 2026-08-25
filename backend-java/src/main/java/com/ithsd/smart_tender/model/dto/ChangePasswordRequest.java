package com.ithsd.smart_tender.model.dto;

import com.fasterxml.jackson.annotation.JsonProperty;
import jakarta.validation.constraints.NotBlank;
import jakarta.validation.constraints.Size;
import lombok.Data;

import java.io.Serializable;

/** 用户本人修改密码的请求。 */
@Data
public class ChangePasswordRequest implements Serializable {

    @JsonProperty("old_password")
    @NotBlank
    private String oldPassword;

    @JsonProperty("new_password")
    @NotBlank
    @Size(min = 6, max = 100)
    private String newPassword;
}