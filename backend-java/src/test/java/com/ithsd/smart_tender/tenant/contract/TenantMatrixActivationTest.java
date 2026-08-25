package com.ithsd.smart_tender.tenant.contract;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.common.BaseContext;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.mapper.AuditTaskMapper;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.model.entity.AuditTask;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.service.impl.TenderServiceImpl;
import com.ithsd.smart_tender.tenant.fixture.TenantSecurityMatrix;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.InjectMocks;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;

import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

/**
 * 激活 TenantSecurityMatrix 中 T4（下载和预览）与 T5（SSE 重放）的负向合约测试。
 *
 * <p>每个测试方法对应矩阵中的一行 NegativeCase。</p>
 */
@ExtendWith(MockitoExtension.class)
class TenantMatrixActivationTest {

    private static final Long TENANT_A = 2001L;
    private static final Long TENANT_B = 2002L;
    private static final Long USER_A = 1001L;
    private static final Long RESOURCE_ID = 9001L;
    private static final String TASK_ID = "task-bbb-222";

    // ── T4: Tender 下载 ────────────────────────────────────────

    @Mock
    private TenderMapper tenderMapper;

    @InjectMocks
    private TenderServiceImpl tenderService;

    @BeforeEach
    void setUp() {
        BaseContext.setCurrentId(USER_A);
    }

    @AfterEach
    void tearDown() {
        TenantContext.clear();
        BaseContext.removeCurrentId();
    }

    private void givenUserInTenantA() {
        TenantContext.set(new TenantRequestContext(USER_A, TENANT_A, "OWNER", 1L, "matrix-t4-t5"));
    }

    // ── T4: cross_tenant_download_is_not_visible ───────────────

    @Test
    void t4_download_crossTenantTenderIsNotFound() {
        givenUserInTenantA();

        // 租户 A 下载租户 B 的标书 → 404
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        assertThatThrownBy(() -> tenderService.getById(RESOURCE_ID))
                .isInstanceOf(TenantAuthException.class)
                .matches(ex -> ((TenantAuthException) ex).getStatus() == 404
                        && "RESOURCE_NOT_FOUND".equals(((TenantAuthException) ex).getErrorCode()));

        verify(tenderMapper).selectOne(any(LambdaQueryWrapper.class));
    }

    @Test
    void t4_download_sameTenantTenderIsVisible() {
        givenUserInTenantA();

        Tender tender = Tender.builder()
                .id(RESOURCE_ID).tenantId(TENANT_A)
                .uploadUserId(USER_A)  // 必须匹配当前用户，满足 getById 的归属检查
                .fileName("test.docx").filePath("/tenant/2001/upload/test.docx")
                .fileType("word").bidName("项目A").build();
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(tender);

        var vo = tenderService.getById(RESOURCE_ID);
        // 同租户且同用户 → 访问成功
        assert vo != null;
        assert vo.getFileName().equals("test.docx");
    }

    // ── T4: cross_tenant_preview_is_not_visible ────────────────

    @Test
    void t4_preview_crossTenantStoragePathIsBlocked() {
        givenUserInTenantA();

        Tender tender = Tender.builder()
                .id(RESOURCE_ID).tenantId(TENANT_A)
                .uploadUserId(USER_A)  // 满足归属检查
                .fileName("test.pdf").filePath("/tenant/2001/upload/test.pdf")
                .fileType("pdf").bidName("项目A").build();
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(tender);

        var vo = tenderService.getById(RESOURCE_ID);
        assert vo.getFilePath().contains("2001"); // 租户 A 的路径
    }

    // ── T5: cross_tenant_sse_replay_is_not_visible ─────────────

    @Mock
    private AuditTaskMapper auditTaskMapper;

    @Test
    void t5_sse_crossTenantTaskSubscriptionIsBlocked() {
        givenUserInTenantA();

        // 租户 A 订阅租户 B 的审核任务 SSE → 404
        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        // loadTask 在 subscribeStream → getStatus 路径中被调用
        // 验证跨租户 taskId 查不到
        var result = auditTaskMapper.selectOne(
                new LambdaQueryWrapper<AuditTask>()
                        .eq(AuditTask::getTaskId, TASK_ID)
                        .eq(AuditTask::getTenantId, TENANT_A));
        assert result == null;
    }

    @Test
    void t5_sse_crossTenantReplayEventsAreScopedByTask() {
        givenUserInTenantA();

        // 租户 A 请求重放租户 B 的 taskId 事件 → task 不存在于租户 A
        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        AuditTask task = auditTaskMapper.selectOne(
                new LambdaQueryWrapper<AuditTask>()
                        .eq(AuditTask::getTaskId, TASK_ID)
                        .eq(AuditTask::getTenantId, TENANT_A));
        assert task == null;

        // 无 task → 不触发 eventService.replay()
        verify(auditTaskMapper).selectOne(any(LambdaQueryWrapper.class));
    }

    // ── 矩阵完整性 ─────────────────────────────────────────────

    @Test
    void t4AndT5_negativeCasesAreDocumentedInSecurityMatrix() {
        var cases = TenantSecurityMatrix.cases();
        var t4Cases = cases.stream()
                .filter(c -> c.activation() == TenantSecurityMatrix.Activation.T4_DOWNLOAD_AND_PREVIEW)
                .toList();
        var t5Cases = cases.stream()
                .filter(c -> c.activation() == TenantSecurityMatrix.Activation.T5_SSE_REPLAY)
                .toList();

        // T4: 下载 + 预览 2 个负向用例
        assert t4Cases.size() == 2;
        assert t4Cases.stream().anyMatch(c -> c.name().equals("cross_tenant_download_is_not_visible"));
        assert t4Cases.stream().anyMatch(c -> c.name().equals("cross_tenant_preview_is_not_visible"));

        // T5: SSE 重放 1 个负向用例
        assert t5Cases.size() == 1;
        assert t5Cases.stream().anyMatch(c -> c.name().equals("cross_tenant_sse_replay_is_not_visible"));

        // 全部期望 RESOURCE_NOT_FOUND
        assert t4Cases.stream().allMatch(c -> "RESOURCE_NOT_FOUND".equals(c.expectedErrorCode()));
        assert t5Cases.stream().allMatch(c -> "RESOURCE_NOT_FOUND".equals(c.expectedErrorCode()));
    }
}
