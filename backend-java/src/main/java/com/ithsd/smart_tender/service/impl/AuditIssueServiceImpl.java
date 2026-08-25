package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.QueryWrapper;
import com.baomidou.mybatisplus.extension.service.impl.ServiceImpl;
import com.ithsd.smart_tender.common.BaseContext;
import com.ithsd.smart_tender.mapper.AuditIssueMapper;
import com.ithsd.smart_tender.mapper.AuditTaskMapper;
import com.ithsd.smart_tender.model.entity.AuditIssue;
import com.ithsd.smart_tender.service.AuditIssueService;
import com.ithsd.smart_tender.service.AuditTaskService;
import com.ithsd.smart_tender.service.TenderService;
import lombok.RequiredArgsConstructor;
import org.springframework.stereotype.Service;

import java.util.List;
import java.util.Map;

/**
 * @ Author：YangYu
 * @ Package：com.ithsd.smart_tender.service.impl
 * @ Project：smart_tender
 * @ Description:
 * @ Date：2026/3/11  10:54
 */
@Service
@RequiredArgsConstructor
public class AuditIssueServiceImpl extends ServiceImpl<AuditIssueMapper, AuditIssue> implements AuditIssueService {

    private final TenderService tenderService;
    private final AuditTaskService auditTaskService;

    @Override
    public Map<String, Long> countByCategory() {
        List<Long> bidIds = tenderService.getBidIdsByUserId(BaseContext.getCurrentId());
        List<Long> auditIds = auditTaskService.getAuditIdsByBidIds(bidIds);
        if(auditIds.isEmpty())
            return Map.of();
        // 统计个人的审核问题类别
        QueryWrapper<AuditIssue> queryWrapper = new QueryWrapper<>();
        queryWrapper.select("category", "count(1) as count")
                .in("audit_id", auditIds)
                .groupBy("category");

        List<Map<String, Object>> result = this.baseMapper.selectMaps(queryWrapper);

        return result.stream()
                .collect(java.util.stream.Collectors.toMap(
                        row -> (String) row.get("category"),
                        row -> ((Number) row.get("count")).longValue(),
                        Long::sum
                ));

    }
}
