-- Manual-only rollback for a disposable pre-backfill environment.
-- Normal incident rollback is app-level: disable tenant.enforce and any tenant
-- gray rollout, pause tenant-sensitive async/Rust consumers, and retain the
-- Expand columns/tables and any tenant data. Do not run this through Flyway.
--
-- The default NO guard is intentional. Set the variable to YES only after
-- confirming that dual-write/enforce are off, no tenant rows were created, all
-- resource tenant_id values are NULL, and a restorable schema backup exists.

SET @tenant_expand_rollback_confirmed = COALESCE(@tenant_expand_rollback_confirmed, 'NO');

DELIMITER $$

DROP PROCEDURE IF EXISTS `tenant_migration_rollback_optional_trace_preflight`$$
CREATE PROCEDURE `tenant_migration_rollback_optional_trace_preflight`(
  IN p_table_name VARCHAR(64),
  IN p_index_name VARCHAR(64)
)
BEGIN
  IF EXISTS (
    SELECT 1
      FROM information_schema.tables
     WHERE table_schema = DATABASE()
       AND table_name = p_table_name
       AND table_type = 'BASE TABLE'
  ) AND EXISTS (
    SELECT 1
      FROM information_schema.columns
     WHERE table_schema = DATABASE()
       AND table_name = p_table_name
       AND column_name = 'tenant_id'
  ) THEN
    SET @tenant_migration_trace_non_null = 0;
    SET @tenant_migration_ddl = CONCAT(
      'SELECT COUNT(*) INTO @tenant_migration_trace_non_null FROM `',
      p_table_name, '` WHERE `tenant_id` IS NOT NULL'
    );
    PREPARE tenant_migration_stmt FROM @tenant_migration_ddl;
    EXECUTE tenant_migration_stmt;
    DEALLOCATE PREPARE tenant_migration_stmt;

    IF @tenant_migration_trace_non_null > 0 THEN
      SIGNAL SQLSTATE '45000'
        SET MESSAGE_TEXT = 'Trace tenant data exists. Retain the optional Expand schema.';
    END IF;

  END IF;
END$$

DROP PROCEDURE IF EXISTS `tenant_migration_rollback_optional_trace_drop`$$
CREATE PROCEDURE `tenant_migration_rollback_optional_trace_drop`(
  IN p_table_name VARCHAR(64),
  IN p_index_name VARCHAR(64)
)
BEGIN
  IF EXISTS (
    SELECT 1
      FROM information_schema.tables
     WHERE table_schema = DATABASE()
       AND table_name = p_table_name
       AND table_type = 'BASE TABLE'
  ) AND EXISTS (
    SELECT 1
      FROM information_schema.columns
     WHERE table_schema = DATABASE()
       AND table_name = p_table_name
       AND column_name = 'tenant_id'
  ) THEN
    IF EXISTS (
      SELECT 1
        FROM information_schema.statistics
       WHERE table_schema = DATABASE()
         AND table_name = p_table_name
         AND index_name = p_index_name
    ) THEN
      SET @tenant_migration_ddl = CONCAT(
        'ALTER TABLE `', p_table_name,
        '` DROP INDEX `', p_index_name, '`'
      );
      PREPARE tenant_migration_stmt FROM @tenant_migration_ddl;
      EXECUTE tenant_migration_stmt;
      DEALLOCATE PREPARE tenant_migration_stmt;
    END IF;

    SET @tenant_migration_ddl = CONCAT(
      'ALTER TABLE `', p_table_name, '` DROP COLUMN `tenant_id`'
    );
    PREPARE tenant_migration_stmt FROM @tenant_migration_ddl;
    EXECUTE tenant_migration_stmt;
    DEALLOCATE PREPARE tenant_migration_stmt;
  END IF;
END$$

DROP PROCEDURE IF EXISTS `tenant_migration_rollback_expand`$$
CREATE PROCEDURE `tenant_migration_rollback_expand`()
BEGIN
  IF COALESCE(@tenant_expand_rollback_confirmed, 'NO') <> 'YES' THEN
    SIGNAL SQLSTATE '45000'
      SET MESSAGE_TEXT = 'Rollback is disabled. Set @tenant_expand_rollback_confirmed = YES after the pre-backfill checks.';
  END IF;

  IF (
    SELECT COUNT(*)
      FROM information_schema.tables
     WHERE table_schema = DATABASE()
       AND table_type = 'BASE TABLE'
       AND table_name IN (
         'tenant', 'tenant_member', 'tenant_invitation', 'tenant_audit_log'
       )
  ) <> 4 THEN
    SIGNAL SQLSTATE '45000'
      SET MESSAGE_TEXT = 'Rollback schema preflight failed: tenant tables are missing.';
  END IF;

  IF (
    SELECT COUNT(DISTINCT c.table_name)
      FROM information_schema.columns AS c
     WHERE c.table_schema = DATABASE()
       AND c.column_name = 'tenant_id'
       AND c.table_name IN (
         'project', 'bid_document', 'audit_task', 'audit_issue', 'audit_report',
         'audit_task_event', 'knowledge_file', 'knowledge_chunk', 'chat_message',
         'document_parse_job', 'rag_trigger_outbox'
       )
  ) <> 11 THEN
    SIGNAL SQLSTATE '45000'
      SET MESSAGE_TEXT = 'Rollback schema preflight failed: required tenant_id columns are missing.';
  END IF;

  IF (
    SELECT COUNT(DISTINCT CONCAT(s.table_name, ':', s.index_name))
      FROM information_schema.statistics AS s
     WHERE s.table_schema = DATABASE()
       AND (
         (s.table_name = 'project' AND s.index_name = 'idx_project_tenant_id_user_id')
         OR (s.table_name = 'bid_document' AND s.index_name = 'idx_bid_document_tenant_id_project_id')
         OR (s.table_name = 'audit_task' AND s.index_name = 'idx_audit_task_tenant_id_bid_id')
         OR (s.table_name = 'audit_issue' AND s.index_name = 'idx_audit_issue_tenant_id_audit_id')
         OR (s.table_name = 'audit_report' AND s.index_name = 'idx_audit_report_tenant_id_audit_id')
         OR (s.table_name = 'audit_task_event' AND s.index_name = 'idx_audit_task_event_tenant_id_task_id_id')
         OR (s.table_name = 'knowledge_file' AND s.index_name = 'idx_knowledge_file_tenant_id_upload_time_id')
         OR (s.table_name = 'knowledge_chunk' AND s.index_name = 'idx_knowledge_chunk_tenant_id_file_id_id')
         OR (s.table_name = 'chat_message' AND s.index_name = 'idx_chat_message_tenant_id_project_id_id')
         OR (s.table_name = 'document_parse_job' AND s.index_name = 'idx_document_parse_job_tenant_id_file_id_id')
         OR (s.table_name = 'rag_trigger_outbox' AND s.index_name = 'idx_rag_trigger_outbox_tenant_id_file_id_id')
       )
  ) <> 11 THEN
    SIGNAL SQLSTATE '45000'
      SET MESSAGE_TEXT = 'Rollback schema preflight failed: required tenant indexes are missing.';
  END IF;

  IF EXISTS (SELECT 1 FROM `tenant` LIMIT 1)
     OR EXISTS (SELECT 1 FROM `tenant_member` LIMIT 1)
     OR EXISTS (SELECT 1 FROM `tenant_invitation` LIMIT 1)
     OR EXISTS (SELECT 1 FROM `tenant_audit_log` LIMIT 1) THEN
    SIGNAL SQLSTATE '45000'
      SET MESSAGE_TEXT = 'Tenant data exists. Use feature-flag rollback and retain the Expand schema.';
  END IF;

  IF EXISTS (SELECT 1 FROM `project` WHERE `tenant_id` IS NOT NULL LIMIT 1)
     OR EXISTS (SELECT 1 FROM `bid_document` WHERE `tenant_id` IS NOT NULL LIMIT 1)
     OR EXISTS (SELECT 1 FROM `audit_task` WHERE `tenant_id` IS NOT NULL LIMIT 1)
     OR EXISTS (SELECT 1 FROM `audit_issue` WHERE `tenant_id` IS NOT NULL LIMIT 1)
     OR EXISTS (SELECT 1 FROM `audit_report` WHERE `tenant_id` IS NOT NULL LIMIT 1)
     OR EXISTS (SELECT 1 FROM `audit_task_event` WHERE `tenant_id` IS NOT NULL LIMIT 1)
     OR EXISTS (SELECT 1 FROM `knowledge_file` WHERE `tenant_id` IS NOT NULL LIMIT 1)
     OR EXISTS (SELECT 1 FROM `knowledge_chunk` WHERE `tenant_id` IS NOT NULL LIMIT 1)
     OR EXISTS (SELECT 1 FROM `chat_message` WHERE `tenant_id` IS NOT NULL LIMIT 1)
     OR EXISTS (SELECT 1 FROM `document_parse_job` WHERE `tenant_id` IS NOT NULL LIMIT 1)
     OR EXISTS (SELECT 1 FROM `rag_trigger_outbox` WHERE `tenant_id` IS NOT NULL LIMIT 1) THEN
    SIGNAL SQLSTATE '45000'
      SET MESSAGE_TEXT = 'A resource has a tenant_id. Rollback is unsafe after backfill.';
  END IF;

  CALL `tenant_migration_rollback_optional_trace_preflight`(
    'trace_event_blocks', 'idx_trace_event_blocks_tenant_id_event_id_id'
  );
  CALL `tenant_migration_rollback_optional_trace_preflight`(
    'trace_events', 'idx_trace_events_tenant_id_session_id_id'
  );
  CALL `tenant_migration_rollback_optional_trace_preflight`(
    'trace_sessions', 'idx_trace_sessions_tenant_id_task_id_id'
  );

  CALL `tenant_migration_rollback_optional_trace_drop`(
    'trace_event_blocks', 'idx_trace_event_blocks_tenant_id_event_id_id'
  );
  CALL `tenant_migration_rollback_optional_trace_drop`(
    'trace_events', 'idx_trace_events_tenant_id_session_id_id'
  );
  CALL `tenant_migration_rollback_optional_trace_drop`(
    'trace_sessions', 'idx_trace_sessions_tenant_id_task_id_id'
  );

  ALTER TABLE `rag_trigger_outbox`
    DROP INDEX `idx_rag_trigger_outbox_tenant_id_file_id_id`,
    DROP COLUMN `tenant_id`;
  ALTER TABLE `document_parse_job`
    DROP INDEX `idx_document_parse_job_tenant_id_file_id_id`,
    DROP COLUMN `tenant_id`;
  ALTER TABLE `chat_message`
    DROP INDEX `idx_chat_message_tenant_id_project_id_id`,
    DROP COLUMN `tenant_id`;
  ALTER TABLE `knowledge_chunk`
    DROP INDEX `idx_knowledge_chunk_tenant_id_file_id_id`,
    DROP COLUMN `tenant_id`;
  ALTER TABLE `knowledge_file`
    DROP INDEX `idx_knowledge_file_tenant_id_upload_time_id`,
    DROP COLUMN `tenant_id`;
  ALTER TABLE `audit_task_event`
    DROP INDEX `idx_audit_task_event_tenant_id_task_id_id`,
    DROP COLUMN `tenant_id`;
  ALTER TABLE `audit_report`
    DROP INDEX `idx_audit_report_tenant_id_audit_id`,
    DROP COLUMN `tenant_id`;
  ALTER TABLE `audit_issue`
    DROP INDEX `idx_audit_issue_tenant_id_audit_id`,
    DROP COLUMN `tenant_id`;
  ALTER TABLE `audit_task`
    DROP INDEX `idx_audit_task_tenant_id_bid_id`,
    DROP COLUMN `tenant_id`;
  ALTER TABLE `bid_document`
    DROP INDEX `idx_bid_document_tenant_id_project_id`,
    DROP COLUMN `tenant_id`;
  ALTER TABLE `project`
    DROP INDEX `idx_project_tenant_id_user_id`,
    DROP COLUMN `tenant_id`;

  DROP TABLE `tenant_audit_log`;
  DROP TABLE `tenant_invitation`;
  DROP TABLE `tenant_member`;
  DROP TABLE `tenant`;
END$$

CALL `tenant_migration_rollback_expand`()$$
DROP PROCEDURE IF EXISTS `tenant_migration_rollback_expand`$$
DROP PROCEDURE IF EXISTS `tenant_migration_rollback_optional_trace_drop`$$
DROP PROCEDURE IF EXISTS `tenant_migration_rollback_optional_trace_preflight`$$

DELIMITER ;
