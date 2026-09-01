package com.ithsd.smart_tender.controller;

import com.ithsd.smart_tender.model.result.Result;
import com.ithsd.smart_tender.service.AuditIssueService;
import lombok.RequiredArgsConstructor;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;

import java.util.Map;

@RestController
@RequestMapping("/api/audit-issues")
@RequiredArgsConstructor
public class AuditIssueController {

    private final AuditIssueService auditIssueService;

    /** 统计个人审核问题类别 */
    @GetMapping("/count-issue")
    public Result<Map<String, Long>> countByCategory() {
        return Result.success(auditIssueService.countByCategory());
    }

    /** 统计个人本月每天发现的问题数 */
    @GetMapping("/count-by-day")
    public Result<Map<String, Long>> countByDayCurrentMonth() {
        return Result.success(auditIssueService.countByDayCurrentMonth());
    }
}