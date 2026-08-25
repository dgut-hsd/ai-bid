package com.ithsd.smart_tender.model.dto.rust;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import lombok.Data;

/**
 * Rust {@code GET /api/v1/review/:doc_id/result} 返回体。
 *
 * <p>异步审核结果查询：</p>
 * <ul>
 *   <li>{@code "completed"} → {@code result} 有值</li>
 *   <li>{@code "pending"} → 审查仍在进行中</li>
 *   <li>{@code "failed"} → {@code error} 有值</li>
 *   <li>{@code "partial_failed"} → 部分 clause 执行失败，但 {@code result} 仍有 findings</li>
 * </ul>
 */
@Data
@JsonIgnoreProperties(ignoreUnknown = true)
public class RustReviewResultResponse {
    /** "pending" | "completed" | "failed" | "partial_failed" */
    private String status;
    /** 审核结果（completed / partial_failed 时有值） */
    private RustReviewResponse result;
    /** 错误消息（仅 failed 时有值） */
    private String error;

    public boolean isCompleted() {
        return "completed".equals(status);
    }

    public boolean isPending() {
        return "pending".equals(status);
    }

    public boolean isFailed() {
        return "failed".equals(status);
    }

    /** 部分失败：有可落库的 findings，但伴随部分 clause 执行失败。 */
    public boolean isPartialFailed() {
        return "partial_failed".equals(status);
    }
}
