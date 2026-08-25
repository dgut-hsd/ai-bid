package com.ithsd.smart_tender.service.impl;

import com.ithsd.smart_tender.common.BaseContext;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.mapper.AuditIssueMapper;
import com.ithsd.smart_tender.service.AuditTaskService;
import com.ithsd.smart_tender.service.TenderService;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.InjectMocks;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;
import org.springframework.test.util.ReflectionTestUtils;

import java.util.List;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.*;

/**
 * {@link AuditIssueServiceImpl#countByCategory()} 的单元测试。
 *
 * <p>覆盖场景：
 * <ul>
 *   <li>正常多分类统计</li>
 *   <li>审核任务列表为空时提前返回</li>
 *   <li>仅有一个分类</li>
 *   <li>有审核 ID 但无对应问题数据</li>
 *   <li>用户无任何标书</li>
 * </ul>
 */
@ExtendWith(MockitoExtension.class)
class AuditIssueServiceImplTest {

    private static final Long CURRENT_USER_ID = 1L;

    @Mock
    private TenderService tenderService;

    @Mock
    private AuditTaskService auditTaskService;

    @Mock
    private AuditIssueMapper auditIssueMapper;

    @InjectMocks
    private AuditIssueServiceImpl auditIssueService;

    @BeforeEach
    void setUp() {
        BaseContext.setCurrentId(CURRENT_USER_ID);
        TenantContext.set(new TenantRequestContext(CURRENT_USER_ID, 20001L, "OWNER", 1L, "audit-issue-test"));
        // baseMapper 是 ServiceImpl<AuditIssueMapper, AuditIssue> 父类的 protected 字段，
        // 无法通过构造函数注入，需要通过反射设置
        ReflectionTestUtils.setField(auditIssueService, "baseMapper", auditIssueMapper);
    }

    @AfterEach
    void tearDown() {
        BaseContext.removeCurrentId();
        TenantContext.clear();
    }

    @Test
    void countByCategory_shouldReturnCategoryCounts_whenMultipleCategoriesExist() {
        // given
        List<Long> bidIds = List.of(10L, 11L);
        List<Long> auditIds = List.of(100L, 101L, 102L);
        when(tenderService.getBidIdsByUserId(CURRENT_USER_ID)).thenReturn(bidIds);
        when(auditTaskService.getAuditIdsByBidIds(bidIds)).thenReturn(auditIds);

        // 模拟 Mapper 返回按 category 分组聚合的结果
        List<Map<String, Object>> mapperRows = List.of(
                Map.of("category", "技术", "count", 3L),
                Map.of("category", "商务", "count", 5L),
                Map.of("category", "资质", "count", 2L)
        );
        when(auditIssueMapper.selectMaps(any())).thenReturn(mapperRows);

        // when
        Map<String, Long> result = auditIssueService.countByCategory();

        // then
        assertThat(result)
                .hasSize(3)
                .containsEntry("技术", 3L)
                .containsEntry("商务", 5L)
                .containsEntry("资质", 2L);

        verify(tenderService).getBidIdsByUserId(CURRENT_USER_ID);
        verify(auditTaskService).getAuditIdsByBidIds(bidIds);
        verify(auditIssueMapper).selectMaps(any());
    }

    @Test
    void countByCategory_shouldReturnEmptyMap_whenAuditIdsIsEmpty() {
        // given
        List<Long> bidIds = List.of(10L);
        when(tenderService.getBidIdsByUserId(CURRENT_USER_ID)).thenReturn(bidIds);
        when(auditTaskService.getAuditIdsByBidIds(bidIds)).thenReturn(List.of());

        // when
        Map<String, Long> result = auditIssueService.countByCategory();

        // then
        assertThat(result).isEmpty();
        verify(auditIssueMapper, never()).selectMaps(any());
    }

    @Test
    void countByCategory_shouldReturnSingleEntry_whenOnlyOneCategoryExists() {
        // given
        List<Long> bidIds = List.of(10L);
        List<Long> auditIds = List.of(100L);
        when(tenderService.getBidIdsByUserId(CURRENT_USER_ID)).thenReturn(bidIds);
        when(auditTaskService.getAuditIdsByBidIds(bidIds)).thenReturn(auditIds);

        List<Map<String, Object>> mapperRows = List.of(
                Map.of("category", "技术", "count", 7L)
        );
        when(auditIssueMapper.selectMaps(any())).thenReturn(mapperRows);

        // when
        Map<String, Long> result = auditIssueService.countByCategory();

        // then
        assertThat(result)
                .hasSize(1)
                .containsEntry("技术", 7L);
    }

    @Test
    void countByCategory_shouldReturnEmptyMap_whenMapperReturnsNoRows() {
        // given — 有审核 ID，但数据库中没有对应的问题记录
        List<Long> bidIds = List.of(10L);
        List<Long> auditIds = List.of(100L);
        when(tenderService.getBidIdsByUserId(CURRENT_USER_ID)).thenReturn(bidIds);
        when(auditTaskService.getAuditIdsByBidIds(bidIds)).thenReturn(auditIds);
        when(auditIssueMapper.selectMaps(any())).thenReturn(List.of());

        // when
        Map<String, Long> result = auditIssueService.countByCategory();

        // then
        assertThat(result).isEmpty();
        verify(auditIssueMapper).selectMaps(any());
    }

    @Test
    void countByCategory_shouldReturnEmptyMap_whenBidIdsIsEmpty() {
        // given — 当前用户没有上传过任何标书
        when(tenderService.getBidIdsByUserId(CURRENT_USER_ID)).thenReturn(List.of());
        when(auditTaskService.getAuditIdsByBidIds(List.of())).thenReturn(List.of());

        // when
        Map<String, Long> result = auditIssueService.countByCategory();

        // then
        assertThat(result).isEmpty();
        verify(tenderService).getBidIdsByUserId(CURRENT_USER_ID);
        verify(auditTaskService).getAuditIdsByBidIds(List.of());
        verify(auditIssueMapper, never()).selectMaps(any());
    }
}
