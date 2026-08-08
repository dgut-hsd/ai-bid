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
@TableName("tenant")
public class Tenant implements Serializable {
    @TableId(value = "id", type = IdType.AUTO)
    private Long id;
    @TableField("tenant_code")
    private String tenantCode;
    private String name;
    private String status;
    @TableField("owner_user_id")
    private Long ownerUserId;
    @TableField("plan_code")
    private String planCode;
    @TableField("settings_json")
    private String settingsJson;
    private Long version;
    @TableField("created_at")
    private LocalDateTime createdAt;
    @TableField("updated_at")
    private LocalDateTime updatedAt;
    @TableField("deleted_at")
    private LocalDateTime deletedAt;
}
