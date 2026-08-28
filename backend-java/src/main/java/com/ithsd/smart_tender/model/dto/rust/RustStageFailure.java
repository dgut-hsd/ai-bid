package com.ithsd.smart_tender.model.dto.rust;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import lombok.Data;

/**
 * Rust {@code execution_summary.failed_stages} 的单个元素。
 */
@Data
@JsonIgnoreProperties(ignoreUnknown = true)
public class RustStageFailure {
    /** 阶段名：pipeline / batch_search / execute / legal_verify / debate / blind_spot / evidence_verify */
    private String stage;
    /** 失败原因描述 */
    private String message;
}