package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.dto.UserLoginDTO;
import com.ithsd.smart_tender.model.dto.UserRegisterDTO;
import com.ithsd.smart_tender.model.entity.User;
import com.ithsd.smart_tender.service.UserService;
import com.ithsd.smart_tender.service.TenantSessionStore;
import com.ithsd.smart_tender.common.BizException;
import com.ithsd.smart_tender.common.util.MD5Util;
import lombok.RequiredArgsConstructor;
import org.springframework.stereotype.Service;
import java.time.LocalDateTime;

@Service
@RequiredArgsConstructor
public class UserServiceImpl implements UserService {

    private final UserMapper userMapper;
    private final TenantSessionStore tenantSessionStore;

    @Override
    public User login(UserLoginDTO userLoginDTO) {
        LambdaQueryWrapper<User> wrapper = new LambdaQueryWrapper<>();
        wrapper.eq(User::getUsername, userLoginDTO.getUsername());
        User user = userMapper.selectOne(wrapper);
        
        if (user == null) {
             throw new RuntimeException("账号或密码错误");
        }
        
        String password = userLoginDTO.getPassword();
        password = MD5Util.encrypt(password);

        if (!user.getPassword().equals(password)) {
            throw new RuntimeException("账号或密码错误");
        }
        if (user.getStatus() == 0) {
            throw new RuntimeException("账户已被禁用");
        }
        return user;
    }

    @Override
    public void register(UserRegisterDTO userRegisterDTO) {
        LambdaQueryWrapper<User> wrapper = new LambdaQueryWrapper<>();
        wrapper.eq(User::getUsername, userRegisterDTO.getUsername());
        User existUser = userMapper.selectOne(wrapper);
        
        if (existUser != null) {
            throw new BizException("用户名已存在");
        }

        LambdaQueryWrapper<User> phoneWrapper = new LambdaQueryWrapper<>();
        phoneWrapper.eq(User::getPhone, userRegisterDTO.getPhone());
        User existPhone = userMapper.selectOne(phoneWrapper);
        if (existPhone != null) {
             throw new BizException("手机号已存在");
        }

        String password = userRegisterDTO.getPassword();
        password = MD5Util.encrypt(password);

        User user = User.builder()
                .username(userRegisterDTO.getUsername())
                .password(password)
                .realName(userRegisterDTO.getRealName())
                .email(userRegisterDTO.getEmail())
                .phone(userRegisterDTO.getPhone())
                .status(1)
                .createTime(LocalDateTime.now())
                .updateTime(LocalDateTime.now())
                .build();
        userMapper.insert(user);
    }

    @Override
    public void changePassword(Long userId, String oldPassword, String newPassword) {
        User user = userMapper.selectById(userId);
        if (user == null) {
            throw new BizException(404, "用户不存在");
        }
        if (!user.getPassword().equals(MD5Util.encrypt(oldPassword))) {
            throw new BizException(400, "原密码错误");
        }
        user.setPassword(MD5Util.encrypt(newPassword));
        user.setUpdateTime(LocalDateTime.now());
        userMapper.updateById(user);
        // 使旧会话失效，改密后需重新登录
        tenantSessionStore.deleteByUserId(userId);
    }
}
