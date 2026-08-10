package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.QueryWrapper;
import com.baomidou.mybatisplus.extension.plugins.pagination.Page;
import com.ithsd.smart_tender.mapper.AuditIssueMapper;
import com.ithsd.smart_tender.mapper.AuditReportMapper;
import com.ithsd.smart_tender.mapper.AuditTaskMapper;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.dto.AuditHistoryPageQueryDTO;
import com.ithsd.smart_tender.model.entity.AuditIssue;
import com.ithsd.smart_tender.model.entity.AuditReport;
import com.ithsd.smart_tender.model.entity.AuditTask;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.model.entity.User;
import com.ithsd.smart_tender.model.enums.AuditTaskStatusEnum;
import com.ithsd.smart_tender.model.result.PageResult;
import com.ithsd.smart_tender.model.vo.AuditHistoryDetailVO;
import com.ithsd.smart_tender.model.vo.AuditHistoryVO;
import com.ithsd.smart_tender.model.vo.AuditIssueVO;
import com.ithsd.smart_tender.service.AuditHistoryService;
import lombok.RequiredArgsConstructor;
import org.springframework.beans.BeanUtils;
import org.springframework.stereotype.Service;
import org.springframework.util.StringUtils;

import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.LocalTime;
import java.time.format.DateTimeFormatter;
import java.util.*;
import java.util.stream.Collectors;

@Service
@RequiredArgsConstructor
public class AuditHistoryServiceImpl implements AuditHistoryService {

    private final AuditTaskMapper auditTaskMapper;
    private final AuditIssueMapper auditIssueMapper;
    private final AuditReportMapper auditReportMapper;
    private final TenderMapper tenderMapper;
    private final UserMapper userMapper;

    @Override
    public PageResult page(AuditHistoryPageQueryDTO dto) {
        Long tenantId = TenantScope.requiredTenantId();
        Page<AuditTask> pageInfo = new Page<>(dto.getPage(), dto.getSize());
        LambdaQueryWrapper<AuditTask> wrapper = new LambdaQueryWrapper<>();

        wrapper.eq(AuditTask::getTenantId, tenantId)
               .eq(dto.getAuditUserId() != null, AuditTask::getAuditUserId, dto.getAuditUserId())
               .ge(dto.getStartDate() != null, AuditTask::getCreateTime, dto.getStartDate().atStartOfDay())
               .le(dto.getEndDate() != null, AuditTask::getCreateTime, dto.getEndDate().atTime(LocalTime.MAX))
               .orderByDesc(AuditTask::getCreateTime);
        
        Page<AuditTask> p = auditTaskMapper.selectPage(pageInfo, wrapper);
        
        List<AuditHistoryVO> vos = p.getRecords().stream().map(task -> {
            AuditHistoryVO vo = new AuditHistoryVO();
            BeanUtils.copyProperties(task, vo);

            Tender tender = tenderMapper.selectOne(new LambdaQueryWrapper<Tender>()
                    .eq(Tender::getId, task.getBidId())
                    .eq(Tender::getTenantId, tenantId));
            if (tender != null) {
                vo.setProjectName(tender.getBidName());
                vo.setFileCategory(tender.getFileCategory());
                vo.setSupplierName(tender.getSupplierName());
                vo.setBudgetAmount(tender.getBudgetAmount());
            }

            if (task.getAuditUserId() != null) {
                User user = userMapper.selectById(task.getAuditUserId());
                if (user != null) {
                    vo.setAuditUserName(user.getRealName());
                }
            }

            return vo;
        }).collect(Collectors.toList());
        
        if (StringUtils.hasText(dto.getFileCategory())) {
            vos = vos.stream()
                    .filter(vo -> dto.getFileCategory().equals(vo.getFileCategory()))
                    .collect(Collectors.toList());
        }
        
        if (StringUtils.hasText(dto.getProjectName())) {
            vos = vos.stream()
                    .filter(vo -> vo.getProjectName() != null && 
                            vo.getProjectName().contains(dto.getProjectName()))
                    .collect(Collectors.toList());
        }
        
        return new PageResult(p.getTotal(), vos);
    }

    @Override
    public AuditHistoryDetailVO getDetailById(Long id) {
        Long tenantId = TenantScope.requiredTenantId();
        AuditTask task = auditTaskMapper.selectOne(new LambdaQueryWrapper<AuditTask>()
                .eq(AuditTask::getId, id)
                .eq(AuditTask::getTenantId, tenantId));
        if (task == null) {
            throw TenantScope.resourceNotFound();
        }

        AuditHistoryDetailVO vo = new AuditHistoryDetailVO();
        BeanUtils.copyProperties(task, vo);

        Tender tender = tenderMapper.selectOne(new LambdaQueryWrapper<Tender>()
                .eq(Tender::getId, task.getBidId())
                .eq(Tender::getTenantId, tenantId));
        if (tender == null) {
            throw TenantScope.resourceNotFound();
        }
        if (tender != null) {
            vo.setFileName(tender.getFileName());
            vo.setFileType(tender.getFileType());
            vo.setProjectName(tender.getBidName());
            vo.setFileCategory(tender.getFileCategory());
            vo.setSupplierName(tender.getSupplierName());
            vo.setBudgetAmount(tender.getBudgetAmount());
            vo.setPageCount(tender.getPageCount());
        }

        if (task.getAuditUserId() != null) {
            User user = userMapper.selectById(task.getAuditUserId());
            if (user != null) {
                vo.setAuditUserName(user.getRealName());
            }
        }

        LambdaQueryWrapper<AuditIssue> issueWrapper = new LambdaQueryWrapper<>();
        issueWrapper.eq(AuditIssue::getAuditId, id)
                    .eq(AuditIssue::getTenantId, tenantId)
                    .orderByAsc(AuditIssue::getSeverity)
                    .orderByAsc(AuditIssue::getCategory);
        List<AuditIssue> issues = auditIssueMapper.selectList(issueWrapper);

        List<AuditIssueVO> issueVOs = issues.stream().map(issue -> {
            AuditIssueVO issueVO = new AuditIssueVO();
            BeanUtils.copyProperties(issue, issueVO);
            return issueVO;
        }).collect(Collectors.toList());
        vo.setIssues(issueVOs);

        LambdaQueryWrapper<AuditReport> reportWrapper = new LambdaQueryWrapper<>();
        reportWrapper.eq(AuditReport::getAuditId, id)
                     .eq(AuditReport::getTenantId, tenantId);
        AuditReport report = auditReportMapper.selectOne(reportWrapper);
        if (report != null) {
            vo.setDocContent(report.getDocContent());
            vo.setReportGenerateTime(report.getGenerateTime());
        }

        return vo;
    }

    @Override
    public void delete(Long id) {
        Long tenantId = TenantScope.requiredTenantId();
        AuditTask task = auditTaskMapper.selectOne(new LambdaQueryWrapper<AuditTask>()
                .eq(AuditTask::getId, id)
                .eq(AuditTask::getTenantId, tenantId));
        if (task == null) {
            throw TenantScope.resourceNotFound();
        }
        {
            // 子表删除同样带 tenant_id，避免越权删除他租户同 audit_id 的行
            LambdaQueryWrapper<AuditIssue> issueWrapper = new LambdaQueryWrapper<>();
            issueWrapper.eq(AuditIssue::getAuditId, id)
                        .eq(AuditIssue::getTenantId, tenantId);
            auditIssueMapper.delete(issueWrapper);

            LambdaQueryWrapper<AuditReport> reportWrapper = new LambdaQueryWrapper<>();
            reportWrapper.eq(AuditReport::getAuditId, id)
                         .eq(AuditReport::getTenantId, tenantId);
            auditReportMapper.delete(reportWrapper);

            auditTaskMapper.delete(new LambdaQueryWrapper<AuditTask>()
                    .eq(AuditTask::getId, id)
                    .eq(AuditTask::getTenantId, tenantId));
        }
    }

    @Override
    public Map<String, Object> getStatistics(AuditHistoryPageQueryDTO dto) {
        Long tenantId = TenantScope.requiredTenantId();
        Map<String, Object> result = new HashMap<>();

        LambdaQueryWrapper<AuditTask> wrapper = new LambdaQueryWrapper<>();
        wrapper.eq(AuditTask::getTenantId, tenantId)
               .eq(AuditTask::getTaskStatus, 2);

        if (dto.getAuditUserId() != null) {
            wrapper.eq("audit_user_id", dto.getAuditUserId());
        }
        if (dto.getStartDate() != null) {
            wrapper.ge("create_time", dto.getStartDate().atStartOfDay());
        }
        if (dto.getEndDate() != null) {
            wrapper.le("create_time", dto.getEndDate().atTime(LocalTime.MAX));
        }

        List<AuditTask> tasks = auditTaskMapper.selectList(wrapper);

        List<Long> bidIds = tasks.stream()
                .map(AuditTask::getBidId)
                .distinct()
                .collect(Collectors.toList());

        Map<Long, Tender> tenderMap = new HashMap<>();
        if (!bidIds.isEmpty()) {
            List<Tender> tenders = tenderMapper.selectList(new LambdaQueryWrapper<Tender>()
                    .in(Tender::getId, bidIds)
                    .eq(Tender::getTenantId, tenantId));
            tenderMap = tenders.stream()
                    .collect(Collectors.toMap(Tender::getId, t -> t));
        }
        
        final Map<Long, Tender> finalTenderMap = tenderMap;
        
        List<AuditTask> filteredTasks = tasks.stream()
                .filter(task -> {
                    Tender tender = finalTenderMap.get(task.getBidId());
                    if (tender == null) return false;
                    
                    if (StringUtils.hasText(dto.getProjectName())) {
                        if (tender.getBidName() == null || 
                            !tender.getBidName().contains(dto.getProjectName())) {
                            return false;
                        }
                    }
                    
                    if (StringUtils.hasText(dto.getFileCategory())) {
                        if (!dto.getFileCategory().equals(tender.getFileCategory())) {
                            return false;
                        }
                    }
                    
                    return true;
                })
                .collect(Collectors.toList());
        
        long totalCount = filteredTasks.size();
        // auditResult 已废弃，基于 taskStatus 统计（COMPLETED=通过, FAILED=拒绝）
        long passCount = filteredTasks.stream()
                .filter(t -> AuditTaskStatusEnum.COMPLETED.getCode().equals(t.getTaskStatus()))
                .count();
        long reviseCount = 0L; // 不再单独跟踪"需修改"状态
        long rejectCount = filteredTasks.stream()
                .filter(t -> AuditTaskStatusEnum.FAILED.getCode().equals(t.getTaskStatus()))
                .count();
        
        List<Map<String, Object>> statusList = new ArrayList<>();
        
        Map<String, Object> allStatus = new HashMap<>();
        allStatus.put("status", "all");
        allStatus.put("label", "全部");
        allStatus.put("count", totalCount);
        statusList.add(allStatus);
        
        Map<String, Object> passStatus = new HashMap<>();
        passStatus.put("status", "pass");
        passStatus.put("label", "已通过");
        passStatus.put("count", passCount);
        statusList.add(passStatus);
        
        Map<String, Object> reviseStatus = new HashMap<>();
        reviseStatus.put("status", "revise");
        reviseStatus.put("label", "需修改");
        reviseStatus.put("count", reviseCount);
        statusList.add(reviseStatus);
        
        Map<String, Object> rejectStatus = new HashMap<>();
        rejectStatus.put("status", "reject");
        rejectStatus.put("label", "不通过");
        rejectStatus.put("count", rejectCount);
        statusList.add(rejectStatus);
        
        result.put("statusList", statusList);
        
        return result;
    }
}
