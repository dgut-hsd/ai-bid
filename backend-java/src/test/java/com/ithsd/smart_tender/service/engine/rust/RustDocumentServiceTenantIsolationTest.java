package com.ithsd.smart_tender.service.engine.rust;

import com.baomidou.mybatisplus.core.conditions.query.QueryWrapper;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.service.StoragePathService;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.ArgumentCaptor;
import org.mockito.InjectMocks;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;

import static org.assertj.core.api.Assertions.assertThat;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.lenient;
import static org.mockito.Mockito.verify;

@ExtendWith(MockitoExtension.class)
class RustDocumentServiceTenantIsolationTest {

    private static final Long TENANT_A = 20001L;

    @Mock
    private RustApiClient rustApiClient;

    @Mock
    private StoragePathService storagePathService;

    @Mock
    private TenderMapper tenderMapper;

    @InjectMocks
    private RustDocumentService documentService;

    @BeforeEach
    void setUp() {
        TenantContext.set(new TenantRequestContext(10001L, TENANT_A, "ADMIN", 1L, "rust-document-test"));
        lenient().when(tenderMapper.selectById(88L)).thenReturn(
                Tender.builder().id(88L).tenantId(20002L).rustDocumentId("rust-doc-b").build());
    }

    @AfterEach
    void tearDown() {
        TenantContext.clear();
    }

    @Test
    void cachedDocumentLookup_hidesAnotherTenantsTender() {
        // 缓存查找契约：跨租户/查不到一律返回 null（不抛 404），保证 recover 优雅降级。
        assertThat(documentService.getCachedDocumentId(88L)).isNull();

        ArgumentCaptor<QueryWrapper<Tender>> query = ArgumentCaptor.forClass(QueryWrapper.class);
        verify(tenderMapper).selectOne(query.capture());
        assertThat(query.getValue().getSqlSegment()).contains("tenant_id");
        assertThat(query.getValue().getParamNameValuePairs()).containsValue(TENANT_A);
    }
}
