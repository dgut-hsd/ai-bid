package com.ithsd.smart_tender.model.entity;

import com.baomidou.mybatisplus.annotation.IdType;
import com.baomidou.mybatisplus.annotation.TableField;
import com.baomidou.mybatisplus.annotation.TableId;
import com.baomidou.mybatisplus.annotation.TableName;

import java.io.Serializable;
import java.time.LocalDateTime;

/**
 * 审查追溯事件 — 每个 ReAct 步骤对应一行。
 *
 * <p>设计文档 §10.1.1 TraceEvent，MySQL 8.0 适配版。</p>
 */
@TableName("trace_events")
public class TraceEventEntity implements Serializable {

    @TableId(value = "id", type = IdType.AUTO)
    private Long id;

    @TableField("tenant_id")
    private Long tenantId;

    @TableField("event_id")
    private String eventId;

    @TableField("session_id")
    private String sessionId;

    @TableField("agent_name")
    private String agentName;

    @TableField("event_type")
    private String eventType;

    @TableField("turn")
    private Integer turn;

    @TableField("timestamp")
    private LocalDateTime timestamp;

    @TableField("clause_id")
    private String clauseId;

    @TableField("risk_id")
    private String riskId;

    @TableField("summary")
    private String summary;

    @TableField("payload")
    private String payload;

    @TableField("created_at")
    private LocalDateTime createdAt;

    // ── Getters / Setters ──

    public Long getId() { return id; }
    public void setId(Long id) { this.id = id; }

    public Long getTenantId() { return tenantId; }
    public void setTenantId(Long tenantId) { this.tenantId = tenantId; }

    public String getEventId() { return eventId; }
    public void setEventId(String eventId) { this.eventId = eventId; }

    public String getSessionId() { return sessionId; }
    public void setSessionId(String sessionId) { this.sessionId = sessionId; }

    public String getAgentName() { return agentName; }
    public void setAgentName(String agentName) { this.agentName = agentName; }

    public String getEventType() { return eventType; }
    public void setEventType(String eventType) { this.eventType = eventType; }

    public Integer getTurn() { return turn; }
    public void setTurn(Integer turn) { this.turn = turn; }

    public LocalDateTime getTimestamp() { return timestamp; }
    public void setTimestamp(LocalDateTime timestamp) { this.timestamp = timestamp; }

    public String getClauseId() { return clauseId; }
    public void setClauseId(String clauseId) { this.clauseId = clauseId; }

    public String getRiskId() { return riskId; }
    public void setRiskId(String riskId) { this.riskId = riskId; }

    public String getSummary() { return summary; }
    public void setSummary(String summary) { this.summary = summary; }

    public String getPayload() { return payload; }
    public void setPayload(String payload) { this.payload = payload; }

    public LocalDateTime getCreatedAt() { return createdAt; }
    public void setCreatedAt(LocalDateTime createdAt) { this.createdAt = createdAt; }
}
