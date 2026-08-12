-- Expand only. Do not add NOT NULL constraints or application enforcement here.
-- This migration intentionally leaves existing resource rows nullable until V6 is
-- completed and the validation queries in tenant_migration_validation.sql pass.

CREATE TABLE IF NOT EXISTS `tenant` (
  `id` BIGINT NOT NULL AUTO_INCREMENT,
  `tenant_code` VARCHAR(64) NOT NULL,
  `name` VARCHAR(128) NOT NULL,
  `status` VARCHAR(16) NOT NULL,
  `owner_user_id` BIGINT NOT NULL,
  `plan_code` VARCHAR(32) NOT NULL DEFAULT 'STANDARD',
  `settings_json` JSON DEFAULT NULL,
  `version` BIGINT NOT NULL DEFAULT 0,
  `created_at` DATETIME(3) NOT NULL,
  `updated_at` DATETIME(3) NOT NULL,
  `deleted_at` DATETIME(3) DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_tenant_tenant_code` (`tenant_code`),
  KEY `idx_tenant_status_updated_at` (`status`, `updated_at`),
  KEY `idx_tenant_owner_user_id` (`owner_user_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='Tenant isolation domain';

CREATE TABLE IF NOT EXISTS `tenant_member` (
  `id` BIGINT NOT NULL AUTO_INCREMENT,
  `tenant_id` BIGINT NOT NULL,
  `user_id` BIGINT NOT NULL,
  `role` VARCHAR(16) NOT NULL,
  `status` VARCHAR(16) NOT NULL,
  `joined_at` DATETIME(3) NOT NULL,
  `invited_by` BIGINT DEFAULT NULL,
  `last_seen_at` DATETIME(3) DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_tenant_member_tenant_user` (`tenant_id`, `user_id`),
  KEY `idx_tenant_member_user_status` (`user_id`, `status`),
  KEY `idx_tenant_member_tenant_status_role` (`tenant_id`, `status`, `role`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='Tenant membership';

CREATE TABLE IF NOT EXISTS `tenant_invitation` (
  `id` BIGINT NOT NULL AUTO_INCREMENT,
  `tenant_id` BIGINT NOT NULL,
  `email` VARCHAR(320) NOT NULL,
  `role` VARCHAR(16) NOT NULL,
  `token_hash` CHAR(64) NOT NULL,
  `invited_by` BIGINT NOT NULL,
  `expires_at` DATETIME(3) NOT NULL,
  `accepted_at` DATETIME(3) DEFAULT NULL,
  `revoked_at` DATETIME(3) DEFAULT NULL,
  `status` VARCHAR(16) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_tenant_invitation_token_hash` (`token_hash`),
  KEY `idx_tenant_invitation_tenant_status` (`tenant_id`, `status`),
  KEY `idx_tenant_invitation_email_status` (`email`, `status`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='Tenant invitation';

CREATE TABLE IF NOT EXISTS `tenant_audit_log` (
  `id` BIGINT NOT NULL AUTO_INCREMENT,
  `tenant_id` BIGINT NOT NULL,
  `actor_user_id` BIGINT DEFAULT NULL,
  `action` VARCHAR(64) NOT NULL,
  `resource_type` VARCHAR(64) NOT NULL,
  `resource_id` VARCHAR(128) DEFAULT NULL,
  `request_id` VARCHAR(64) NOT NULL,
  `before_json` JSON DEFAULT NULL,
  `after_json` JSON DEFAULT NULL,
  `ip_address` VARCHAR(45) DEFAULT NULL,
  `user_agent` VARCHAR(512) DEFAULT NULL,
  `created_at` DATETIME(3) NOT NULL,
  PRIMARY KEY (`id`),
  KEY `idx_tenant_audit_log_tenant_created_at` (`tenant_id`, `created_at`),
  KEY `idx_tenant_audit_log_actor_created_at` (`actor_user_id`, `created_at`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='Tenant append-only audit log';

DELIMITER $$

DROP PROCEDURE IF EXISTS `tenant_migration_ensure_column`$$
CREATE PROCEDURE `tenant_migration_ensure_column`(IN p_table_name VARCHAR(64))
BEGIN
  IF NOT EXISTS (
    SELECT 1
      FROM information_schema.columns
     WHERE table_schema = DATABASE()
       AND table_name = p_table_name
       AND column_name = 'tenant_id'
  ) THEN
    SET @tenant_migration_ddl = CONCAT(
      'ALTER TABLE `', p_table_name,
      '` ADD COLUMN `tenant_id` BIGINT NULL'
    );
    PREPARE tenant_migration_stmt FROM @tenant_migration_ddl;
    EXECUTE tenant_migration_stmt;
    DEALLOCATE PREPARE tenant_migration_stmt;
  ELSEIF NOT EXISTS (
    SELECT 1
      FROM information_schema.columns
     WHERE table_schema = DATABASE()
       AND table_name = p_table_name
       AND column_name = 'tenant_id'
       AND data_type = 'bigint'
       AND is_nullable = 'YES'
  ) THEN
    SIGNAL SQLSTATE '45000'
      SET MESSAGE_TEXT = 'Existing tenant_id must be nullable BIGINT.';
  END IF;
END$$

DROP PROCEDURE IF EXISTS `tenant_migration_ensure_index`$$
CREATE PROCEDURE `tenant_migration_ensure_index`(
  IN p_table_name VARCHAR(64),
  IN p_index_name VARCHAR(64),
  IN p_index_columns VARCHAR(255)
)
BEGIN
  DECLARE v_index_column_count BIGINT DEFAULT 0;
  DECLARE v_index_columns VARCHAR(255) DEFAULT NULL;

  IF NOT EXISTS (
    SELECT 1
      FROM information_schema.statistics
     WHERE table_schema = DATABASE()
       AND table_name = p_table_name
       AND index_name = p_index_name
  ) THEN
    SET @tenant_migration_ddl = CONCAT(
      'ALTER TABLE `', p_table_name,
      '` ADD INDEX `', p_index_name,
      '` (', p_index_columns, ')'
    );
    PREPARE tenant_migration_stmt FROM @tenant_migration_ddl;
    EXECUTE tenant_migration_stmt;
    DEALLOCATE PREPARE tenant_migration_stmt;
  ELSE
    SELECT
      COUNT(*),
      GROUP_CONCAT(
        CONCAT('`', s.column_name, '`')
        ORDER BY s.seq_in_index
        SEPARATOR ', '
      )
    INTO v_index_column_count, v_index_columns
    FROM information_schema.statistics AS s
    WHERE s.table_schema = DATABASE()
      AND s.table_name = p_table_name
      AND s.index_name = p_index_name;

    IF v_index_column_count <> (
         LENGTH(p_index_columns)
         - LENGTH(REPLACE(p_index_columns, ',', ''))
         + 1
       )
       OR COALESCE(v_index_columns, '') <> p_index_columns THEN
      SIGNAL SQLSTATE '45000'
        SET MESSAGE_TEXT = 'Existing tenant index has unexpected columns.';
    END IF;
  END IF;
END$$

-- Trace tables are created by the optional trace_schema.sql initializer. The
-- required V1 resource calls below remain unguarded and must fail if missing.
DROP PROCEDURE IF EXISTS `tenant_migration_ensure_optional_column`$$
CREATE PROCEDURE `tenant_migration_ensure_optional_column`(IN p_table_name VARCHAR(64))
BEGIN
  IF EXISTS (
    SELECT 1
      FROM information_schema.tables
     WHERE table_schema = DATABASE()
       AND table_name = p_table_name
       AND table_type = 'BASE TABLE'
  ) THEN
    CALL `tenant_migration_ensure_column`(p_table_name);
  END IF;
END$$

DROP PROCEDURE IF EXISTS `tenant_migration_ensure_optional_index`$$
CREATE PROCEDURE `tenant_migration_ensure_optional_index`(
  IN p_table_name VARCHAR(64),
  IN p_index_name VARCHAR(64),
  IN p_index_columns VARCHAR(255)
)
BEGIN
  IF EXISTS (
    SELECT 1
      FROM information_schema.tables
     WHERE table_schema = DATABASE()
       AND table_name = p_table_name
       AND table_type = 'BASE TABLE'
  ) THEN
    CALL `tenant_migration_ensure_index`(
      p_table_name, p_index_name, p_index_columns
    );
  END IF;
END$$

CALL `tenant_migration_ensure_column`('project')$$
CALL `tenant_migration_ensure_column`('bid_document')$$
CALL `tenant_migration_ensure_column`('audit_task')$$
CALL `tenant_migration_ensure_column`('audit_issue')$$
CALL `tenant_migration_ensure_column`('audit_report')$$
CALL `tenant_migration_ensure_column`('audit_task_event')$$
CALL `tenant_migration_ensure_column`('knowledge_file')$$
CALL `tenant_migration_ensure_column`('knowledge_chunk')$$
CALL `tenant_migration_ensure_column`('chat_message')$$
CALL `tenant_migration_ensure_column`('document_parse_job')$$
CALL `tenant_migration_ensure_column`('rag_trigger_outbox')$$

CALL `tenant_migration_ensure_index`(
  'project', 'idx_project_tenant_id_user_id', '`tenant_id`, `user_id`, `id`'
)$$
CALL `tenant_migration_ensure_index`(
  'bid_document', 'idx_bid_document_tenant_id_project_id', '`tenant_id`, `project_id`, `id`'
)$$
CALL `tenant_migration_ensure_index`(
  'audit_task', 'idx_audit_task_tenant_id_bid_id', '`tenant_id`, `bid_id`, `id`'
)$$
CALL `tenant_migration_ensure_index`(
  'audit_issue', 'idx_audit_issue_tenant_id_audit_id', '`tenant_id`, `audit_id`, `id`'
)$$
CALL `tenant_migration_ensure_index`(
  'audit_report', 'idx_audit_report_tenant_id_audit_id', '`tenant_id`, `audit_id`, `id`'
)$$
CALL `tenant_migration_ensure_index`(
  'audit_task_event', 'idx_audit_task_event_tenant_id_task_id_id', '`tenant_id`, `task_id`, `id`'
)$$
CALL `tenant_migration_ensure_index`(
  'knowledge_file', 'idx_knowledge_file_tenant_id_upload_time_id', '`tenant_id`, `upload_time`, `id`'
)$$
CALL `tenant_migration_ensure_index`(
  'knowledge_chunk', 'idx_knowledge_chunk_tenant_id_file_id_id', '`tenant_id`, `file_id`, `id`'
)$$
CALL `tenant_migration_ensure_index`(
  'chat_message', 'idx_chat_message_tenant_id_project_id_id', '`tenant_id`, `project_id`, `id`'
)$$
CALL `tenant_migration_ensure_index`(
  'document_parse_job', 'idx_document_parse_job_tenant_id_file_id_id', '`tenant_id`, `file_id`, `id`'
)$$
CALL `tenant_migration_ensure_index`(
  'rag_trigger_outbox', 'idx_rag_trigger_outbox_tenant_id_file_id_id', '`tenant_id`, `file_id`, `id`'
)$$

CALL `tenant_migration_ensure_optional_column`('trace_sessions')$$
CALL `tenant_migration_ensure_optional_column`('trace_events')$$
CALL `tenant_migration_ensure_optional_column`('trace_event_blocks')$$

CALL `tenant_migration_ensure_optional_index`(
  'trace_sessions', 'idx_trace_sessions_tenant_id_task_id_id', '`tenant_id`, `task_id`, `id`'
)$$
CALL `tenant_migration_ensure_optional_index`(
  'trace_events', 'idx_trace_events_tenant_id_session_id_id', '`tenant_id`, `session_id`, `id`'
)$$
CALL `tenant_migration_ensure_optional_index`(
  'trace_event_blocks', 'idx_trace_event_blocks_tenant_id_event_id_id', '`tenant_id`, `event_id`, `id`'
)$$

DROP PROCEDURE IF EXISTS `tenant_migration_ensure_optional_index`$$
DROP PROCEDURE IF EXISTS `tenant_migration_ensure_optional_column`$$
DROP PROCEDURE IF EXISTS `tenant_migration_ensure_index`$$
DROP PROCEDURE IF EXISTS `tenant_migration_ensure_column`$$

DELIMITER ;
