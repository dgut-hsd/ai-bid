package com.ithsd.smart_tender.service;

import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.stereotype.Service;
import org.springframework.util.StringUtils;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.time.LocalDate;
import java.time.format.DateTimeFormatter;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;

@Service
public class StoragePathService {

    @Value("${file.storage.path}")
    private String storageRoot;

    @Value("${file.storage.tender-dir:tenders}")
    private String tenderDir;

    @Value("${file.storage.knowledge-dir:knowledge/uploads}")
    private String knowledgeDir;

    @Value("${preview.cache.path:}")
    private String previewCacheDir;

    public Path rootPath() {
        return Paths.get(storageRoot).toAbsolutePath().normalize();
    }

    public Path tenderRootPath() {
        return rootPath().resolve(tenderDir).normalize();
    }

    public Path knowledgeRootPath() {
        return rootPath().resolve(knowledgeDir).normalize();
    }

    public Path tenantRootPath(Long tenantId) {
        if (tenantId == null || tenantId <= 0) {
            throw new IllegalArgumentException("tenantId must be positive");
        }
        return rootPath().resolve("tenant").resolve(String.valueOf(tenantId)).normalize();
    }

    public Path tenantKnowledgeRootPath() {
        TenantRequestContext context = currentTenantContext();
        return tenantRootPath(context.tenantId()).resolve(knowledgeDir).normalize();
    }

    public Path tenantTenderRootPath(Long tenantId) {
        if (tenantId == null || tenantId <= 0) {
            throw new IllegalArgumentException("tenantId must be positive");
        }
        return tenantRootPath(tenantId).resolve(tenderDir).normalize();
    }

    public Path previewCachePath() {
        if (StringUtils.hasText(previewCacheDir)) {
            return Paths.get(previewCacheDir).normalize();
        }
        return rootPath().resolve("preview-cache").normalize();
    }

    public Path buildTenderUploadPath(String originalFilename) {
        TenantRequestContext context = currentTenantContext();
        return buildUploadPath(tenantTenderRootPath(context.tenantId()), originalFilename);
    }

    public Path buildKnowledgeUploadPath(String originalFilename) {
        return buildUploadPath(tenantKnowledgeRootPath(), originalFilename);
    }

    public String toStoredPath(Path absolutePath) {
        Path normalized = absolutePath.toAbsolutePath().normalize();
        Path root = rootPath().toAbsolutePath().normalize();
        ensureWithinRoot(normalized);
        ensureTenantPathBelongsToCurrentTenant(normalized);
        String relative = root.relativize(normalized).toString();
        return relative.replace("\\", "/");
    }

    public Path resolveStoredPath(String storedPath) {
        if (!StringUtils.hasText(storedPath)) {
            return null;
        }
        String normalizedRaw = storedPath.trim().replace("\\", "/");
        Path root = rootPath().toAbsolutePath().normalize();
        List<Path> candidates = new ArrayList<>();
        Path input = Paths.get(storedPath);
        if (input.isAbsolute()) {
            candidates.add(input.normalize());
        } else {
            candidates.add(root.resolve(normalizedRaw).normalize());
            candidates.add(knowledgeRootPath().resolve(normalizedRaw).normalize());
            candidates.add(tenderRootPath().resolve(normalizedRaw).normalize());
        }

        if (normalizedRaw.startsWith("data/uploads/")) {
            candidates.add(knowledgeRootPath().resolve(normalizedRaw.substring("data/uploads/".length())).normalize());
        }
        if (normalizedRaw.startsWith("uploads/")) {
            candidates.add(knowledgeRootPath().resolve(normalizedRaw.substring("uploads/".length())).normalize());
        }
        if (normalizedRaw.startsWith("tenders/")) {
            candidates.add(tenderRootPath().resolve(normalizedRaw.substring("tenders/".length())).normalize());
        }

        for (Path candidate : candidates) {
            candidate = candidate.toAbsolutePath().normalize();
            ensureWithinRoot(candidate);
            // 读路径不做租户命名空间校验：旧格式文件（tenant 迁移前上传）不在
            // tenant/{id}/ 下，校验会阻断预览和审核。文件归属已由外层 SQL 按
            // tenantId 过滤保证安全性。
            if (Files.exists(candidate)) {
                return candidate;
            }
        }
        Path fallback = candidates.get(0).toAbsolutePath().normalize();
        ensureWithinRoot(fallback);
        return fallback;
    }

    public void ensureParentDirectory(Path path) throws IOException {
        Path normalized = path.toAbsolutePath().normalize();
        ensureWithinRoot(normalized);
        ensureTenantPathBelongsToCurrentTenant(normalized);
        Path parent = normalized.getParent();
        if (parent != null && !Files.exists(parent)) {
            Files.createDirectories(parent);
        }
    }

    private Path buildUploadPath(Path baseDir, String originalFilename) {
        Path normalizedBase = baseDir.toAbsolutePath().normalize();
        ensureWithinRoot(normalizedBase);
        String extension = "";
        if (StringUtils.hasText(originalFilename)) {
            int dot = originalFilename.lastIndexOf(".");
            if (dot >= 0) {
                extension = originalFilename.substring(dot);
            }
        }
        String dateFolder = LocalDate.now().format(DateTimeFormatter.ofPattern("yyyy-MM-dd"));
        String uniqueFileName = UUID.randomUUID() + extension;
        Path result = normalizedBase.resolve(dateFolder).resolve(uniqueFileName).toAbsolutePath().normalize();
        ensureWithinRoot(result);
        ensureTenantPathBelongsToCurrentTenant(result);
        return result;
    }

    private TenantRequestContext currentTenantContext() {
        TenantRequestContext context = TenantContext.get();
        if (context == null || context.tenantId() == null) {
            String requestId = context == null ? UUID.randomUUID().toString() : context.requestId();
            throw new TenantAuthException(400, "TENANT_REQUIRED", "A current tenant is required", requestId);
        }
        return context;
    }

    private void ensureWithinRoot(Path candidate) {
        Path root = rootPath().toAbsolutePath().normalize();
        if (!candidate.toAbsolutePath().normalize().startsWith(root)) {
            throw new SecurityException("Storage path escapes configured root");
        }
    }

    private void ensureTenantPathBelongsToCurrentTenant(Path candidate) {
        Path normalized = candidate.toAbsolutePath().normalize();
        Path tenantNamespace = rootPath().resolve("tenant").toAbsolutePath().normalize();
        Path previewCache = previewCachePath().toAbsolutePath().normalize();
        // preview-cache 不在 tenant/ 命名空间下，明确放行
        if (normalized.startsWith(previewCache)) {
            return;
        }
        // 所有其他路径必须在 tenant/ 命名空间内，否则拒绝
        if (!normalized.startsWith(tenantNamespace)) {
            throw new SecurityException(
                    "Storage path must reside inside a tenant namespace: " + normalized);
        }
        TenantRequestContext context = currentTenantContext();
        if (!normalized.startsWith(tenantRootPath(context.tenantId()))) {
            throw new SecurityException("Storage path belongs to another tenant");
        }
    }
}
