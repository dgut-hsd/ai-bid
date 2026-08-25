package com.ithsd.smart_tender.config;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.common.util.MD5Util;
import com.ithsd.smart_tender.mapper.TenantMapper;
import com.ithsd.smart_tender.mapper.TenantMemberMapper;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.entity.Tenant;
import com.ithsd.smart_tender.model.entity.TenantMember;
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
 * 私有化单企业部署的安装初始化。
 *
 * <p>首次启动时自动创建「财务部门」企业租户和初始 OWNER 管理员账号，使系统
 * 安装后即可登录后台管理用户。全部操作幂等：已存在时不会重复创建。</p>
 *
 * <p>初始管理员密码来自配置（默认 admin/admin123），生产安装必须通过环境变量
 * 覆盖并在首次登录后立即修改。</p>
 */
@Component
@RequiredArgsConstructor
public class EnterpriseBootstrap implements ApplicationRunner {

    private static final Logger log = LoggerFactory.getLogger(EnterpriseBootstrap.class);

    private final TenantMapper tenantMapper;
    private final TenantMemberMapper tenantMemberMapper;
    private final UserMapper userMapper;

    @Value("${app.bootstrap.enabled:true}")
    private boolean enabled;
    @Value("${app.bootstrap.tenant-code:finance}")
    private String tenantCode;
    @Value("${app.bootstrap.tenant-name:财务部门}")
    private String tenantName;
    @Value("${app.bootstrap.admin-username:admin}")
    private String adminUsername;
    @Value("${app.bootstrap.admin-password:admin123}")
    private String adminPassword;
    @Value("${app.bootstrap.admin-realname:系统管理员}")
    private String adminRealName;

    @Override
    public void run(ApplicationArguments args) {
        if (!enabled) {
            return;
        }
        try {
            bootstrap();
        } catch (Exception ex) {
            log.error("企业初始化失败（系统仍可启动，但可能缺少初始租户/管理员）：{}", ex.getMessage(), ex);
        }
    }

    private void bootstrap() {
        // 1. 初始管理员账号（幂等）
        User admin = userMapper.selectOne(
                new LambdaQueryWrapper<User>().eq(User::getUsername, adminUsername));
        if (admin == null) {
            LocalDateTime now = LocalDateTime.now(ZoneOffset.UTC);
            admin = User.builder()
                    .username(adminUsername)
                    .password(MD5Util.encrypt(adminPassword))
                    .realName(adminRealName)
                    .status(1)
                    .createTime(now)
                    .updateTime(now)
                    .build();
            userMapper.insert(admin);
            log.info("已创建初始管理员账号「{}」，请登录后立即修改密码", adminUsername);
        }

        // 2. 企业租户（幂等，owner 指向初始管理员）
        Tenant tenant = tenantMapper.selectOne(
                new LambdaQueryWrapper<Tenant>().eq(Tenant::getTenantCode, tenantCode));
        if (tenant == null) {
            LocalDateTime now = LocalDateTime.now(ZoneOffset.UTC);
            tenant = Tenant.builder()
                    .tenantCode(tenantCode)
                    .name(tenantName)
                    .status("ACTIVE")
                    .ownerUserId(admin.getId())
                    .planCode("STANDARD")
                    .version(0L)
                    .createdAt(now)
                    .updatedAt(now)
                    .build();
            tenantMapper.insert(tenant);
            log.info("已创建企业租户「{}」（tenant_code={}）", tenantName, tenantCode);
        }

        // 3. 初始管理员的 OWNER 成员关系（幂等）
        Long memberCount = tenantMemberMapper.selectCount(
                new LambdaQueryWrapper<TenantMember>()
                        .eq(TenantMember::getTenantId, tenant.getId())
                        .eq(TenantMember::getUserId, admin.getId()));
        if (memberCount == null || memberCount == 0) {
            TenantMember owner = TenantMember.builder()
                    .tenantId(tenant.getId())
                    .userId(admin.getId())
                    .role("OWNER")
                    .status("ACTIVE")
                    .joinedAt(LocalDateTime.now(ZoneOffset.UTC))
                    .build();
            tenantMemberMapper.insert(owner);
            log.info("已将「{}」设为企业租户 OWNER", adminUsername);
        }
    }
}