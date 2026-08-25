package com.ithsd.smart_tender.service;

import java.util.Map;

/**
 * @ Author：YangYu
 * @ Package：com.ithsd.smart_tender.service
 * @ Project：smart_tender
 * @ Description:
 * @ Date：2026/3/11  10:54
 */
public interface AuditIssueService {
    Map<String, Long> countByCategory();

    /** 统计当前用户本月每天发现的问题数（key=当月第几日，value=问题数） */
    Map<String, Long> countByDayCurrentMonth();
}
