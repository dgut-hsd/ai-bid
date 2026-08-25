package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.QueryWrapper;
import com.ithsd.smart_tender.mapper.AuditIssueMapper;
import com.ithsd.smart_tender.mapper.AuditReportMapper;
import com.ithsd.smart_tender.mapper.AuditTaskMapper;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.entity.AuditIssue;
import com.ithsd.smart_tender.model.entity.AuditReport;
import com.ithsd.smart_tender.model.entity.AuditTask;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.model.entity.User;
import com.ithsd.smart_tender.model.enums.AuditTaskStatusEnum;
import com.ithsd.smart_tender.model.vo.ReportVO;
import com.ithsd.smart_tender.service.ReportService;
import lombok.RequiredArgsConstructor;
import org.springframework.beans.BeanUtils;
import org.springframework.stereotype.Service;

import java.time.LocalDateTime;
import java.time.format.DateTimeFormatter;
import java.util.List;
import java.util.Map;
import java.util.stream.Collectors;

@Service
@RequiredArgsConstructor
public class ReportServiceImpl implements ReportService {

    private final AuditTaskMapper auditTaskMapper;
    private final AuditIssueMapper auditIssueMapper;
    private final AuditReportMapper auditReportMapper;
    private final TenderMapper tenderMapper;
    private final UserMapper userMapper;

    @Override
    public ReportVO generateReport(String auditIdOrTaskId) {
        Long tenantId = TenantScope.requiredTenantId();
        Long auditId = resolveAuditId(auditIdOrTaskId);
        AuditTask task = auditTaskMapper.selectOne(new QueryWrapper<AuditTask>()
                .eq("id", auditId)
                .eq("tenant_id", tenantId));
        if (task == null) {
            throw TenantScope.resourceNotFound();
        }
        
        if (task.getTaskStatus() != 2) {
            throw new RuntimeException("审核任务尚未完成，无法生成报告");
        }

        QueryWrapper<AuditReport> existWrapper = new QueryWrapper<>();
        existWrapper.eq("audit_id", auditId)
                .eq("tenant_id", tenantId);
        AuditReport existReport = auditReportMapper.selectOne(existWrapper);
        if (existReport != null) {
            ReportVO vo = new ReportVO();
            BeanUtils.copyProperties(existReport, vo);
            return vo;
        }

        Tender tender = tenderMapper.selectOne(new QueryWrapper<Tender>()
                .eq("id", task.getBidId())
                .eq("tenant_id", tenantId));
        if (tender == null) {
            throw TenantScope.resourceNotFound();
        }

        QueryWrapper<AuditIssue> issueWrapper = new QueryWrapper<>();
        issueWrapper.eq("audit_id", auditId)
                    .eq("tenant_id", tenantId)
                    .orderByAsc("severity")
                    .orderByAsc("category");
        List<AuditIssue> issues = auditIssueMapper.selectList(issueWrapper);

        User auditor = null;
        if (task.getAuditUserId() != null) {
            auditor = userMapper.selectById(task.getAuditUserId());
        }

        String docContent = generateMarkdownReport(task, tender, issues, auditor);

        AuditReport report = new AuditReport();
        report.setTenantId(tenantId);
        report.setAuditId(auditId);
        report.setDocContent(docContent);
        report.setGenerateTime(LocalDateTime.now());
        
        auditReportMapper.insert(report);

        ReportVO vo = new ReportVO();
        BeanUtils.copyProperties(report, vo);
        return vo;
    }

    @Override
    public String getReportContent(String auditIdOrTaskId) {
        Long tenantId = TenantScope.requiredTenantId();
        Long auditId = resolveAuditId(auditIdOrTaskId);
        QueryWrapper<AuditReport> wrapper = new QueryWrapper<>();
        wrapper.eq("audit_id", auditId)
                .eq("tenant_id", tenantId);
        AuditReport report = auditReportMapper.selectOne(wrapper);
        
        if (report == null) {
            throw TenantScope.resourceNotFound();
        }
        
        return report.getDocContent();
    }

    private Long resolveAuditId(String auditIdOrTaskId) {
        Long tenantId = TenantScope.requiredTenantId();
        if (auditIdOrTaskId == null || auditIdOrTaskId.isBlank()) {
            throw TenantScope.resourceNotFound();
        }
        try {
            Long numericId = Long.parseLong(auditIdOrTaskId);
            // 1) 前端路由参数当前语义是 bidId：优先按 bid_id 找最新任务，避免与 audit_task.id 数值冲突
            QueryWrapper<AuditTask> byBidIdWrapper = new QueryWrapper<>();
            byBidIdWrapper.eq("bid_id", numericId)
                    .eq("tenant_id", tenantId)
                    .orderByDesc("create_time")
                    .last("LIMIT 1");
            AuditTask byBidId = auditTaskMapper.selectOne(byBidIdWrapper);
            if (byBidId != null && byBidId.getId() != null) {
                return byBidId.getId();
            }
            // 2) 回退兼容：若不是 bidId，再按 audit_task 主键(id)解释
            AuditTask byAuditId = auditTaskMapper.selectOne(new QueryWrapper<AuditTask>()
                    .eq("id", numericId)
                    .eq("tenant_id", tenantId));
            if (byAuditId != null && byAuditId.getId() != null) {
                return byAuditId.getId();
            }
            if (TenantScope.requiredTenantId() != null) {
                throw TenantScope.resourceNotFound();
            }
            throw new RuntimeException("审核任务不存在");
        } catch (NumberFormatException ignore) {
            QueryWrapper<AuditTask> wrapper = new QueryWrapper<>();
            wrapper.eq("task_id", auditIdOrTaskId)
                    .eq("tenant_id", tenantId)
                    .last("LIMIT 1");
            AuditTask task = auditTaskMapper.selectOne(wrapper);
            if (task == null || task.getId() == null) {
                if (TenantScope.requiredTenantId() != null) {
                    throw TenantScope.resourceNotFound();
                }
                throw new RuntimeException("审核任务不存在");
            }
            return task.getId();
        }
    }

    private String generateMarkdownReport(AuditTask task, Tender tender, List<AuditIssue> issues, User auditor) {
        StringBuilder md = new StringBuilder();
        
        md.append("# 标书审核报告\n\n");
        
        md.append("**项目名称：** ").append(tender.getBidName() != null ? tender.getBidName() : "").append("  \n");
        md.append("**供应商名称：** ").append(tender.getSupplierName() != null ? tender.getSupplierName() : "").append("  \n");
        md.append("**审核日期：** ").append(LocalDateTime.now().format(DateTimeFormatter.ofPattern("yyyy年MM月dd日"))).append("  \n");
        md.append("**审核人：** ").append(auditor != null && auditor.getRealName() != null ? auditor.getRealName() : "").append("  \n");
        md.append("\n---\n\n");
        
        md.append("## 一、标书基本信息\n\n");
        md.append("| 项目 | 内容 |\n");
        md.append("| --- | --- |\n");
        md.append("| 项目名称 | ").append(tender.getBidName() != null ? tender.getBidName() : "").append(" |\n");
        md.append("| 标书类型 | ").append(getFileCategoryText(tender.getFileCategory())).append(" |\n");
        md.append("| 供应商名称 | ").append(tender.getSupplierName() != null ? tender.getSupplierName() : "").append(" |\n");
        md.append("| 预算金额 | ").append(tender.getBudgetAmount() != null ? tender.getBudgetAmount().toString() + "元" : "").append(" |\n");
        md.append("| 标书页数 | ").append(tender.getPageCount() != null ? tender.getPageCount().toString() + "页" : "").append(" |\n");
        md.append("| 上传时间 | ").append(tender.getUploadTime() != null ? 
                tender.getUploadTime().format(DateTimeFormatter.ofPattern("yyyy-MM-dd HH:mm:ss")) : "").append(" |\n");
        md.append("\n---\n\n");
        
        md.append("## 二、审核结论\n\n");
        md.append("**审核结果：** ").append(getAuditResultText(task)).append("  \n\n");
        md.append("**综合评价：** ").append(getEvaluationText(task, issues)).append("  \n");
        md.append("\n---\n\n");
        
        // 从 issues 列表计算统计（不再依赖 AuditTask 中的废弃字段）
        long issueCount = issues.size();
        long criticalCount = issues.stream().filter(i -> Boolean.TRUE.equals(i.getIsCritical())).count();
        long highCount = issues.stream()
                .filter(i -> !Boolean.TRUE.equals(i.getIsCritical()) && "high".equals(i.getSeverity()))
                .count();
        long warningCount = issues.stream()
                .filter(i -> "medium".equals(i.getSeverity()) || "low".equals(i.getSeverity()))
                .count();
        long infoCount = issues.stream().filter(i -> "info".equals(i.getSeverity())).count();

        md.append("## 三、问题汇总统计\n\n");
        md.append("| 统计项 | 数量 |\n");
        md.append("| --- | --- |\n");
        md.append("| 问题总数 | ").append(issueCount).append(" |\n");
        md.append("| 重大/红线问题数量 | ").append(criticalCount).append(" |\n");
        md.append("| 其他高风险问题数量 | ").append(highCount).append(" |\n");
        md.append("| 一般问题数量 | ").append(warningCount).append(" |\n");
        md.append("| 提示信息数量 | ").append(infoCount).append(" |\n");
        
        Map<String, Long> categoryCount = issues.stream()
                .filter(i -> i.getCategory() != null)
                .collect(Collectors.groupingBy(AuditIssue::getCategory, Collectors.counting()));
        
        StringBuilder categorySb = new StringBuilder();
        categoryCount.forEach((k, v) -> categorySb.append(getCategoryText(k)).append(": ").append(v).append("个; "));
        md.append("| 各类型问题分布 | ").append(categorySb.toString()).append(" |\n");
        md.append("\n---\n\n");
        
        md.append("## 四、详细问题列表\n\n");
        
        if (issues.isEmpty()) {
            md.append("本次审核未发现问题。\n");
        } else {
            Map<String, List<AuditIssue>> groupedIssues = issues.stream()
                    .collect(Collectors.groupingBy(
                            issue -> {
                                if (Boolean.TRUE.equals(issue.getIsCritical())) return "critical";
                                String severity = issue.getSeverity();
                                if ("high".equals(severity)) return "high";
                                if ("medium".equals(severity) || "low".equals(severity)) return "warning";
                                return "info";
                            }
                    ));
            
            appendIssuesBySeverity(md, groupedIssues, "critical", "重大/红线问题");
            appendIssuesBySeverity(md, groupedIssues, "high", "其他高风险问题");
            appendIssuesBySeverity(md, groupedIssues, "warning", "一般问题");
            appendIssuesBySeverity(md, groupedIssues, "info", "提示信息");
        }
        
        md.append("\n---\n\n");
        
        md.append("## 五、审核说明\n\n");
        md.append("**审核依据：** 学校采购制度文件、价格参考标准等相关标准库文件  \n");
        md.append("**审核时间：** ").append(LocalDateTime.now().format(DateTimeFormatter.ofPattern("yyyy年MM月dd日 HH:mm:ss"))).append("  \n");
        md.append("**审核人签名：** ").append(auditor != null && auditor.getRealName() != null ? auditor.getRealName() : "").append("  \n");
        
        return md.toString();
    }

    private void appendIssuesBySeverity(StringBuilder md, Map<String, List<AuditIssue>> groupedIssues, 
                                        String severity, String severityText) {
        List<AuditIssue> severityIssues = groupedIssues.get(severity);
        if (severityIssues == null || severityIssues.isEmpty()) {
            return;
        }

        md.append("### ").append(severityText).append("\n\n");
        
        int index = 1;
        for (AuditIssue issue : severityIssues) {
            md.append("#### ").append(index).append(". ").append(getCategoryText(issue.getCategory())).append("\n\n");
            md.append("- **问题描述：** ").append(issue.getDescription() != null ? issue.getDescription() : "").append("\n");

            if (Boolean.TRUE.equals(issue.getIsCritical())
                    && issue.getCriticalReason() != null
                    && !issue.getCriticalReason().isEmpty()) {
                md.append("- **重大问题依据：** ").append(issue.getCriticalReason()).append("\n");
            }
            
            if (issue.getPageNumber() != null) {
                md.append("- **所在位置：** 第").append(issue.getPageNumber()).append("页");
                if (issue.getSectionName() != null) {
                    md.append(" - ").append(issue.getSectionName());
                }
                md.append("\n");
            }
            
            if (issue.getSuggestion() != null && !issue.getSuggestion().isEmpty()) {
                md.append("- **修改建议：** ").append(issue.getSuggestion()).append("\n");
            }
            
            if (issue.getReference() != null && !issue.getReference().isEmpty()) {
                md.append("- **标准依据：** ").append(issue.getReference()).append("\n");
            }
            
            md.append("\n");
            index++;
        }
    }

    private String getFileCategoryText(String fileCategory) {
        if (fileCategory == null) return "";
        return switch (fileCategory) {
            case "bid" -> "投标文件";
            case "contract" -> "合同文件";
            default -> fileCategory;
        };
    }

    private String getAuditResultText(AuditTask task) {
        if (task.getTaskStatus() == null) return "待审核";
        Integer status = task.getTaskStatus();
        if (AuditTaskStatusEnum.COMPLETED.getCode().equals(status)) return "✓ 已完成";
        if (AuditTaskStatusEnum.FAILED.getCode().equals(status)) return "✗ 失败";
        if (AuditTaskStatusEnum.PROCESSING.getCode().equals(status)) return "⏳ 进行中";
        return "待审核";
    }

    private String getEvaluationText(AuditTask task, List<AuditIssue> issues) {
        if (!AuditTaskStatusEnum.COMPLETED.getCode().equals(task.getTaskStatus())) {
            return "审核尚未完成";
        }

        long criticalCount = issues.stream().filter(i -> Boolean.TRUE.equals(i.getIsCritical())).count();
        long highCount = issues.stream().filter(i -> "high".equals(i.getSeverity())).count();
        long warningCount = issues.stream()
                .filter(i -> "medium".equals(i.getSeverity()) || "low".equals(i.getSeverity()))
                .count();

        if (criticalCount > 0) {
            return "标书存在重大/红线问题，建议不通过或整改后重新审核。";
        } else if (highCount > 0) {
            return "标书存在高风险问题，建议优先修改并由专业人员复核。";
        } else if (warningCount > 0) {
            return "标书存在一般性问题，建议供应商进行修改完善。";
        } else {
            return "标书审核通过，符合相关规范要求。";
        }
    }

    // category 列存的是 Rust 引擎的 risk_type（如"地域歧视"/"品牌指定"/"程序违规"），直接展示即可
    private String getCategoryText(String category) {
        return (category == null || category.isBlank()) ? "其他" : category;
    }
}
