package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.extension.plugins.pagination.Page;
import com.baomidou.mybatisplus.extension.service.impl.ServiceImpl;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.mapper.KnowledgeFileMapper;
import com.ithsd.smart_tender.model.entity.KnowledgeFile;
import com.ithsd.smart_tender.model.result.PageResult;
import com.ithsd.smart_tender.service.KnowledgeFileService;
import com.ithsd.smart_tender.service.TenantAuthorizationService;

import com.ithsd.smart_tender.service.StoragePathService;
import lombok.extern.slf4j.Slf4j;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;
import org.springframework.web.multipart.MultipartFile;
import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.baomidou.mybatisplus.core.conditions.update.LambdaUpdateWrapper;
import java.io.File;
import java.io.IOException;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.format.DateTimeFormatter;
import java.util.UUID;
import java.util.List;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.entity.User;
import org.springframework.beans.BeanUtils;
import org.springframework.util.StringUtils;

@Service
@Slf4j
public class KnowledgeFileServiceImpl extends ServiceImpl<KnowledgeFileMapper, KnowledgeFile> implements KnowledgeFileService {

    @Autowired
    private KnowledgeFileMapper knowledgeMapper;
    

    @Autowired
    private UserMapper userMapper;
    @Autowired
    private StoragePathService storagePathService;

    @Autowired
    private TenantAuthorizationService authorization;

    @Override
    public KnowledgeFile getVisibleById(Long fileId) {
        if (fileId == null) {
            return null;
        }
        TenantRequestContext context = currentContext();
        return knowledgeMapper.findByIdAndTenantId(fileId, context.tenantId());
    }

    @Override
    @Transactional(rollbackFor = Exception.class)
    public void uploadFile(MultipartFile file, String fileName, String category, String tags, String applicableScope, String description, Integer status) {
        TenantRequestContext context = currentContext();


        //解析文件格式按日期分文件夹存入本地
        String originalFilename = file.getOriginalFilename();
        if (originalFilename == null || originalFilename.isEmpty()) {
            throw new RuntimeException("文件名为空");
        }

        String extension = originalFilename.substring(originalFilename.lastIndexOf("."));
        Path absoluteFilePath = storagePathService.buildKnowledgeUploadPath(originalFilename);
        String storedPath = storagePathService.toStoredPath(absoluteFilePath);
        String filePath = absoluteFilePath.toString();

        // 确保目录存在
        try {
            storagePathService.ensureParentDirectory(absoluteFilePath);
        } catch (IOException e) {
            throw new RuntimeException("文件目录创建失败：" + e.getMessage(), e);
        }

        // 先保存文件到本地临时位置（带.tmp 后缀）
        String tempFilePath = absoluteFilePath + ".tmp";
        System.out.println(tempFilePath);
        System.out.println("------------------------------------------------------------------------------------");
        File savedFile = new File(tempFilePath);
        try {
            file.transferTo(savedFile);
        } catch (IOException e) {
            throw new RuntimeException("文件保存失败：" + e.getMessage(), e);
        }

        //计算文件的大小
        long fileSize = file.getSize();

        // 如果前端没有传 fileName，默认使用文件的原始名称
        if (fileName == null || fileName.trim().isEmpty()) {
            fileName = originalFilename;
        }

        //用 builder 构建实体类
        KnowledgeFile knowledgeFile = KnowledgeFile.builder()
                .tenantId(context.tenantId())
                .fileName(fileName)
                .filePath(storedPath)
                .fileSize(fileSize)
                .fileType(extension.startsWith(".") ? extension.substring(1) : extension)
                .category(category)
                .tags(tags)
                .description(description)
                .applicableScope(applicableScope)
                .status(status == null ? 1 : status)
                .version(1)
                .uploadUserId(context.userId())
                .build();

        try {
            // 保存到数据库
            this.save(knowledgeFile);

            // 数据库保存成功后，重命名临时文件为正式文件
            File finalFile = absoluteFilePath.toFile();
            if (!savedFile.renameTo(finalFile)) {
                // 如果重命名失败，删除临时文件并抛出异常
                savedFile.delete();
                throw new RuntimeException("文件重命名失败");
            }

        } catch (Exception e) {
            // 数据库保存失败，删除已上传的临时文件
            if (savedFile.exists()) {
                savedFile.delete();
            }
            throw new RuntimeException("文件上传失败：" + e.getMessage(), e);
        }
    }

    @Override
    public PageResult getKnowledgeFilePage(int page, int size, String category, String tags, String applicableScope, String sortBy) {
        Page<KnowledgeFile> pageInfo = new Page<>(page, size);
        // 构建查询条件
        LambdaQueryWrapper<KnowledgeFile> queryWrapper = new LambdaQueryWrapper<>();
        queryWrapper.eq(KnowledgeFile::getTenantId, currentContext().tenantId());
        
        // 确保参数非空
        if (category != null && !category.isEmpty()) {
            queryWrapper.eq(KnowledgeFile::getCategory, category);
        }
        if (tags != null && !tags.isEmpty()) {
            queryWrapper.like(KnowledgeFile::getTags, tags);
        }
        if (applicableScope != null && !applicableScope.isEmpty()) {
            queryWrapper.eq(KnowledgeFile::getApplicableScope, applicableScope);
        }
        
        // 不展示删除状态的文件
        queryWrapper.ne(KnowledgeFile::getStatus, 2);
        
        // 排序逻辑
        if ("asc".equalsIgnoreCase(sortBy)) {
            queryWrapper.orderByAsc(KnowledgeFile::getUploadTime);
        } else {
            queryWrapper.orderByDesc(KnowledgeFile::getUploadTime);
        }


        // 执行分页查询
        this.page(pageInfo, queryWrapper);

        // 获取查询结果
        List<KnowledgeFile> records = pageInfo.getRecords();
        // 计算记录数
        
        // 将 KnowledgeFile 转化为 KnowledgeFileVO
        List<com.ithsd.smart_tender.model.vo.KnowledgeFileVO> voList = new java.util.ArrayList<>();
        for (KnowledgeFile knowledgeFile : records) {
            com.ithsd.smart_tender.model.vo.KnowledgeFileVO vo = new com.ithsd.smart_tender.model.vo.KnowledgeFileVO();
            BeanUtils.copyProperties(knowledgeFile, vo);
            
            // 查询并设置上传用户姓名
            if (knowledgeFile.getUploadUserId() != null) {
                User user = userMapper.selectById(knowledgeFile.getUploadUserId());
                if (user != null) {
                    vo.setUploadUserName(StringUtils.hasText(user.getRealName()) ? user.getRealName() : "-");
                }
            }

            // 处理文件大小格式化
            vo.setFileSize(formatFileSize(knowledgeFile.getFileSize()));
            // 处理状态名称
            vo.setStatus(getStatusName(knowledgeFile.getStatus()));
            // 处理上传时间格式化
            vo.setUploadTime(formatDateTime(knowledgeFile.getUploadTime()));
            // 处理更新时间格式化
            vo.setUpdateTime(formatDateTime(knowledgeFile.getUpdateTime()));
            voList.add(vo);
        }

        //返回结果用pageresult包装  
        return new PageResult(pageInfo.getTotal(), voList);
    }
    
    @Override
    public PageResult searchKnowledgeFilePage(int page, int size, String keyword, String sortBy) {
        Page<KnowledgeFile> pageInfo = new Page<>(page, size);
        // 构建查询条件
        LambdaQueryWrapper<KnowledgeFile> queryWrapper = new LambdaQueryWrapper<>();
        queryWrapper.eq(KnowledgeFile::getTenantId, currentContext().tenantId());
        
        // 关键词搜索
        if (keyword != null && !keyword.isEmpty()) {
            queryWrapper.like(KnowledgeFile::getFileName, keyword);
        }
        
        // 不展示删除状态的文件
        queryWrapper.ne(KnowledgeFile::getStatus, 2);
        
        // 排序逻辑
        if ("asc".equalsIgnoreCase(sortBy)) {
            queryWrapper.orderByAsc(KnowledgeFile::getUploadTime);
        } else {
            queryWrapper.orderByDesc(KnowledgeFile::getUploadTime);
        }


        // 执行分页查询
        this.page(pageInfo, queryWrapper);

        // 获取查询结果
        List<KnowledgeFile> records = pageInfo.getRecords();
        // 计算记录数
        
        // 将 KnowledgeFile 转化为 KnowledgeFileVO
        List<com.ithsd.smart_tender.model.vo.KnowledgeFileVO> voList = new java.util.ArrayList<>();
        for (KnowledgeFile knowledgeFile : records) {
            com.ithsd.smart_tender.model.vo.KnowledgeFileVO vo = new com.ithsd.smart_tender.model.vo.KnowledgeFileVO();
            BeanUtils.copyProperties(knowledgeFile, vo);
            
            // 查询并设置上传用户姓名
            if (knowledgeFile.getUploadUserId() != null) {
                User user = userMapper.selectById(knowledgeFile.getUploadUserId());
                if (user != null) {
                    vo.setUploadUserName(StringUtils.hasText(user.getRealName()) ? user.getRealName() : "-");
                }
            }

            // 处理文件大小格式化
            vo.setFileSize(formatFileSize(knowledgeFile.getFileSize()));
            // 处理状态名称
            vo.setStatus(getStatusName(knowledgeFile.getStatus()));
            // 处理上传时间格式化
            vo.setUploadTime(formatDateTime(knowledgeFile.getUploadTime()));
            // 处理更新时间格式化
            vo.setUpdateTime(formatDateTime(knowledgeFile.getUpdateTime()));
            voList.add(vo);
        }

        //返回结果用pageresult包装  
        return new PageResult(pageInfo.getTotal(), voList);
    }
    
    // 格式化文件大小
    private String formatFileSize(long fileSize) {
        if (fileSize < 1024) {
            return fileSize + "B";
        } else if (fileSize < 1024 * 1024) {
            return String.format("%.2fKB", fileSize / 1024.0);
        } else if (fileSize < 1024 * 1024 * 1024) {
            return String.format("%.2fMB", fileSize / (1024.0 * 1024.0));
        } else {
            return String.format("%.2fGB", fileSize / (1024.0 * 1024.0 * 1024.0));
        }
    }
    
    // 获取状态名称
    private String getStatusName(int status) {
        switch (status) {
            case 0:
                return "停用";
            case 1:
                return "启用";
            case 2:
                return "已删除";
            default:
                return "未知";
        }
    }
    
    // 格式化日期时间
    private String formatDateTime(java.time.LocalDateTime dateTime) {
        if (dateTime == null) {
            return "";
        }
        return dateTime.format(java.time.format.DateTimeFormatter.ofPattern("yyyy-MM-dd HH:mm:ss"));
    }
    
    @Override
    @Transactional(rollbackFor = Exception.class)
    public void deleteKnowledgeFile(Long fileId) {
        KnowledgeFile knowledgeFile = getVisibleById(fileId);
        if (knowledgeFile == null) {
            throw resourceNotFound();
        }
        knowledgeFile.setStatus(2); // 2表示已删除
        requireScopedUpdate(knowledgeFile, currentContext().tenantId());
    }
    
    @Override
    @Transactional(rollbackFor = Exception.class)
    public void updateKnowledgeFile(Long fileId, MultipartFile file, String fileName, String category, String tags, String applicableScope, String description, Integer status) {
        TenantRequestContext context = currentContext();
        // 获取旧文件信息
        KnowledgeFile oldFile = getVisibleById(fileId);
        if (oldFile == null) {
            throw resourceNotFound();
        }
        
        // 判断是否上传了新文件
        if (file != null && !file.isEmpty()) {
            // 将旧文件标记为历史版本（状态设置为 0）
            oldFile.setStatus(0);
            requireScopedUpdate(oldFile, context.tenantId());
            
            // 生成新版本号
            int newVersion = oldFile.getVersion() + 1;
            
            // 解析文件格式按日期分文件夹存入本地
            String originalFilename = file.getOriginalFilename();
            if (originalFilename == null || originalFilename.isEmpty()) {
                throw new RuntimeException("文件名为空");
            }
            
            String extension = originalFilename.substring(originalFilename.lastIndexOf("."));
            Path absoluteFilePath = storagePathService.buildKnowledgeUploadPath(originalFilename);
            String storedPath = storagePathService.toStoredPath(absoluteFilePath);
            String filePath = absoluteFilePath.toString();
            
            // 确保目录存在
            try {
                storagePathService.ensureParentDirectory(absoluteFilePath);
            } catch (IOException e) {
                throw new RuntimeException("文件目录创建失败：" + e.getMessage(), e);
            }
            
            // 先保存文件到本地临时位置（带.tmp 后缀）
            String tempFilePath = absoluteFilePath + ".tmp";
            File savedFile = new File(tempFilePath);
            try {
                file.transferTo(savedFile);
            } catch (IOException e) {
                throw new RuntimeException("文件保存失败：" + e.getMessage(), e);
            }
            
            //计算文件的大小
            long fileSize = file.getSize();
            
            //用 builder 构建实体类
            KnowledgeFile knowledgeFile = KnowledgeFile.builder()
                    .tenantId(context.tenantId())
                    .fileName(originalFilename)
                    .filePath(storedPath)
                    .fileSize(fileSize)
                    .fileType(extension.startsWith(".") ? extension.substring(1) : extension)
                    .category(category)
                    .tags(tags)
                    .description(description)
                    .applicableScope(applicableScope)
                    .status(status == null ? 1 : status)
                    .version(newVersion)
                    .uploadUserId(context.userId())
                    .build();
            
            try {
                // 保存到数据库
                this.save(knowledgeFile);
                
                // 数据库保存成功后，重命名临时文件为正式文件
                File finalFile = absoluteFilePath.toFile();
                if (!savedFile.renameTo(finalFile)) {
                    // 如果重命名失败，删除临时文件并抛出异常
                    savedFile.delete();
                    throw new RuntimeException("文件重命名失败");
                }
                
            } catch (Exception e) {
                // 数据库保存失败，删除已上传的临时文件
                if (savedFile.exists()) {
                    savedFile.delete();
                }
                throw new RuntimeException("文件更新失败：" + e.getMessage(), e);
            }
        } else {
            // 没有上传新文件，直接更新旧文件的信息，版本不更新
            oldFile.setFileName(fileName);
            oldFile.setCategory(category);
            oldFile.setTags(tags);
            oldFile.setApplicableScope(applicableScope);
            oldFile.setDescription(description);
            if (status != null) {
                oldFile.setStatus(status);
            }
            requireScopedUpdate(oldFile, context.tenantId());
        }
    }

    @Override
    public void updateKnowledgeFileStatus(Long fileId, int status) {
        KnowledgeFile knowledgeFile = getVisibleById(fileId);
        if (knowledgeFile == null) {
            throw resourceNotFound();
        }
        KnowledgeFile update = new KnowledgeFile();
        update.setStatus(status);
        update.setUpdateTime(LocalDateTime.now());
        requireScopedUpdate(update, currentContext().tenantId(), fileId);
    }

    private TenantRequestContext currentContext() {
        return authorization.requireCurrentTenant();
    }

    private TenantAuthException resourceNotFound() {
        return new TenantAuthException(
                404,
                "RESOURCE_NOT_FOUND",
                "Knowledge file not found",
                currentContext().requestId()
        );
    }

    private void requireScopedUpdate(KnowledgeFile entity, Long tenantId) {
        requireScopedUpdate(entity, tenantId, entity.getId());
    }

    private void requireScopedUpdate(KnowledgeFile entity, Long tenantId, Long fileId) {
        LambdaUpdateWrapper<KnowledgeFile> update = new LambdaUpdateWrapper<>();
        update.eq(KnowledgeFile::getId, fileId)
                .eq(KnowledgeFile::getTenantId, tenantId);
        int affectedRows = knowledgeMapper.update(entity, update);
        if (affectedRows != 1) {
            throw resourceNotFound();
        }
    }

}
