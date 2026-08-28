package com.ithsd.smart_tender.model.vo;

import java.util.List;

/**
 * 审核问题 VO — 对齐 Rust RiskFinding 全字段（22 字段全量透传）。
 */
public class IssueVO {
    private String issueNo;
    private String riskId;
    private String severity;
    private Boolean isCritical;
    private String criticalReason;
    private String category;
    private String agentName;
    private String description;
    private LocationVO location;
    private String suggestion;
    private String reference;
    private String sourceQuote;
    private List<String> legalBasis;
    private List<String> caseRefs;
    private Float confidence;
    // ── PDF 文本锚定 ──
    private String anchorQuote;
    private Integer anchorPage;
    private String anchorSection;
    private List<String> anchorTokens;
    private List<Integer> anchorCharsRange;

    // ── Rust RiskFinding 新增字段 ──
    private Boolean noRisk;
    private String initialTier;
    private String finalTier;
    private Boolean tierEscalated;
    private Boolean truncated;
    private List<String> clauseIds;
    /** 关联的原始 block_id 列表（用于前端 bbox-based PDF 精确高亮） */
    private List<String> blockIds;
    /** 词级精确高亮矩形（来自 Rust highlight_rects，非空时前端优先渲染） */
    private List<HighlightRectVO> highlightRects;
    private List<CitationVO> citations;
    private SuggestedAgentVO suggestedAgent;
    private String agent;

    // ── Getters / Setters (original) ──

    public String getIssueNo() { return issueNo; }
    public void setIssueNo(String issueNo) { this.issueNo = issueNo; }
    public String getRiskId() { return riskId; }
    public void setRiskId(String riskId) { this.riskId = riskId; }
    public String getSeverity() { return severity; }
    public void setSeverity(String severity) { this.severity = severity; }
    public Boolean getIsCritical() { return isCritical; }
    public void setIsCritical(Boolean isCritical) { this.isCritical = isCritical; }
    public String getCriticalReason() { return criticalReason; }
    public void setCriticalReason(String criticalReason) { this.criticalReason = criticalReason; }
    public String getCategory() { return category; }
    public void setCategory(String category) { this.category = category; }
    public String getAgentName() { return agentName; }
    public void setAgentName(String agentName) { this.agentName = agentName; }
    public String getDescription() { return description; }
    public void setDescription(String description) { this.description = description; }
    public LocationVO getLocation() { return location; }
    public void setLocation(LocationVO location) { this.location = location; }
    public String getSuggestion() { return suggestion; }
    public void setSuggestion(String suggestion) { this.suggestion = suggestion; }
    public String getReference() { return reference; }
    public void setReference(String reference) { this.reference = reference; }
    public String getSourceQuote() { return sourceQuote; }
    public void setSourceQuote(String sourceQuote) { this.sourceQuote = sourceQuote; }
    public List<String> getLegalBasis() { return legalBasis; }
    public void setLegalBasis(List<String> legalBasis) { this.legalBasis = legalBasis; }
    public List<String> getCaseRefs() { return caseRefs; }
    public void setCaseRefs(List<String> caseRefs) { this.caseRefs = caseRefs; }
    public Float getConfidence() { return confidence; }
    public void setConfidence(Float confidence) { this.confidence = confidence; }
    public String getAnchorQuote() { return anchorQuote; }
    public void setAnchorQuote(String anchorQuote) { this.anchorQuote = anchorQuote; }
    public Integer getAnchorPage() { return anchorPage; }
    public void setAnchorPage(Integer anchorPage) { this.anchorPage = anchorPage; }
    public String getAnchorSection() { return anchorSection; }
    public void setAnchorSection(String anchorSection) { this.anchorSection = anchorSection; }
    public List<String> getAnchorTokens() { return anchorTokens; }
    public void setAnchorTokens(List<String> anchorTokens) { this.anchorTokens = anchorTokens; }
    public List<Integer> getAnchorCharsRange() { return anchorCharsRange; }
    public void setAnchorCharsRange(List<Integer> anchorCharsRange) { this.anchorCharsRange = anchorCharsRange; }

    // ── Getters / Setters (new) ──

    public Boolean getNoRisk() { return noRisk; }
    public void setNoRisk(Boolean noRisk) { this.noRisk = noRisk; }
    public String getInitialTier() { return initialTier; }
    public void setInitialTier(String initialTier) { this.initialTier = initialTier; }
    public String getFinalTier() { return finalTier; }
    public void setFinalTier(String finalTier) { this.finalTier = finalTier; }
    public Boolean getTierEscalated() { return tierEscalated; }
    public void setTierEscalated(Boolean tierEscalated) { this.tierEscalated = tierEscalated; }
    public Boolean getTruncated() { return truncated; }
    public void setTruncated(Boolean truncated) { this.truncated = truncated; }
    public List<String> getClauseIds() { return clauseIds; }
    public void setClauseIds(List<String> clauseIds) { this.clauseIds = clauseIds; }
    public List<String> getBlockIds() { return blockIds; }
    public void setBlockIds(List<String> blockIds) { this.blockIds = blockIds; }
    public List<HighlightRectVO> getHighlightRects() { return highlightRects; }
    public void setHighlightRects(List<HighlightRectVO> highlightRects) { this.highlightRects = highlightRects; }
    public List<CitationVO> getCitations() { return citations; }
    public void setCitations(List<CitationVO> citations) { this.citations = citations; }
    public SuggestedAgentVO getSuggestedAgent() { return suggestedAgent; }
    public void setSuggestedAgent(SuggestedAgentVO suggestedAgent) { this.suggestedAgent = suggestedAgent; }
    public String getAgent() { return agent; }
    public void setAgent(String agent) { this.agent = agent; }

    // ── Location ──

    public static class LocationVO {
        private Integer pageNumber;
        private String sectionName;
        private String context;

        public Integer getPageNumber() { return pageNumber; }
        public void setPageNumber(Integer pageNumber) { this.pageNumber = pageNumber; }
        public String getSectionName() { return sectionName; }
        public void setSectionName(String sectionName) { this.sectionName = sectionName; }
        public String getContext() { return context; }
        public void setContext(String context) { this.context = context; }
    }

    // ── CitationVO（对齐 Rust Citation） ──

    public static class CitationVO {
        private String title;
        private String url;
        private String siteName;

        public String getTitle() { return title; }
        public void setTitle(String title) { this.title = title; }
        public String getUrl() { return url; }
        public void setUrl(String url) { this.url = url; }
        public String getSiteName() { return siteName; }
        public void setSiteName(String siteName) { this.siteName = siteName; }
    }

    // ── SuggestedAgentVO（对齐 Rust SuggestedAgent） ──

    public static class SuggestedAgentVO {
        private String agentName;
        private String agentPrompt;
        private List<String> sectionKeywords;
        private String reason;

        public String getAgentName() { return agentName; }
        public void setAgentName(String agentName) { this.agentName = agentName; }
        public String getAgentPrompt() { return agentPrompt; }
        public void setAgentPrompt(String agentPrompt) { this.agentPrompt = agentPrompt; }
        public List<String> getSectionKeywords() { return sectionKeywords; }
        public void setSectionKeywords(List<String> sectionKeywords) { this.sectionKeywords = sectionKeywords; }
        public String getReason() { return reason; }
        public void setReason(String reason) { this.reason = reason; }
    }

    // ── HighlightRectVO（对齐 Rust HighlightRect） ──

    public static class HighlightRectVO {
        private Integer page;
        private Double x0;
        private Double top;
        private Double x1;
        private Double bottom;
        private Double pageWidth;

        public Integer getPage() { return page; }
        public void setPage(Integer page) { this.page = page; }
        public Double getX0() { return x0; }
        public void setX0(Double x0) { this.x0 = x0; }
        public Double getTop() { return top; }
        public void setTop(Double top) { this.top = top; }
        public Double getX1() { return x1; }
        public void setX1(Double x1) { this.x1 = x1; }
        public Double getBottom() { return bottom; }
        public void setBottom(Double bottom) { this.bottom = bottom; }
        public Double getPageWidth() { return pageWidth; }
        public void setPageWidth(Double pageWidth) { this.pageWidth = pageWidth; }
    }
}
