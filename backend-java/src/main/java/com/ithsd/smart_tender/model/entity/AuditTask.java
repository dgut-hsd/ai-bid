package com.ithsd.smart_tender.model.entity;

import com.baomidou.mybatisplus.annotation.IdType;
import com.baomidou.mybatisplus.annotation.TableField;
import com.baomidou.mybatisplus.annotation.TableId;
import com.baomidou.mybatisplus.annotation.TableName;
import com.baomidou.mybatisplus.annotation.Version;
import com.ithsd.smart_tender.common.typehandler.StringListJsonTypeHandler;
import lombok.AllArgsConstructor;
import lombok.Builder;
import lombok.Data;
import lombok.NoArgsConstructor;

import java.io.Serializable;
import java.time.LocalDateTime;
import java.util.ArrayList;
import java.util.List;

@Data
@Builder
@NoArgsConstructor
@AllArgsConstructor
@TableName("audit_task")
public class AuditTask implements Serializable {
    @TableId(value = "id", type = IdType.AUTO)
    private Long id;

    @TableField("tenant_id")
    private Long tenantId;

    private String taskId;

    private Long bidId;

    private Integer taskStatus;

    private LocalDateTime startTime;

    private LocalDateTime endTime;

    private Long auditUserId;

    private LocalDateTime createTime;

    // --- Fields merged from AuditTaskEntity (JPA-only) ---

    private String stage;

    @Builder.Default
    private Integer progress = 0;

    @TableField(typeHandler = StringListJsonTypeHandler.class)
    @Builder.Default
    private List<String> enabledChecks = new ArrayList<>();

    @TableField(typeHandler = StringListJsonTypeHandler.class)
    @Builder.Default
    private List<String> failedStages = new ArrayList<>();

    private String errorMsg;

    private LocalDateTime updatedAt;

    @Version
    @Builder.Default
    private Long version = 0L;
}
