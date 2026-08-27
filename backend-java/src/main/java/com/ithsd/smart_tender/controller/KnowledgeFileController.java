package com.ithsd.smart_tender.controller;

import com.ithsd.smart_tender.model.entity.KnowledgeFile;
import com.ithsd.smart_tender.model.result.PageResult;
import com.ithsd.smart_tender.model.result.Result;
import com.ithsd.smart_tender.service.KnowledgeFileService;
import com.ithsd.smart_tender.service.DocumentPreviewService;
import com.ithsd.smart_tender.service.StoragePathService;
import lombok.extern.slf4j.Slf4j;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.web.bind.annotation.DeleteMapping;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.PatchMapping;
import org.springframework.web.bind.annotation.PutMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;
import org.springframework.web.multipart.MultipartFile;
import org.springframework.core.io.Resource;
import org.springframework.core.io.UrlResource;
import org.springframework.http.HttpHeaders;
import org.springframework.http.MediaType;
import org.springframework.http.ResponseEntity;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.Files;
import java.net.MalformedURLException;
import org.springframework.util.StringUtils;

@Slf4j
@RestController
@RequestMapping("/api/knowledge-files")
public class KnowledgeFileController {

    @Autowired
    private KnowledgeFileService knowledgeFileService;
    @Autowired
    private DocumentPreviewService documentPreviewService;
    @Autowired
    private StoragePathService storagePathService;

    @PostMapping(value = "/upload", consumes = MediaType.MULTIPART_FORM_DATA_VALUE)
    public Result uploadFile( @RequestParam("file") MultipartFile file,
                                               @RequestParam(value = "fileName", required = false) String fileName,
                                               @RequestParam("category") String category,
                                               @RequestParam(value = "tags", required = false) String tags,
                                               @RequestParam(value = "applicableScope", required = false) String applicableScope,
                                               @RequestParam(value = "description", required = false) String description,
                                               @RequestParam(value = "status", required = false) Integer status) {
        log.info("上传标准库文件：fileName={}, category={}, applicableScope={}", fileName, category, applicableScope);
        //将参数传入service逻辑处理
        knowledgeFileService.uploadFile(file, fileName, category, tags, applicableScope, description, status);
     

        return Result.success();
    }

    @GetMapping
    public Result<PageResult> getKnowledgeFileList(
            @RequestParam(value = "category", required = false) String category,
            @RequestParam(value = "tags", required = false) String tags,
            @RequestParam(value = "applicableScope", required = false) String applicableScope,
            @RequestParam(value = "sortBy", defaultValue = "asc") String sortBy,
            @RequestParam(value = "page", defaultValue = "1") int page,
            @RequestParam(value = "size", defaultValue = "10") int size) {
        log.info("查询标准库列表：category={}, tags={}, applicableScope={}, sortBy={}, page={}, size={}", 
                category, tags, applicableScope, sortBy, page, size);
        PageResult knowledgeFilePage = knowledgeFileService.getKnowledgeFilePage(page, size, category, tags, applicableScope, sortBy);

        //打印查询到的信息到控制台
        log.info("查询到的标准库文件数量：{} 文件列表：{}", knowledgeFilePage.getTotal(), knowledgeFilePage.getRecords());

        return Result.success(knowledgeFilePage);
    }
    
    @GetMapping("/search")
    public Result<PageResult> searchKnowledgeFileList(
            @RequestParam(value = "keyword", required = false) String keyword,
            @RequestParam(value = "sortBy", required = false) String sortBy,
            @RequestParam(value = "page", defaultValue = "1") int page,
            @RequestParam(value = "size", defaultValue = "10") int size) {
        log.info("搜索标准库：keyword={}, sortBy={}, page={}, size={}", 
                keyword, sortBy, page, size);
        PageResult knowledgeFilePage = knowledgeFileService.searchKnowledgeFilePage(page, size, keyword, sortBy);

        //打印查询到的信息到控制台
        log.info("搜索到的标准库文件数量：{} 文件列表：{}", knowledgeFilePage.getTotal(), knowledgeFilePage.getRecords());

        return Result.success(knowledgeFilePage);
    }
    
    @DeleteMapping("/{fileId}")
    public Result deleteKnowledgeFile(
            @PathVariable("fileId") Long fileId) {
        log.info("删除标准库文件：fileId={}", fileId);
        knowledgeFileService.deleteKnowledgeFile(fileId);
        return Result.success();
    }
    
    @PutMapping("/{fileId}")
    public Result updateKnowledgeFile(
            @PathVariable("fileId") Long fileId,
            @RequestParam(value ="file", required = false) MultipartFile file,
            @RequestParam(value = "fileName", required = false) String fileName,
            @RequestParam("category") String category,
            @RequestParam(value = "tags", required = false) String tags,
            @RequestParam(value = "applicableScope", required = false) String applicableScope,
            @RequestParam(value = "description", required = false) String description,
            @RequestParam(value = "status", required = false) Integer status) {
        log.info("更新标准库文件：fileId={}, fileName={}, category={}", fileId, fileName, category);
        knowledgeFileService.updateKnowledgeFile(fileId, file, fileName, category, tags, applicableScope, description, status);
        return Result.success();
    }
    
    @PatchMapping("/{fileId}/status")
    public Result updateKnowledgeFileStatus(
            @PathVariable("fileId") Long fileId,
            @RequestBody StatusUpdateRequest statusUpdateRequest) {
        log.info("更新标准库文件状态：fileId={}, status={}", fileId, statusUpdateRequest.getStatus());
        knowledgeFileService.updateKnowledgeFileStatus(fileId, statusUpdateRequest.getStatus());
        return Result.success();
    }
    
    // 状态更新请求体
    static class StatusUpdateRequest {
        private int status;
        
        public int getStatus() {
            return status;
        }
        
        public void setStatus(int status) {
            this.status = status;
        }
    }

    @GetMapping("/{fileId}/download")
    public ResponseEntity<Resource> downloadFile(@PathVariable("fileId") Long fileId) {
        log.info("下载标准库文件：fileId={}", fileId);
        KnowledgeFile file = knowledgeFileService.getVisibleById(fileId);
        if (file == null || file.getFilePath() == null) {
            return ResponseEntity.notFound().build();
        }
        
        try {
            Path path = storagePathService.resolveStoredPath(file.getFilePath());
            Resource resource = new UrlResource(path.toUri());
            
            if (resource.exists() || resource.isReadable()) {
                return ResponseEntity.ok()
                        .header(HttpHeaders.CONTENT_DISPOSITION, "attachment; filename=\"" + java.net.URLEncoder.encode(file.getFileName(), java.nio.charset.StandardCharsets.UTF_8) + "\"")
                        .body(resource);
            } else {
                log.error("文件无法读取: {}", path);
                return ResponseEntity.notFound().build();
            }
        } catch (MalformedURLException e) {
            log.error("文件路径错误", e);
            return ResponseEntity.internalServerError().build();
        }
    }

    @GetMapping("/{fileId}/preview")
    public ResponseEntity<?> previewFile(@PathVariable("fileId") Long fileId) {
        log.info("预览标准库文件：fileId={}", fileId);
        KnowledgeFile file = knowledgeFileService.getVisibleById(fileId);
        if (file == null || file.getFilePath() == null) {
            return ResponseEntity.notFound().build();
        }

        try {
            Path path = storagePathService.resolveStoredPath(file.getFilePath());
            if (!path.toFile().exists()) {
                log.error("预览文件不存在: {}", path);
                return ResponseEntity.notFound().build();
            }
            String fileType = file.getFileType() == null ? "" : file.getFileType().toLowerCase();
            String fileName = file.getFileName() == null ? "document" : file.getFileName();

            if ("pdf".equals(fileType) || fileName.toLowerCase().endsWith(".pdf")) {
                Resource resource = new UrlResource(path.toUri());
                if (!resource.exists() || !resource.isReadable()) {
                    return ResponseEntity.notFound().build();
                }
                return ResponseEntity.ok()
                        .header(HttpHeaders.CONTENT_DISPOSITION,
                                "inline; filename*=UTF-8''" + java.net.URLEncoder.encode(
                                        fileName,
                                        java.nio.charset.StandardCharsets.UTF_8
                                ).replace("+", "%20"))
                        .contentType(MediaType.APPLICATION_PDF)
                        .body(resource);
            }

            if ("doc".equals(fileType) || "docx".equals(fileType)
                    || fileName.toLowerCase().endsWith(".doc")
                    || fileName.toLowerCase().endsWith(".docx")) {
                String displayName = fileName;
                if (displayName.toLowerCase().endsWith(".docx")) {
                    displayName = displayName.substring(0, displayName.length() - 5);
                } else if (displayName.toLowerCase().endsWith(".doc")) {
                    displayName = displayName.substring(0, displayName.length() - 4);
                }
                Path pdfPath = documentPreviewService.ensurePdfPreviewFile(path);
                Resource resource = new UrlResource(pdfPath.toUri());
                if (!resource.exists() || !resource.isReadable()) {
                    return ResponseEntity.notFound().build();
                }
                return ResponseEntity.ok()
                        .header(HttpHeaders.CONTENT_DISPOSITION,
                                "inline; filename*=UTF-8''" + java.net.URLEncoder.encode(
                                        displayName + ".pdf",
                                        java.nio.charset.StandardCharsets.UTF_8
                                ).replace("+", "%20"))
                        .contentType(MediaType.APPLICATION_PDF)
                        .body(resource);
            }

            return ResponseEntity.status(415).body("该文件类型暂不支持在线预览，请使用下载功能");
        } catch (Exception e) {
            log.error("预览标准库文件失败: fileId={}", fileId, e);
            return ResponseEntity.internalServerError().body("预览失败");
        }
    }

}
