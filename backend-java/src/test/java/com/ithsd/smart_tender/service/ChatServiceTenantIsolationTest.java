package com.ithsd.smart_tender.service;

import com.baomidou.mybatisplus.core.conditions.query.QueryWrapper;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.mapper.ChatMessageMapper;
import com.ithsd.smart_tender.mapper.ProjectMapper;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.model.dto.ChatRequestDTO;
import com.ithsd.smart_tender.model.entity.ChatMessage;
import com.ithsd.smart_tender.model.entity.Project;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.service.engine.rust.RustApiClient;
import com.ithsd.smart_tender.service.engine.rust.RustDocumentService;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.ArgumentCaptor;
import org.mockito.InjectMocks;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;

import java.lang.reflect.Field;
import static org.assertj.core.api.Assertions.assertThat;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.doAnswer;
import static org.mockito.Mockito.times;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

@ExtendWith(MockitoExtension.class)
class ChatServiceTenantIsolationTest {

    private static final Long USER_ID = 10001L;
    private static final Long TENANT_ID = 20001L;

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
        TenantContext.set(new TenantRequestContext(USER_ID, TENANT_ID, "ADMIN", 1L, "chat-test"));
    }

    @AfterEach
    void tearDown() {
        TenantContext.clear();
    }

    @Test
    void chat_validatesTenantOwnedProjectAndTenderBeforeWritingAndPersistsTenant() {
        ChatRequestDTO request = new ChatRequestDTO();
        request.setProjectId(10L);
        request.setBidId(20L);
        request.setContent("What are the payment terms?");

        when(projectMapper.selectOne(any())).thenReturn(Project.builder().id(10L).tenantId(TENANT_ID).build());
        when(tenderMapper.selectOne(any())).thenReturn(
                Tender.builder().id(20L).projectId(10L).tenantId(TENANT_ID).build());
        when(rustDocumentService.ensureUploaded(20L))
                .thenThrow(new IllegalStateException("Rust is unavailable in this unit test"));
        doAnswer(invocation -> 1).when(chatMessageMapper).insert(any(ChatMessage.class));

        chatService.chat(request);

        ArgumentCaptor<QueryWrapper<Project>> projectQuery =
                ArgumentCaptor.forClass(QueryWrapper.class);
        ArgumentCaptor<QueryWrapper<Tender>> tenderQuery =
                ArgumentCaptor.forClass(QueryWrapper.class);
        verify(projectMapper).selectOne(projectQuery.capture());
        verify(tenderMapper).selectOne(tenderQuery.capture());
        assertTenantPredicate(projectQuery.getValue());
        assertTenantPredicate(tenderQuery.getValue());

        ArgumentCaptor<ChatMessage> messages = ArgumentCaptor.forClass(ChatMessage.class);
        verify(chatMessageMapper, times(2)).insert(messages.capture());
        assertThat(messages.getAllValues()).extracting(ChatServiceTenantIsolationTest::tenantIdOf)
                .containsExactly(TENANT_ID, TENANT_ID);
    }

    private static void assertTenantPredicate(QueryWrapper<?> wrapper) {
        assertThat(wrapper.getSqlSegment()).contains("tenant_id");
        assertThat(wrapper.getParamNameValuePairs()).containsValue(TENANT_ID);
    }

    private static Long tenantIdOf(Object entity) {
        try {
            Field field = entity.getClass().getDeclaredField("tenantId");
            field.setAccessible(true);
            return (Long) field.get(entity);
        } catch (ReflectiveOperationException ignored) {
            return null;
        }
    }
}
