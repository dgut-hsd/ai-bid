-- V10：为 knowledge_file 补充 Rust/Qdrant 文档关联 ID 列。
--
-- 背景：KnowledgeFile 实体（@TableField("rust_document_id")）在一轮「知识库 RAG 全链路接线」
-- 提交里新增了该字段，用于向量化入库成功后回填 document_id、删除文件时联动清理 Qdrant 向量；
-- 但当时只给 bid_document 加了该列（V3），遗漏了 knowledge_file 的迁移。
-- 结果：实体映射列在 SQL 里出现，而表中不存在该列，导致知识库列表查询、向量回填 UPDATE
-- 报 Unknown column 'rust_document_id'，上传后在列表里看不到文件。
--
-- 使用幂等守卫：线上库可能已通过手动 ALTER 补过该列，避免重复执行时报「Duplicate column」。

DROP PROCEDURE IF EXISTS `v10_knowledge_file_ensure_rust_document_id`;

DELIMITER $$
CREATE PROCEDURE `v10_knowledge_file_ensure_rust_document_id`()
BEGIN
  IF NOT EXISTS (
    SELECT 1
      FROM information_schema.columns
     WHERE table_schema = DATABASE()
       AND table_name = 'knowledge_file'
       AND column_name = 'rust_document_id'
  ) THEN
    ALTER TABLE `knowledge_file`
      ADD COLUMN `rust_document_id` varchar(64) DEFAULT NULL COMMENT 'Rust/Qdrant 文档ID' AFTER `chunk_count`,
      ADD INDEX `idx_rust_document_id` (`rust_document_id`);
  END IF;
END$$
DELIMITER ;

CALL `v10_knowledge_file_ensure_rust_document_id`();
DROP PROCEDURE `v10_knowledge_file_ensure_rust_document_id`;