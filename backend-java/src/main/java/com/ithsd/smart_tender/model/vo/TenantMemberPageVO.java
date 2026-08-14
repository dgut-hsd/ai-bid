package com.ithsd.smart_tender.model.vo;

import lombok.AllArgsConstructor;
import lombok.Builder;
import lombok.Data;
import lombok.NoArgsConstructor;

import java.io.Serializable;
import java.util.List;

@Data
@Builder
@NoArgsConstructor
@AllArgsConstructor
public class TenantMemberPageVO implements Serializable {

    private int page;
    private int size;
    private long total;
    private List<TenantMemberVO> items;
}
