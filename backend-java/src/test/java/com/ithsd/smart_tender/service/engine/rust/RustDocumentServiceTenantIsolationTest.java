package com.ithsd.smart_tender.service.engine.rust;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.service.StoragePathService;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.InjectMocks;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.*;

/**
 * 跨租户隔离测试 — RustDocumentService。
 *
 * <p>tenantId 由调用方显式传入（而非依赖 ThreadLocal），用于租户隔离校验。
 * 不属于本租户的 bidId 查不到 Tender → 抛 {@code RESOURCE_NOT_FOUND}。</p>
 */
@ExtendWith(MockitoExtension.class)
class RustDocumentServiceTenantIsolationTest {

    private static final Long TENANT_A = 2001L;
    private static final Long BID_ID = 5001L;
    private static final String RUST_DOC_ID = "doc-uuid-123";

    @Mock
    private RustApiClient rustApiClient;
    @Mock
    private StoragePathService storagePathService;
    @Mock
    private TenderMapper tenderMapper;

    @InjectMocks
    private RustDocumentService service;

    @BeforeEach
    void setUp() {
        TenantContext.set(new TenantRequestContext(1001L, TENANT_A, "ADMIN", 1L, "rust-document-test"));
    }

    @AfterEach
    void tearDown() {
        TenantContext.clear();
    }

    // ── ensureUploaded ─────────────────────────────────────────

    @Test
    void ensureUploaded_shouldRejectCrossTenantBid() {
        // 不属于本租户的 bidId → Tender 查询返回 null → 抛 RESOURCE_NOT_FOUND
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        assertThatThrownBy(() -> service.ensureUploaded(BID_ID, TENANT_A))
                .isInstanceOf(TenantAuthException.class)
                .matches(ex -> ((TenantAuthException) ex).getErrorCode().equals("RESOURCE_NOT_FOUND"));

        verify(tenderMapper).selectOne(any(LambdaQueryWrapper.class));
        verify(rustApiClient, never()).getDocument(any());
    }

    @Test
    void ensureUploaded_shouldUseCachedDocForSameTenantBid() {
        Tender tender = Tender.builder()
                .id(BID_ID).tenantId(TENANT_A)
                .rustDocumentId(RUST_DOC_ID).build();
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(tender);
        when(rustApiClient.getDocument(RUST_DOC_ID))
                .thenThrow(new com.ithsd.smart_tender.common.BizException(500, "Rust unreachable"));

        String result = service.ensureUploaded(BID_ID, TENANT_A);

        assertThat(result).isEqualTo(RUST_DOC_ID);
        verify(tenderMapper).selectOne(any(LambdaQueryWrapper.class));
        verify(tenderMapper, never()).update(any(), any());
    }

    // ── getCachedDocumentId ────────────────────────────────────

    @Test
    void getCachedDocumentId_shouldReturnNullForCrossTenantBid() {
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        String result = service.getCachedDocumentId(BID_ID, TENANT_A);

        assertThat(result).isNull();
        verify(tenderMapper).selectOne(any(LambdaQueryWrapper.class));
    }

    @Test
    void getCachedDocumentId_shouldReturnDocIdForSameTenantBid() {
        Tender tender = Tender.builder()
                .id(BID_ID).tenantId(TENANT_A)
                .rustDocumentId(RUST_DOC_ID).build();
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(tender);

        String result = service.getCachedDocumentId(BID_ID, TENANT_A);

        assertThat(result).isEqualTo(RUST_DOC_ID);
    }
}
