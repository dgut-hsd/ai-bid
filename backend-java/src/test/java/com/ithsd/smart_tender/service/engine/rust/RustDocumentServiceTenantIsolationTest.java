package com.ithsd.smart_tender.service.engine.rust;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.common.BizException;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.service.StoragePathService;
import com.ithsd.smart_tender.tenant.fixture.TenantQueryAssertions;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.ArgumentCaptor;
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
 * <p>验证传入错误 tenantId 时无法访问其他租户的标书文档。
 * tenantId 由调用方显式传入，不依赖 ThreadLocal。</p>
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

    // ── ensureUploaded ─────────────────────────────────────────

    @Test
    void ensureUploaded_shouldRejectCrossTenantBid() {
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        assertThatThrownBy(() -> service.ensureUploaded(BID_ID, TENANT_A))
                .isInstanceOf(BizException.class)
                .matches(ex -> ((BizException) ex).getCode() == 5704);

        ArgumentCaptor<LambdaQueryWrapper<Tender>> captor = ArgumentCaptor.forClass(LambdaQueryWrapper.class);
        verify(tenderMapper).selectOne(captor.capture());
        TenantQueryAssertions.assertTenantScoped(captor.getValue(), TENANT_A);
        verify(rustApiClient, never()).getDocument(any());
    }

    @Test
    void ensureUploaded_shouldUseCachedDocForSameTenantBid() {
        Tender tender = Tender.builder()
                .id(BID_ID).tenantId(TENANT_A)
                .rustDocumentId(RUST_DOC_ID).build();
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(tender);

        when(rustApiClient.getDocument(RUST_DOC_ID))
                .thenThrow(new BizException(500, "Rust unreachable"));

        String result = service.ensureUploaded(BID_ID, TENANT_A);

        assertThat(result).isEqualTo(RUST_DOC_ID);
        ArgumentCaptor<LambdaQueryWrapper<Tender>> captor = ArgumentCaptor.forClass(LambdaQueryWrapper.class);
        verify(tenderMapper).selectOne(captor.capture());
        TenantQueryAssertions.assertTenantScoped(captor.getValue(), TENANT_A);
        verify(tenderMapper, never()).updateById(any());
    }

    // ── getCachedDocumentId ────────────────────────────────────

    @Test
    void getCachedDocumentId_shouldReturnNullForCrossTenantBid() {
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        String result = service.getCachedDocumentId(BID_ID, TENANT_A);

        assertThat(result).isNull();
        ArgumentCaptor<LambdaQueryWrapper<Tender>> captor = ArgumentCaptor.forClass(LambdaQueryWrapper.class);
        verify(tenderMapper).selectOne(captor.capture());
        TenantQueryAssertions.assertTenantScoped(captor.getValue(), TENANT_A);
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
