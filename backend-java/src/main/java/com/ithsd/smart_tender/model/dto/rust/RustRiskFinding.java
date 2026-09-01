package com.ithsd.smart_tender.model.dto.rust;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import com.fasterxml.jackson.annotation.JsonProperty;
import lombok.Data;

import java.util.ArrayList;
import java.util.List;

/**
 * 对应 Rust RiskFinding — Multi-Agent 审核引擎的单条风险发现。
 *
 * <p>框架自动填充字段（rust 侧在 handler 中注入）：
 * <ul>
 *   <li>{@code pageNumber} — 起始页码 (0-based)，从关联 Chunk 反查</li>
 *   <li>{@code sectionPath} — 章节标题链</li>
 *   <li>{@code context} — 条款原文上下文</li>
 * </ul>
 */
@Data
@JsonIgnoreProperties(ignoreUnknown = true)
public class RustRiskFinding {

    // ── 框架填充（LLM 不输出） ──
    private String riskId;
    private List<String> clauseIds = new ArrayList<>();
    /** 关联的原始 block_id 列表（用于前端 bbox-based PDF 精确高亮） */
    private List<String> blockIds = new ArrayList<>();
    /** 词级精确高亮矩形（后端 source_quote → 命中词的逐行 union bbox） */
    private List<RustHighlightRect> highlightRects = new ArrayList<>();
    private String agent;

    // ── 核心判定 ──
    private boolean noRisk;
    private String severity;   // "high" | "medium" | "low" | "info"
    /** 重大/红线问题标志；与四级 severity 正交，重大问题仍为 high */
    @JsonProperty("is_critical")
    private boolean critical;
    /** 重大问题判定依据 */
    @JsonProperty("critical_reason")
    private String criticalReason;
    private String riskType;   // "地域歧视" / "品牌指定" / "程序违规" / …

    // ── 证据 ──
    private String sourceQuote;
    private List<String> legalBasis = new ArrayList<>();
    private List<String> caseRefs = new ArrayList<>();

    // ── 推理 ──
    private String reason;
    private String suggestion;
    /** 证据核验结论：support / refute / insufficient（EvidenceVerifier 回写） */
    @JsonProperty("evidence_verdict")
    private String evidenceVerdict;
    /** 证据核验理由（EvidenceVerifier 回写的一句话结论） */
    @JsonProperty("verifier_reason")
    private String verifierReason;

    // ── 置信度 ──
    private float confidence;

    // ── 分级追踪（框架填充） ──
    @JsonProperty("_initial_tier")
    private String initialTier;
    @JsonProperty("_final_tier")
    private String finalTier;
    @JsonProperty("_tier_escalated")
    private boolean tierEscalated;
    @JsonProperty("_truncated")
    private boolean truncated;

    // ── 动态 Agent ──
    private RustSuggestedAgent suggestedAgent;

    // ── 引用 ──
    private List<RustCitation> citations = new ArrayList<>();

    // ── ★ 框架自动填充的定位字段 ──
    private Integer pageNumber;
    private List<String> sectionPath;
    private String context;

    // ── 内嵌类型 ──

    @Data
    @JsonIgnoreProperties(ignoreUnknown = true)
    public static class RustSuggestedAgent {
        private String agentName;
        private String agentPrompt;
        private List<String> sectionKeywords;
        private String reason;
    }

    @Data
    @JsonIgnoreProperties(ignoreUnknown = true)
    public static class RustCitation {
        private String title;
        private String url;
        private String siteName;
    }

    @Data
    @JsonIgnoreProperties(ignoreUnknown = true)
    public static class RustHighlightRect {
        /** 所在页码 (0-based) */
        private Integer page;
        private double x0;
        private double top;
        private double x1;
        private double bottom;
        /** 原始 PDF 页面宽度 (pt) */
        private double pageWidth;
    }

    // ── 便捷方法 ──

    /** 是否应被过滤（无风险且非截断） */
    public boolean shouldSkip() {
        return noRisk && !truncated;
    }

    /** 将 Rust severity 透传（保留 4 级：high / medium / low / info） */
    public String mappedSeverity() {
        if (severity == null) return "info";
        return switch (severity.toLowerCase()) {
            case "high" -> "high";
            case "medium" -> "medium";
            case "low" -> "low";
            default -> "info";
        };
    }
}
