package com.ithsd.smart_tender.service;

import com.baomidou.mybatisplus.extension.service.IService;
import com.ithsd.smart_tender.model.entity.KnowledgeFile;
import com.ithsd.smart_tender.model.result.PageResult;
import org.springframework.web.multipart.MultipartFile;

public interface KnowledgeFileService extends IService<KnowledgeFile> {

    /** Returns a file only when it belongs to the current tenant. */
    KnowledgeFile getVisibleById(Long fileId);

    /**上传文件
     * 
     * @param file
     * 
     */
    void uploadFile(MultipartFile file, String fileName, String category, String tags, String applicableScope, String description, Integer status);
    
    /**
     *  文件查询
     * @param page
     * @param size
     * @param category
     * @param tags
     * @param applicableScope
     * @return
     */
    PageResult getKnowledgeFilePage(int page, int size, String category, String tags, String applicableScope, String sortBy);
    
    /**
     *  文件搜索
     * @param page
     * @param size
     * @param keyword
     * @param sortBy
     * @return
     */
    PageResult searchKnowledgeFilePage(int page, int size, String keyword, String sortBy);
    
    /**
     *  删除文件
     * @param fileId
     */
    void deleteKnowledgeFile(Long fileId);
    
    /**
     *  更新文件
     * @param fileId
     * @param file
     * @param fileName
     * @param category
     * @param tags
     * @param applicableScope
     * @param description
     */
    void updateKnowledgeFile(Long fileId, MultipartFile file, String fileName, String category, String tags, String applicableScope, String description, Integer status);
    
    /**
     *  更新文件状态
     * @param fileId
     * @param status
     */
    void updateKnowledgeFileStatus(Long fileId, int status);
}
