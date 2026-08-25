-- ───────────────────────────────────────────────────────────────
-- V7 — 将可选的审查追溯表（trace_*）正式纳入 Flyway 版本管理。
--
-- 原 trace_schema.sql 依赖 spring.sql.init(mode: always) 每次启动建表，
-- 且以 DROP TABLE IF EXISTS 开头，会清空追溯历史。现改为幂等迁移。
--
-- 表结构已并入 V5 的 optional 扩展结果：每张表直接包含 tenant_id 列
-- 与 tenant-leading 索引，与 tenant_migration 语义保持一致。
-- ───────────────────────────────────────────────────────────────

-- 追溯会话表 —— 一次 "Agent 审查一条 clause" 对应一行
CREATE TABLE IF NOT EXISTS `trace_sessions` (
    `id`              CHAR(36) PRIMARY KEY,          -- UUID
    `task_id`         VARCHAR(64) NOT NULL,           -- audit_task.task_id
    `doc_id`          VARCHAR(64) NOT NULL,           -- Rust document_id
    `agent_name`      VARCHAR(64) NOT NULL,
    `clause_id`       VARCHAR(64) NOT NULL,           -- Rust chunk_id (e.g. "ch_042")

    -- Risk tier
    `initial_tier`    VARCHAR(4) NOT NULL DEFAULT 'L2',
    `final_tier`      VARCHAR(4) NOT NULL DEFAULT 'L2',
    `tier_escalated`  TINYINT(1) NOT NULL DEFAULT 0,

    -- Results
    `status`          VARCHAR(32) NOT NULL DEFAULT 'pending',
    `risk_id`         VARCHAR(64) DEFAULT NULL,
    `severity`        VARCHAR(16) DEFAULT NULL,
    `confidence`      DOUBLE DEFAULT NULL,

    -- Statistics (denormalized for fast listing)
    `total_turns`         INT NOT NULL DEFAULT 0,
    `total_tool_calls`    INT NOT NULL DEFAULT 0,
    `total_search_calls`  INT NOT NULL DEFAULT 0,
    `event_count`         INT NOT NULL DEFAULT 0,

    -- Timestamps
    `started_at`      DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    `finished_at`     DATETIME DEFAULT NULL,

    -- Metadata
    `error_message`   TEXT,
    `meta`            JSON DEFAULT NULL,

    -- Tenant isolation (与 V5 optional 扩展一致)
    `tenant_id`       BIGINT DEFAULT NULL,

    INDEX `idx_ts_task_id`     (`task_id`),
    INDEX `idx_ts_doc_id`      (`doc_id`),
    INDEX `idx_ts_risk_id`     (`risk_id`),
    INDEX `idx_ts_severity`    (`severity`),
    INDEX `idx_ts_agent`       (`agent_name`, `started_at`),
    INDEX `idx_ts_status`      (`status`),
    UNIQUE KEY `uk_session`    (`task_id`, `agent_name`, `clause_id`),
    INDEX `idx_trace_sessions_tenant_id_task_id_id` (`tenant_id`, `task_id`, `id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='审查追溯会话表';

-- 追溯事件表 —— 每个 ReAct 步骤对应一行
CREATE TABLE IF NOT EXISTS `trace_events` (
    `id`              BIGINT AUTO_INCREMENT PRIMARY KEY,
    `event_id`        CHAR(36) NOT NULL UNIQUE,       -- business UUID
    `session_id`      CHAR(36) NOT NULL,              -- FK to trace_sessions.id

    `agent_name`      VARCHAR(64) NOT NULL,
    `event_type`      VARCHAR(32) NOT NULL,
    `turn`            INT NOT NULL,
    `timestamp`       DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,

    -- Associations
    `clause_id`       VARCHAR(64) DEFAULT NULL,
    `risk_id`         VARCHAR(64) DEFAULT NULL,

    -- Content
    `summary`         TEXT NOT NULL,
    `payload`         JSON DEFAULT NULL,

    `created_at`      DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,

    -- Tenant isolation (与 V5 optional 扩展一致)
    `tenant_id`       BIGINT DEFAULT NULL,

    INDEX `idx_te_session`   (`session_id`, `turn`),
    INDEX `idx_te_risk`      (`risk_id`),
    INDEX `idx_te_type`      (`event_type`, `timestamp`),
    INDEX `idx_te_clause`    (`clause_id`, `turn`),
    INDEX `idx_trace_events_tenant_id_session_id_id` (`tenant_id`, `session_id`, `id`),
    CONSTRAINT `fk_te_session` FOREIGN KEY (`session_id`) REFERENCES `trace_sessions`(`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='审查追溯事件表';

-- block_id 关联表 —— 支持反向查找（block → events）
CREATE TABLE IF NOT EXISTS `trace_event_blocks` (
    `id`              BIGINT AUTO_INCREMENT PRIMARY KEY,
    `event_id`        CHAR(36) NOT NULL,              -- FK to trace_events.event_id
    `block_id`        VARCHAR(64) NOT NULL,            -- PDF block ID (e.g. "b_3_7")

    -- Tenant isolation (与 V5 optional 扩展一致)
    `tenant_id`       BIGINT DEFAULT NULL,

    INDEX `idx_teb_event`    (`event_id`),
    INDEX `idx_teb_block`    (`block_id`),
    INDEX `idx_trace_event_blocks_tenant_id_event_id_id` (`tenant_id`, `event_id`, `id`),
    CONSTRAINT `fk_teb_event` FOREIGN KEY (`event_id`) REFERENCES `trace_events`(`event_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='审查追溯事件 block 关联表';