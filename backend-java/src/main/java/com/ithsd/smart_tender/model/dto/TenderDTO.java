package com.ithsd.smart_tender.model.dto;

import lombok.Data;
import java.io.Serializable;
import java.math.BigDecimal;

@Data
public class TenderDTO implements Serializable {
    private Long id; // 可选，作为更新
    /** Client supplied values are ignored; the service uses TenantContext. */
    private Long tenantId;
    private String fileCategory; // bid/contract
    private String bidName;
    private String supplierName;
    private BigDecimal budgetAmount;
    private Integer version;
    private Long projectId;
}
