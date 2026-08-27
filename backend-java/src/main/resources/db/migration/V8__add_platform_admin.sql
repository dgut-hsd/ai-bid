-- 平台管理员（系统管理员）标识。
-- 平台管理员是全局标量，与租户成员角色(tenant_member.role)正交：
--   is_platform_admin=1 的用户由「系统管理」页管理所有企业；
--   企业 OWNER/ADMIN 等仍是 tenant_member.role，只在本企业内生效。
ALTER TABLE `sys_user`
    ADD COLUMN `is_platform_admin` TINYINT(1) NOT NULL DEFAULT 0;