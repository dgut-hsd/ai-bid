package com.ithsd.smart_tender.service.impl;


import com.ithsd.smart_tender.common.BaseContext;
import com.ithsd.smart_tender.common.BizException;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.mapper.AuditIssueMapper;
import com.ithsd.smart_tender.mapper.AuditTaskMapper;
import com.ithsd.smart_tender.mapper.KnowledgeFileMapper;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.model.dto.CreateAuditTaskRequest;
import com.ithsd.smart_tender.model.dto.rust.RustBlockBBoxResponse;
import com.ithsd.smart_tender.model.dto.rust.RustReviewResponse;
import com.ithsd.smart_tender.model.dto.rust.RustReviewResultResponse;
import com.ithsd.smart_tender.model.dto.rust.RustRiskFinding;
import com.ithsd.smart_tender.model.entity.AuditIssue;
import com.ithsd.smart_tender.model.entity.AuditTask;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.model.enums.AuditStageEnum;
import com.ithsd.smart_tender.model.enums.AuditTaskStatusEnum;
import com.ithsd.smart_tender.model.enums.SseEventTypeEnum;
import com.ithsd.smart_tender.model.vo.AuditTaskCreateVO;
import com.ithsd.smart_tender.model.vo.AuditTaskStatusVO;
import com.ithsd.smart_tender.model.vo.IssueVO;
import com.ithsd.smart_tender.model.vo.ResultVO;
import com.ithsd.smart_tender.service.TenderService;
import com.ithsd.smart_tender.service.engine.queue.AuditTaskDispatcher;
import com.ithsd.smart_tender.service.engine.rust.RustApiClient;
import com.ithsd.smart_tender.sse.AuditTaskEventService;
import com.ithsd.smart_tender.sse.SseHub;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.ArgumentCaptor;
import org.mockito.InjectMocks;
import org.mockito.Mock;

import org.mockito.MockedStatic;
import org.mockito.junit.jupiter.MockitoExtension;
import org.springframework.transaction.support.TransactionSynchronizationManager;

import java.time.LocalDate;
import java.time.format.DateTimeFormatter;
import java.time.temporal.TemporalAdjusters;
import java.time.DayOfWeek;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.*;
import static org.mockito.ArgumentMatchers.*;
import static org.mockito.Mockito.*;

/**
 * AuditTaskServiceImpl 的单元测试。
 *
 * <p>使用 JUnit 5 + Mockito (@ExtendWith(MockitoExtension.class))，
 * 所有依赖均通过 @Mock 注入，仅测试 AuditTaskServiceImpl 本身的逻辑编排。</p>
 */
@ExtendWith(MockitoExtension.class)
class AuditTaskServiceImplTest {

    // ── 被模拟的依赖 ─────────────────────────────────────────────────

    @Mock
    private AuditTaskMapper auditTaskMapper;

    @Mock
    private AuditIssueMapper auditIssueMapper;

    @Mock
    private TenderService tenderService;

    @Mock
    private AuditTaskDispatcher taskDispatcher;

    @Mock
    private AuditTaskEventService eventService;

    @Mock
    private SseHub sseHub;

    @Mock
    private TenderMapper tenderMapper;

    @Mock
    private KnowledgeFileMapper knowledgeFileMapper;

    @Mock
    private RustApiClient rustApiClient;

    // ── 被测对象 ─────────────────────────────────────────────────────

    @InjectMocks
    private AuditTaskServiceImpl auditTaskService;

    // ── 测试常量 ─────────────────────────────────────────────────────

    private static final Long CURRENT_USER_ID = 1L;
    private static final Long CURRENT_TENANT_ID = 20001L;
    private static final Long ANOTHER_TENANT_ID = 20002L;
    private static final Long ANOTHER_USER_ID = 2L;
    private static final Long TENDER_UPLOADER_ID = 3L;
    private static final String TASK_ID = "task_1712345678_a1b2c3d4";
    private static final Long BID_ID = 100L;
    private static final Long BID_ID_2 = 200L;
    private static final String RUST_DOC_ID = "doc-uuid-12345";

    // ── 生命周期 ─────────────────────────────────────────────────────

    @BeforeEach
    void setUp() {
        BaseContext.setCurrentId(CURRENT_USER_ID);
        TenantContext.set(new TenantRequestContext(
                CURRENT_USER_ID, CURRENT_TENANT_ID, "OWNER", 1L, "audit-task-test"));
        lenient().when(tenderMapper.selectOne(any()))
                .thenReturn(Tender.builder()
                        .id(BID_ID)
                        .tenantId(CURRENT_TENANT_ID)
                        .uploadUserId(TENDER_UPLOADER_ID)
                        .build());
    }

    @AfterEach
    void tearDown() {
        TenantContext.clear();
        BaseContext.removeCurrentId();
    }

    // ═══════════════════════════════════════════════════════════════════
    // createTask 测试
    // ═══════════════════════════════════════════════════════════════════

    /**
     * createTask 正常路径：创建审核任务，验证实体字段和返回值。
     */
    @Test
    void createTask_shouldCreateAuditTaskAndReturnVO() {
        try (MockedStatic<TransactionSynchronizationManager> tsm =
                     mockStatic(TransactionSynchronizationManager.class)) {
            tsm.when(() -> TransactionSynchronizationManager.registerSynchronization(any()))
               .thenAnswer(invocation -> null);

            CreateAuditTaskRequest request = new CreateAuditTaskRequest();
            request.setBidId(BID_ID);
            request.setEnabledAgents(null);

            ArgumentCaptor<AuditTask> captor = ArgumentCaptor.forClass(AuditTask.class);
            when(auditTaskMapper.insert(captor.capture())).thenReturn(1);

            AuditTaskCreateVO result = auditTaskService.createTask(request);

            assertNotNull(result, "返回的 VO 不应为 null");
            assertNotNull(result.getTaskId(), "TaskId 不应为 null");
            assertTrue(result.getTaskId().startsWith("task_"),
                    "TaskId 应以 task_ 开头");

            AuditTask entity = captor.getValue();
            assertNotNull(entity, "插入的实体不应为 null");
            assertEquals(BID_ID, entity.getBidId());
            assertEquals(Integer.valueOf(0), entity.getTaskStatus(),
                    "初始状态应为 PENDING(0)");
            assertEquals("UPLOADING", entity.getStage());
            assertEquals(Integer.valueOf(0), entity.getProgress());
            assertEquals(CURRENT_USER_ID, entity.getAuditUserId(),
                    "应使用当前登录用户的 ID");
            assertTrue(entity.getEnabledChecks().isEmpty(),
                    "enabledAgents 为 null 时应为空列表");
            assertTrue(entity.getFailedStages().isEmpty(),
                    "初始 failedStages 应为空列表");
            assertNotNull(entity.getCreateTime());
            assertNotNull(entity.getUpdatedAt());

            assertEquals(entity.getTaskId(), result.getTaskId(),
                    "返回值 TaskId 应与实体一致");
        }
    }

    /**
     * createTask 带 enabledAgents：验证自定义 agent 列表被正确设置。
     */
    @Test
    void createTask_shouldSetEnabledAgents() {
        try (MockedStatic<TransactionSynchronizationManager> tsm =
                     mockStatic(TransactionSynchronizationManager.class)) {
            tsm.when(() -> TransactionSynchronizationManager.registerSynchronization(any()))
               .thenAnswer(invocation -> null);

            List<String> agents = List.of("factcheck", "procedure", "semanticrisk");
            CreateAuditTaskRequest request = new CreateAuditTaskRequest();
            request.setBidId(BID_ID);
            request.setEnabledAgents(agents);

            ArgumentCaptor<AuditTask> captor = ArgumentCaptor.forClass(AuditTask.class);
            when(auditTaskMapper.insert(captor.capture())).thenReturn(1);

            auditTaskService.createTask(request);

            AuditTask entity = captor.getValue();
            assertEquals(agents, entity.getEnabledChecks(),
                    "enabledChecks 应与请求的 enabledAgents 一致");
            assertEquals(3, entity.getEnabledChecks().size());
            assertTrue(entity.getEnabledChecks().contains("factcheck"));
            assertTrue(entity.getEnabledChecks().contains("procedure"));
            assertTrue(entity.getEnabledChecks().contains("semanticrisk"));
        }
    }

    /**
     * createTask 当前用户为 null 时：应回退到标书记录的 uploadUserId。
     */
    @Test
    void createTask_shouldFallbackToTenderUploaderWhenNoCurrentUser() {
        BaseContext.removeCurrentId();

        try (MockedStatic<TransactionSynchronizationManager> tsm =
                     mockStatic(TransactionSynchronizationManager.class)) {
            tsm.when(() -> TransactionSynchronizationManager.registerSynchronization(any()))
               .thenAnswer(invocation -> null);

            CreateAuditTaskRequest request = new CreateAuditTaskRequest();
            request.setBidId(BID_ID);

            Tender tender = Tender.builder()
                    .id(BID_ID)
                    .uploadUserId(TENDER_UPLOADER_ID)
                    .build();
            when(tenderMapper.selectOne(any())).thenReturn(tender);

            ArgumentCaptor<AuditTask> captor = ArgumentCaptor.forClass(AuditTask.class);
            when(auditTaskMapper.insert(captor.capture())).thenReturn(1);

            auditTaskService.createTask(request);

            AuditTask entity = captor.getValue();
            assertEquals(TENDER_UPLOADER_ID, entity.getAuditUserId(),
                    "应为标书上传者的用户 ID");
        }
    }

    /**
     * createTask 带空 enabledAgents：显式传入空列表应仍能创建。
     */
    @Test
    void createTask_withEmptyEnabledAgents_shouldUseEmptyList() {
        try (MockedStatic<TransactionSynchronizationManager> tsm =
                     mockStatic(TransactionSynchronizationManager.class)) {
            tsm.when(() -> TransactionSynchronizationManager.registerSynchronization(any()))
               .thenAnswer(invocation -> null);

            CreateAuditTaskRequest request = new CreateAuditTaskRequest();
            request.setBidId(BID_ID);
            request.setEnabledAgents(List.of());

            ArgumentCaptor<AuditTask> captor = ArgumentCaptor.forClass(AuditTask.class);
            when(auditTaskMapper.insert(captor.capture())).thenReturn(1);

            auditTaskService.createTask(request);

            AuditTask entity = captor.getValue();
            assertTrue(entity.getEnabledChecks().isEmpty(),
                    "空列表应保持空列表而非 fallback 到默认");
        }
    }

    @Test
    void tenantIsolation_IgnoresClientTenantAndBlocksOtherTenantTask() {
        TenantContext.set(new TenantRequestContext(
                CURRENT_USER_ID, ANOTHER_TENANT_ID, "OWNER", 1L, "audit-task-test-b"));
        Tender otherTenantTender = Tender.builder()
                .id(BID_ID)
                .tenantId(ANOTHER_TENANT_ID)
                .uploadUserId(TENDER_UPLOADER_ID)
                .build();
        when(tenderMapper.selectOne(any())).thenReturn(otherTenantTender);

        CreateAuditTaskRequest request = new CreateAuditTaskRequest();
        request.setBidId(BID_ID);
        request.setTenantId(CURRENT_TENANT_ID);

        try (MockedStatic<TransactionSynchronizationManager> tsm =
                     mockStatic(TransactionSynchronizationManager.class)) {
            tsm.when(() -> TransactionSynchronizationManager.registerSynchronization(any()))
                    .thenAnswer(invocation -> null);
            ArgumentCaptor<AuditTask> captor = ArgumentCaptor.forClass(AuditTask.class);
            when(auditTaskMapper.insert(captor.capture())).thenReturn(1);

            auditTaskService.createTask(request);

            assertEquals(ANOTHER_TENANT_ID, captor.getValue().getTenantId());
        }

        TenantContext.set(new TenantRequestContext(
                CURRENT_USER_ID, CURRENT_TENANT_ID, "OWNER", 1L, "audit-task-test-a"));
        when(auditTaskMapper.selectOne(any())).thenReturn(null);

        TenantAuthException readError = assertThrows(TenantAuthException.class,
                () -> auditTaskService.getStatus(TASK_ID));
        TenantAuthException processingError = assertThrows(TenantAuthException.class,
                () -> auditTaskService.markTaskProcessing(TASK_ID));
        TenantAuthException failedError = assertThrows(TenantAuthException.class,
                () -> auditTaskService.markTaskFailed(TASK_ID, "越权修改"));
        assertEquals(404, readError.getStatus());
        assertEquals("RESOURCE_NOT_FOUND", readError.getErrorCode());
        assertEquals(404, processingError.getStatus());
        assertEquals("RESOURCE_NOT_FOUND", processingError.getErrorCode());
        assertEquals(404, failedError.getStatus());
        assertEquals("RESOURCE_NOT_FOUND", failedError.getErrorCode());
        verify(auditTaskMapper, never()).update(any(), any());
    }

    // ═══════════════════════════════════════════════════════════════════
    // getStatus 测试
    // ═══════════════════════════════════════════════════════════════════

    /**
     * getStatus 正常路径：PENDING 任务返回对应状态信息。
     */
    @Test
    void getStatus_shouldReturnPendingStatus() {
        AuditTask task = AuditTask.builder()
                .id(100L)
                .taskId(TASK_ID)
                .bidId(BID_ID)
                .taskStatus(AuditTaskStatusEnum.PENDING.getCode())
                .stage(AuditStageEnum.UPLOADING.name())
                .progress(0)
                .auditUserId(CURRENT_USER_ID)
                .failedStages(new ArrayList<>())
                .build();

        when(auditTaskMapper.selectOne(any())).thenReturn(task);
        when(auditTaskMapper.selectCount(argThat(x -> x != null)))
                .thenReturn(42L, 5L, 3L, 1L);

        AuditTaskStatusVO vo = auditTaskService.getStatus(TASK_ID);

        assertEquals(TASK_ID, vo.getTaskId());
        assertEquals("pending", vo.getStatus());
        assertEquals("UPLOADING", vo.getStage());
        assertEquals(Integer.valueOf(0), vo.getProgress());
        assertEquals(Long.valueOf(42L), vo.getTotalFileCount());
        assertEquals(Long.valueOf(5L), vo.getPendingFileCount());
        assertEquals(Long.valueOf(3L), vo.getProcessingFileCount());
        assertEquals(Long.valueOf(1L), vo.getFailedFileCount());
        assertEquals(Integer.valueOf(0), vo.getIssueCount());
        assertTrue(vo.getFailedStages().isEmpty());
    }

    /**
     * getStatus 任务不存在时抛出 404 资源错误。
     */
    @Test
    void getStatus_shouldThrow404WhenTaskNotFound() {
        when(auditTaskMapper.selectOne(any())).thenReturn(null);

        TenantAuthException ex = assertThrows(TenantAuthException.class,
                () -> auditTaskService.getStatus(TASK_ID));
        assertEquals(404, ex.getStatus());
        assertEquals("RESOURCE_NOT_FOUND", ex.getErrorCode());
    }

    /**
     * getStatus 非任务所有者访问时抛出 403 BizException。
     */
    @Test
    void getStatus_shouldThrow403WhenNotAuthorized() {
        AuditTask task = AuditTask.builder()
                .id(100L)
                .taskId(TASK_ID)
                .bidId(BID_ID)
                .taskStatus(AuditTaskStatusEnum.PENDING.getCode())
                .auditUserId(ANOTHER_USER_ID)
                .build();

        when(auditTaskMapper.selectOne(any())).thenReturn(task);
        // 标书上传者也是 ANOTHER_USER_ID，非当前用户
        Tender tender = Tender.builder()
                .id(BID_ID)
                .uploadUserId(ANOTHER_USER_ID)
                .build();
        when(tenderMapper.selectOne(any())).thenReturn(tender);

        BizException ex = assertThrows(BizException.class,
                () -> auditTaskService.getStatus(TASK_ID));
        assertEquals(403, ex.getCode());
        assertEquals("无权访问该任务", ex.getMessage());
    }

    /**
     * getStatus 标书上传者（非任务创建者）应有权限访问。
     */
    @Test
    void getStatus_shouldAllowTenderUploaderAccess() {
        BaseContext.setCurrentId(TENDER_UPLOADER_ID);

        AuditTask task = AuditTask.builder()
                .id(100L)
                .taskId(TASK_ID)
                .bidId(BID_ID)
                .taskStatus(AuditTaskStatusEnum.COMPLETED.getCode())
                .stage("SUMMARY")
                .progress(100)
                .auditUserId(ANOTHER_USER_ID)
                .failedStages(new ArrayList<>())
                .build();

        when(auditTaskMapper.selectOne(any())).thenReturn(task);
        when(tenderMapper.selectOne(any())).thenReturn(
                Tender.builder().id(BID_ID).uploadUserId(TENDER_UPLOADER_ID).build());
        when(auditTaskMapper.selectCount(argThat(x -> x != null)))
                .thenReturn(10L, 2L, 1L, 0L);

        AuditTaskStatusVO vo = auditTaskService.getStatus(TASK_ID);

        assertEquals(TASK_ID, vo.getTaskId());
        assertEquals("completed", vo.getStatus());
    }

    /**
     * getStatus 任务没有被关联标书：isTaskOwner 应通过 auditUserId 匹配。
     */
    @Test
    void getStatus_withNullBidId_shouldCheckAuditUserIdOnly() {
        AuditTask task = AuditTask.builder()
                .id(100L)
                .taskId(TASK_ID)
                .bidId(null)
                .taskStatus(AuditTaskStatusEnum.PENDING.getCode())
                .stage(AuditStageEnum.UPLOADING.name())
                .progress(0)
                .auditUserId(CURRENT_USER_ID)
                .failedStages(new ArrayList<>())
                .build();

        when(auditTaskMapper.selectOne(any())).thenReturn(task);
        when(auditTaskMapper.selectCount(argThat(x -> x != null)))
                .thenReturn(5L, 2L, 1L, 0L);

        AuditTaskStatusVO vo = auditTaskService.getStatus(TASK_ID);
        assertEquals(TASK_ID, vo.getTaskId());
    }

    // ═══════════════════════════════════════════════════════════════════
    // getResult 测试
    // ═══════════════════════════════════════════════════════════════════

    /**
     * getResult 任务已完成且 Rust 可用：返回完整审核结果。
     */
    @Test
    void getResult_shouldReturnIssuesFromRustWhenCompleted() {
        AuditTask task = AuditTask.builder()
                .id(100L)
                .taskId(TASK_ID)
                .bidId(BID_ID)
                .taskStatus(AuditTaskStatusEnum.COMPLETED.getCode())
                .auditUserId(CURRENT_USER_ID)
                .build();

        Tender tender = Tender.builder()
                .id(BID_ID)
                .rustDocumentId(RUST_DOC_ID)
                .build();

        // 构造 Rust 返回的风险发现
        RustRiskFinding finding = new RustRiskFinding();
        finding.setRiskId("R001");
        finding.setSeverity("high");
        finding.setRiskType("资质风险");
        finding.setReason("企业资质不符合招标要求");
        finding.setSuggestion("请提供符合要求的 xx 级资质证书");
        finding.setSourceQuote("投标人须具备 xx 级及以上资质");
        finding.setPageNumber(3);
        finding.setSectionPath(List.of("第一章", "投标人资格要求"));

        RustReviewResponse review = new RustReviewResponse();
        review.setFindings(List.of(finding));
        review.setDocumentId(RUST_DOC_ID);

        RustReviewResultResponse rustResult = new RustReviewResultResponse();
        rustResult.setStatus("completed");
        rustResult.setResult(review);

        when(auditTaskMapper.selectOne(any())).thenReturn(task);
        when(tenderMapper.selectOne(any())).thenReturn(tender);
        when(rustApiClient.getReviewResult(RUST_DOC_ID)).thenReturn(rustResult);

        ResultVO result = auditTaskService.getResult(TASK_ID, null, null, null);

        assertNotNull(result);
        assertEquals(TASK_ID, result.getTaskId());
        assertEquals("revise", result.getAuditResult(),
                "存在 risk 时应为 revise");
        assertNotNull(result.getSummary());
        assertEquals(Integer.valueOf(1), result.getSummary().getTotalIssues());
        assertEquals(Integer.valueOf(1), result.getSummary().getHigh());
        assertEquals(Integer.valueOf(0), result.getSummary().getMedium());
        assertEquals(Integer.valueOf(0), result.getSummary().getLow());
        assertEquals(Integer.valueOf(0), result.getSummary().getInfo());

        assertNotNull(result.getIssues());
        assertEquals(1, result.getIssues().size());
        IssueVO issue = result.getIssues().get(0);
        assertEquals("ISSUE-R001", issue.getIssueNo());
        assertEquals("high", issue.getSeverity());
        assertEquals("资质风险", issue.getCategory());
        assertEquals("企业资质不符合招标要求", issue.getDescription());
        assertEquals("请提供符合要求的 xx 级资质证书", issue.getSuggestion());

        assertNotNull(issue.getLocation());
        assertEquals(Integer.valueOf(3), issue.getLocation().getPageNumber());
        assertEquals("第一章 > 投标人资格要求", issue.getLocation().getSectionName());
    }

    /**
     * getResult 任务已完成但无风险发现：返回 pass。
     */
    @Test
    void getResult_shouldReturnPassWhenNoFindings() {
        AuditTask task = AuditTask.builder()
                .id(100L)
                .taskId(TASK_ID)
                .bidId(BID_ID)
                .taskStatus(AuditTaskStatusEnum.COMPLETED.getCode())
                .auditUserId(CURRENT_USER_ID)
                .build();

        Tender tender = Tender.builder()
                .id(BID_ID)
                .rustDocumentId(RUST_DOC_ID)
                .build();

        RustReviewResponse review = new RustReviewResponse();
        review.setFindings(List.of());
        review.setDocumentId(RUST_DOC_ID);

        RustReviewResultResponse rustResult = new RustReviewResultResponse();
        rustResult.setStatus("completed");
        rustResult.setResult(review);

        when(auditTaskMapper.selectOne(any())).thenReturn(task);
        when(tenderMapper.selectOne(any())).thenReturn(tender);
        when(rustApiClient.getReviewResult(RUST_DOC_ID)).thenReturn(rustResult);

        ResultVO result = auditTaskService.getResult(TASK_ID, null, null, null);

        assertEquals("pass", result.getAuditResult(),
                "无风险时应为 pass");
        assertEquals(Integer.valueOf(0), result.getSummary().getTotalIssues());
        assertTrue(result.getIssues().isEmpty());
    }

    /**
     * getResult 任务尚未完成：返回 pending 空结果。
     */
    @Test
    void getResult_shouldReturnPendingWhenTaskNotCompleted() {
        AuditTask task = AuditTask.builder()
                .id(100L)
                .taskId(TASK_ID)
                .bidId(BID_ID)
                .taskStatus(AuditTaskStatusEnum.PROCESSING.getCode())
                .auditUserId(CURRENT_USER_ID)
                .build();

        when(auditTaskMapper.selectOne(any())).thenReturn(task);

        ResultVO result = auditTaskService.getResult(TASK_ID, null, null, null);

        assertEquals(TASK_ID, result.getTaskId());
        assertEquals("pending", result.getAuditResult());
        assertNotNull(result.getSummary());
        assertNull(result.getSummary().getTotalIssues(),
                "新创建的 SummaryVO totalIssues 应为 null");
        assertTrue(result.getIssues().isEmpty());
    }

    /**
     * getResult 任务已完成但 Rust 不可用：回退到 audit_issue 表。
     */
    @Test
    void getResult_shouldFallbackToDbWhenRustUnavailable() {
        AuditTask task = AuditTask.builder()
                .id(100L)
                .taskId(TASK_ID)
                .bidId(BID_ID)
                .taskStatus(AuditTaskStatusEnum.COMPLETED.getCode())
                .auditUserId(CURRENT_USER_ID)
                .build();

        Tender tender = Tender.builder()
                .id(BID_ID)
                .rustDocumentId(RUST_DOC_ID)
                .build();

        AuditIssue issue = AuditIssue.builder()
                .id(1L)
                .auditId(100L)
                .issueNo("I001")
                .severity("medium")
                .category("程序违规")
                .description("未提供法定代表人身份证明")
                .suggestion("请补充法定代表人身份证明")
                .pageNumber(5)
                .sectionName("形式评审 > 资格证明文件")
                .context("投标人须提供法定代表人身份证明")
                .reference("《招标投标法》第25条; 《招标投标法实施条例》第34条")
                .build();

        when(auditTaskMapper.selectOne(any())).thenReturn(task);
        when(tenderMapper.selectOne(any())).thenReturn(tender);
        when(rustApiClient.getReviewResult(RUST_DOC_ID))
                .thenThrow(new RuntimeException("Rust 服务不可用"));
        when(auditIssueMapper.selectList(any())).thenReturn(List.of(issue));

        ResultVO result = auditTaskService.getResult(TASK_ID, null, null, null);

        assertEquals(TASK_ID, result.getTaskId());
        assertEquals("revise", result.getAuditResult());
        assertEquals(Integer.valueOf(1), result.getSummary().getTotalIssues());
        assertEquals(Integer.valueOf(1), result.getSummary().getMedium());

        assertEquals(1, result.getIssues().size());
        IssueVO vo = result.getIssues().get(0);
        assertEquals("ISSUE-I001", vo.getIssueNo());
        assertEquals("medium", vo.getSeverity());
        assertEquals("程序违规", vo.getCategory());
        assertEquals("未提供法定代表人身份证明", vo.getDescription());
        assertEquals("请补充法定代表人身份证明", vo.getSuggestion());
        assertNotNull(vo.getLegalBasis());
        assertEquals(2, vo.getLegalBasis().size());
        assertTrue(vo.getLegalBasis().contains("《招标投标法》第25条"));
        assertTrue(vo.getLegalBasis().contains("《招标投标法实施条例》第34条"));
    }

    /**
     * getResult 已完成但没有 rustDocumentId：返回 pending。
     */
    @Test
    void getResult_shouldReturnPendingWhenNoRustDocumentId() {
        AuditTask task = AuditTask.builder()
                .id(100L)
                .taskId(TASK_ID)
                .bidId(BID_ID)
                .taskStatus(AuditTaskStatusEnum.COMPLETED.getCode())
                .auditUserId(CURRENT_USER_ID)
                .build();

        Tender tender = Tender.builder()
                .id(BID_ID)
                .rustDocumentId(null)
                .build();

        when(auditTaskMapper.selectOne(any())).thenReturn(task);
        when(tenderMapper.selectOne(any())).thenReturn(tender);

        ResultVO result = auditTaskService.getResult(TASK_ID, null, null, null);

        assertEquals("pending", result.getAuditResult());
        assertTrue(result.getIssues().isEmpty());
    }

    /**
     * getResult noRisk 的 finding 应被过滤掉（shouldSkip = true）。
     */
    @Test
    void getResult_shouldFilterNoRiskFindings() {
        AuditTask task = AuditTask.builder()
                .id(100L)
                .taskId(TASK_ID)
                .bidId(BID_ID)
                .taskStatus(AuditTaskStatusEnum.COMPLETED.getCode())
                .auditUserId(CURRENT_USER_ID)
                .build();

        Tender tender = Tender.builder()
                .id(BID_ID)
                .rustDocumentId(RUST_DOC_ID)
                .build();

        // 一个正常的 high 风险发现
        RustRiskFinding realFinding = new RustRiskFinding();
        realFinding.setRiskId("R001");
        realFinding.setSeverity("high");
        realFinding.setRiskType("品牌指定");
        realFinding.setReason("指定了唯一品牌");
        realFinding.setSourceQuote("推荐使用 XX 品牌产品");

        // 一个应被过滤的无风险发现（noRisk=true, truncated=false）
        RustRiskFinding noRiskFinding = new RustRiskFinding();
        noRiskFinding.setRiskId("R002");
        noRiskFinding.setSeverity("info");
        noRiskFinding.setNoRisk(true);
        noRiskFinding.setTruncated(false);
        noRiskFinding.setRiskType("建议");

        RustReviewResponse review = new RustReviewResponse();
        review.setFindings(List.of(realFinding, noRiskFinding));

        RustReviewResultResponse rustResult = new RustReviewResultResponse();
        rustResult.setStatus("completed");
        rustResult.setResult(review);

        when(auditTaskMapper.selectOne(any())).thenReturn(task);
        when(tenderMapper.selectOne(any())).thenReturn(tender);
        when(rustApiClient.getReviewResult(RUST_DOC_ID)).thenReturn(rustResult);

        ResultVO result = auditTaskService.getResult(TASK_ID, null, null, null);

        assertEquals("revise", result.getAuditResult());
        assertEquals(Integer.valueOf(1), result.getSummary().getTotalIssues(),
                "noRisk finding 应被过滤");
        assertEquals(1, result.getIssues().size());
        assertEquals("ISSUE-R001", result.getIssues().get(0).getIssueNo());
    }

    // ═══════════════════════════════════════════════════════════════════
    // countByWeek 测试
    // ═══════════════════════════════════════════════════════════════════

    /**
     * countByWeek 正常路径：返回按星期聚合的统计。
     */
    @Test
    void countByWeek_shouldReturnWeeklyCounts() {
        when(tenderService.getBidIdsByUserId(CURRENT_USER_ID))
                .thenReturn(List.of(BID_ID, BID_ID_2));

        LocalDate today = LocalDate.now();
        LocalDate monday = today.with(TemporalAdjusters.previousOrSame(DayOfWeek.MONDAY));
        DateTimeFormatter fmt = DateTimeFormatter.ofPattern("yyyy-MM-dd");

        Map<String, Object> rowMonday = new HashMap<>();
        rowMonday.put("day_date", monday.format(fmt));
        rowMonday.put("count", 2L);

        Map<String, Object> rowWednesday = new HashMap<>();
        rowWednesday.put("day_date", monday.plusDays(2).format(fmt));
        rowWednesday.put("count", 5L);

        when(auditTaskMapper.countByWeek(anyLong(), eq(List.of(BID_ID, BID_ID_2))))
                .thenReturn(List.of(rowMonday, rowWednesday));

        Map<String, Long> result = auditTaskService.countByWeek();

        assertNotNull(result);
        assertEquals(7, result.size(), "应包含周一至周日共 7 天的 key");

        // 验证实际返回的统计值
        String monName = capitalizeDay(monday.getDayOfWeek().name());
        String wedName = capitalizeDay(monday.plusDays(2).getDayOfWeek().name());
        assertEquals(2L, result.get(monName).longValue(),
                "周一应有 2 个任务");
        assertEquals(5L, result.get(wedName).longValue(),
                "周三应有 5 个任务");
    }

    /**
     * countByWeek 当前用户为 null：返回全零初始 map。
     */
    @Test
    void countByWeek_shouldReturnEmptyWhenNoUser() {
        BaseContext.removeCurrentId();

        Map<String, Long> result = auditTaskService.countByWeek();

        assertNotNull(result);
        assertEquals(7, result.size());
        result.values().forEach(v -> assertEquals(0L, v));

        // 不应调用 tenderService
        verifyNoInteractions(tenderService);
    }

    /**
     * countByWeek 当前用户无标书：返回全零初始 map。
     */
    @Test
    void countByWeek_shouldReturnEmptyMapWhenNoBidIds() {
        when(tenderService.getBidIdsByUserId(CURRENT_USER_ID))
                .thenReturn(List.of());

        Map<String, Long> result = auditTaskService.countByWeek();

        assertNotNull(result);
        assertEquals(7, result.size());
        result.values().forEach(v -> assertEquals(0L, v));
        verify(auditTaskMapper, never()).countByWeek(anyLong(), any());
    }

    // ═══════════════════════════════════════════════════════════════════
    // getBlockBboxes 测试
    // ═══════════════════════════════════════════════════════════════════

    /**
     * getBlockBboxes 正常路径：正确委托 RustApiClient。
     */
    @Test
    void getBlockBboxes_shouldReturnBboxData() {
        AuditTask task = AuditTask.builder()
                .id(100L)
                .taskId(TASK_ID)
                .bidId(BID_ID)
                .taskStatus(AuditTaskStatusEnum.COMPLETED.getCode())
                .auditUserId(CURRENT_USER_ID)
                .build();

        Tender tender = Tender.builder()
                .id(BID_ID)
                .rustDocumentId(RUST_DOC_ID)
                .build();

        RustBlockBBoxResponse bbox = new RustBlockBBoxResponse();
        bbox.setBlockId("block-001");
        bbox.setPage(1);

        when(auditTaskMapper.selectOne(any())).thenReturn(task);
        when(tenderMapper.selectOne(any())).thenReturn(tender);
        when(rustApiClient.getBlockBboxes(RUST_DOC_ID, "block-001,block-002"))
                .thenReturn(List.of(bbox));

        List<RustBlockBBoxResponse> result =
                auditTaskService.getBlockBboxes(TASK_ID, "block-001,block-002");

        assertNotNull(result);
        assertEquals(1, result.size());
        assertEquals("block-001", result.get(0).getBlockId());
        assertEquals(1, result.get(0).getPage());
    }

    /**
     * getBlockBboxes 标书不存在：返回空列表。
     */
    @Test
    void getBlockBboxes_shouldReturnEmptyWhenTenderNotFound() {
        AuditTask task = AuditTask.builder()
                .id(100L)
                .taskId(TASK_ID)
                .bidId(BID_ID)
                .taskStatus(AuditTaskStatusEnum.PENDING.getCode())
                .auditUserId(CURRENT_USER_ID)
                .build();

        when(auditTaskMapper.selectOne(any())).thenReturn(task);
        when(tenderMapper.selectOne(any())).thenReturn(null);

        List<RustBlockBBoxResponse> result =
                auditTaskService.getBlockBboxes(TASK_ID, "block-001");

        assertNotNull(result);
        assertTrue(result.isEmpty());
        verify(rustApiClient, never()).getBlockBboxes(anyString(), anyString());
    }

    /**
     * getBlockBboxes 标书没有 rustDocumentId：返回空列表。
     */
    @Test
    void getBlockBboxes_shouldReturnEmptyWhenNoRustDocId() {
        AuditTask task = AuditTask.builder()
                .id(100L)
                .taskId(TASK_ID)
                .bidId(BID_ID)
                .taskStatus(AuditTaskStatusEnum.COMPLETED.getCode())
                .auditUserId(CURRENT_USER_ID)
                .build();

        Tender tender = Tender.builder()
                .id(BID_ID)
                .rustDocumentId("")
                .build();

        when(auditTaskMapper.selectOne(any())).thenReturn(task);
        when(tenderMapper.selectOne(any())).thenReturn(tender);

        List<RustBlockBBoxResponse> result =
                auditTaskService.getBlockBboxes(TASK_ID, "block-001");

        assertNotNull(result);
        assertTrue(result.isEmpty());
        verify(rustApiClient, never()).getBlockBboxes(anyString(), anyString());
    }

    // ═══════════════════════════════════════════════════════════════════
    // markTaskProcessing / markTaskFailed 测试
    // ═══════════════════════════════════════════════════════════════════

    /**
     * markTaskProcessing PENDING → PROCESSING：验证状态迁移和字段更新。
     */
    @Test
    void markTaskProcessing_shouldTransitionToProcessing() {
        AuditTask task = AuditTask.builder()
                .id(100L)
                .taskId(TASK_ID)
                .bidId(BID_ID)
                .taskStatus(AuditTaskStatusEnum.PENDING.getCode())
                .auditUserId(CURRENT_USER_ID)
                .progress(0)
                .build();

        when(auditTaskMapper.selectOne(any())).thenReturn(task);
        when(eventService.persist(anyString(), any(SseEventTypeEnum.class), any()))
                .thenReturn("evt-001");
        doNothing().when(sseHub).emit(anyString(), any(SseEventTypeEnum.class),
                any(), anyString());

        auditTaskService.markTaskProcessing(TASK_ID);

        ArgumentCaptor<AuditTask> captor = ArgumentCaptor.forClass(AuditTask.class);
        verify(auditTaskMapper).update(captor.capture(), any());

        AuditTask updated = captor.getValue();
        assertEquals(AuditTaskStatusEnum.PROCESSING.getCode(), updated.getTaskStatus(),
                "状态应从 PENDING 变为 PROCESSING");
        assertEquals("UPLOADING", updated.getStage());
        assertTrue(updated.getProgress() >= 5,
                "进度应至少为 5");
        assertNotNull(updated.getStartTime(),
                "应记录开始时间");
        assertNotNull(updated.getUpdatedAt());
    }

    /**
     * markTaskProcessing 已完成的任务不应再变更。
     */
    @Test
    void markTaskProcessing_shouldSkipCompletedTask() {
        AuditTask task = AuditTask.builder()
                .id(100L)
                .taskId(TASK_ID)
                .bidId(BID_ID)
                .taskStatus(AuditTaskStatusEnum.COMPLETED.getCode())
                .auditUserId(CURRENT_USER_ID)
                .build();

        when(auditTaskMapper.selectOne(any())).thenReturn(task);

        auditTaskService.markTaskProcessing(TASK_ID);

        verify(auditTaskMapper, never()).update(any(), any());
    }

    /**
     * markTaskFailed PENDING → FAILED：验证错误信息写入。
     */
    @Test
    void markTaskFailed_shouldTransitionToFailed() {
        AuditTask task = AuditTask.builder()
                .id(100L)
                .taskId(TASK_ID)
                .bidId(BID_ID)
                .taskStatus(AuditTaskStatusEnum.PROCESSING.getCode())
                .auditUserId(CURRENT_USER_ID)
                .build();

        when(auditTaskMapper.selectOne(any())).thenReturn(task);
        when(eventService.persist(anyString(), any(SseEventTypeEnum.class), any()))
                .thenReturn("evt-002");
        doNothing().when(sseHub).emit(anyString(), any(SseEventTypeEnum.class),
                any(), anyString());

        auditTaskService.markTaskFailed(TASK_ID, "上传失败：文件格式不支持");

        ArgumentCaptor<AuditTask> captor = ArgumentCaptor.forClass(AuditTask.class);
        verify(auditTaskMapper).update(captor.capture(), any());

        AuditTask updated = captor.getValue();
        assertEquals(AuditTaskStatusEnum.FAILED.getCode(), updated.getTaskStatus(),
                "状态应从 PROCESSING 变为 FAILED");
        assertEquals("上传失败：文件格式不支持", updated.getErrorMsg());
        assertNotNull(updated.getEndTime());
    }

    /**
     * markTaskFailed completed 任务不应被覆盖。
     */
    @Test
    void markTaskFailed_shouldSkipCompletedTask() {
        AuditTask task = AuditTask.builder()
                .id(100L)
                .taskId(TASK_ID)
                .bidId(BID_ID)
                .taskStatus(AuditTaskStatusEnum.COMPLETED.getCode())
                .auditUserId(CURRENT_USER_ID)
                .build();

        when(auditTaskMapper.selectOne(any())).thenReturn(task);

        auditTaskService.markTaskFailed(TASK_ID, "some error");

        verify(auditTaskMapper, never()).update(any(), any());
    }

    // ═══════════════════════════════════════════════════════════════════
    // processAuditResult（已废弃）测试
    // ═══════════════════════════════════════════════════════════════════

    /**
     * processAuditResult 是已废弃的回调路径：仅打日志不应有任何副作用。
     */
    @Test
    void processAuditResult_shouldBeNoOp() {
        assertDoesNotThrow(() ->
                auditTaskService.processAuditResult(TASK_ID, "{}"));

        // 所有被模拟的 bean 都不应有交互
        verifyNoInteractions(auditTaskMapper, tenderMapper, rustApiClient,
                auditIssueMapper, taskDispatcher);
    }

    // ═══════════════════════════════════════════════════════════════════
    // getAuditIdsByBidIds 测试
    // ═══════════════════════════════════════════════════════════════════

    /**
     * getAuditIdsByBidIds 正常查询。
     * 需要 MyBatis-Plus Lambda 缓存运行时；纯单元测试中不可用。
     */
    @Test
    @org.junit.jupiter.api.Disabled("MyBatis-Plus Lambda 缓存需要 Spring 容器初始化")
    void getAuditIdsByBidIds_shouldReturnIds() {
        when(auditTaskMapper.selectObjs(any())).thenReturn(List.of(1L, 2L, 3L));

        List<Long> result = auditTaskService.getAuditIdsByBidIds(
                List.of(BID_ID, BID_ID_2));

        assertNotNull(result);
        assertEquals(3, result.size());
        assertEquals(List.of(1L, 2L, 3L), result);
    }

    /**
     * getAuditIdsByBidIds 空输入应直接返回空列表。
     */
    @Test
    void getAuditIdsByBidIds_shouldReturnEmptyForEmptyInput() {
        List<Long> result = auditTaskService.getAuditIdsByBidIds(List.of());

        assertNotNull(result);
        assertTrue(result.isEmpty());
        verify(auditTaskMapper, never()).selectObjs(any());
    }

    /**
     * getAuditIdsByBidIds 返回值中的非 Long 类型应被过滤。
     * 需要 MyBatis-Plus Lambda 缓存运行时；纯单元测试中不可用。
     */
    @Test
    @org.junit.jupiter.api.Disabled("MyBatis-Plus Lambda 缓存需要 Spring 容器初始化")
    void getAuditIdsByBidIds_shouldFilterNonLongResults() {
        List<Object> mixed = new ArrayList<>();
        mixed.add(1L);
        mixed.add(null);
        mixed.add("not-a-long");
        mixed.add(2L);

        when(auditTaskMapper.selectObjs(any())).thenReturn(mixed);

        List<Long> result = auditTaskService.getAuditIdsByBidIds(
                List.of(BID_ID, BID_ID_2));

        assertEquals(2, result.size());
        assertEquals(List.of(1L, 2L), result);
    }

    // ═══════════════════════════════════════════════════════════════════
    // subscribeStream 测试
    // ═══════════════════════════════════════════════════════════════════

    /**
     * subscribeStream 订阅任务 SSE 流，无 lastEventId 时发送初始 PROGRESS。
     */
    @Test
    void subscribeStream_shouldCreateEmitterAndEmitProgress()
            throws Exception {
        AuditTask task = AuditTask.builder()
                .id(100L)
                .taskId(TASK_ID)
                .bidId(BID_ID)
                .taskStatus(AuditTaskStatusEnum.PENDING.getCode())
                .stage("UPLOADING")
                .progress(0)
                .auditUserId(CURRENT_USER_ID)
                .failedStages(new ArrayList<>())
                .build();

        when(auditTaskMapper.selectOne(any())).thenReturn(task);
        when(auditTaskMapper.selectCount(argThat(x -> x != null)))
                .thenReturn(10L, 3L, 2L, 0L);

        var emitter = new org.springframework.web.servlet.mvc.method.annotation.SseEmitter();
        when(sseHub.subscribe(TASK_ID)).thenReturn(emitter);
        doNothing().when(sseHub).emitToEmitter(
                eq(emitter), eq(SseEventTypeEnum.PROGRESS), any(), isNull());

        var result = auditTaskService.subscribeStream(TASK_ID, null);

        assertNotNull(result);
        verify(sseHub).subscribe(TASK_ID);
        verify(sseHub).emitToEmitter(
                eq(emitter), eq(SseEventTypeEnum.PROGRESS), any(), isNull());
    }

    // ═══════════════════════════════════════════════════════════════════
    // 辅助方法
    // ═══════════════════════════════════════════════════════════════════

    private static String capitalizeDay(String dayName) {
        return dayName.substring(0, 1).toUpperCase()
                + dayName.substring(1).toLowerCase();
    }
}
