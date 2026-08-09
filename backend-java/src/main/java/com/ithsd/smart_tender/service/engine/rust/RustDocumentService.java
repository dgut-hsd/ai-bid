package com.ithsd.smart_tender.service.engine.rust;

import com.ithsd.smart_tender.common.BizException;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.model.dto.rust.RustProcessResponse;
import com.ithsd.smart_tender.service.StoragePathService;
import com.ithsd.smart_tender.service.impl.TenantScope;
import com.baomidou.mybatisplus.core.conditions.query.QueryWrapper;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;
import org.springframework.util.StringUtils;

import java.nio.file.Files;
import java.nio.file.Path;
import java.time.LocalDateTime;

/**
 * 管理 Java Tender ↔ Rust 文档的生命周期映射。
 *
 * <p>核心职责：
 * <ol>
 *   <li>确保标书文件已上传到 Rust（懒加载、断线重传）</li>
 *   <li>在 Tender 实体上缓存 {@code rustDocumentId}</li>
 *   <li>Rust 重启后自动检测失效并重新上传</li>
 * </ol>
 */
@Service
public class RustDocumentService {

    private static final Logger log = LoggerFactory.getLogger(RustDocumentService.class);

    private final RustApiClient rustApiClient;
    private final StoragePathService storagePathService;
    private final TenderMapper tenderMapper;

    public RustDocumentService(
            RustApiClient rustApiClient,
            StoragePathService storagePathService,
            TenderMapper tenderMapper
    ) {
        this.rustApiClient = rustApiClient;
        this.storagePathService = storagePathService;
        this.tenderMapper = tenderMapper;
    }

    /**
     * 确保标书文件已上传到 Rust，返回 Rust 侧 document_id（UUID）。
     *
     * <p>幂等：若 Tender 上已有缓存的 rustDocumentId 且 Rust 侧仍存在，
     * 直接返回，不重复上传。</p>
     *
     * <p>自动恢复：若 Rust 侧返回 404（服务重启、内存清空），
     * 清除缓存并重新上传。</p>
     *
     * @param bidId Java 侧 Tender 主键
     * @return Rust document_id（UUID）
     * @throws BizException 上传失败或文件不存在
     */
    @Transactional
    public String ensureUploaded(Long bidId) {
        Long tenantId = TenantScope.requiredTenantId();
        Tender tender = tenderMapper.selectOne(new QueryWrapper<Tender>()
                .eq("id", bidId)
                .eq("tenant_id", tenantId));
        if (tender == null) {
            throw TenantScope.resourceNotFound();
        }
        // 已有缓存 → 验证有效性
        if (StringUtils.hasText(tender.getRustDocumentId())) {
            if (verifyExists(tender.getRustDocumentId())) {
                log.debug("Rust document still valid: bidId={}, rustDocId={}", bidId, tender.getRustDocumentId());
                return tender.getRustDocumentId();
            }
            // Rust 重启了，缓存失效
            log.warn("Rust document lost (restart?), re-uploading: bidId={}, oldRustDocId={}",
                    bidId, tender.getRustDocumentId());
            tender.setRustDocumentId(null);
        }

        // 首次上传或重新上传
        return uploadToRust(bidId, tenantId, tender);
    }

    /**
     * 仅返回数据库中已缓存的 Rust 文档 ID，不验证 Rust 内存中的文档是否仍存在。
     * 用于任务恢复：Rust 的最终结果有磁盘 fallback，即使服务重启后文档对象
     * 尚未恢复，旧 document_id 对应的结果仍然可以读取。
     */
    public String getCachedDocumentId(Long bidId) {
        Long tenantId = TenantScope.requiredTenantId();
        Tender tender = tenderMapper.selectOne(new QueryWrapper<Tender>()
                .eq("id", bidId)
                .eq("tenant_id", tenantId));
        if (tender == null) {
            throw TenantScope.resourceNotFound();
        }
        return tender.getRustDocumentId();
    }

    // ── 私有方法 ──────────────────────────────────────────────────

    private boolean verifyExists(String rustDocumentId) {
        try {
            return rustApiClient.getDocument(rustDocumentId) != null;
        } catch (BizException e) {
            // 网络错误等 → 保守地认为存在，避免重复上传
            log.warn("Rust connectivity issue during verify, assuming doc exists: {}", e.getMessage());
            return true;
        }
    }

    private String uploadToRust(Long bidId, Long tenantId, Tender tender) {
        // 1. 解析文件物理路径
        Path filePath = storagePathService.resolveStoredPath(tender.getFilePath());
        if (filePath == null) {
            throw new BizException(5703, "无法解析文件路径: " + tender.getFilePath());
        }
        if (!Files.exists(filePath)) {
            String tried = storagePathService.rootPath().resolve(
                tender.getFilePath().replace("\\", "/")).toAbsolutePath().toString();
            throw new BizException(5703,
                "标书文件不存在 — storedPath=" + tender.getFilePath()
                + ", resolved=" + filePath.toAbsolutePath()
                + ", root=" + storagePathService.rootPath().toAbsolutePath());
        }

        String filename = StringUtils.hasText(tender.getFileName())
                ? tender.getFileName()
                : filePath.getFileName().toString();

        log.info("Uploading to Rust: bidId={}, path={}, filename={}", bidId, filePath, filename);

        // 2. 上传到 Rust
        RustProcessResponse result = rustApiClient.uploadDocument(filePath, filename);

        // 3. 回写缓存
        tender.setRustDocumentId(result.getDocumentId());
        tender.setPageCount(result.getTotalPages());  // 顺便更新页数
        tender.setTenantId(tenantId);
        tenderMapper.update(tender, new QueryWrapper<Tender>()
                .eq("id", bidId)
                .eq("tenant_id", tenantId));

        log.info("Rust upload complete: bidId={}, rustDocId={}, chunks={}, pages={}",
                bidId, result.getDocumentId(), result.getTotalChunks(), result.getTotalPages());

        return result.getDocumentId();
    }
}
