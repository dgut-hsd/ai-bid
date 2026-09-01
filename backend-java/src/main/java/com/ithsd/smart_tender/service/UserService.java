package com.ithsd.smart_tender.service;

import com.ithsd.smart_tender.model.dto.UserLoginDTO;
import com.ithsd.smart_tender.model.entity.User;

public interface UserService {
    User login(UserLoginDTO userLoginDTO);

    /** 用户本人修改密码：校验旧密码，设置新密码，并使旧会话失效。 */
    void changePassword(Long userId, String oldPassword, String newPassword);
}
