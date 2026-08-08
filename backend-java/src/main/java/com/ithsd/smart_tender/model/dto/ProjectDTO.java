package com.ithsd.smart_tender.model.dto;

import lombok.Data;
import java.io.Serializable;

@Data
public class ProjectDTO implements Serializable {
    private Long id; // 修改时使用
    /** Client supplied values are ignored; the service uses TenantContext. */
    private Long tenantId;
    private String projectName;
    private String supplierName;
}
