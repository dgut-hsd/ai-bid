package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.baomidou.mybatisplus.core.conditions.query.QueryWrapper;
import com.baomidou.mybatisplus.core.toolkit.CollectionUtils;
import com.ithsd.smart_tender.common.BaseContext;
import com.ithsd.smart_tender.mapper.AuditIssueMapper;
import com.ithsd.smart_tender.mapper.AuditReportMapper;
import com.ithsd.smart_tender.mapper.AuditTaskMapper;
import com.ithsd.smart_tender.mapper.ProjectMapper;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.model.dto.ProjectDTO;
import com.ithsd.smart_tender.model.entity.AuditIssue;
import com.ithsd.smart_tender.model.entity.AuditReport;
import com.ithsd.smart_tender.model.entity.AuditTask;
import com.ithsd.smart_tender.model.entity.Project;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.model.enums.AuditTaskStatusEnum;
import com.ithsd.smart_tender.model.vo.ProjectVO;
import com.ithsd.smart_tender.model.vo.TenderWithAuditVO;
import com.ithsd.smart_tender.service.ProjectService;
import lombok.RequiredArgsConstructor;
import org.springframework.beans.BeanUtils;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;

import java.io.File;
import java.time.LocalDateTime;
import java.util.ArrayList;
import java.util.List;
import java.util.stream.Collectors;

@Service
@RequiredArgsConstructor
public class ProjectServiceImpl implements ProjectService {

    private final ProjectMapper projectMapper;
    private final TenderMapper tenderMapper;
    private final AuditTaskMapper auditTaskMapper;
    private final AuditIssueMapper auditIssueMapper;
    private final AuditReportMapper auditReportMapper;

    @Override
    public ProjectVO create(ProjectDTO projectDTO) {
        Long tenantId = TenantScope.requiredTenantId();
        Project project = new Project();
        BeanUtils.copyProperties(projectDTO, project);
        project.setTenantId(tenantId);
        project.setUserId(BaseContext.getCurrentId());
        project.setParseStatus(0);
        project.setLatestVersion(0);
        project.setCreateTime(LocalDateTime.now());
        project.setUpdateTime(LocalDateTime.now());
        
        projectMapper.insert(project);
        
        ProjectVO vo = new ProjectVO();
        BeanUtils.copyProperties(project, vo);
        return vo;
    }

    @Override
    @Transactional
    public void delete(Long id) {
        Long tenantId = TenantScope.requiredTenantId();
        Project project = projectMapper.selectOne(new LambdaQueryWrapper<Project>()
                .eq(Project::getId, id)
                .eq(Project::getTenantId, tenantId));
        if (project == null) {
            throw TenantScope.resourceNotFound();
        }
        // 1. 查找项目下的所有标书
        List<Tender> tenders = tenderMapper.selectList(
                new LambdaQueryWrapper<Tender>().eq(Tender::getProjectId, id)
                        .eq(Tender::getTenantId, tenantId)
        );

        List<Long> bidIds = tenders.stream().map(Tender::getId).collect(Collectors.toList());
        if (CollectionUtils.isNotEmpty(bidIds)) {
            // 2. 按 bidId 批量查出审核任务，再级联清理历史数据（问题/报告/任务）
            List<AuditTask> tasks = auditTaskMapper.selectList(
                    new LambdaQueryWrapper<AuditTask>().in(AuditTask::getBidId, bidIds)
                            .eq(AuditTask::getTenantId, tenantId)
            );
            List<Long> auditIds = tasks.stream().map(AuditTask::getId).collect(Collectors.toList());
            if (CollectionUtils.isNotEmpty(auditIds)) {
                auditIssueMapper.delete(
                        new LambdaQueryWrapper<AuditIssue>().in(AuditIssue::getAuditId, auditIds)
                );
                auditReportMapper.delete(
                        new LambdaQueryWrapper<AuditReport>().in(AuditReport::getAuditId, auditIds)
                );
                auditTaskMapper.delete(
                        new LambdaQueryWrapper<AuditTask>().in(AuditTask::getId, auditIds)
                                .eq(AuditTask::getTenantId, tenantId)
                );
            }
        }

        for (Tender tender : tenders) {
            // 3. 删除标书文件
            if (tender.getFilePath() != null) {
                File file = new File(tender.getFilePath());
                if (file.exists()) {
                    file.delete();
                }
            }

            // 4. 删除标书记录
            tenderMapper.delete(new LambdaQueryWrapper<Tender>()
                    .eq(Tender::getId, tender.getId())
                    .eq(Tender::getTenantId, tenantId));
        }

        // 5. 删除项目记录
        projectMapper.delete(new LambdaQueryWrapper<Project>()
                .eq(Project::getId, id)
                .eq(Project::getTenantId, tenantId));
    }

    @Override
    public ProjectVO update(ProjectDTO projectDTO) {
        Long tenantId = TenantScope.requiredTenantId();
        Project project = projectMapper.selectOne(new LambdaQueryWrapper<Project>()
                .eq(Project::getId, projectDTO.getId())
                .eq(Project::getTenantId, tenantId));
        if (project == null) {
            throw TenantScope.resourceNotFound();
        }
        if (project != null) {
            if (projectDTO.getProjectName() != null) {
                project.setProjectName(projectDTO.getProjectName());
            }
            if (projectDTO.getSupplierName() != null) {
                project.setSupplierName(projectDTO.getSupplierName());
            }
            project.setUpdateTime(LocalDateTime.now());
            project.setTenantId(tenantId);
            projectMapper.update(project, new LambdaQueryWrapper<Project>()
                    .eq(Project::getId, projectDTO.getId())
                    .eq(Project::getTenantId, tenantId));
            
            ProjectVO vo = new ProjectVO();
            BeanUtils.copyProperties(project, vo);
            return vo;
        }
        return null;
    }

    @Override
    public List<ProjectVO> listAll() {
        Long tenantId = TenantScope.requiredTenantId();
        Long userId = BaseContext.getCurrentId();
        List<Project> projects = projectMapper.selectList(
                new LambdaQueryWrapper<Project>()
                        .eq(Project::getTenantId, tenantId)
                        .eq(Project::getUserId, userId)
                        .orderByDesc(Project::getCreateTime)
        );

        List<ProjectVO> projectVOs = new ArrayList<>();

        for (Project project : projects) {
            ProjectVO pVO = new ProjectVO();
            BeanUtils.copyProperties(project, pVO);

            // 获取项目下的所有标书
            List<Tender> tenders = tenderMapper.selectList(
                    new LambdaQueryWrapper<Tender>()
                            .eq(Tender::getProjectId, project.getId())
                            .eq(Tender::getTenantId, tenantId)
                            .orderByDesc(Tender::getVersion)
            );

            if (!tenders.isEmpty()) {
                pVO.setFileCategory(tenders.get(0).getFileCategory());
            }

            List<TenderWithAuditVO> tenderWithAuditVOs = new ArrayList<>();

            for (Tender tender : tenders) {
                TenderWithAuditVO tVO = new TenderWithAuditVO();
                tVO.setTender(tender);

                // 获取标书最新的一条审核任务
                AuditTask task = auditTaskMapper.selectOne(
                        new LambdaQueryWrapper<AuditTask>()
                                .eq(AuditTask::getBidId, tender.getId())
                                .eq(AuditTask::getTenantId, tenantId)
                                .orderByDesc(AuditTask::getCreateTime)
                                .last("LIMIT 1")
                );

                if (task != null) {
                    tVO.setAuditTask(task);
                    AuditReport report = auditReportMapper.selectOne(
                            new LambdaQueryWrapper<AuditReport>().eq(AuditReport::getAuditId, task.getId())
                    );
                    tVO.setAuditReport(report);
                }

                tenderWithAuditVOs.add(tVO);
            }

            pVO.setTenders(tenderWithAuditVOs);
            Tender latestTender = tenders.isEmpty() ? null : tenders.get(0);
            Integer currentParseStatus =
                    latestTender == null ? 0 : resolveParseStatusFromLatestTask(latestTender.getId());
            pVO.setParseStatus(currentParseStatus);
            pVO.setAuditResult(latestTender == null ? null : resolveAuditResultFromLatestTask(latestTender.getId()));
            pVO.setLatestVersion(latestTender == null ? project.getLatestVersion() : latestTender.getVersion());

            projectVOs.add(pVO);
        }

        return projectVOs;
    }

    @Override
    public List<ProjectVO> getMyProjects() {
        Long tenantId = TenantScope.requiredTenantId();
        Long userId = BaseContext.getCurrentId();
        List<Project> projects = projectMapper.selectList(
                new LambdaQueryWrapper<Project>()
                        .eq(Project::getTenantId, tenantId)
                        .eq(Project::getUserId, userId)
                        .orderByDesc(Project::getCreateTime)
        );

        List<ProjectVO> projectVOs = new ArrayList<>();
        for (Project project : projects) {
            ProjectVO pVO = new ProjectVO();
            BeanUtils.copyProperties(project, pVO);
            // 这里不需要查询标书，只需要 project 表的数据
            projectVOs.add(pVO);
        }
        return projectVOs;
    }

    @Override
    public boolean exists(String projectName) {
        Long tenantId = TenantScope.requiredTenantId();
        // 判断是否已存在该名字的项目
        QueryWrapper<Project> projectQueryWrapper = new QueryWrapper<>();
        projectQueryWrapper.eq("tenant_id", tenantId)
                .eq("project_name", projectName);
        Project project = projectMapper.selectOne(projectQueryWrapper);
        return project != null;
    }

    private AuditTask findLatestTaskByBidId(Long bidId) {
        if (bidId == null) {
            return null;
        }
        return auditTaskMapper.selectOne(
                new LambdaQueryWrapper<AuditTask>()
                        .eq(AuditTask::getBidId, bidId)
                        .eq(AuditTask::getTenantId, TenantScope.requiredTenantId())
                        .orderByDesc(AuditTask::getCreateTime)
                        .last("LIMIT 1")
        );
    }

    private Integer resolveParseStatusFromLatestTask(Long bidId) {
        AuditTask task = findLatestTaskByBidId(bidId);
        if (task == null || task.getTaskStatus() == null) {
            return 0;
        }
        Integer taskStatus = task.getTaskStatus();
        if (AuditTaskStatusEnum.PENDING.getCode().equals(taskStatus)
                || AuditTaskStatusEnum.PROCESSING.getCode().equals(taskStatus)) {
            return 1;
        }
        if (AuditTaskStatusEnum.COMPLETED.getCode().equals(taskStatus)) {
            return 2;
        }
        if (AuditTaskStatusEnum.FAILED.getCode().equals(taskStatus)) {
            return 3;
        }
        return 0;
    }

    private String resolveAuditResultFromLatestTask(Long bidId) {
        AuditTask task = findLatestTaskByBidId(bidId);
        if (task == null) {
            return null;
        }
        // auditResult 已废弃，基于 taskStatus 映射结果
        Integer status = task.getTaskStatus();
        if (AuditTaskStatusEnum.COMPLETED.getCode().equals(status)) return "pass";
        if (AuditTaskStatusEnum.FAILED.getCode().equals(status)) return "reject";
        return "pending";
    }
}
