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
import java.math.BigDecimal;
import java.time.LocalDateTime;

@Data
@Builder
@NoArgsConstructor
@AllArgsConstructor
@TableName("bid_document")
public class Tender implements Serializable {
    @TableId(value = "id", type = IdType.AUTO)
    private Long id;

    @TableField("tenant_id")
    private Long tenantId;
    private String fileName;
    private String filePath;
    private Long fileSize;
    private String fileType; // word/pdf
    private String fileCategory; // bid/contract
    private String bidName;
    private String supplierName;
    private BigDecimal budgetAmount;
    private Integer pageCount;
    private Integer parseStatus; // 0:Pending, 1:Processing, 2:Completed, 3:Failed
    private Long uploadUserId;
    private LocalDateTime uploadTime;
    private Integer version;
    private Long projectId;
    /** Rust 侧文档 ID（UUID），上传后由 RustDocumentService 回写，持久化到 DB */
    @TableField("rust_document_id")
    private String rustDocumentId;
}
