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

    private String reference;

    private LocalDateTime createTime;
}
