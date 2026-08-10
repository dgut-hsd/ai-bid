package com.ithsd.smart_tender.service;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.common.BaseContext;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.mapper.ChatMessageMapper;
import com.ithsd.smart_tender.mapper.ProjectMapper;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.model.dto.ChatRequestDTO;
import com.ithsd.smart_tender.model.entity.ChatMessage;
import com.ithsd.smart_tender.model.entity.Project;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.model.vo.ChatMessageVO;
import com.ithsd.smart_tender.service.engine.rust.RustApiClient;
import com.ithsd.smart_tender.service.engine.rust.RustDocumentService;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.InjectMocks;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;

import java.util.List;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.*;

/**
 * 跨租户隔离测试 — ChatService。
 *
 * <p>验证租户 A 的用户无法通过已知 projectId/bidId 读取租户 B 的标书聊天内容。
 * resolveChatResource 在 chat()/getHistory() 入口即完成 project/bid 的租户归属校验，
 * 不归属本租户时抛 {@code RESOURCE_NOT_FOUND}。</p>
 */
@ExtendWith(MockitoExtension.class)
class ChatServiceTenantIsolationTest {

    private static final Long TENANT_A = 2001L;
    private static final Long USER_A = 1001L;
    private static final Long BID_ID = 5001L;
    private static final Long PROJECT_ID = 3001L;

    @Mock
    private ChatMessageMapper chatMessageMapper;
    @Mock
    private ProjectMapper projectMapper;
    @Mock
    private TenderMapper tenderMapper;
    @Mock
    private RustApiClient rustApiClient;
    @Mock
    private RustDocumentService rustDocumentService;

    @InjectMocks
    private ChatService chatService;

    @BeforeEach
    void setUp() {
        BaseContext.setCurrentId(USER_A);
    }

    @AfterEach
    void tearDown() {
        TenantContext.clear();
        BaseContext.removeCurrentId();
    }

    /** 租户 A 用户上下文 */
    private void givenUserInTenantA() {
        TenantContext.set(new TenantRequestContext(USER_A, TENANT_A, "OWNER", 1L, "chat-test-a"));
    }

    private ChatRequestDTO buildChatRequest() {
        ChatRequestDTO dto = new ChatRequestDTO();
        dto.setProjectId(PROJECT_ID);
        dto.setBidId(BID_ID);
        dto.setContent("测试问题");
        return dto;
    }

    // ── chat() ─────────────────────────────────────────────────

    @Test
    void chat_shouldThrowWhenNoTenantContext() {
        assertThatThrownBy(() -> chatService.chat(buildChatRequest()))
                .isInstanceOf(TenantAuthException.class)
                .matches(ex -> ((TenantAuthException) ex).getErrorCode().equals("TENANT_REQUIRED"));
    }

    @Test
    void chat_shouldRejectWhenProjectNotBelongsToTenant() {
        givenUserInTenantA();
        // project 查不到（不属于本租户）→ 入口即抛 RESOURCE_NOT_FOUND
        when(projectMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        assertThatThrownBy(() -> chatService.chat(buildChatRequest()))
                .isInstanceOf(TenantAuthException.class)
                .matches(ex -> ((TenantAuthException) ex).getErrorCode().equals("RESOURCE_NOT_FOUND"));

        verify(projectMapper).selectOne(any(LambdaQueryWrapper.class));
        verify(tenderMapper, never()).selectOne(any(LambdaQueryWrapper.class));
        verify(rustDocumentService, never()).ensureUploaded(any(), any());
    }

    @Test
    void chat_shouldRejectWhenTenderNotBelongsToTenant() {
        givenUserInTenantA();
        when(projectMapper.selectOne(any(LambdaQueryWrapper.class)))
                .thenReturn(Project.builder().id(PROJECT_ID).tenantId(TENANT_A).build());
        // tender 查不到（不属于本租户）→ 抛 RESOURCE_NOT_FOUND
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        assertThatThrownBy(() -> chatService.chat(buildChatRequest()))
                .isInstanceOf(TenantAuthException.class)
                .matches(ex -> ((TenantAuthException) ex).getErrorCode().equals("RESOURCE_NOT_FOUND"));

        verify(rustDocumentService, never()).ensureUploaded(any(), any());
    }

    @Test
    void chat_shouldProceedForSameTenantBid() {
        givenUserInTenantA();
        when(projectMapper.selectOne(any(LambdaQueryWrapper.class)))
                .thenReturn(Project.builder().id(PROJECT_ID).tenantId(TENANT_A).build());
        Tender tender = Tender.builder().id(BID_ID).projectId(PROJECT_ID).tenantId(TENANT_A).build();
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(tender);
        // Rust 不可用 → chat() 捕获异常返回友好提示，不应抛租户异常
        when(rustDocumentService.ensureUploaded(any(), any()))
                .thenThrow(new RuntimeException("Rust unreachable"));
        doAnswer(inv -> 1).when(chatMessageMapper).insert(any(ChatMessage.class));

        var response = chatService.chat(buildChatRequest());

        assertThat(response).isNotNull();
        assertThat(response.getContent()).contains("文档正在处理中");
        verify(rustDocumentService).ensureUploaded(eq(BID_ID), eq(TENANT_A));
    }

    // ── getHistory() ───────────────────────────────────────────

    @Test
    void getHistory_shouldThrowWhenNoTenantContext() {
        assertThatThrownBy(() -> chatService.getHistory(PROJECT_ID, BID_ID, 10))
                .isInstanceOf(TenantAuthException.class)
                .matches(ex -> ((TenantAuthException) ex).getErrorCode().equals("TENANT_REQUIRED"));
    }

    @Test
    void getHistory_shouldRejectWhenProjectNotBelongsToTenant() {
        givenUserInTenantA();
        when(projectMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        assertThatThrownBy(() -> chatService.getHistory(PROJECT_ID, BID_ID, 10))
                .isInstanceOf(TenantAuthException.class)
                .matches(ex -> ((TenantAuthException) ex).getErrorCode().equals("RESOURCE_NOT_FOUND"));

        verify(chatMessageMapper, never()).selectList(any(LambdaQueryWrapper.class));
    }

    @Test
    void getHistory_shouldReturnMessagesForSameTenantBid() {
        givenUserInTenantA();
        when(projectMapper.selectOne(any(LambdaQueryWrapper.class)))
                .thenReturn(Project.builder().id(PROJECT_ID).tenantId(TENANT_A).build());
        Tender tender = Tender.builder().id(BID_ID).projectId(PROJECT_ID).tenantId(TENANT_A).build();
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(tender);
        when(chatMessageMapper.selectList(any(LambdaQueryWrapper.class))).thenReturn(List.of());

        List<ChatMessageVO> history = chatService.getHistory(PROJECT_ID, BID_ID, 10);

        assertThat(history).isEmpty();
        verify(chatMessageMapper).selectList(any(LambdaQueryWrapper.class));
    }
}
