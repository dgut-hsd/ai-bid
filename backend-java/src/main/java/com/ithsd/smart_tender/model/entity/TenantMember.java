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
@TableName("tenant_member")
public class TenantMember implements Serializable {
    @TableId(value = "id", type = IdType.AUTO)
    private Long id;
    @TableField("tenant_id")
    private Long tenantId;
    @TableField("user_id")
    private Long userId;
    private String role;
    private String status;
    @TableField("joined_at")
    private LocalDateTime joinedAt;
    @TableField("invited_by")
    private Long invitedBy;
    @TableField("last_seen_at")
    private LocalDateTime lastSeenAt;
}
