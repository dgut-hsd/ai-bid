package com.ithsd.smart_tender.model.entity;

import com.baomidou.mybatisplus.annotation.*;
import com.baomidou.mybatisplus.annotation.IdType;
import com.baomidou.mybatisplus.annotation.TableId;
import com.baomidou.mybatisplus.annotation.TableName;
import lombok.AllArgsConstructor;
import lombok.Builder;
import lombok.Data;
import lombok.NoArgsConstructor;

import java.io.Serializable;
import java.time.LocalDateTime;

/**
 * 标准库文件实体类
 * 对应数据库表：knowledge_file
 * 封装文件的所有元数据信息
 */
@Data
@Builder
@NoArgsConstructor
@AllArgsConstructor
@TableName("knowledge_file")
public class KnowledgeFile implements Serializable {

    private static final long serialVersionUID = 1L;

    /**
     * 主键ID
     */
    @TableId(type = IdType.AUTO)
    private Long id;

    @TableField("tenant_id")
    private Long tenantId;

    /**
     * 文件名
     */
    private String fileName;

    /**
     * 存储路径
     */
    private String filePath;

    /**
     * 文件大小
     */
    private Long fileSize;

    /**
     * 文件类型（如：pdf、docx、xlsx等）
     */
    private String fileType;

    /**
     * 一级分类
     */
    private String category;

    /**
     * 二级标签（多个标签用逗号分隔）
     */
    private String tags;

    /**
     * 用途描述
     */
    private String description;

    /**
     * 适用范围（如：procurement, engineering, general）
     * 前端对应 applicableScope
     */
    private String applicableScope;

    /**
     * 状态（0停用 1启用 2已删除）
     */
    private Integer status;

    /**
     * 版本号
     */
    private Integer version;

    /**
     * 分块数量
     */
    private Integer chunkCount;

    /**
     * 上传用户ID
     */
    @TableField(value = "upload_user_id",fill = FieldFill.INSERT)
    private Long uploadUserId;

    /**
     * 上传时间
     */
    @TableField(value = "upload_time",fill = FieldFill.INSERT)
    private LocalDateTime uploadTime;

    /**
     * 更新时间
     */
    @TableField(value = "update_time",fill = FieldFill.INSERT_UPDATE)
    private LocalDateTime updateTime;
    
    /**
     * 判断文件是否有效
     */
    public boolean isValid() {
        return this.status != 2; // 不是已删除状态
    }

    
}
