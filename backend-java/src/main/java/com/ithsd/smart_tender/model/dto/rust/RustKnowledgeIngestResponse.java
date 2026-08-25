package com.ithsd.smart_tender.model.dto.rust;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import lombok.Data;

/**
 * Rust {@code POST /api/v1/knowledge/ingest} 返回体。
 *
 * <p>文件向量化入库成功后返回，其中 {@code documentId} 用于后续删除文件时
 * 联动清理 Qdrant 向量（{@code DELETE /api/v1/knowledge/document/:document_id}）。</p>
 */
@Data
@JsonIgnoreProperties(ignoreUnknown = true)
public class RustKnowledgeIngestResponse {
    /** Qdrant 侧的 document_id（Rust 生成 UUID） */
    private String documentId;
    private String documentName;
    /** 实际切分并写入 Qdrant 的 chunk 数量 */
    private Integer chunkCount;
    private Integer dimension;
    private String collection;
    private Long elapsedMs;
    private String message;
}