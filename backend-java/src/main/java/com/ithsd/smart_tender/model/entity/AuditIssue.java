package com.ithsd.smart_tender.model.entity;

import com.baomidou.mybatisplus.annotation.IdType;
import com.baomidou.mybatisplus.annotation.TableField;
import com.baomidou.mybatisplus.annotation.TableId;
import com.baomidou.mybatisplus.annotation.TableName;
import lombok.AllArgsConstructor;
import lombok.Builder;
import lombok.Data;
import lombok.NoArgsConstructor;

import java.io.Serializable;
import java.time.LocalDateTime;

@Data
@Builder
@NoArgsConstructor
@AllArgsConstructor
@TableName("audit_issue")
public class AuditIssue implements Serializable {
    @TableId(value = "id", type = IdType.AUTO)
    private Long id;

    @TableField("tenant_id")
    private Long tenantId;

    private Long auditId;

    private String issueNo;

    @TableField("risk_id")
    private String riskId;

    private String severity;

    @TableField("is_critical")
    private Boolean isCritical;

    @TableField("critical_reason")
    private String criticalReason;

    private String category;

    private String description;

    private String suggestion;

    private Integer pageNumber;

    private String sectionName;

    private String context;

    @TableField("block_ids")
    private String blockIds;

    @TableField("highlight_rects")
    private String highlightRects;

    private String reference;

    /** 置信度 [0,1]；审核完成后 GET /result 从 DB 回退读取时保证不丢 */
    private Double confidence;

    private LocalDateTime createTime;
}
