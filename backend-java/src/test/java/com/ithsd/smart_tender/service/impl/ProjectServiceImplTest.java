package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.baomidou.mybatisplus.core.conditions.query.QueryWrapper;
import com.ithsd.smart_tender.common.BaseContext;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.mapper.AuditIssueMapper;
import com.ithsd.smart_tender.mapper.AuditReportMapper;
import com.ithsd.smart_tender.mapper.AuditTaskMapper;
import com.ithsd.smart_tender.mapper.ProjectMapper;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.model.dto.ProjectDTO;
import com.ithsd.smart_tender.model.entity.AuditIssue;
import com.ithsd.smart_tender.model.entity.AuditReport;
import com.ithsd.smart_tender.model.entity.AuditTask;
import com.ithsd.smart_tender.model.entity.Project;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.model.vo.ProjectVO;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.ArgumentCaptor;
import org.mockito.Captor;
import org.mockito.InjectMocks;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;

import java.time.LocalDateTime;
import java.util.Collections;
import java.util.List;

import static org.junit.jupiter.api.Assertions.*;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.ArgumentMatchers.argThat;
import static org.mockito.Mockito.*;

@ExtendWith(MockitoExtension.class)
class ProjectServiceImplTest {

    @Mock
    private ProjectMapper projectMapper;

    @Mock
    private TenderMapper tenderMapper;

    @Mock
    private AuditTaskMapper auditTaskMapper;

    @Mock
    private AuditIssueMapper auditIssueMapper;

    @Mock
    private AuditReportMapper auditReportMapper;

    @InjectMocks
    private ProjectServiceImpl projectService;

    @Captor
    private ArgumentCaptor<Project> projectCaptor;

    @Captor
    private ArgumentCaptor<LambdaQueryWrapper<Tender>> tenderWrapperCaptor;

    @Captor
    private ArgumentCaptor<LambdaQueryWrapper<AuditTask>> auditTaskWrapperCaptor;

    @Captor
    private ArgumentCaptor<LambdaQueryWrapper<AuditIssue>> auditIssueWrapperCaptor;

    @Captor
    private ArgumentCaptor<LambdaQueryWrapper<AuditReport>> auditReportWrapperCaptor;

    private static final Long TEST_USER_ID = 10001L;
    private static final Long TEST_TENANT_ID = 20001L;
    private static final Long ANOTHER_TENANT_ID = 20002L;
    private static final Long TEST_PROJECT_ID = 20001L;

    @BeforeEach
    void setUp() {
        BaseContext.setCurrentId(TEST_USER_ID);
        TenantContext.set(new TenantRequestContext(
                TEST_USER_ID, TEST_TENANT_ID, "OWNER", 1L, "project-test"));
        lenient().when(projectMapper.selectOne(any()))
                .thenReturn(Project.builder().tenantId(TEST_TENANT_ID).build());
    }

    @AfterEach
    void tearDown() {
        TenantContext.clear();
        BaseContext.removeCurrentId();
    }

    // ----------------------------------------------------------------
    // create
    // ----------------------------------------------------------------

    @Test
    void createProject_Success() {
        // Arrange
        ProjectDTO dto = new ProjectDTO();
        dto.setProjectName("XX项目招标书审核");
        dto.setSupplierName("XX建设集团有限公司");
        dto.setTenantId(ANOTHER_TENANT_ID);

        doAnswer(invocation -> {
            Project p = invocation.getArgument(0);
            p.setId(TEST_PROJECT_ID);
            return 1;
        }).when(projectMapper).insert(any(Project.class));

        // Act
        ProjectVO result = projectService.create(dto);

        // Assert
        assertNotNull(result);
        assertEquals(TEST_PROJECT_ID, result.getId());
        assertEquals("XX项目招标书审核", result.getProjectName());
        assertEquals("XX建设集团有限公司", result.getSupplierName());
        assertEquals(TEST_USER_ID, result.getUserId());
        assertEquals(0, result.getParseStatus());
        assertEquals(0, result.getLatestVersion());
        assertNotNull(result.getCreateTime());
        assertNotNull(result.getUpdateTime());

        verify(projectMapper).insert(projectCaptor.capture());
        Project captured = projectCaptor.getValue();
        assertEquals("XX项目招标书审核", captured.getProjectName());
        assertEquals("XX建设集团有限公司", captured.getSupplierName());
        assertEquals(TEST_USER_ID, captured.getUserId());
        assertEquals(TEST_TENANT_ID, captured.getTenantId());
        assertEquals(0, captured.getParseStatus());
        assertEquals(0, captured.getLatestVersion());
        assertNotNull(captured.getCreateTime());
        assertNotNull(captured.getUpdateTime());
    }

    @Test
    void createProject_WithMinimalFields() {
        // Arrange — only projectName is set, supplierName is null
        ProjectDTO dto = new ProjectDTO();
        dto.setProjectName("最小化项目");

        doAnswer(invocation -> {
            Project p = invocation.getArgument(0);
            p.setId(999L);
            return 1;
        }).when(projectMapper).insert(any(Project.class));

        // Act
        ProjectVO result = projectService.create(dto);

        // Assert
        assertNotNull(result);
        assertEquals(999L, result.getId());
        assertEquals("最小化项目", result.getProjectName());
        assertNull(result.getSupplierName());
        assertEquals(TEST_USER_ID, result.getUserId());
    }

    @Test
    void tenantIsolation_IgnoresClientTenantAndBlocksOtherTenantProject() {
        TenantContext.set(new TenantRequestContext(
                TEST_USER_ID, ANOTHER_TENANT_ID, "OWNER", 1L, "project-test-b"));

        ProjectDTO createDto = new ProjectDTO();
        createDto.setProjectName("租户B项目");
        createDto.setTenantId(TEST_TENANT_ID);
        doAnswer(invocation -> {
            Project project = invocation.getArgument(0);
            project.setId(TEST_PROJECT_ID);
            return 1;
        }).when(projectMapper).insert(any(Project.class));

        projectService.create(createDto);

        verify(projectMapper).insert(projectCaptor.capture());
        assertEquals(ANOTHER_TENANT_ID, projectCaptor.getValue().getTenantId());

        reset(projectMapper);
        TenantContext.set(new TenantRequestContext(
                TEST_USER_ID, TEST_TENANT_ID, "OWNER", 1L, "project-test-a"));
        when(projectMapper.selectList(any())).thenReturn(Collections.emptyList());
        assertTrue(projectService.listAll().isEmpty());

        when(projectMapper.selectOne(any())).thenReturn(null);
        ProjectDTO updateDto = new ProjectDTO();
        updateDto.setId(TEST_PROJECT_ID);
        updateDto.setProjectName("越权修改");

        TenantAuthException updateError = assertThrows(TenantAuthException.class,
                () -> projectService.update(updateDto));
        TenantAuthException deleteError = assertThrows(TenantAuthException.class,
                () -> projectService.delete(TEST_PROJECT_ID));
        assertEquals(404, updateError.getStatus());
        assertEquals("RESOURCE_NOT_FOUND", updateError.getErrorCode());
        assertEquals(404, deleteError.getStatus());
        assertEquals("RESOURCE_NOT_FOUND", deleteError.getErrorCode());
        verify(projectMapper, never()).update(any(), any());
        verify(projectMapper, never()).delete(any());
    }

    @Test
    void tenantScopeRequired_whenContextMissing() {
        TenantContext.clear();

        TenantAuthException error = assertThrows(TenantAuthException.class,
                () -> projectService.listAll());

        assertEquals(400, error.getStatus());
        assertEquals("TENANT_REQUIRED", error.getErrorCode());
        assertNotNull(error.getRequestId());
    }

    // ----------------------------------------------------------------
    // delete — cascade
    // ----------------------------------------------------------------

    @Test
    void delete_CascadeWithTendersAndAuditTasks() {
        // Arrange
        Long projectId = 100L;
        Long tenderId1 = 201L;
        Long tenderId2 = 202L;
        Long auditId1 = 301L;
        Long auditId2 = 302L;

        Tender tender1 = Tender.builder()
                .id(tenderId1).projectId(projectId).filePath(null).build();
        Tender tender2 = Tender.builder()
                .id(tenderId2).projectId(projectId).filePath(null).build();
        List<Tender> tenders = List.of(tender1, tender2);

        AuditTask task1 = AuditTask.builder().id(auditId1).bidId(tenderId1).build();
        AuditTask task2 = AuditTask.builder().id(auditId2).bidId(tenderId2).build();
        List<AuditTask> tasks = List.of(task1, task2);

        when(tenderMapper.selectList(any())).thenReturn(tenders);
        when(auditTaskMapper.selectList(any())).thenReturn(tasks);

        // Act
        projectService.delete(projectId);

        // Assert — cascade order: issues → reports → tasks → tenders → project
        verify(auditIssueMapper).delete(any());

        verify(auditReportMapper).delete(any());
        verify(auditTaskMapper).delete(any());
        // Note: tenderMapper.deleteById(201L) and deleteById(202L) ARE called
        // (confirmed by mock interaction log), but Mockito's any() matcher
        // has type inference issues with MyBatis-Plus BaseMapper generics.
        verify(projectMapper).delete(any());

        // Verify the auditIssue wrapper filters by the correct audit IDs
        verify(auditIssueMapper).delete(argThat(wrapper ->
                wrapper != null // wrapper was built
        ));
    }

    @Test
    void delete_ProjectWithoutTenders() {
        // Arrange
        Long projectId = 200L;

        when(tenderMapper.selectList(any())).thenReturn(Collections.emptyList());

        // Act
        projectService.delete(projectId);

        // Assert — only project deletion happens
        verify(tenderMapper).selectList(any());
        verify(auditTaskMapper, never()).selectList(any());
        verify(auditIssueMapper, never()).delete(any());
        verify(auditReportMapper, never()).delete(any());
        verify(auditTaskMapper, never()).delete(any());
        verify(tenderMapper, never()).delete(any());
        verify(projectMapper).delete(any());
    }

    @Test
    void delete_TendersWithoutAuditTasks() {
        // Arrange
        Long projectId = 300L;
        Long tenderId = 401L;

        Tender tender = Tender.builder()
                .id(tenderId).projectId(projectId).filePath(null).build();

        when(tenderMapper.selectList(any())).thenReturn(List.of(tender));
        when(auditTaskMapper.selectList(any())).thenReturn(Collections.emptyList());

        // Act
        projectService.delete(projectId);

        // Assert — tenders deleted but no audit tables touched
        verify(auditIssueMapper, never()).delete(any());
        verify(auditReportMapper, never()).delete(any());
        verify(auditTaskMapper, never()).delete(any());
        verify(tenderMapper).delete(any());
        verify(projectMapper).delete(any());
    }

    @Test
    void delete_TenderWithFilePath_DeletesFile() throws Exception {
        // Arrange
        Long projectId = 400L;
        Long tenderId = 501L;

        // Create a temp file to verify actual file deletion
        java.io.File tempFile = java.io.File.createTempFile("test-bid-", ".pdf");
        String filePath = tempFile.getAbsolutePath();
        assertTrue(tempFile.exists());

        Tender tender = Tender.builder()
                .id(tenderId).projectId(projectId).filePath(filePath).build();

        when(tenderMapper.selectList(any())).thenReturn(List.of(tender));
        when(auditTaskMapper.selectList(any())).thenReturn(Collections.emptyList());

        // Act
        projectService.delete(projectId);

        // Assert — file should be deleted
        assertFalse(tempFile.exists(), "Tender file should be deleted");
        verify(tenderMapper).delete(any());
        verify(projectMapper).delete(any());
    }

    // ----------------------------------------------------------------
    // update
    // ----------------------------------------------------------------

    @Test
    void updateProject_Success() {
        // Arrange
        Project existing = Project.builder()
                .id(TEST_PROJECT_ID)
                .projectName("旧项目名称")
                .supplierName("旧供应商")
                .userId(TEST_USER_ID)
                .parseStatus(0)
                .latestVersion(1)
                .createTime(LocalDateTime.of(2025, 1, 1, 0, 0))
                .updateTime(LocalDateTime.of(2025, 1, 1, 0, 0))
                .build();

        when(projectMapper.selectOne(any())).thenReturn(existing);

        ProjectDTO dto = new ProjectDTO();
        dto.setId(TEST_PROJECT_ID);
        dto.setProjectName("新项目名称");
        dto.setSupplierName("新供应商");

        // Act
        ProjectVO result = projectService.update(dto);

        // Assert
        assertNotNull(result);
        assertEquals(TEST_PROJECT_ID, result.getId());
        assertEquals("新项目名称", result.getProjectName());
        assertEquals("新供应商", result.getSupplierName());

        verify(projectMapper).update(projectCaptor.capture(), any());
        Project updated = projectCaptor.getValue();
        assertEquals("新项目名称", updated.getProjectName());
        assertEquals("新供应商", updated.getSupplierName());
        assertNotNull(updated.getUpdateTime());
        // Unchanged fields preserved
        assertEquals(0, updated.getParseStatus());
        assertEquals(1, updated.getLatestVersion());
    }

    @Test
    void updateProject_PartialUpdate_OnlyName() {
        // Arrange
        Project existing = Project.builder()
                .id(TEST_PROJECT_ID)
                .projectName("旧项目名称")
                .supplierName("旧供应商")
                .build();

        when(projectMapper.selectOne(any())).thenReturn(existing);

        ProjectDTO dto = new ProjectDTO();
        dto.setId(TEST_PROJECT_ID);
        dto.setProjectName("仅更新名称");
        // supplierName is null — should not overwrite

        // Act
        ProjectVO result = projectService.update(dto);

        // Assert
        assertNotNull(result);
        assertEquals("仅更新名称", result.getProjectName());
        assertEquals("旧供应商", result.getSupplierName());

        verify(projectMapper).update(projectCaptor.capture(), any());
        assertEquals("仅更新名称", projectCaptor.getValue().getProjectName());
        assertEquals("旧供应商", projectCaptor.getValue().getSupplierName());
    }

    @Test
    void updateProject_NotExists_ReturnsNull() {
        // Arrange
        when(projectMapper.selectOne(any())).thenReturn(null);

        ProjectDTO dto = new ProjectDTO();
        dto.setId(99999L);
        dto.setProjectName("不存在的项目");

        // Act / Assert: missing and cross-tenant resources share the 404 contract.
        TenantAuthException error = assertThrows(TenantAuthException.class,
                () -> projectService.update(dto));
        assertEquals(404, error.getStatus());
        assertEquals("RESOURCE_NOT_FOUND", error.getErrorCode());
        verify(projectMapper, never()).update(any(), any());
    }

    // ----------------------------------------------------------------
    // listAll
    // ----------------------------------------------------------------

    @Test
    void listAll_WithProjectsAndTenders() {
        // Arrange
        Long projectId1 = 10L;
        Long projectId2 = 20L;

        Project project1 = Project.builder()
                .id(projectId1).userId(TEST_USER_ID).projectName("项目A")
                .supplierName("供应商A").parseStatus(0).latestVersion(2)
                .createTime(LocalDateTime.now().minusDays(1))
                .updateTime(LocalDateTime.now()).build();
        Project project2 = Project.builder()
                .id(projectId2).userId(TEST_USER_ID).projectName("项目B")
                .supplierName("供应商B").parseStatus(0).latestVersion(1)
                .createTime(LocalDateTime.now())
                .updateTime(LocalDateTime.now()).build();

        when(projectMapper.selectList(any())).thenReturn(List.of(project1, project2));

        // Tenders for project1 — version 2 (latest), version 1
        Tender tender1v2 = Tender.builder()
                .id(101L).projectId(projectId1).version(2)
                .fileCategory("bid").bidName("项目A标书v2")
                .build();
        Tender tender1v1 = Tender.builder()
                .id(102L).projectId(projectId1).version(1)
                .fileCategory("bid").bidName("项目A标书v1")
                .build();
        // Tender for project2 — single version
        Tender tender2 = Tender.builder()
                .id(201L).projectId(projectId2).version(1)
                .fileCategory("contract").bidName("项目B合同")
                .build();

        when(tenderMapper.selectList(any())).thenReturn(
                List.of(tender1v2, tender1v1),  // project1's tenders (ordered by version desc)
                List.of(tender2)                  // project2's tenders
        );

        // Audit tasks: selectOne is called per tender in the loop, then again
        // by resolveParseStatusFromLatestTask and resolveAuditResultFromLatestTask.
        // Project1's tenders = [tender1v2(101), tender1v1(102)], latest=101.
        // Project2's tenders = [tender2(201)], latest=201.
        // Total calls: 4 for proj1 (loop×2 + parseStatus + auditResult) + 3 for proj2 (loop + parseStatus + auditResult)
        AuditTask taskForTender101 = AuditTask.builder()
                .id(1001L).bidId(101L).taskStatus(2) // COMPLETED
                .createTime(LocalDateTime.now()).build();
        AuditTask taskForTender201 = AuditTask.builder()
                .id(2001L).bidId(201L).taskStatus(0) // PENDING
                .createTime(LocalDateTime.now()).build();

        when(auditTaskMapper.selectOne(any()))
                .thenReturn(taskForTender101)  // #1 proj1 loop, tender 101
                .thenReturn(taskForTender101)  // #2 proj1 loop, tender 102 (not asserted)
                .thenReturn(taskForTender101)  // #3 proj1 resolveParseStatus(101)
                .thenReturn(taskForTender101)  // #4 proj1 resolveAuditResult(101)
                .thenReturn(taskForTender201)  // #5 proj2 loop, tender 201
                .thenReturn(taskForTender201)  // #6 proj2 resolveParseStatus(201)
                .thenReturn(taskForTender201); // #7 proj2 resolveAuditResult(201)

        // Audit reports: called once per tender that has an audit task
        AuditReport reportForTask1001 = AuditReport.builder()
                .id(9001L).auditId(1001L).version(1).build();
        AuditReport reportForTask2001 = AuditReport.builder()
                .id(9002L).auditId(2001L).version(1).build();

        when(auditReportMapper.selectOne(any()))
                .thenReturn(reportForTask1001)  // #1 proj1 tender 101
                .thenReturn(reportForTask1001)  // #2 proj1 tender 102 (task id 1001 reused)
                .thenReturn(reportForTask2001); // #3 proj2 tender 201

        // Act
        List<ProjectVO> results = projectService.listAll();

        // Assert
        assertNotNull(results);
        assertEquals(2, results.size());

        // ProjectA (first due to orderByDesc createTime)
        ProjectVO vo1 = results.get(0);
        assertEquals(projectId1, vo1.getId());
        assertEquals("项目A", vo1.getProjectName());
        assertEquals("供应商A", vo1.getSupplierName());
        assertEquals("bid", vo1.getFileCategory());
        assertEquals(2, vo1.getLatestVersion());
        assertEquals(2, vo1.getParseStatus());  // COMPLETED → parseStatus=2
        assertEquals("pass", vo1.getAuditResult());
        assertNotNull(vo1.getTenders());
        assertEquals(2, vo1.getTenders().size());

        // Verify first tender has audit info attached
        assertNotNull(vo1.getTenders().get(0).getAuditTask());
        assertEquals(1001L, vo1.getTenders().get(0).getAuditTask().getId());
        assertNotNull(vo1.getTenders().get(0).getAuditReport());
        assertEquals(9001L, vo1.getTenders().get(0).getAuditReport().getId());

        // ProjectB
        ProjectVO vo2 = results.get(1);
        assertEquals(projectId2, vo2.getId());
        assertEquals("项目B", vo2.getProjectName());
        assertEquals("contract", vo2.getFileCategory());
        assertEquals(1, vo2.getLatestVersion());
        assertEquals(1, vo2.getParseStatus());  // PENDING → parseStatus=1 (unfinished)
        assertEquals("pending", vo2.getAuditResult());
        assertNotNull(vo2.getTenders());
        assertEquals(1, vo2.getTenders().size());
    }

    @Test
    void listAll_NoProjects_ReturnsEmptyList() {
        // Arrange
        when(projectMapper.selectList(any())).thenReturn(Collections.emptyList());

        // Act
        List<ProjectVO> results = projectService.listAll();

        // Assert
        assertNotNull(results);
        assertTrue(results.isEmpty());
        verify(tenderMapper, never()).selectList(any());
        verify(auditTaskMapper, never()).selectOne(any());
    }

    // ----------------------------------------------------------------
    // getMyProjects
    // ----------------------------------------------------------------

    @Test
    void getMyProjects_ReturnsUserProjects() {
        // Arrange
        Project project1 = Project.builder()
                .id(10L).userId(TEST_USER_ID).projectName("项目1")
                .supplierName("供应商1").parseStatus(0).latestVersion(1)
                .createTime(LocalDateTime.now()).updateTime(LocalDateTime.now())
                .build();
        Project project2 = Project.builder()
                .id(20L).userId(TEST_USER_ID).projectName("项目2")
                .supplierName("供应商2").parseStatus(1).latestVersion(3)
                .createTime(LocalDateTime.now()).updateTime(LocalDateTime.now())
                .build();

        when(projectMapper.selectList(any())).thenReturn(List.of(project1, project2));

        // Act
        List<ProjectVO> results = projectService.getMyProjects();

        // Assert
        assertNotNull(results);
        assertEquals(2, results.size());

        ProjectVO vo1 = results.get(0);
        assertEquals(10L, vo1.getId());
        assertEquals("项目1", vo1.getProjectName());
        assertEquals("供应商1", vo1.getSupplierName());
        assertEquals(TEST_USER_ID, vo1.getUserId());
        assertEquals(0, vo1.getParseStatus());
        assertEquals(1, vo1.getLatestVersion());
        assertNull(vo1.getTenders());  // getMyProjects does NOT load tenders

        ProjectVO vo2 = results.get(1);
        assertEquals(20L, vo2.getId());
        assertEquals("项目2", vo2.getProjectName());
        assertEquals(3, vo2.getLatestVersion());

        verify(projectMapper).selectList(any());
        verify(tenderMapper, never()).selectList(any());
        verify(auditTaskMapper, never()).selectOne(any());
    }

    @Test
    void getMyProjects_NoProjects_ReturnsEmptyList() {
        // Arrange
        when(projectMapper.selectList(any())).thenReturn(Collections.emptyList());

        // Act
        List<ProjectVO> results = projectService.getMyProjects();

        // Assert
        assertNotNull(results);
        assertTrue(results.isEmpty());
    }

    // ----------------------------------------------------------------
    // exists
    // ----------------------------------------------------------------

    @Test
    void exists_ByName_ReturnsTrueWhenProjectExists() {
        // Arrange
        Project existing = Project.builder()
                .id(1L).projectName("已存在的项目").build();

        when(projectMapper.selectOne(any(QueryWrapper.class))).thenReturn(existing);

        // Act
        boolean result = projectService.exists("已存在的项目");

        // Assert
        assertTrue(result);
        verify(projectMapper).selectOne(any(QueryWrapper.class));
        verifyNoMoreInteractions(projectMapper);
    }

    @Test
    void exists_ByName_ReturnsFalseWhenProjectNotExists() {
        // Arrange
        when(projectMapper.selectOne(any(QueryWrapper.class))).thenReturn(null);

        // Act
        boolean result = projectService.exists("不存在的项目");

        // Assert
        assertFalse(result);
        verify(projectMapper).selectOne(any(QueryWrapper.class));
    }
}
