package com.ithsd.smart_tender.model.dto.rust;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import lombok.Data;

import java.util.ArrayList;
import java.util.List;

/**
 * Rust {@code POST /api/v1/documents/:id/review} 返回体。
 */
@Data
@JsonIgnoreProperties(ignoreUnknown = true)
public class RustReviewResponse {
    private String documentId;
    private List<RustRiskFinding> findings = new ArrayList<>();
    private RustRoutingSummary routingSummary;
    /** Rust CoordinatorOutput.executionSummary（含 status + failed_stages，用于区分 completed / partial_failed） */
    private RustExecutionSummary executionSummary;
    /** Rust CoordinatorOutput.graphSnapshot（可选，知识图谱用） */
    private java.util.Map<String, Object> graphSnapshot;
}
