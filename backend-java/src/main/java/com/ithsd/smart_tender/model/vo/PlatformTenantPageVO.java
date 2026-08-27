package com.ithsd.smart_tender.model.vo;

import lombok.AllArgsConstructor;
import lombok.Builder;
import lombok.Data;
import lombok.NoArgsConstructor;

import java.io.Serializable;
import java.util.List;

/** 平台管理员的企业分页结果。 */
@Data
@Builder
@NoArgsConstructor
@AllArgsConstructor
public class PlatformTenantPageVO implements Serializable {

    private int page;
    private int size;
    private long total;
    private List<PlatformTenantVO> items;
}