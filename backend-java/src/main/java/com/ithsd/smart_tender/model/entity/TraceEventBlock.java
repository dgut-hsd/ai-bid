package com.ithsd.smart_tender.model.entity;

import com.baomidou.mybatisplus.annotation.IdType;
import com.baomidou.mybatisplus.annotation.TableField;
import com.baomidou.mybatisplus.annotation.TableId;
import com.baomidou.mybatisplus.annotation.TableName;

import java.io.Serializable;

/**
 * block_id 关联表 — 替代 PostgreSQL TEXT[] + GIN 索引。
 *
 * <p>支持反向查找：给定 PDF block_id，查询所有引用过它的 trace events。</p>
 */
@TableName("trace_event_blocks")
public class TraceEventBlock implements Serializable {

    @TableId(value = "id", type = IdType.AUTO)
    private Long id;

    @TableField("tenant_id")
    private Long tenantId;

    @TableField("event_id")
    private String eventId;

    @TableField("block_id")
    private String blockId;

    // ── Getters / Setters ──

    public Long getId() { return id; }
    public void setId(Long id) { this.id = id; }

    public Long getTenantId() { return tenantId; }
    public void setTenantId(Long tenantId) { this.tenantId = tenantId; }

    public String getEventId() { return eventId; }
    public void setEventId(String eventId) { this.eventId = eventId; }

    public String getBlockId() { return blockId; }
    public void setBlockId(String blockId) { this.blockId = blockId; }
}
