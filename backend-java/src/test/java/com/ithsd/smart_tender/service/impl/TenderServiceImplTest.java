package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.baomidou.mybatisplus.core.conditions.query.QueryWrapper;
import com.baomidou.mybatisplus.extension.plugins.pagination.Page;
import com.ithsd.smart_tender.common.BaseContext;
import com.ithsd.smart_tender.common.BizException;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.mapper.AuditTaskMapper;
import com.ithsd.smart_tender.mapper.ProjectMapper;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.dto.TenderDTO;
import com.ithsd.smart_tender.model.dto.TenderPageQueryDTO;
import com.ithsd.smart_tender.model.entity.AuditTask;
import com.ithsd.smart_tender.model.entity.Project;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.model.entity.User;
import com.ithsd.smart_tender.model.result.PageResult;
import com.ithsd.smart_tender.model.vo.TenderProjectVO;
import com.ithsd.smart_tender.model.vo.TenderStatsVO;
import com.ithsd.smart_tender.model.vo.TenderVO;
import com.ithsd.smart_tender.service.AuditTaskService;
import com.ithsd.smart_tender.service.StoragePathService;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.ArgumentCaptor;
import org.mockito.Captor;
import org.mockito.InjectMocks;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;
import org.mockito.junit.jupiter.MockitoSettings;
import org.mockito.quality.Strictness;
import org.springframework.mock.web.MockMultipartFile;
import org.springframework.web.multipart.MultipartFile;

import java.io.IOException;
import java.math.BigDecimal;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.util.*;

import static org.junit.jupiter.api.Assertions.*;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.*;

@ExtendWith(MockitoExtension.class)
@MockitoSettings(strictness = Strictness.LENIENT)
class TenderServiceImplTest {

    @Mock
    private TenderMapper tenderMapper;

    @Mock
    private AuditTaskMapper auditTaskMapper;

    @Mock
    private UserMapper userMapper;

    @Mock
    private ProjectMapper projectMapper;

    @Mock
    private AuditTaskService auditTaskService;

    @Mock
    private StoragePathService storagePathService;

    @InjectMocks
    private TenderServiceImpl tenderService;

    @Captor
    private ArgumentCaptor<Tender> tenderCaptor;

    @Captor
    private ArgumentCaptor<Project> projectCaptor;

    private static final Long CURRENT_USER_ID = 10001L;
    private static final Long CURRENT_TENANT_ID = 20001L;

    @BeforeEach
    void setUp() {
        BaseContext.setCurrentId(CURRENT_USER_ID);
        TenantContext.set(new TenantRequestContext(
                CURRENT_USER_ID, CURRENT_TENANT_ID, "OWNER", 1L, "tender-test"));
    }

    @AfterEach
    void tearDown() {
        TenantContext.clear();
        BaseContext.removeCurrentId();
    }

    // ========================= upload =========================

    @Test
    void upload_shouldSucceed_whenValidFileAndDto() throws Exception {
        byte[] content = "fake pdf content".getBytes();
        MultipartFile file = mock(MultipartFile.class);

        TenderDTO dto = new TenderDTO();
        dto.setBidName("测试标书");
        dto.setProjectId(100L);
        dto.setTenantId(30002L);
        dto.setSupplierName("测试供应商");
        dto.setBudgetAmount(new BigDecimal("1000000"));
        dto.setFileCategory("标书");
        dto.setVersion(1);

        Path destPath = Paths.get("data", "uploads", "tenders", "2024-01-01", "uuid.pdf")
                .toAbsolutePath().normalize();
        when(file.isEmpty()).thenReturn(false);
        when(file.getOriginalFilename()).thenReturn("test.pdf");
        when(file.getSize()).thenReturn(12L);
        when(file.getContentType()).thenReturn("application/pdf");
        when(storagePathService.buildTenderUploadPath("test.pdf")).thenReturn(destPath);
        when(storagePathService.toStoredPath(destPath))
                .thenReturn("tenders/2024-01-01/uuid.pdf");

        Project project = Project.builder()
                .id(100L)
                .latestVersion(null)
                .build();
        when(projectMapper.selectOne(any())).thenReturn(project);

        TenderVO result = tenderService.upload(file, dto);

        assertNotNull(result);
        assertEquals("测试标书", result.getBidName());
        assertEquals("测试供应商", result.getSupplierName());
        assertEquals("pdf", result.getFileType());
        assertEquals("bid", result.getFileCategory());
        assertEquals("test.pdf", result.getFileName());
        assertEquals(file.getSize(), result.getFileSize());
        assertEquals(100L, result.getProjectId());
        assertEquals(1, result.getVersion());

        verify(storagePathService).ensureParentDirectory(destPath);
        verify(tenderMapper).insert(tenderCaptor.capture());
        Tender captured = tenderCaptor.getValue();
        assertEquals("测试标书", captured.getBidName());
        assertEquals("test.pdf", captured.getFileName());
        assertEquals("tenders/2024-01-01/uuid.pdf", captured.getFilePath());
        assertEquals(file.getSize(), captured.getFileSize());
        assertEquals("pdf", captured.getFileType());
        assertEquals("bid", captured.getFileCategory());
        assertEquals(0, captured.getParseStatus());
        assertEquals(CURRENT_USER_ID, captured.getUploadUserId());
        assertEquals(CURRENT_TENANT_ID, captured.getTenantId());
        assertEquals(1, captured.getVersion());
        assertEquals(100L, captured.getProjectId());
        assertNotNull(captured.getUploadTime());

        verify(projectMapper, times(2)).selectOne(any());
        verify(projectMapper).update(projectCaptor.capture(), any());
        assertEquals(1, projectCaptor.getValue().getLatestVersion());
        assertEquals(0, projectCaptor.getValue().getParseStatus());
        assertNotNull(projectCaptor.getValue().getUpdateTime());
    }

    @Test
    void upload_shouldThrow_whenFileIsEmpty() {
        MultipartFile file = new MockMultipartFile("file", "test.pdf",
                "application/pdf", new byte[0]);
        TenderDTO dto = new TenderDTO();

        BizException ex = assertThrows(BizException.class,
                () -> tenderService.upload(file, dto));
        assertEquals(400, ex.getCode());
        assertTrue(ex.getMessage().contains("文件为空"));
    }

    @Test
    void upload_shouldThrow_whenFileNameIsNull() {
        MultipartFile file = mock(MultipartFile.class);
        when(file.isEmpty()).thenReturn(false);
        when(file.getOriginalFilename()).thenReturn(null);

        TenderDTO dto = new TenderDTO();

        BizException ex = assertThrows(BizException.class,
                () -> tenderService.upload(file, dto));
        assertEquals(400, ex.getCode());
        assertTrue(ex.getMessage().contains("文件名不能为空"));
    }

    @Test
    void upload_shouldThrow_whenDirectoryCreationFails() throws Exception {
        MultipartFile file = mock(MultipartFile.class);

        TenderDTO dto = new TenderDTO();
        dto.setProjectId(100L);
        when(projectMapper.selectOne(any()))
                .thenReturn(Project.builder()
                        .id(100L)
                        .tenantId(CURRENT_TENANT_ID)
                        .build());

        Path destPath = Paths.get("data", "uploads", "tenders", "2024-01-01", "uuid.pdf")
                .toAbsolutePath().normalize();
        when(file.isEmpty()).thenReturn(false);
        when(file.getOriginalFilename()).thenReturn("test.pdf");
        when(file.getSize()).thenReturn(12L);
        when(file.getContentType()).thenReturn("application/pdf");
        when(storagePathService.buildTenderUploadPath("test.pdf")).thenReturn(destPath);
        doThrow(new IOException("磁盘空间不足"))
                .when(storagePathService).ensureParentDirectory(destPath);

        BizException ex = assertThrows(BizException.class,
                () -> tenderService.upload(file, dto));
        assertEquals(500, ex.getCode());
        assertTrue(ex.getMessage().contains("文件目录创建失败"));
    }

    @Test
    void upload_shouldThrow404_whenProjectBelongsToOtherTenant() {
        MultipartFile file = new MockMultipartFile(
                "file", "test.pdf", "application/pdf", "data".getBytes());
        TenderDTO dto = new TenderDTO();
        dto.setProjectId(999L);
        dto.setTenantId(30002L);
        when(projectMapper.selectOne(any())).thenReturn(null);

        TenantAuthException ex = assertThrows(TenantAuthException.class,
                () -> tenderService.upload(file, dto));

        assertEquals(404, ex.getStatus());
        assertEquals("RESOURCE_NOT_FOUND", ex.getErrorCode());
        verify(tenderMapper, never()).insert(any());
    }

    @Test
    void upload_shouldDetectFileType_whenWordDocument() throws Exception {
        byte[] content = "fake docx content".getBytes();
        MultipartFile file = mock(MultipartFile.class);

        TenderDTO dto = new TenderDTO();
        dto.setProjectId(101L);
        dto.setBidName("文档标书");
        dto.setFileCategory("标书");

        Path destPath = Paths.get("data", "uploads", "tenders", "2024-01-01", "uuid.docx")
                .toAbsolutePath().normalize();
        when(file.isEmpty()).thenReturn(false);
        when(file.getOriginalFilename()).thenReturn("report.docx");
        when(file.getSize()).thenReturn((long) content.length);
        when(file.getContentType()).thenReturn("application/vnd.openxmlformats-officedocument.wordprocessingml.document");
        when(storagePathService.buildTenderUploadPath("report.docx")).thenReturn(destPath);
        when(storagePathService.toStoredPath(destPath))
                .thenReturn("tenders/2024-01-01/uuid.docx");

        Project project = Project.builder()
                .id(101L)
                .latestVersion(2)
                .build();
        when(projectMapper.selectOne(any())).thenReturn(project);

        TenderVO result = tenderService.upload(file, dto);

        assertNotNull(result);
        assertEquals("word", result.getFileType());
        assertEquals("bid", result.getFileCategory());

        verify(tenderMapper).insert(tenderCaptor.capture());
        Tender captured = tenderCaptor.getValue();
        assertEquals("word", captured.getFileType());
        assertEquals("bid", captured.getFileCategory());
    }

    @Test
    void upload_shouldSetFileCategoryToContract_whenDtoSaysContract() throws Exception {
        byte[] content = "data".getBytes();
        MultipartFile file = mock(MultipartFile.class);

        TenderDTO dto = new TenderDTO();
        dto.setProjectId(102L);
        dto.setFileCategory("合同");

        Path destPath = Paths.get("data", "uploads", "tenders", "2024-01-01", "uuid.pdf")
                .toAbsolutePath().normalize();
        when(file.isEmpty()).thenReturn(false);
        when(file.getOriginalFilename()).thenReturn("contract.pdf");
        when(file.getSize()).thenReturn((long) content.length);
        when(file.getContentType()).thenReturn("application/pdf");
        when(storagePathService.buildTenderUploadPath("contract.pdf")).thenReturn(destPath);
        when(storagePathService.toStoredPath(destPath))
                .thenReturn("tenders/2024-01-01/uuid.pdf");
        when(projectMapper.selectOne(any())).thenReturn(Project.builder().id(102L).build());

        TenderVO result = tenderService.upload(file, dto);

        assertNotNull(result);
        assertEquals("contract", result.getFileCategory());

        verify(tenderMapper).insert(tenderCaptor.capture());
        assertEquals("contract", tenderCaptor.getValue().getFileCategory());
    }

    @Test
    void upload_shouldDefaultToBidCategory_whenNoCategoryInDto() throws Exception {
        byte[] content = "data".getBytes();
        MultipartFile file = mock(MultipartFile.class);

        TenderDTO dto = new TenderDTO();
        dto.setProjectId(103L);
        // fileCategory intentionally null

        Path destPath = Paths.get("data", "uploads", "tenders", "2024-01-01", "uuid.pdf")
                .toAbsolutePath().normalize();
        when(file.isEmpty()).thenReturn(false);
        when(file.getOriginalFilename()).thenReturn("default.pdf");
        when(file.getSize()).thenReturn((long) content.length);
        when(file.getContentType()).thenReturn("application/pdf");
        when(storagePathService.buildTenderUploadPath("default.pdf")).thenReturn(destPath);
        when(storagePathService.toStoredPath(destPath))
                .thenReturn("tenders/2024-01-01/uuid.pdf");
        when(projectMapper.selectOne(any())).thenReturn(Project.builder().id(103L).build());

        TenderVO result = tenderService.upload(file, dto);

        assertNotNull(result);
        assertEquals("bid", result.getFileCategory());

        verify(tenderMapper).insert(tenderCaptor.capture());
        assertEquals("bid", tenderCaptor.getValue().getFileCategory());
    }

    // ========================= page =========================

    @Test
    void page_shouldReturnFilteredResults() {
        TenderPageQueryDTO dto = new TenderPageQueryDTO();
        dto.setPage(1);
        dto.setSize(10);
        dto.setBidName("测试");
        dto.setUploadStartTime(LocalDate.of(2024, 1, 1));
        dto.setUploadEndTime(LocalDate.of(2024, 12, 31));

        Project project = Project.builder()
                .id(200L)
                .projectName("测试项目")
                .supplierName("供应商A")
                .createTime(LocalDateTime.of(2024, 6, 1, 10, 0))
                .latestVersion(2)
                .build();

        Page<Project> pageResult = new Page<>(1, 10, 1);
        pageResult.setRecords(Collections.singletonList(project));

        when(projectMapper.selectPage(any(Page.class), any(LambdaQueryWrapper.class)))
                .thenReturn(pageResult);

        Tender latestTender = Tender.builder()
                .id(10L)
                .fileName("test.pdf")
                .filePath("tenders/2024-01-01/uuid.pdf")
                .fileSize(1024L)
                .fileType("pdf")
                .fileCategory("bid")
                .version(2)
                .uploadUserId(CURRENT_USER_ID)
                .bidName("测试标书")
                .pageCount(10)
                .budgetAmount(new BigDecimal("500000"))
                .build();

        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class)))
                .thenReturn(latestTender);

        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class)))
                .thenReturn(null);

        PageResult result = tenderService.page(dto);

        assertNotNull(result);
        assertEquals(1L, result.getTotal());
        assertEquals(1, result.getRecords().size());
        TenderVO vo = (TenderVO) result.getRecords().get(0);
        assertEquals("测试项目", vo.getBidName());
        assertEquals("供应商A", vo.getSupplierName());
        assertEquals(10L, vo.getId());
        assertEquals("test.pdf", vo.getFileName());
        assertEquals("标书", vo.getFileCategory());
        assertEquals(0, vo.getParseStatus());
        assertNull(vo.getAuditorName());

        verify(projectMapper).selectPage(any(Page.class), any(LambdaQueryWrapper.class));
        verify(tenderMapper).selectOne(any(LambdaQueryWrapper.class));
    }

    @Test
    void page_shouldApplyInMemoryStatusFilter() {
        TenderPageQueryDTO dto = new TenderPageQueryDTO();
        dto.setPage(1);
        dto.setSize(10);
        dto.setStatus(2); // only completed

        Project project = Project.builder()
                .id(300L)
                .projectName("已完成项目")
                .createTime(LocalDateTime.now())
                .latestVersion(1)
                .build();

        Page<Project> pageResult = new Page<>(1, 10, 2);
        pageResult.setRecords(Collections.singletonList(project));

        when(projectMapper.selectPage(any(Page.class), any(LambdaQueryWrapper.class)))
                .thenReturn(pageResult);

        Tender tender = Tender.builder()
                .id(30L)
                .fileCategory("bid")
                .version(1)
                .uploadUserId(CURRENT_USER_ID)
                .build();

        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class)))
                .thenReturn(tender);

        AuditTask completedTask = AuditTask.builder()
                .id(1L)
                .bidId(30L)
                .taskStatus(2) // COMPLETED
                .build();

        // auditTaskMapper.selectOne is called multiple times; always return completed task
        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class)))
                .thenReturn(completedTask);

        PageResult result = tenderService.page(dto);

        assertNotNull(result);
        assertEquals(1, result.getRecords().size());
        TenderVO vo = (TenderVO) result.getRecords().get(0);
        assertEquals(2, vo.getParseStatus()); // COMPLETED -> parseStatus=2
    }

    @Test
    void page_shouldApplyInMemoryCategoryFilter() {
        TenderPageQueryDTO dto = new TenderPageQueryDTO();
        dto.setPage(1);
        dto.setSize(10);
        dto.setFileCategory("contract");

        Project project = Project.builder()
                .id(400L)
                .projectName("合同项目")
                .createTime(LocalDateTime.now())
                .latestVersion(1)
                .build();

        Page<Project> pageResult = new Page<>(1, 10, 1);
        pageResult.setRecords(Collections.singletonList(project));

        when(projectMapper.selectPage(any(Page.class), any(LambdaQueryWrapper.class)))
                .thenReturn(pageResult);

        Tender tender = Tender.builder()
                .id(40L)
                .fileCategory("contract")
                .version(1)
                .uploadUserId(CURRENT_USER_ID)
                .build();

        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class)))
                .thenReturn(tender);

        PageResult result = tenderService.page(dto);

        assertEquals(1, result.getRecords().size());
        TenderVO vo = (TenderVO) result.getRecords().get(0);
        assertEquals("合同", vo.getFileCategory()); // contract -> 合同

        // verify that the filter excluded the "bid" type VO
        verify(tenderMapper).selectOne(any(LambdaQueryWrapper.class));
    }

    @Test
    void page_shouldHandleEmptyProjectList() {
        TenderPageQueryDTO dto = new TenderPageQueryDTO();
        dto.setPage(1);
        dto.setSize(10);

        Page<Project> emptyPage = new Page<>(1, 10, 0);
        emptyPage.setRecords(Collections.emptyList());

        when(projectMapper.selectPage(any(Page.class), any(LambdaQueryWrapper.class)))
                .thenReturn(emptyPage);

        PageResult result = tenderService.page(dto);

        assertNotNull(result);
        assertEquals(0L, result.getTotal());
        assertTrue(result.getRecords().isEmpty());
    }

    // ========================= getStats =========================

    private Map<String, Object> statsRow(long total, long pending, long processing, long completed, long failed) {
        Map<String, Object> map = new HashMap<>();
        map.put("total", total);
        map.put("pending", pending);
        map.put("processing", processing);
        map.put("completed", completed);
        map.put("failed", failed);
        return map;
    }

    @Test
    void getStats_shouldAggregateCorrectly() {
        TenderPageQueryDTO dto = new TenderPageQueryDTO();

        when(projectMapper.countByStatus(CURRENT_TENANT_ID, CURRENT_USER_ID))
                .thenReturn(statsRow(6L, 1L, 2L, 3L, 0L));

        TenderStatsVO stats = tenderService.getStats(dto);

        assertNotNull(stats);
        assertEquals(6L, stats.getAllCount());
        assertEquals(1L, stats.getPendingCount());
        assertEquals(2L, stats.getProcessingCount());
        assertEquals(3L, stats.getCompletedCount());
        assertEquals(0L, stats.getFailedCount());
    }

    @Test
    void getStats_shouldHandleNullMap() {
        TenderPageQueryDTO dto = new TenderPageQueryDTO();

        when(projectMapper.countByStatus(CURRENT_TENANT_ID, CURRENT_USER_ID))
                .thenReturn(null);

        TenderStatsVO stats = tenderService.getStats(dto);

        assertNotNull(stats);
        assertEquals(0L, stats.getAllCount());
        assertEquals(0L, stats.getPendingCount());
        assertEquals(0L, stats.getProcessingCount());
        assertEquals(0L, stats.getCompletedCount());
        assertEquals(0L, stats.getFailedCount());
    }

    @Test
    void getStats_shouldHandleEmptyMap() {
        TenderPageQueryDTO dto = new TenderPageQueryDTO();

        when(projectMapper.countByStatus(CURRENT_TENANT_ID, CURRENT_USER_ID))
                .thenReturn(Collections.emptyMap());

        TenderStatsVO stats = tenderService.getStats(dto);

        assertNotNull(stats);
        assertEquals(0L, stats.getAllCount());
        assertEquals(0L, stats.getPendingCount());
        assertEquals(0L, stats.getProcessingCount());
        assertEquals(0L, stats.getCompletedCount());
        assertEquals(0L, stats.getFailedCount());
    }

    @Test
    void getStats_shouldHandleSparseMap() {
        TenderPageQueryDTO dto = new TenderPageQueryDTO();

        // 某些状态为 null 时（异常数据），应回退为 0
        Map<String, Object> sparse = statsRow(1L, 0L, 0L, 0L, 0L);
        sparse.put("processing", null);
        when(projectMapper.countByStatus(CURRENT_TENANT_ID, CURRENT_USER_ID))
                .thenReturn(sparse);

        TenderStatsVO stats = tenderService.getStats(dto);

        assertNotNull(stats);
        assertEquals(1L, stats.getAllCount());
        assertEquals(0L, stats.getPendingCount());
        assertEquals(0L, stats.getProcessingCount());
        assertEquals(0L, stats.getCompletedCount());
        assertEquals(0L, stats.getFailedCount());
    }

    // ========================= getById =========================

    @Test
    void getById_shouldIncludeAuditStatus_whenAuditTaskExists() {
        Long tenderId = 42L;
        Tender tender = Tender.builder()
                .id(tenderId)
                .bidName("标书A")
                .fileName("doc.pdf")
                .fileType("pdf")
                .fileCategory("bid")
                .uploadUserId(CURRENT_USER_ID)
                .uploadTime(LocalDateTime.now())
                .version(1)
                .projectId(200L)
                .build();

        when(tenderMapper.selectOne(any())).thenReturn(tender);

        AuditTask completedTask = AuditTask.builder()
                .id(1L)
                .bidId(tenderId)
                .taskStatus(2) // COMPLETED
                .auditUserId(5L)
                .createTime(LocalDateTime.now())
                .build();

        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class)))
                .thenReturn(completedTask);

        User auditor = User.builder()
                .id(5L)
                .realName("审核员张三")
                .build();
        when(userMapper.selectById(5L)).thenReturn(auditor);

        TenderVO result = tenderService.getById(tenderId);

        assertNotNull(result);
        assertEquals("标书A", result.getBidName());
        assertEquals(2, result.getParseStatus()); // COMPLETED -> parseStatus=2
        assertEquals("审核员张三", result.getAuditorName());
    }

    @Test
    void getById_shouldMapParseStatus_whenTaskIsProcessing() {
        Long tenderId = 43L;
        Tender tender = Tender.builder()
                .id(tenderId)
                .fileType("pdf")
                .fileCategory("bid")
                .uploadUserId(CURRENT_USER_ID)
                .uploadTime(LocalDateTime.now())
                .version(1)
                .build();

        when(tenderMapper.selectOne(any())).thenReturn(tender);

        AuditTask processingTask = AuditTask.builder()
                .id(2L)
                .bidId(tenderId)
                .taskStatus(1) // PROCESSING
                .build();

        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class)))
                .thenReturn(processingTask);

        TenderVO result = tenderService.getById(tenderId);

        assertNotNull(result);
        assertEquals(1, result.getParseStatus()); // PROCESSING -> parseStatus=1
    }

    @Test
    void getById_shouldReturn404_whenTenderNotFound() {
        when(tenderMapper.selectOne(any())).thenReturn(null);

        TenantAuthException ex = assertThrows(TenantAuthException.class,
                () -> tenderService.getById(999L));
        assertEquals(404, ex.getStatus());
        assertEquals("RESOURCE_NOT_FOUND", ex.getErrorCode());
    }

    @Test
    void getById_shouldThrow_whenNotOwner() {
        Long tenderId = 42L;
        Tender tender = Tender.builder()
                .id(tenderId)
                .uploadUserId(999L) // different user
                .build();

        when(tenderMapper.selectOne(any())).thenReturn(tender);

        BizException ex = assertThrows(BizException.class,
                () -> tenderService.getById(tenderId));
        assertEquals(403, ex.getCode());
        assertTrue(ex.getMessage().contains("无权访问"));
    }

    // ========================= getVersionsByProjectId =========================

    @Test
    void getVersionsByProjectId_shouldReturnAllVersionsInDescOrder() {
        Long projectId = 100L;

        Project project = Project.builder()
                .id(projectId)
                .userId(CURRENT_USER_ID)
                .projectName("测试项目")
                .build();

        when(projectMapper.selectOne(any())).thenReturn(project);

        Tender v1 = Tender.builder()
                .id(1L)
                .bidName("标书v1")
                .version(1)
                .projectId(projectId)
                .fileType("pdf")
                .fileCategory("bid")
                .uploadUserId(CURRENT_USER_ID)
                .uploadTime(LocalDateTime.of(2024, 1, 1, 10, 0))
                .build();

        Tender v2 = Tender.builder()
                .id(2L)
                .bidName("标书v2")
                .version(2)
                .projectId(projectId)
                .fileType("pdf")
                .fileCategory("bid")
                .uploadUserId(CURRENT_USER_ID)
                .uploadTime(LocalDateTime.of(2024, 6, 1, 10, 0))
                .build();

        // returned in desc order by version, matching the code's .orderByDesc(Tender::getVersion)
        when(tenderMapper.selectList(any(LambdaQueryWrapper.class)))
                .thenReturn(Arrays.asList(v2, v1));

        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class)))
                .thenReturn(null);

        List<TenderVO> versions = tenderService.getVersionsByProjectId(projectId);

        assertNotNull(versions);
        assertEquals(2, versions.size());
        assertEquals("标书v2", versions.get(0).getBidName());
        assertEquals(2, versions.get(0).getVersion());
        assertEquals("标书v1", versions.get(1).getBidName());
        assertEquals(1, versions.get(1).getVersion());
    }

    @Test
    void getVersionsByProjectId_shouldThrow_whenProjectNotFound() {
        when(projectMapper.selectOne(any())).thenReturn(null);

        TenantAuthException ex = assertThrows(TenantAuthException.class,
                () -> tenderService.getVersionsByProjectId(999L));
        assertEquals(404, ex.getStatus());
        assertEquals("RESOURCE_NOT_FOUND", ex.getErrorCode());
    }

    @Test
    void getVersionsByProjectId_shouldThrow_whenProjectNotOwned() {
        Long projectId = 100L;
        Project project = Project.builder()
                .id(projectId)
                .userId(999L) // different user
                .build();

        when(projectMapper.selectOne(any())).thenReturn(project);

        BizException ex = assertThrows(BizException.class,
                () -> tenderService.getVersionsByProjectId(projectId));
        assertEquals(403, ex.getCode());
        assertTrue(ex.getMessage().contains("无权访问"));
    }

    @Test
    void getVersionsByProjectId_shouldWork_whenCurrentUserIdIsNull() {
        // When BaseContext.getCurrentId() returns null, ownership check is skipped
        BaseContext.removeCurrentId();

        Long projectId = 100L;
        when(projectMapper.selectOne(any())).thenReturn(
                Project.builder().id(projectId).tenantId(CURRENT_TENANT_ID).build());
        List<TenderVO> result = tenderService.getVersionsByProjectId(projectId);

        // Without current user, the query proceeds without ownership check
        assertNotNull(result);
        verify(tenderMapper).selectList(any());

        BaseContext.setCurrentId(CURRENT_USER_ID);
    }

    // ========================= getProjects =========================

    @Test
    void getProjects_shouldReturnUserProjectsWithDetails() {
        Project p1 = Project.builder()
                .id(1L)
                .userId(CURRENT_USER_ID)
                .projectName("项目A")
                .supplierName("供应商A")
                .createTime(LocalDateTime.of(2024, 3, 1, 10, 0))
                .latestVersion(2)
                .build();

        when(projectMapper.selectList(any(LambdaQueryWrapper.class)))
                .thenReturn(Collections.singletonList(p1));

        User currentUser = User.builder()
                .id(CURRENT_USER_ID)
                .realName("当前用户")
                .build();
        when(userMapper.selectById(CURRENT_USER_ID)).thenReturn(currentUser);

        Tender latestTender = Tender.builder()
                .id(10L)
                .projectId(1L)
                .fileCategory("bid")
                .version(2)
                .build();

        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class)))
                .thenReturn(latestTender);

        AuditTask task = AuditTask.builder()
                .id(1L)
                .bidId(10L)
                .auditUserId(5L)
                .createTime(LocalDateTime.now())
                .build();

        when(auditTaskMapper.selectOne(any(LambdaQueryWrapper.class)))
                .thenReturn(task);

        User auditor = User.builder()
                .id(5L)
                .realName("审核员")
                .build();
        when(userMapper.selectById(5L)).thenReturn(auditor);

        List<TenderProjectVO> projects = tenderService.getProjects();

        assertNotNull(projects);
        assertEquals(1, projects.size());
        TenderProjectVO vo = projects.get(0);
        assertEquals(1L, vo.getProjectId());
        assertEquals("项目A", vo.getProjectName());
        assertEquals("当前用户", vo.getCreatorName());
        assertEquals(2, vo.getLatestVersion());
        assertEquals("供应商A", vo.getSupplierName());
        assertEquals("bid", vo.getFileCategory());
        assertEquals("审核员", vo.getAuditorName());
    }

    @Test
    void getProjects_shouldHandleProjectWithoutTender() {
        Project p1 = Project.builder()
                .id(2L)
                .userId(CURRENT_USER_ID)
                .projectName("空项目")
                .supplierName("供应商B")
                .createTime(LocalDateTime.now())
                .latestVersion(0)
                .build();

        when(projectMapper.selectList(any(LambdaQueryWrapper.class)))
                .thenReturn(Collections.singletonList(p1));

        User currentUser = User.builder()
                .id(CURRENT_USER_ID)
                .realName("当前用户")
                .build();
        when(userMapper.selectById(CURRENT_USER_ID)).thenReturn(currentUser);

        // No latest tender → null
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class)))
                .thenReturn(null);

        List<TenderProjectVO> projects = tenderService.getProjects();

        assertNotNull(projects);
        assertEquals(1, projects.size());
        TenderProjectVO vo = projects.get(0);
        assertEquals(2L, vo.getProjectId());
        assertNull(vo.getFileCategory());
        assertNull(vo.getAuditorName());
    }

    // ========================= delete =========================

    @Test
    void delete_shouldRemoveTenderAndFile_whenAuthorized() {
        Long tenderId = 50L;
        Tender tender = Tender.builder()
                .id(tenderId)
                .filePath("tenders/2024-01-01/to-delete.pdf")
                .uploadUserId(CURRENT_USER_ID)
                .build();

        when(tenderMapper.selectOne(any())).thenReturn(tender);

        Path storedPath = Paths.get("data", "uploads", "tenders",
                "2024-01-01", "to-delete.pdf").toAbsolutePath().normalize();
        when(storagePathService.resolveStoredPath("tenders/2024-01-01/to-delete.pdf"))
                .thenReturn(storedPath);

        assertDoesNotThrow(() -> tenderService.delete(tenderId));

        verify(tenderMapper).delete(any());
        verify(storagePathService).resolveStoredPath("tenders/2024-01-01/to-delete.pdf");
    }

    @Test
    void delete_shouldReturn404_whenTenderNotFound() {
        when(tenderMapper.selectOne(any())).thenReturn(null);

        TenantAuthException ex = assertThrows(TenantAuthException.class,
                () -> tenderService.delete(999L));
        assertEquals(404, ex.getStatus());
        assertEquals("RESOURCE_NOT_FOUND", ex.getErrorCode());

        verify(tenderMapper, never()).delete(any());
        verify(storagePathService, never()).resolveStoredPath(any());
    }

    @Test
    void delete_shouldThrow_whenNotOwner() {
        Long tenderId = 50L;
        Tender tender = Tender.builder()
                .id(tenderId)
                .filePath("tenders/x.pdf")
                .uploadUserId(999L) // different user
                .build();

        when(tenderMapper.selectOne(any())).thenReturn(tender);

        BizException ex = assertThrows(BizException.class,
                () -> tenderService.delete(tenderId));
        assertEquals(403, ex.getCode());
        assertTrue(ex.getMessage().contains("无权删除"));

        verify(tenderMapper, never()).delete(any());
    }

    // ========================= getBidIdsByUserId =========================

    @Test
    void getBidIdsByUserId_shouldReturnListOfIds() {
        Long userId = 200L;
        Tender t1 = Tender.builder().id(1L).build();
        Tender t2 = Tender.builder().id(2L).build();

        when(tenderMapper.selectList(any(QueryWrapper.class)))
                .thenReturn(Arrays.asList(t1, t2));

        List<Long> ids = tenderService.getBidIdsByUserId(userId);

        assertNotNull(ids);
        assertEquals(2, ids.size());
        assertTrue(ids.contains(1L));
        assertTrue(ids.contains(2L));
    }

    @Test
    void getBidIdsByUserId_shouldReturnEmptyList_whenNoTenders() {
        when(tenderMapper.selectList(any(QueryWrapper.class)))
                .thenReturn(Collections.emptyList());

        List<Long> ids = tenderService.getBidIdsByUserId(999L);

        assertNotNull(ids);
        assertTrue(ids.isEmpty());
    }
}
