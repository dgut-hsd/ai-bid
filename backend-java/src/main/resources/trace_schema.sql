-- ═══════════════════════════════════════════════════════
-- ⚠️ DEPRECATED — 本文件已由 Flyway 接管（db/migration/V7__add_trace_schema.sql）。
-- 禁止再挂到 spring.sql.init.schema-locations：本文件以 DROP TABLE 开头，
-- 配合 sql.init.mode=always 会导致每次应用重启清空 trace_* 追溯数据。
-- 原始说明：审查追溯表 — 设计文档 §10.1.4 (PostgreSQL → MySQL 8.0 适配)
-- ═══════════════════════════════════════════════════════

-- 追溯会话表 —— 一次 "Agent 审查一条 clause" 对应一行
DROP TABLE IF EXISTS trace_event_blocks;
DROP TABLE IF EXISTS trace_events;
DROP TABLE IF EXISTS trace_sessions;

CREATE TABLE trace_sessions (
    id              CHAR(36) PRIMARY KEY,          -- UUID
    task_id         VARCHAR(64) NOT NULL,           -- audit_task.task_id
    doc_id          VARCHAR(64) NOT NULL,           -- Rust document_id
    agent_name      VARCHAR(64) NOT NULL,
    clause_id       VARCHAR(64) NOT NULL,           -- Rust chunk_id (e.g. "ch_042")

    -- Risk tier
    initial_tier    VARCHAR(4) NOT NULL DEFAULT 'L2',
    final_tier      VARCHAR(4) NOT NULL DEFAULT 'L2',
    tier_escalated  TINYINT(1) NOT NULL DEFAULT 0,

    -- Results
    status          VARCHAR(32) NOT NULL DEFAULT 'pending',  -- pending/running/completed/max_turns_exceeded/error
    risk_id         VARCHAR(64),                    -- e.g. "R_017"
    severity        VARCHAR(16),                    -- high/medium/low/info
    confidence      DOUBLE,

    -- Statistics (denormalized for fast listing)
    total_turns          INT NOT NULL DEFAULT 0,
    total_tool_calls     INT NOT NULL DEFAULT 0,
    total_search_calls   INT NOT NULL DEFAULT 0,
    event_count          INT NOT NULL DEFAULT 0,

    -- Timestamps
    started_at      DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    finished_at     DATETIME,

    -- Metadata
    error_message   TEXT,
    meta            JSON,                           -- extensible JSON blob

    INDEX idx_ts_task_id     (task_id),
    INDEX idx_ts_doc_id      (doc_id),
    INDEX idx_ts_risk_id     (risk_id),
    INDEX idx_ts_severity    (severity),
    INDEX idx_ts_agent       (agent_name, started_at),
    INDEX idx_ts_status      (status),
    UNIQUE KEY uk_session     (task_id, agent_name, clause_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- 追溯事件表 —— 每个 ReAct 步骤对应一行
CREATE TABLE trace_events (
    id              BIGINT AUTO_INCREMENT PRIMARY KEY,
    event_id        CHAR(36) NOT NULL UNIQUE,       -- business UUID
    session_id      CHAR(36) NOT NULL,              -- FK to trace_sessions.id

    agent_name      VARCHAR(64) NOT NULL,
    event_type      VARCHAR(32) NOT NULL,            -- turn_start / agent_thought / tool_call / tool_result / output_finding / ...
    turn            INT NOT NULL,
    `timestamp`     DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,

    -- Associations
    clause_id       VARCHAR(64),
    risk_id         VARCHAR(64),

    -- Content
    summary         TEXT NOT NULL,
    payload         JSON,                            -- structured detail

    created_at      DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,

    INDEX idx_te_session   (session_id, turn),
    INDEX idx_te_risk      (risk_id),
    INDEX idx_te_type      (event_type, `timestamp`),
    INDEX idx_te_clause    (clause_id, turn),
    CONSTRAINT fk_te_session FOREIGN KEY (session_id) REFERENCES trace_sessions(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- block_id 关联表 —— 替代 PostgreSQL TEXT[] + GIN 索引，支持反向查找
CREATE TABLE trace_event_blocks (
    id              BIGINT AUTO_INCREMENT PRIMARY KEY,
    event_id        CHAR(36) NOT NULL,              -- FK to trace_events.event_id
    block_id        VARCHAR(64) NOT NULL,            -- PDF block ID (e.g. "b_3_7")

    INDEX idx_teb_event    (event_id),
    INDEX idx_teb_block    (block_id),              -- supports reverse lookup: block → events
    CONSTRAINT fk_teb_event FOREIGN KEY (event_id) REFERENCES trace_events(event_id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
