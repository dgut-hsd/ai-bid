-- 角色收敛：将 AUDITOR（审核员）/ VIEWER（只读）合并为 MEMBER（成员）。
-- 企业租户角色最终只保留 OWNER / ADMIN / MEMBER 三种。
UPDATE `tenant_member`
SET `role` = 'MEMBER'
WHERE `role` IN ('AUDITOR', 'VIEWER');