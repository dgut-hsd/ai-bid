package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.baomidou.mybatisplus.core.conditions.query.QueryWrapper;
import com.baomidou.mybatisplus.extension.plugins.pagination.Page;
import com.ithsd.smart_tender.common.BaseContext;
import com.ithsd.smart_tender.common.BizException;
import com.ithsd.smart_tender.mapper.AuditTaskMapper;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.model.entity.AuditTask;
import com.ithsd.smart_tender.model.enums.AuditTaskStatusEnum;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.entity.User;
import com.ithsd.smart_tender.model.entity.Project;
import com.ithsd.smart_tender.model.dto.TenderDTO;
import com.ithsd.smart_tender.model.dto.TenderPageQueryDTO;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.model.result.PageResult;
import com.ithsd.smart_tender.model.vo.TenderProjectVO;
import com.ithsd.smart_tender.model.vo.TenderStatsVO;
import com.ithsd.smart_tender.model.vo.TenderVO;

import com.ithsd.smart_tender.service.AuditTaskService;
import com.ithsd.smart_tender.service.StoragePathService;
import com.ithsd.smart_tender.model.dto.CreateAuditTaskRequest;
import com.ithsd.smart_tender.service.TenderService;
import lombok.RequiredArgsConstructor;
import lombok.extern.slf4j.Slf4j;
import org.springframework.beans.BeanUtils;
import org.springframework.stereotype.Service;
import org.springframework.context.annotation.Lazy;
import org.springframework.util.StringUtils;
import org.springframework.web.multipart.MultipartFile;

import java.io.File;
import java.io.IOException;
import java.time.LocalDateTime;
import java.util.List;
import java.util.Map;
import java.util.stream.Collectors;
import java.nio.file.Path;

@Service
@Slf4j
public class TenderServiceImpl implements TenderService {

    private final TenderMapper tenderMapper;
    private final AuditTaskMapper auditTaskMapper;
    private final UserMapper userMapper;
    private final com.ithsd.smart_tender.mapper.ProjectMapper projectMapper;
    private final AuditTaskService auditTaskService;
    private final StoragePathService storagePathService;

    public TenderServiceImpl(TenderMapper tenderMapper, AuditTaskMapper auditTaskMapper, UserMapper userMapper, com.ithsd.smart_tender.mapper.ProjectMapper projectMapper, @Lazy AuditTaskService auditTaskService, StoragePathService storagePathService) {
        this.tenderMapper = tenderMapper;
        this.auditTaskMapper = auditTaskMapper;
        this.userMapper = userMapper;
        this.projectMapper = projectMapper;
        this.auditTaskService = auditTaskService;
        this.storagePathService = storagePathService;
    }

    @Override
    public TenderStatsVO getStats(TenderPageQueryDTO dto) {
        Long tenantId = TenantScope.requiredTenantId();
        // 使用 QueryWrapper 分组统计
        QueryWrapper<Tender> wrapper = new QueryWrapper<>();
        wrapper.select("parse_status", "count(*) as count");
        wrapper.eq("tenant_id", tenantId);
        // 动态查询条件，与page接口保持一致（除status外）
        wrapper.like(StringUtils.hasText(dto.getBidName()), "bid_name", dto.getBidName());
        wrapper.eq(StringUtils.hasText(dto.getFileCategory()), "file_category", dto.getFileCategory());
        // 统计时不能根据 status 过滤，否则只能统计到单一状态的数量

        // 加上当前用户的限制
        wrapper.eq("upload_user_id", BaseContext.getCurrentId());

        wrapper.groupBy("parse_status");

        List<Map<String, Object>> list = tenderMapper.selectMaps(wrapper);

        long unreviewed = 0;
        long completed = 0;

        for (Map<String, Object> map : list) {
            Integer status = (Integer) map.get("parse_status");
            long count = ((Number) map.get("count")).longValue();

            if (status != null) {
                if (status == 0) { // 0:未审核
                    unreviewed += count;
                } else if (status == 1) { // 1:已审核
                    completed += count;
                }
            }
        }

        return TenderStatsVO.builder()
                .allCount(unreviewed + completed)
                .unreviewedCount(unreviewed)
                .completedCount(completed)
                .build();
    }

    @Override
    public TenderVO upload(MultipartFile file, TenderDTO tenderDTO) {
        Long tenantId = TenantScope.requiredTenantId();
        if (file.isEmpty()) {
            throw new BizException(400, "文件为空");
        }

        if (tenderDTO.getProjectId() != null) {
            Project project = projectMapper.selectOne(new LambdaQueryWrapper<Project>()
                    .eq(Project::getId, tenderDTO.getProjectId())
                    .eq(Project::getTenantId, tenantId));
            if (project == null) {
                throw TenantScope.resourceNotFound();
            }
        }

        String originalFilename = file.getOriginalFilename();
        if (originalFilename == null) {
            throw new BizException(400, "文件名不能为空");
        }

        String extension = "";
        if (originalFilename.lastIndexOf(".") > 0) {
            extension = originalFilename.substring(originalFilename.lastIndexOf("."));
        }

        Path dest = storagePathService.buildTenderUploadPath(originalFilename);
        try {
            storagePathService.ensureParentDirectory(dest);
        } catch (IOException e) {
            throw new BizException(500, "文件目录创建失败: " + e.getMessage());
        }

        try {
            file.transferTo(dest.toFile());
        } catch (IOException e) {
            throw new BizException(500, "文件上传失败: " + e.getMessage());
        }

        Tender tender = new Tender();
        BeanUtils.copyProperties(tenderDTO, tender);
        tender.setTenantId(tenantId);

        tender.setFileName(originalFilename); // 默认情况下，将使用原始名称作为文件名，或者根据数据传输对象DTO来指定文件名
        if (StringUtils.hasText(tenderDTO.getBidName())) {
            // 如果提供了bidName这个参数，就将其用作bidName；但fileName通常指的是文件
            // 该表格包含fileName和bidName这两列
            tender.setBidName(tenderDTO.getBidName());
        }

        tender.setFilePath(storagePathService.toStoredPath(dest));
        tender.setFileSize(file.getSize());

        // 确定文件类型
        String lowerExt = extension.toLowerCase();
        if (lowerExt.contains("pdf")) {
            tender.setFileType("pdf");
        } else if (lowerExt.contains("doc")) {
            tender.setFileType("word");
        } else {
            tender.setFileType("other");
        }

        tender.setParseStatus(0); // 待审核
        try {
            tender.setUploadUserId(BaseContext.getCurrentId());
        } catch (Exception e) {
            // 如果无法获取用户ID（例如测试时绕过登录），则设为默认值
            tender.setUploadUserId(1L);
        }
        tender.setUploadTime(LocalDateTime.now());
        // 版本号由系统自动递增：同项目已有最大版本 + 1（首个版本为 V1）。
        // 前端不再参与版本编号，避免用户手填版本号导致重复或乱序。
        tender.setVersion(nextVersionForProject(tenderDTO.getProjectId(), tenantId));
        if(tenderDTO.getFileCategory() != null && !tenderDTO.getFileCategory().isEmpty()) {
            // 文件角色：招标文件/标书 → bid；合同 → contract（本期以招标文件为主）
            tender.setFileCategory("合同".equals(tenderDTO.getFileCategory()) || "contract".equals(tenderDTO.getFileCategory()) ? "contract" : "bid");
        } else {
            tender.setFileCategory("bid"); // 默认类型：招标文件
        }

        tender.setProjectId(tenderDTO.getProjectId());
        tenderMapper.insert(tender);
        refreshProjectLatestVersion(tender.getProjectId(), tender.getVersion());

        TenderVO vo = new TenderVO();
        BeanUtils.copyProperties(tender, vo);
        return vo;
    }

    private void refreshProjectLatestVersion(Long projectId, Integer version) {
        if (projectId == null || version == null) {
            return;
        }
        Long tenantId = TenantScope.requiredTenantId();
        Project project = projectMapper.selectOne(new LambdaQueryWrapper<Project>()
                .eq(Project::getId, projectId)
                .eq(Project::getTenantId, tenantId));
        if (project == null) {
            return;
        }
        Integer currentLatestVersion = project.getLatestVersion();
        if (currentLatestVersion == null || version > currentLatestVersion) {
            project.setLatestVersion(version);
            project.setParseStatus(0);
            project.setUpdateTime(LocalDateTime.now());
            project.setTenantId(tenantId);
            projectMapper.update(project, new LambdaQueryWrapper<Project>()
                    .eq(Project::getId, projectId)
                    .eq(Project::getTenantId, tenantId));
        }
    }

    /**
     * 计算项目下一个可用版本号：同项目已有最大版本 + 1（没有则首版 V1）。
     * 版本由服务端统一自动生成，前端不参与编号。
     */
    private Integer nextVersionForProject(Long projectId, Long tenantId) {
        if (projectId == null) {
            return 1;
        }
        LambdaQueryWrapper<Tender> wrapper = new LambdaQueryWrapper<>();
        wrapper.eq(Tender::getProjectId, projectId)
               .eq(Tender::getTenantId, tenantId)
               .orderByDesc(Tender::getVersion)
               .last("LIMIT 1");
        Tender latestTender = tenderMapper.selectOne(wrapper);
        return (latestTender != null && latestTender.getVersion() != null ? latestTender.getVersion() : 0) + 1;
    }

    @Override
    public PageResult page(TenderPageQueryDTO dto) {
        Long tenantId = TenantScope.requiredTenantId();
        Page<Project> pageInfo = new Page<>(dto.getPage(), dto.getSize());

        LocalDateTime uploadStart = dto.getUploadStartTime() != null
                ? dto.getUploadStartTime().atStartOfDay() : null;
        LocalDateTime uploadEnd = dto.getUploadEndTime() != null
                ? dto.getUploadEndTime().atTime(java.time.LocalTime.MAX) : null;

        // 状态与文件类型过滤下推到 SQL（见 ProjectMapper#selectProjectPageWithStatus），
        // 保证分页 total 与当前页数据严格一致，修复「先分页再内存过滤导致 total 失配」问题。
        Page<Project> p = projectMapper.selectProjectPageWithStatus(
                pageInfo,
                tenantId,
                BaseContext.getCurrentId(),
                dto.getBidName(),
                dto.getFileCategory(),
                dto.getStatus(),
                uploadStart,
                uploadEnd);

        List<TenderVO> vos = p.getRecords().stream().map(project -> {
            TenderVO vo = new TenderVO();
            vo.setProjectId(project.getId());
            vo.setBidName(project.getProjectName());
            vo.setSupplierName(project.getSupplierName());
            vo.setParseStatus(0);
            vo.setUploadTime(project.getCreateTime());
            vo.setVersion(project.getLatestVersion());
            
            // 查最新版本的标书获取文件相关信息
            LambdaQueryWrapper<Tender> tenderWrapper = new LambdaQueryWrapper<>();
            tenderWrapper.eq(Tender::getProjectId, project.getId())
                         .eq(Tender::getTenantId, tenantId)
                         .orderByDesc(Tender::getVersion)
                         .last("LIMIT 1");
            Tender latestTender = tenderMapper.selectOne(tenderWrapper);
            
            if (latestTender != null) {
                vo.setId(latestTender.getId());
                vo.setVersion(latestTender.getVersion());
                vo.setFileName(latestTender.getFileName());
                vo.setFilePath(latestTender.getFilePath());
                vo.setFileSize(latestTender.getFileSize());
                vo.setFileType(latestTender.getFileType());
                vo.setBudgetAmount(latestTender.getBudgetAmount());
                vo.setPageCount(latestTender.getPageCount());
                vo.setUploadUserId(latestTender.getUploadUserId());
                // 文件类型英文转中文
                vo.setFileCategory(latestTender.getFileCategory() != null && latestTender.getFileCategory().equals("bid") ? "标书" : "合同");
                vo.setParseStatus(resolveParseStatusFromLatestTask(latestTender.getId()));
                fillAuditorName(vo);
            } else {
                vo.setFileCategory(null);
                vo.setAuditorName(null);
                vo.setParseStatus(0);
            }

            return vo;
        }).collect(Collectors.toList());

        return new PageResult(p.getTotal(), vos);
    }

    @Override
    public TenderVO getById(Long id) {
        Long tenantId = TenantScope.requiredTenantId();
        Tender tender = tenderMapper.selectOne(new LambdaQueryWrapper<Tender>()
                .eq(Tender::getId, id)
                .eq(Tender::getTenantId, tenantId));
        if (tender == null) {
            throw TenantScope.resourceNotFound();
        }
        // 验证资源归属：只有标书上传者才能查看详情
        Long currentUserId = BaseContext.getCurrentId();
        if (currentUserId != null && !currentUserId.equals(tender.getUploadUserId())) {
            throw new BizException(403, "无权访问该标书");
        }
        TenderVO vo = new TenderVO();
        BeanUtils.copyProperties(tender, vo);
        vo.setParseStatus(resolveParseStatusFromLatestTask(tender.getId()));

        fillAuditorName(vo);

        return vo;
    }

    @Override
    public List<TenderVO> getVersionsByProjectId(Long projectId) {
        Long tenantId = TenantScope.requiredTenantId();
        // 验证项目归属：只有项目所有者才能查看版本列表
        Long currentUserId = BaseContext.getCurrentId();
        Project project = projectMapper.selectOne(new LambdaQueryWrapper<Project>()
                .eq(Project::getId, projectId)
                .eq(Project::getTenantId, tenantId));
        if (project == null) {
            throw TenantScope.resourceNotFound();
        }
        if (currentUserId != null) {
            if (!currentUserId.equals(project.getUserId())) {
                throw new BizException(403, "无权访问该项目");
            }
        }
        LambdaQueryWrapper<Tender> wrapper = new LambdaQueryWrapper<>();
        wrapper.eq(Tender::getProjectId, projectId)
                .eq(Tender::getTenantId, tenantId)
                .orderByDesc(Tender::getVersion); // 按版本号倒序排列

        List<Tender> tenders = tenderMapper.selectList(wrapper);

        return tenders.stream().map(tender -> {
            TenderVO vo = new TenderVO();
            BeanUtils.copyProperties(tender, vo);
            vo.setParseStatus(resolveParseStatusFromLatestTask(tender.getId()));
            fillAuditorName(vo);

            return vo;
        }).collect(Collectors.toList());
    }

    /**
     * 填充审核人姓名
     * 逻辑：根据标书ID查询审核任务，再根据审核任务中的audit_user_id查询用户姓名
     */
    private void fillAuditorName(TenderVO vo) {
        AuditTask task = findLatestTaskByBidId(vo.getId());
        Long auditUserId = task != null ? task.getAuditUserId() : null;
        if (auditUserId == null) {
            LambdaQueryWrapper<AuditTask> fallbackWrapper = new LambdaQueryWrapper<>();
            fallbackWrapper.eq(AuditTask::getBidId, vo.getId())
                    .eq(AuditTask::getTenantId, TenantScope.requiredTenantId())
                    .isNotNull(AuditTask::getAuditUserId)
                    .orderByDesc(AuditTask::getCreateTime)
                    .last("LIMIT 1");
            AuditTask fallbackTask = auditTaskMapper.selectOne(fallbackWrapper);
            if (fallbackTask != null) {
                auditUserId = fallbackTask.getAuditUserId();
            }
        }
        if (auditUserId != null) {
            User user = userMapper.selectById(auditUserId);
            if (user != null) {
                vo.setAuditorName(user.getRealName());
            }
        }
    }

    private AuditTask findLatestTaskByBidId(Long bidId) {
        if (bidId == null) {
            return null;
        }
        LambdaQueryWrapper<AuditTask> taskWrapper = new LambdaQueryWrapper<>();
        taskWrapper.eq(AuditTask::getBidId, bidId)
                .eq(AuditTask::getTenantId, TenantScope.requiredTenantId())
                .orderByDesc(AuditTask::getCreateTime)
                .last("LIMIT 1");
        return auditTaskMapper.selectOne(taskWrapper);
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

    @Override
    public List<TenderProjectVO> getProjects() {
        Long tenantId = TenantScope.requiredTenantId();
        Long userId = BaseContext.getCurrentId();
        
        // 1. 去 project 表查当前用户的项目
        LambdaQueryWrapper<Project> projectWrapper = new LambdaQueryWrapper<>();
        projectWrapper.eq(Project::getTenantId, tenantId)
                .eq(Project::getUserId, userId)
                .orderByDesc(Project::getCreateTime);
        List<Project> projects = projectMapper.selectList(projectWrapper);
        
        // 获取创建人真实姓名
        User user = userMapper.selectById(userId);
        String creatorName = user != null ? user.getRealName() : null;

        return projects.stream().map(project -> {
            TenderProjectVO vo = new TenderProjectVO();
            vo.setProjectId(project.getId());
            vo.setProjectName(project.getProjectName());
            vo.setCreateTime(project.getCreateTime());
            vo.setLatestVersion(project.getLatestVersion());
            vo.setSupplierName(project.getSupplierName());
            vo.setCreatorName(creatorName);

            // 2. 去标书表里查相关字段
            LambdaQueryWrapper<Tender> tenderWrapper = new LambdaQueryWrapper<>();
            tenderWrapper.eq(Tender::getProjectId, project.getId())
                         .eq(Tender::getTenantId, tenantId)
                         .orderByDesc(Tender::getVersion)
                         .last("LIMIT 1");
            Tender latestTender = tenderMapper.selectOne(tenderWrapper);

            if (latestTender != null) {
                // 找到标书，设置标书类型（DB 存英文码 bid/contract → 前端中文标签 标书/合同）
                String rawCategory = latestTender.getFileCategory();
                vo.setFileCategory("bid".equals(rawCategory) ? "标书" : "合同");
                
                // 3. 查最新版本标书的审核人
                LambdaQueryWrapper<AuditTask> taskWrapper = new LambdaQueryWrapper<>();
                taskWrapper.eq(AuditTask::getBidId, latestTender.getId())
                           .eq(AuditTask::getTenantId, tenantId)
                           .orderByDesc(AuditTask::getCreateTime)
                           .last("LIMIT 1");
                AuditTask task = auditTaskMapper.selectOne(taskWrapper);
                if (task != null && task.getAuditUserId() != null) {
                    User auditor = userMapper.selectById(task.getAuditUserId());
                    if (auditor != null) {
                        vo.setAuditorName(auditor.getRealName());
                    }
                }
            } else {
                // 查不到说明是空项目，返回 null 或空
                vo.setFileCategory(null);
                vo.setAuditorName(null);
            }
            
            return vo;
        }).collect(Collectors.toList());
    }

    @Override
    public void delete(Long id) {
        Long tenantId = TenantScope.requiredTenantId();
        Tender tender = tenderMapper.selectOne(new LambdaQueryWrapper<Tender>()
                .eq(Tender::getId, id)
                .eq(Tender::getTenantId, tenantId));
        if (tender == null) {
            throw TenantScope.resourceNotFound();
        }
        // 验证资源归属：只有标书上传者才能删除
        Long currentUserId = BaseContext.getCurrentId();
        if (currentUserId != null && !currentUserId.equals(tender.getUploadUserId())) {
            throw new BizException(403, "无权删除该标书");
        }
        // 如果存在，删除文件
        Path stored = storagePathService.resolveStoredPath(tender.getFilePath());
        File file = stored.toFile();
        if (file.exists()) {
            file.delete();
        }
        tenderMapper.delete(new LambdaQueryWrapper<Tender>()
                .eq(Tender::getId, id)
                .eq(Tender::getTenantId, tenantId));
    }

    @Override
    public List<Long> getBidIdsByUserId(Long userId) {
        Long tenantId = TenantScope.requiredTenantId();
        QueryWrapper<Tender> queryWrapper = new QueryWrapper<>();
        queryWrapper.eq("tenant_id", tenantId)
                .eq("upload_user_id", userId);
        return tenderMapper.selectList(queryWrapper).stream().map(Tender::getId).toList();
    }
}
