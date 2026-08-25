-- ═══════════════════════════════════════════════════════════════
-- ⚠️ DEPRECATED — 本文件已由 Flyway 接管（db/migration/V1__baseline.sql）。
-- 禁止再挂到 spring.sql.init.schema-locations：本文件以 DROP TABLE 开头，
-- 配合 sql.init.mode=always 会导致每次应用重启清空 audit_task_event、
-- document_parse_job、rag_trigger_outbox 表的数据。
-- ═══════════════════════════════════════════════════════════════
DROP TABLE IF EXISTS audit_task_event;

CREATE TABLE audit_task_event (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    task_id VARCHAR(64) NOT NULL,
    event_type VARCHAR(32) NOT NULL,
    event_data LONGTEXT  NOT NULL,
    created_at DATETIME NOT NULL
);

CREATE INDEX idx_audit_task_event_task_id_id ON audit_task_event(task_id, id);
CREATE INDEX idx_audit_task_event_created_at ON audit_task_event(created_at);

DROP TABLE IF EXISTS document_parse_job;

CREATE TABLE document_parse_job (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    job_id VARCHAR(64) NOT NULL,
    request_id VARCHAR(64) NULL,
    file_id BIGINT NOT NULL,
    file_name VARCHAR(255) NULL,
    source_type VARCHAR(16) NOT NULL,
    priority VARCHAR(16) NOT NULL,
    trigger_rag TINYINT(1) NOT NULL DEFAULT 1,
    strategy_version VARCHAR(64) NOT NULL,
    status VARCHAR(24) NOT NULL,
    stage VARCHAR(64) NULL,
    progress INT NOT NULL DEFAULT 0,
    chunk_count INT NOT NULL DEFAULT 0,
    failed_stages JSON NULL,
    error_msg VARCHAR(1000) NULL,
    start_time DATETIME NULL,
    end_time DATETIME NULL,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    UNIQUE KEY uk_document_parse_job_job_id (job_id),
    KEY idx_document_parse_job_file_id (file_id),
    KEY idx_document_parse_job_status (status),
    KEY idx_document_parse_job_created_at (created_at),
    KEY idx_document_parse_job_request_id (request_id)
);

DROP TABLE IF EXISTS rag_trigger_outbox;

CREATE TABLE rag_trigger_outbox (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    request_id VARCHAR(64) NOT NULL,
    job_id VARCHAR(64) NOT NULL,
    file_id BIGINT NOT NULL,
    chunk_count INT NOT NULL,
    strategy_version VARCHAR(64) NOT NULL,
    payload_hash VARCHAR(64) NOT NULL,
    idempotency_key VARCHAR(160) NOT NULL,
    endpoint VARCHAR(255) NOT NULL,
    status VARCHAR(24) NOT NULL,
    retry_count INT NOT NULL DEFAULT 0,
    max_retry INT NOT NULL DEFAULT 3,
    next_retry_at DATETIME NOT NULL,
    last_status_code INT NULL,
    last_error_msg VARCHAR(1000) NULL,
    response_body LONGTEXT NULL,
    sent_at DATETIME NULL,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    UNIQUE KEY uk_rag_trigger_outbox_idempotency (idempotency_key),
    KEY idx_rag_trigger_outbox_status_next (status, next_retry_at),
    KEY idx_rag_trigger_outbox_file_id (file_id),
    KEY idx_rag_trigger_outbox_created_at (created_at)
);
