package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.mapper.TenantMapper;
import com.ithsd.smart_tender.mapper.TenantMemberMapper;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.dto.UserLoginDTO;
import com.ithsd.smart_tender.model.dto.UserRegisterDTO;
import com.ithsd.smart_tender.model.entity.Tenant;
import com.ithsd.smart_tender.model.entity.TenantMember;
import com.ithsd.smart_tender.model.entity.User;
import com.ithsd.smart_tender.service.UserService;
import com.ithsd.smart_tender.common.BizException;
import com.ithsd.smart_tender.common.util.MD5Util;
import lombok.RequiredArgsConstructor;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;

import java.time.LocalDateTime;

@Service
@RequiredArgsConstructor
public class UserServiceImpl implements UserService {

    private final UserMapper userMapper;
    private final TenantMapper tenantMapper;
    private final TenantMemberMapper tenantMemberMapper;

    @Override
    public User login(UserLoginDTO userLoginDTO) {
        LambdaQueryWrapper<User> wrapper = new LambdaQueryWrapper<>();
        wrapper.eq(User::getPhone, userLoginDTO.getPhone());
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
    @Transactional
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

        // 注册后自动创建个人租户 + OWNER 成员，对齐 V6 回填逻辑
        // 避免新用户因无租户导致所有 TenantScope.requiredTenantId() 调用失败
        createPersonalTenant(user);
    }

    /**
     * 为新注册用户创建个人工作空间（对齐 db/migration/V6__backfill_tenant_data.sql 的模式）。
     * 幂等：如果租户已存在（极少数竞态或其他途径已建），跳过创建。
     */
    private void createPersonalTenant(User user) {
        String tenantCode = "user-" + user.getId();
        Tenant existing = tenantMapper.selectOne(
                new LambdaQueryWrapper<Tenant>().eq(Tenant::getTenantCode, tenantCode));
        if (existing != null) {
            return;
        }

        LocalDateTime now = LocalDateTime.now();
        Tenant tenant = Tenant.builder()
                .tenantCode(tenantCode)
                .name("Personal workspace " + user.getId())
                .status("ACTIVE")
                .ownerUserId(user.getId())
                .planCode("STANDARD")
                .version(0L)
                .createdAt(now)
                .updatedAt(now)
                .build();
        tenantMapper.insert(tenant);

        TenantMember member = TenantMember.builder()
                .tenantId(tenant.getId())
                .userId(user.getId())
                .role("OWNER")
                .status("ACTIVE")
                .joinedAt(now)
                .build();
        tenantMemberMapper.insert(member);
    }
}
