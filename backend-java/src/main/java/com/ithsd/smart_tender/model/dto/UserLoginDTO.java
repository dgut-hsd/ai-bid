package com.ithsd.smart_tender.model.dto;

import lombok.Data;
import java.io.Serializable;

@Data
public class UserLoginDTO implements Serializable {
    /** 登录账号，对应 sys_user.username */
    private String username;
    private String password;
}