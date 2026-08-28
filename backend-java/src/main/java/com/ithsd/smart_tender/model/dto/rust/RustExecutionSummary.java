package com.ithsd.smart_tender.model.dto.rust;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import lombok.Data;

import java.util.ArrayList;
import java.util.List;

/**
 * Rust {@code ReviewResponse.execution_summary} 片段。
 *
 * <p>用于透传审核的「部分失败」信号：当 EvidenceVerify 等阶段被跳过/超时，
 * Rust 会把 {@code status} 置为 {@code "partial_failed"} 并在 {@code failed_stages}
 * 中列出失败阶段名。</p>
 *
 * <p>仅声明 Java 侧需要的字段；{@code failed_agents}/{@code failed_clauses}/
 * {@code budget} 等大字段由 {@code ignoreUnknown} 跳过，避免为超大文档物化数百条
 * 失败条款对象。</p>
 */
@Data
@JsonIgnoreProperties(ignoreUnknown = true)
public class RustExecutionSummary {
    /** "completed" | "partial_failed" */
    private String status;
    /** 失败的管线阶段（元素含 stage / message 两字段） */
    private List<RustStageFailure> failedStages = new ArrayList<>();
}