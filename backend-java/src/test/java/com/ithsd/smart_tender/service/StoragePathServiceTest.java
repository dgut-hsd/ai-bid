package com.ithsd.smart_tender.service;

import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;
import org.springframework.test.util.ReflectionTestUtils;

import java.nio.file.Path;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

class StoragePathServiceTest {

    @TempDir
    Path root;

    @AfterEach
    void clearContext() {
        TenantContext.clear();
    }

    @Test
    void knowledgePathUsesTenantPrefixAndRejectsTraversal() {
        StoragePathService service = new StoragePathService();
        ReflectionTestUtils.setField(service, "storageRoot", root.toString());
        ReflectionTestUtils.setField(service, "tenderDir", "tenders");
        ReflectionTestUtils.setField(service, "knowledgeDir", "knowledge/uploads");
        ReflectionTestUtils.setField(service, "previewCacheDir", "");
        TenantContext.set(new TenantRequestContext(1001L, 2001L, "OWNER", 1L, "request"));

        Path path = service.buildKnowledgeUploadPath("guide.pdf");
        assertThat(path.toAbsolutePath().normalize().startsWith(
                root.resolve("tenant").resolve("2001").resolve("knowledge/uploads")
                        .toAbsolutePath().normalize())).isTrue();
        assertThat(service.toStoredPath(path)).startsWith("tenant/2001/");

        Path externalPreviewCache = root.getParent().resolve("preview-cache-outside-root");
        ReflectionTestUtils.setField(service, "previewCacheDir", externalPreviewCache.toString());
        assertThat(service.previewCachePath()).isEqualTo(externalPreviewCache.normalize());

        assertThatThrownBy(() -> service.resolveStoredPath("../outside.pdf"))
                .isInstanceOf(SecurityException.class);
        assertThatThrownBy(() -> service.resolveStoredPath("tenant/2002/knowledge/uploads/file.pdf"))
                .isInstanceOf(SecurityException.class);
    }
}
