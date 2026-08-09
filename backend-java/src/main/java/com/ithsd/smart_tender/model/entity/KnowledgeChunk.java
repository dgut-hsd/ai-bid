package com.ithsd.smart_tender.model.entity;

import com.baomidou.mybatisplus.annotation.IdType;
import com.baomidou.mybatisplus.annotation.TableField;
import com.baomidou.mybatisplus.annotation.TableId;
import com.baomidou.mybatisplus.annotation.TableName;
import lombok.Data;
import java.util.Date;

@Data
@TableName("knowledge_chunk")
public class KnowledgeChunk {
    @TableId(value = "id", type = IdType.AUTO)
    private Long id;

    @TableField("tenant_id")
    private Long tenantId;
    private Long fileId;
    private Integer chunkIndex;
    private String chunkText;
    private Integer chunkLength;
    private String vectorId;
    
    @TableField(exist = false)
    private String stableHash;
    @TableField(exist = false)
    private String strategyVersion;
    @TableField(exist = false)
    private String anchorJson;
    @TableField(exist = false)
    private String titlePath;
    @TableField(exist = false)
    private Integer pageStart;
    @TableField(exist = false)
    private Integer pageEnd;
    
    private Integer pageNumber;
    private String sectionName;
    private Date createTime;
}
