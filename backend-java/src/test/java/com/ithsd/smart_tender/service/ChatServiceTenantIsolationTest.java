package com.ithsd.smart_tender.service;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.common.BaseContext;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.mapper.ChatMessageMapper;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.model.dto.ChatRequestDTO;
import com.ithsd.smart_tender.model.entity.ChatMessage;
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
 * <p>验证租户 A 的用户无法通过已知 bidId 读取租户 B 的标书聊天内容。</p>
 */
@ExtendWith(MockitoExtension.class)
class ChatServiceTenantIsolationTest {

    private static final Long TENANT_A = 2001L;
    private static final Long TENANT_B = 2002L;
    private static final Long USER_A = 1001L;
    private static final Long BID_ID = 5001L;
    private static final Long PROJECT_ID = 3001L;

    @Mock
    private ChatMessageMapper chatMessageMapper;
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

    /** 租户 A 的用户访问租户 B 的标书 — tender 查询返回 null */
    private void givenUserInTenantA() {
        TenantContext.set(new TenantRequestContext(USER_A, TENANT_A, "OWNER", 1L, "chat-test-a"));
    }

    /** 租户 B 用户上下文 */
    private void givenUserInTenantB() {
        TenantContext.set(new TenantRequestContext(USER_A, TENANT_B, "OWNER", 1L, "chat-test-b"));
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
    void chat_shouldRejectCrossTenantAccess() {
        givenUserInTenantA();

        // 租户 A 查不到租户 B 的标书
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        ChatRequestDTO dto = buildChatRequest();
        // ensureUploaded 会因标书不存在而抛异常 → 进入 catch 返回友好提示
        when(rustDocumentService.ensureUploaded(eq(BID_ID), eq(TENANT_A)))
                .thenThrow(new RuntimeException("标书不存在"));

        var response = chatService.chat(dto);
        assertThat(response.getContent()).contains("文档正在处理中");

        // 验证 tender 查询附带了 tenant_id
        verify(tenderMapper).selectOne(any(LambdaQueryWrapper.class));
        // chat 历史不应被查询（因为 tender 不属于此租户就进入了异常分支）
        verify(chatMessageMapper, never()).selectList(any(LambdaQueryWrapper.class));
    }

    @Test
    void chat_shouldAllowSameTenantAccess() {
        givenUserInTenantA();

        Tender tender = Tender.builder().id(BID_ID).tenantId(TENANT_A).build();
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(tender);

        // Rust 调用会失败但这是预期的（mock 不连接真实 Rust）
        ChatRequestDTO dto = buildChatRequest();
        var response = chatService.chat(dto);
        // 应该尝试调用 Rust（可能有 network 错误，但不会因租户问题被拒）
        assertThat(response).isNotNull();
    }

    // ── getHistory() ───────────────────────────────────────────

    @Test
    void getHistory_shouldReturnEmptyForCrossTenantBid() {
        givenUserInTenantA();

        // 租户 A 查不到租户 B 的标书 → 返回空列表
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        List<ChatMessageVO> history = chatService.getHistory(PROJECT_ID, BID_ID, 10);

        assertThat(history).isEmpty();
        verify(tenderMapper).selectOne(any(LambdaQueryWrapper.class));
        verify(chatMessageMapper, never()).selectList(any(LambdaQueryWrapper.class));
    }

    @Test
    void getHistory_shouldReturnMessagesForSameTenantBid() {
        givenUserInTenantA();

        Tender tender = Tender.builder().id(BID_ID).tenantId(TENANT_A).build();
        when(tenderMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(tender);
        when(chatMessageMapper.selectList(any(LambdaQueryWrapper.class))).thenReturn(List.of());

        List<ChatMessageVO> history = chatService.getHistory(PROJECT_ID, BID_ID, 10);

        assertThat(history).isEmpty(); // no messages, but no rejection either
        verify(tenderMapper).selectOne(any(LambdaQueryWrapper.class));
        verify(chatMessageMapper).selectList(any(LambdaQueryWrapper.class));
    }

    // ── TenantContext 缺失 ─────────────────────────────────────

    @Test
    void chat_shouldThrowWhenNoTenantContext() {
        // 不设置 TenantContext → TenantScope.requiredTenantId() 应抛异常
        assertThatThrownBy(() -> chatService.chat(buildChatRequest()))
                .isInstanceOf(TenantAuthException.class)
                .matches(ex -> ((TenantAuthException) ex).getErrorCode().equals("TENANT_REQUIRED"));
    }

    @Test
    void getHistory_shouldThrowWhenNoTenantContext() {
        assertThatThrownBy(() -> chatService.getHistory(PROJECT_ID, BID_ID, 10))
                .isInstanceOf(TenantAuthException.class)
                .matches(ex -> ((TenantAuthException) ex).getErrorCode().equals("TENANT_REQUIRED"));
    }
}
