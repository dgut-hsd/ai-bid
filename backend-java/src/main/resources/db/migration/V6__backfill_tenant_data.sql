-- Idempotent backfill. No NOT NULL change, legacy columns are retained, and no
-- application dual-write or tenant enforcement is enabled by this migration.
-- Every update is keyed by its source table primary key and guarded by
-- tenant_id IS NULL, so a rerun never changes an already assigned resource.

INSERT IGNORE INTO `tenant` (
  `tenant_code`,
  `name`,
  `status`,
  `owner_user_id`,
  `plan_code`,
  `settings_json`,
  `version`,
  `created_at`,
  `updated_at`,
  `deleted_at`
)
SELECT
  CONCAT('user-', u.`id`),
  CONCAT('Personal workspace ', u.`id`),
  'ACTIVE',
  u.`id`,
  'STANDARD',
  NULL,
  0,
  COALESCE(u.`create_time`, u.`update_time`, UTC_TIMESTAMP(3)),
  COALESCE(u.`update_time`, u.`create_time`, UTC_TIMESTAMP(3)),
  NULL
FROM `sys_user` AS u;

INSERT IGNORE INTO `tenant_member` (
  `tenant_id`,
  `user_id`,
  `role`,
  `status`,
  `joined_at`,
  `invited_by`,
  `last_seen_at`
)
SELECT
  t.`id`,
  u.`id`,
  'OWNER',
  'ACTIVE',
  COALESCE(u.`create_time`, u.`update_time`, UTC_TIMESTAMP(3)),
  NULL,
  NULL
FROM `sys_user` AS u
JOIN `tenant` AS t
  ON t.`tenant_code` = CONCAT('user-', u.`id`);

-- project.user_id is the highest-confidence ownership signal in V1.
UPDATE `project` AS p
JOIN `tenant` AS t
  ON t.`tenant_code` = CONCAT('user-', p.`user_id`)
SET p.`tenant_id` = t.`id`
WHERE p.`tenant_id` IS NULL;

-- Prefer bid_document.upload_user_id. If it is absent, an already resolved
-- project owner is safe to inherit. Conflicting explicit owners remain NULL.
UPDATE `bid_document` AS b
LEFT JOIN `tenant` AS upload_t
  ON upload_t.`tenant_code` = CONCAT('user-', b.`upload_user_id`)
LEFT JOIN `project` AS p
  ON p.`id` = b.`project_id`
SET b.`tenant_id` = CASE
  WHEN upload_t.`id` IS NOT NULL
       AND (p.`tenant_id` IS NULL OR p.`tenant_id` = upload_t.`id`)
    THEN upload_t.`id`
  WHEN upload_t.`id` IS NULL AND p.`tenant_id` IS NOT NULL
    THEN p.`tenant_id`
  ELSE b.`tenant_id`
END
WHERE b.`tenant_id` IS NULL
  AND (
    (
      upload_t.`id` IS NOT NULL
      AND (p.`tenant_id` IS NULL OR p.`tenant_id` = upload_t.`id`)
    )
    OR (upload_t.`id` IS NULL AND p.`tenant_id` IS NOT NULL)
  );

-- audit_task.audit_user_id is explicit. Otherwise inherit the resolved bid or
-- project owner. A disagreement between explicit and parent owners is left for
-- manual reconciliation instead of being guessed.
UPDATE `audit_task` AS a
LEFT JOIN `tenant` AS audit_t
  ON audit_t.`tenant_code` = CONCAT('user-', a.`audit_user_id`)
LEFT JOIN `bid_document` AS b
  ON b.`id` = a.`bid_id`
LEFT JOIN `project` AS p
  ON p.`id` = b.`project_id`
SET a.`tenant_id` = CASE
  WHEN audit_t.`id` IS NOT NULL
       AND (
         COALESCE(b.`tenant_id`, p.`tenant_id`) IS NULL
         OR COALESCE(b.`tenant_id`, p.`tenant_id`) = audit_t.`id`
       )
    THEN audit_t.`id`
  WHEN audit_t.`id` IS NULL
       AND COALESCE(b.`tenant_id`, p.`tenant_id`) IS NOT NULL
    THEN COALESCE(b.`tenant_id`, p.`tenant_id`)
  ELSE a.`tenant_id`
END
WHERE a.`tenant_id` IS NULL
  AND (
    (
      audit_t.`id` IS NOT NULL
      AND (
        COALESCE(b.`tenant_id`, p.`tenant_id`) IS NULL
        OR COALESCE(b.`tenant_id`, p.`tenant_id`) = audit_t.`id`
      )
    )
    OR (
      audit_t.`id` IS NULL
      AND COALESCE(b.`tenant_id`, p.`tenant_id`) IS NOT NULL
    )
  );

-- Children inherit only from a resolved parent resource.
UPDATE `audit_issue` AS i
JOIN `audit_task` AS a
  ON a.`id` = i.`audit_id`
SET i.`tenant_id` = a.`tenant_id`
WHERE i.`tenant_id` IS NULL
  AND a.`tenant_id` IS NOT NULL;

UPDATE `audit_report` AS r
JOIN `audit_task` AS a
  ON a.`id` = r.`audit_id`
SET r.`tenant_id` = a.`tenant_id`
WHERE r.`tenant_id` IS NULL
  AND a.`tenant_id` IS NOT NULL;

UPDATE `audit_task_event` AS e
JOIN `audit_task` AS a
  ON a.`task_id` = e.`task_id`
SET e.`tenant_id` = a.`tenant_id`
WHERE e.`tenant_id` IS NULL
  AND a.`tenant_id` IS NOT NULL;

UPDATE `knowledge_file` AS k
JOIN `tenant` AS t
  ON t.`tenant_code` = CONCAT('user-', k.`upload_user_id`)
SET k.`tenant_id` = t.`id`
WHERE k.`tenant_id` IS NULL;

UPDATE `knowledge_chunk` AS c
JOIN `knowledge_file` AS k
  ON k.`id` = c.`file_id`
SET c.`tenant_id` = k.`tenant_id`
WHERE c.`tenant_id` IS NULL
  AND k.`tenant_id` IS NOT NULL;

UPDATE `chat_message` AS m
JOIN `tenant` AS t
  ON t.`tenant_code` = CONCAT('user-', m.`user_id`)
SET m.`tenant_id` = t.`id`
WHERE m.`tenant_id` IS NULL;

UPDATE `document_parse_job` AS j
JOIN `bid_document` AS b
  ON b.`id` = j.`file_id`
SET j.`tenant_id` = b.`tenant_id`
WHERE j.`tenant_id` IS NULL
  AND b.`tenant_id` IS NOT NULL;

-- Prefer the parse job parent, with the V1 bid_document as a compatible
-- fallback. A mismatch is deliberately left unresolved.
UPDATE `rag_trigger_outbox` AS o
LEFT JOIN `document_parse_job` AS j
  ON j.`job_id` = o.`job_id`
LEFT JOIN `bid_document` AS b
  ON b.`id` = o.`file_id`
SET o.`tenant_id` = CASE
  WHEN j.`tenant_id` IS NOT NULL
       AND (b.`tenant_id` IS NULL OR b.`tenant_id` = j.`tenant_id`)
    THEN j.`tenant_id`
  WHEN j.`tenant_id` IS NULL AND b.`tenant_id` IS NOT NULL
    THEN b.`tenant_id`
  ELSE o.`tenant_id`
END
WHERE o.`tenant_id` IS NULL
  AND (
    (
      j.`tenant_id` IS NOT NULL
      AND (b.`tenant_id` IS NULL OR b.`tenant_id` = j.`tenant_id`)
    )
    OR (j.`tenant_id` IS NULL AND b.`tenant_id` IS NOT NULL)
  );

-- Trace schema is optional. Each top-level prepared statement falls back to a
-- harmless SELECT when the child, parent, or required tenant_id/key column is
-- absent. No routine DDL is used, so the core DML is not followed by an
-- implicit-commit routine schema change.
SET @sql = IF(
  EXISTS (
    SELECT 1 FROM information_schema.tables
     WHERE table_schema = DATABASE()
       AND table_name = 'trace_sessions'
       AND table_type = 'BASE TABLE'
  ) AND EXISTS (
    SELECT 1 FROM information_schema.columns
     WHERE table_schema = DATABASE()
       AND table_name = 'trace_sessions'
       AND column_name IN ('tenant_id', 'task_id')
     GROUP BY table_name
    HAVING COUNT(DISTINCT column_name) = 2
  ) AND EXISTS (
    SELECT 1 FROM information_schema.tables
     WHERE table_schema = DATABASE()
       AND table_name = 'audit_task'
       AND table_type = 'BASE TABLE'
  ) AND EXISTS (
    SELECT 1 FROM information_schema.columns
     WHERE table_schema = DATABASE()
       AND table_name = 'audit_task'
       AND column_name IN ('tenant_id', 'task_id')
     GROUP BY table_name
    HAVING COUNT(DISTINCT column_name) = 2
  ),
  'UPDATE `trace_sessions` AS s JOIN `audit_task` AS a ON a.`task_id` = s.`task_id` SET s.`tenant_id` = a.`tenant_id` WHERE s.`tenant_id` IS NULL AND a.`tenant_id` IS NOT NULL',
  'SELECT 1'
);
PREPARE tenant_migration_stmt FROM @sql;
EXECUTE tenant_migration_stmt;
DEALLOCATE PREPARE tenant_migration_stmt;

SET @sql = IF(
  EXISTS (
    SELECT 1 FROM information_schema.tables
     WHERE table_schema = DATABASE()
       AND table_name = 'trace_events'
       AND table_type = 'BASE TABLE'
  ) AND EXISTS (
    SELECT 1 FROM information_schema.columns
     WHERE table_schema = DATABASE()
       AND table_name = 'trace_events'
       AND column_name IN ('tenant_id', 'session_id')
     GROUP BY table_name
    HAVING COUNT(DISTINCT column_name) = 2
  ) AND EXISTS (
    SELECT 1 FROM information_schema.tables
     WHERE table_schema = DATABASE()
       AND table_name = 'trace_sessions'
       AND table_type = 'BASE TABLE'
  ) AND EXISTS (
    SELECT 1 FROM information_schema.columns
     WHERE table_schema = DATABASE()
       AND table_name = 'trace_sessions'
       AND column_name IN ('tenant_id', 'id')
     GROUP BY table_name
    HAVING COUNT(DISTINCT column_name) = 2
  ),
  'UPDATE `trace_events` AS e JOIN `trace_sessions` AS s ON s.`id` = e.`session_id` SET e.`tenant_id` = s.`tenant_id` WHERE e.`tenant_id` IS NULL AND s.`tenant_id` IS NOT NULL',
  'SELECT 1'
);
PREPARE tenant_migration_stmt FROM @sql;
EXECUTE tenant_migration_stmt;
DEALLOCATE PREPARE tenant_migration_stmt;

SET @sql = IF(
  EXISTS (
    SELECT 1 FROM information_schema.tables
     WHERE table_schema = DATABASE()
       AND table_name = 'trace_event_blocks'
       AND table_type = 'BASE TABLE'
  ) AND EXISTS (
    SELECT 1 FROM information_schema.columns
     WHERE table_schema = DATABASE()
       AND table_name = 'trace_event_blocks'
       AND column_name IN ('tenant_id', 'event_id')
     GROUP BY table_name
    HAVING COUNT(DISTINCT column_name) = 2
  ) AND EXISTS (
    SELECT 1 FROM information_schema.tables
     WHERE table_schema = DATABASE()
       AND table_name = 'trace_events'
       AND table_type = 'BASE TABLE'
  ) AND EXISTS (
    SELECT 1 FROM information_schema.columns
     WHERE table_schema = DATABASE()
       AND table_name = 'trace_events'
       AND column_name IN ('tenant_id', 'event_id')
     GROUP BY table_name
    HAVING COUNT(DISTINCT column_name) = 2
  ),
  'UPDATE `trace_event_blocks` AS b JOIN `trace_events` AS e ON e.`event_id` = b.`event_id` SET b.`tenant_id` = e.`tenant_id` WHERE b.`tenant_id` IS NULL AND e.`tenant_id` IS NOT NULL',
  'SELECT 1'
);
PREPARE tenant_migration_stmt FROM @sql;
EXECUTE tenant_migration_stmt;
DEALLOCATE PREPARE tenant_migration_stmt;
