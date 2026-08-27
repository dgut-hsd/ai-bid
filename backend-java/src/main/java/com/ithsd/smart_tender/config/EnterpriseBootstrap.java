package com.ithsd.smart_tender.config;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.common.util.PasswordService;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.entity.User;
import lombok.RequiredArgsConstructor;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.boot.ApplicationArguments;
import org.springframework.boot.ApplicationRunner;
import org.springframework.stereotype.Component;

import java.time.LocalDateTime;
import java.time.ZoneOffset;

/**
 * 平台管理员（系统管理者）初始化。
 *
 * <p>首次启动时只播种一个 {@code is_platform_admin=1} 的系统管理者账号，供其登录
 * 后进入「系统管理」页管理所有企业。企业（租户）不再由启动器自动创建，而是由平台
 * 管理员在页面上创建；企业 OWNER 再由企业在「企业管理」页创建本企业用户。</p>
 *
 * <p>全部操作幂等：账号已存在时不会重复创建，也不会覆盖已有密码。初始密码来自配置
 * （默认 platform-admin/123456），生产安装必须通过
 * {@code AIBID_PLATFORM_ADMIN_PASSWORD} 覆盖并在首次登录后立即修改。</p>
 */
@Component
@RequiredArgsConstructor
public class EnterpriseBootstrap implements ApplicationRunner {

    private static final Logger log = LoggerFactory.getLogger(EnterpriseBootstrap.class);

    private final UserMapper userMapper;
    private final PasswordService passwordService;

    @Value("${app.bootstrap.enabled:true}")
    private boolean enabled;
    @Value("${app.bootstrap.platform-username:platform-admin}")
    private String platformUsername;
    @Value("${app.bootstrap.platform-password:123456}")
    private String platformPassword;
    @Value("${app.bootstrap.platform-realname:系统管理员}")
    private String platformRealName;

    @Override
    public void run(ApplicationArguments args) {
        if (!enabled) {
            return;
        }
        try {
            bootstrapPlatformAdmin();
        } catch (Exception ex) {
            log.error("平台管理员初始化失败（系统仍可启动，但可能缺少系统管理者账号）：{}",
                    ex.getMessage(), ex);
        }
    }

    private void bootstrapPlatformAdmin() {
        User existing = userMapper.selectOne(
                new LambdaQueryWrapper<User>().eq(User::getUsername, platformUsername));
        if (existing != null) {
            return;
        }
        LocalDateTime now = LocalDateTime.now(ZoneOffset.UTC);
        User platformAdmin = User.builder()
                .username(platformUsername)
                .password(passwordService.encode(platformPassword))
                .realName(platformRealName)
                .status(1)
                .isPlatformAdmin(true)
                .createTime(now)
                .updateTime(now)
                .build();
        userMapper.insert(platformAdmin);
        log.info("已创建平台管理员账号「{}」（is_platform_admin=1），请登录后立即修改密码",
                platformUsername);
    }
}