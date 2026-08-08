package com.ithsd.smart_tender.service.impl;

import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.mapper.KnowledgeFileMapper;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.entity.KnowledgeFile;
import com.ithsd.smart_tender.service.StoragePathService;
import com.ithsd.smart_tender.service.TenantAuthorizationService;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.InjectMocks;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.never;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

@ExtendWith(MockitoExtension.class)
class KnowledgeFileTenantIsolationTest {

    @Mock
    private KnowledgeFileMapper knowledgeMapper;
    @Mock
    private UserMapper userMapper;
    @Mock
    private StoragePathService storagePathService;
    @Mock
    private TenantAuthorizationService authorization;

    @InjectMocks
    private KnowledgeFileServiceImpl knowledgeFileService;

    @Test
    void tenantA_cannotReadOrDeleteTenantBFile() {
        TenantRequestContext tenantA = new TenantRequestContext(1001L, 2001L, "OWNER", 1L, "request-a");
        when(authorization.requireCurrentTenant()).thenReturn(tenantA);
        when(knowledgeMapper.findByIdAndTenantId(any(), any())).thenReturn(null);

        assertThat(knowledgeFileService.getVisibleById(9002L)).isNull();
        assertThatThrownBy(() -> knowledgeFileService.deleteKnowledgeFile(9002L))
                .isInstanceOfSatisfying(TenantAuthException.class, ex -> {
                    assertThat(ex.getStatus()).isEqualTo(404);
                    assertThat(ex.getErrorCode()).isEqualTo("RESOURCE_NOT_FOUND");
                });

        verify(knowledgeMapper, org.mockito.Mockito.times(2))
                .findByIdAndTenantId(9002L, 2001L);
        verify(knowledgeMapper, never()).updateById(any(KnowledgeFile.class));
    }

    @Test
    void tenantA_cannotUpdateOrChangeStatusOfTenantBFile() {
        TenantRequestContext tenantA = new TenantRequestContext(1001L, 2001L, "OWNER", 1L, "request-a");
        when(authorization.requireCurrentTenant()).thenReturn(tenantA);
        when(knowledgeMapper.findByIdAndTenantId(any(), any())).thenReturn(null);

        assertThatThrownBy(() -> knowledgeFileService.updateKnowledgeFile(
                        9002L, null, "renamed.pdf", "general", null, null, null, null))
                .isInstanceOfSatisfying(TenantAuthException.class, ex -> {
                    assertThat(ex.getStatus()).isEqualTo(404);
                    assertThat(ex.getErrorCode()).isEqualTo("RESOURCE_NOT_FOUND");
                });
        assertThatThrownBy(() -> knowledgeFileService.updateKnowledgeFileStatus(9002L, 2))
                .isInstanceOfSatisfying(TenantAuthException.class, ex -> {
                    assertThat(ex.getStatus()).isEqualTo(404);
                    assertThat(ex.getErrorCode()).isEqualTo("RESOURCE_NOT_FOUND");
                });

        verify(knowledgeMapper, org.mockito.Mockito.times(2))
                .findByIdAndTenantId(9002L, 2001L);
        verify(knowledgeMapper, never()).update(any(KnowledgeFile.class), any());
        verify(knowledgeMapper, never()).updateById(any(KnowledgeFile.class));
    }
}
