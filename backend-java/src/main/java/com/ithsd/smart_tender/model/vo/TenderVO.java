package com.ithsd.smart_tender.model.vo;

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
public class TenderVO implements Serializable {
    private Long id;
    private String fileName;
    private String filePath;
    private Long fileSize;
    private String fileType; // word/pdf
    private String fileCategory; // bid/contract
    private String bidName;
    private String supplierName;
    private BigDecimal budgetAmount;
    private Integer pageCount;
    private Integer parseStatus; // 0:已审核 1:未审核
    private Long uploadUserId;
    private LocalDateTime uploadTime;
    private Integer version;
    private Long projectId;
    private String auditorName; // 审核人姓名
}
