-- ============================================================================
-- ai-bid 数据库手工迁移（一次性补丁，用于重建库或新环境）
-- 应用对象：MySQL smart_tender_system 库（容器 aib-mysql）
-- 应用方式：
--   P=$(grep -E '^MYSQL_ROOT_PASSWORD=' deploy/.env | cut -d= -f2-)
--   docker exec -i -e MYSQL_PWD="$P" aib-mysql mysql -uroot \
--       --default-character-set=utf8mb4 smart_tender_system < deploy/db-migrations/001-audit-consistency.sql
--
-- 说明：生产库已在 2026-08-27 手动执行过等价语句，重复执行下面带 DDL 的部分
--       会因「列/索引已存在」报错，属预期——仅需在新库/新环境执行一次。
-- ============================================================================

-- 1) 审核报告正文扩容：54+ 问题的报告正文超 64KB(text) 上限导致
--    "Data too long" 500。MEDIUMTEXT 上限 16MB。
ALTER TABLE audit_report MODIFY COLUMN doc_content MEDIUMTEXT NULL COMMENT '审核报告正文(DOCX JSON/HTML)';

-- 2) audit_issue 增量落库去重键（P2）：
--    risk_id 存 Rust 原始风险 ID（如 R_105），issue_no 仍为展示号 ISSUE-R_105。
ALTER TABLE audit_issue ADD COLUMN risk_id VARCHAR(64) NULL COMMENT 'Rust 风险原始ID' AFTER issue_no;

-- 唯一键保证 (tenant, audit, risk) 维度 upsert；NULL risk_id 不参与唯一约束
-- （MySQL 唯一索引允许多个 NULL），历史行不受影响。
ALTER TABLE audit_issue ADD UNIQUE KEY uk_audit_issue_risk (tenant_id, audit_id, risk_id);

-- 3) 既有行回填 risk_id（issue_no 形如 ISSUE-R_105 → R_105），
--    修复 DB 兜底重建时的 "ISSUE-ISSUE-R_xxx" 双前缀。
UPDATE audit_issue
   SET risk_id = SUBSTRING(issue_no, 7)
 WHERE risk_id IS NULL
   AND issue_no LIKE 'ISSUE-%';