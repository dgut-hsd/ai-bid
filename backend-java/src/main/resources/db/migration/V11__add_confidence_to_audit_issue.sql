-- 审核完成后 GET /result 从 audit_issue 回退读取时置信度会丢失（此前表内无该列，
-- toIssue 落库未存、getResult DB 回退映射也未回填），导致前端问题列表"置信度"恒为 0%。
ALTER TABLE `audit_issue`
  ADD COLUMN `confidence` double DEFAULT NULL COMMENT '置信度 [0,1]' AFTER `reference`;