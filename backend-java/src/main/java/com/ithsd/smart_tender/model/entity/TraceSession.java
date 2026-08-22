package com.ithsd.smart_tender.model.entity;

import com.baomidou.mybatisplus.annotation.IdType;
import com.baomidou.mybatisplus.annotation.TableField;
import com.baomidou.mybatisplus.annotation.TableId;
import com.baomidou.mybatisplus.annotation.TableName;

import java.io.Serializable;
import java.time.LocalDateTime;

/**
 * 审查追溯会话 — 一次 "Agent 审查一条 clause" 对应一行。
 *
 * <p>设计文档 §10.1.2 TraceSession，MySQL 8.0 适配版。</p>
 */
@TableName("trace_sessions")
public class TraceSession implements Serializable {

    @TableId(value = "id", type = IdType.ASSIGN_UUID)
    private String id;

    @TableField("tenant_id")
    private Long tenantId;

    @TableField("task_id")
    private String taskId;

    @TableField("doc_id")
    private String docId;

    @TableField("agent_name")
    private String agentName;

    @TableField("clause_id")
    private String clauseId;

    @TableField("initial_tier")
    private String initialTier;

    @TableField("final_tier")
    private String finalTier;

    @TableField("tier_escalated")
    private Integer tierEscalated;

    @TableField("status")
    private String status;

    @TableField("risk_id")
    private String riskId;

    @TableField("severity")
    private String severity;

    @TableField("confidence")
    private Double confidence;

    @TableField("total_turns")
    private Integer totalTurns;

    @TableField("total_tool_calls")
    private Integer totalToolCalls;

    @TableField("total_search_calls")
    private Integer totalSearchCalls;

    @TableField("event_count")
    private Integer eventCount;

    @TableField("started_at")
    private LocalDateTime startedAt;

    @TableField("finished_at")
    private LocalDateTime finishedAt;

    @TableField("error_message")
    private String errorMessage;

    @TableField("meta")
    private String meta;

    // ── Getters / Setters ──

    public String getId() { return id; }
    public void setId(String id) { this.id = id; }

    public Long getTenantId() { return tenantId; }
    public void setTenantId(Long tenantId) { this.tenantId = tenantId; }

    public String getTaskId() { return taskId; }
    public void setTaskId(String taskId) { this.taskId = taskId; }

    public String getDocId() { return docId; }
    public void setDocId(String docId) { this.docId = docId; }

    public String getAgentName() { return agentName; }
    public void setAgentName(String agentName) { this.agentName = agentName; }

    public String getClauseId() { return clauseId; }
    public void setClauseId(String clauseId) { this.clauseId = clauseId; }

    public String getInitialTier() { return initialTier; }
    public void setInitialTier(String initialTier) { this.initialTier = initialTier; }

    public String getFinalTier() { return finalTier; }
    public void setFinalTier(String finalTier) { this.finalTier = finalTier; }

    public Integer getTierEscalated() { return tierEscalated; }
    public void setTierEscalated(Integer tierEscalated) { this.tierEscalated = tierEscalated; }

    public String getStatus() { return status; }
    public void setStatus(String status) { this.status = status; }

    public String getRiskId() { return riskId; }
    public void setRiskId(String riskId) { this.riskId = riskId; }

    public String getSeverity() { return severity; }
    public void setSeverity(String severity) { this.severity = severity; }

    public Double getConfidence() { return confidence; }
    public void setConfidence(Double confidence) { this.confidence = confidence; }

    public Integer getTotalTurns() { return totalTurns; }
    public void setTotalTurns(Integer totalTurns) { this.totalTurns = totalTurns; }

    public Integer getTotalToolCalls() { return totalToolCalls; }
    public void setTotalToolCalls(Integer totalToolCalls) { this.totalToolCalls = totalToolCalls; }

    public Integer getTotalSearchCalls() { return totalSearchCalls; }
    public void setTotalSearchCalls(Integer totalSearchCalls) { this.totalSearchCalls = totalSearchCalls; }

    public Integer getEventCount() { return eventCount; }
    public void setEventCount(Integer eventCount) { this.eventCount = eventCount; }

    public LocalDateTime getStartedAt() { return startedAt; }
    public void setStartedAt(LocalDateTime startedAt) { this.startedAt = startedAt; }

    public LocalDateTime getFinishedAt() { return finishedAt; }
    public void setFinishedAt(LocalDateTime finishedAt) { this.finishedAt = finishedAt; }

    public String getErrorMessage() { return errorMessage; }
    public void setErrorMessage(String errorMessage) { this.errorMessage = errorMessage; }

    public String getMeta() { return meta; }
    public void setMeta(String meta) { this.meta = meta; }
}
