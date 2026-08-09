package com.ithsd.smart_tender.model.dto;

import jakarta.validation.constraints.NotNull;
import jakarta.validation.constraints.Positive;

import java.util.List;

/**
 * 创建审核任务请求。
 * <p>{@code enabledAgents} 直接传递 Rust Agent 名称：
 * {@code factcheck}, {@code procedure}, {@code ruleengine}, {@code semanticrisk},
 * {@code scoring}, {@code demand}, {@code contract}。为空则使用 Rust 默认全部。</p>
 */
public class CreateAuditTaskRequest {
    @NotNull(message = "bidId不能为空")
    @Positive(message = "bidId必须为正整数")
    private Long bidId;

    /** Client supplied values are ignored; the service uses TenantContext. */
    private Long tenantId;

    /** Rust Agent 名称列表（小写），为空则全部启用 */
    private List<String> enabledAgents;
    private Boolean forceRefresh;

    public Long getBidId() {
        return bidId;
    }

    public void setBidId(Long bidId) {
        this.bidId = bidId;
    }

    public Long getTenantId() {
        return tenantId;
    }

    public void setTenantId(Long tenantId) {
        this.tenantId = tenantId;
    }

    public List<String> getEnabledAgents() {
        return enabledAgents;
    }

    public void setEnabledAgents(List<String> enabledAgents) {
        this.enabledAgents = enabledAgents;
    }

    public Boolean getForceRefresh() {
        return forceRefresh;
    }

    public void setForceRefresh(Boolean forceRefresh) {
        this.forceRefresh = forceRefresh;
    }
}
