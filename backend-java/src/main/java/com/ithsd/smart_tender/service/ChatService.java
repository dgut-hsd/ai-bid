package com.ithsd.smart_tender.service;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.baomidou.mybatisplus.core.conditions.query.QueryWrapper;
import com.ithsd.smart_tender.common.BaseContext;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.mapper.ChatMessageMapper;
import com.ithsd.smart_tender.mapper.ProjectMapper;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.model.dto.ChatRequestDTO;
import com.ithsd.smart_tender.model.entity.ChatMessage;
import com.ithsd.smart_tender.model.entity.Project;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.model.vo.ChatMessageVO;
import com.ithsd.smart_tender.model.vo.ChatResponseVO;
import com.ithsd.smart_tender.service.engine.rust.RustApiClient;
import com.ithsd.smart_tender.service.engine.rust.RustDocumentService;
import com.ithsd.smart_tender.service.impl.TenantScope;
import com.ithsd.smart_tender.model.dto.rust.RustChatRequest;
import com.ithsd.smart_tender.model.dto.rust.RustChatResponse;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import lombok.RequiredArgsConstructor;
import lombok.extern.slf4j.Slf4j;
import org.springframework.beans.BeanUtils;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;
import org.springframework.web.servlet.mvc.method.annotation.SseEmitter;

import java.io.IOException;
import java.time.LocalDateTime;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;

import java.util.stream.Collectors;

/**
 * Chat 服务 — 委托 Rust ChatAgent 进行文档智能问答。
 * <p>所有 RAG/LLM 能力由 Rust 侧提供，Java 仅做消息持久化和透传。</p>
 */
@Service
@Slf4j
@RequiredArgsConstructor
public class ChatService {

    private final ChatMessageMapper chatMessageMapper;
    private final ProjectMapper projectMapper;
    private final TenderMapper tenderMapper;
    private final RustApiClient rustApiClient;
    private final RustDocumentService rustDocumentService;

    @Transactional
    public ChatResponseVO chat(ChatRequestDTO requestDTO) {
        ChatResource resource = resolveChatResource(requestDTO.getProjectId(), requestDTO.getBidId());
        Long rawUserId = BaseContext.getCurrentId();
        final Long userId = rawUserId != null ? rawUserId : 0L;
        Long projectId = requestDTO.getProjectId();
        Long bidId = requestDTO.getBidId();
        // resolveChatResource 已完成 project/bid 的租户归属校验，直接复用其结果，
        // 避免再查一次 tender。
        Long tenantId = resource.tenantId();
        Tender tender = resource.tender();

        // 1. 保存用户消息
        ChatMessage userMsg = ChatMessage.builder()
                .tenantId(tenantId).projectId(projectId).bidId(bidId).userId(userId)
                .role("user").content(requestDTO.getContent())
                .createTime(LocalDateTime.now()).build();
        chatMessageMapper.insert(userMsg);

        // 2. 标书已由 resolveChatResource 取出（含租户归属校验）

        // 3. 确保文件已上传到 Rust（幂等，Rust 重启后自动重传）
        ChatResponseVO response;
        try {
            String rustDocId = rustDocumentService.ensureUploaded(bidId, tenantId);
            response = chatViaRust(rustDocId, tender, requestDTO, projectId, bidId, userId);
        } catch (Exception e) {
            log.warn("chat: Rust document upload failed, bidId={}: {}", bidId, e.getMessage());
            response = ChatResponseVO.builder()
                    .content("文档正在处理中，请稍后再试。若持续出现此提示，请重新上传文件。")
                    .citations(List.of()).build();
        }

        // 4. 保存 AI 回复
        ChatMessage aiMsg = ChatMessage.builder()
                .tenantId(tenantId).projectId(projectId).bidId(bidId).userId(userId)
                .role("assistant").content(response.getContent())
                .createTime(LocalDateTime.now()).build();
        chatMessageMapper.insert(aiMsg);

        // 5. saveToKnowledgeBase — 暂不支持（Rust 尚无知识库写入端点）
        if (Boolean.TRUE.equals(requestDTO.getSaveToKnowledgeBase())) {
            log.info("saveToKnowledgeBase requested but not yet supported via Rust, bidId={}", bidId);
        }

        return response;
    }

    // ── SSE 流式对话 ────────────────────────────────────────────────

    private final ObjectMapper objectMapper = new ObjectMapper()
            .setPropertyNamingStrategy(com.fasterxml.jackson.databind.PropertyNamingStrategies.SNAKE_CASE);

    /**
     * 流式对话入口（SSE）。
     *
     * <p>保存用户消息 → 连接 Rust Chat SSE → 实时转发事件到前端 →
     * done 时保存 AI 回复到 DB 并关闭 SseEmitter。</p>
     */
    public SseEmitter chatStream(ChatRequestDTO requestDTO) {
        Long projectId = requestDTO.getProjectId();
        Long bidId = requestDTO.getBidId();
        ChatResource resource = resolveChatResource(projectId, bidId);
        Long tenantId = resource.tenantId();
        Tender tender = resource.tender();

        Long rawUserId = BaseContext.getCurrentId();
        final Long userId = rawUserId != null ? rawUserId : 0L;

        log.info("chatStream called: projectId={}, bidId={}, contentLen={}", projectId, bidId,
                requestDTO.getContent() != null ? requestDTO.getContent().length() : 0);

        // 1. Save user message
        ChatMessage userMsg = ChatMessage.builder()
                .tenantId(tenantId).projectId(projectId).bidId(bidId).userId(userId)
                .role("user").content(requestDTO.getContent())
                .createTime(LocalDateTime.now()).build();
        chatMessageMapper.insert(userMsg);

        // 2. tender 已由 resolveChatResource 取出；此处只确保已上传到 Rust（懒上传）
        log.info("chatStream tender: bidId={}, found={}, rustDocId={}", bidId,
                tender != null, tender != null ? tender.getRustDocumentId() : "N/A");

        SseEmitter emitter = new SseEmitter(300_000L); // 5 min timeout

        if (tender == null) {
            log.warn("chatStream: tender not found for bidId={}, sending error", bidId);
            sendErrorAsync(emitter, "标书不存在", bidId);
            return emitter;
        }

        // ★ 触发懒上传到 Rust（对齐同步 chat() 的 chatViaRust 逻辑）
        final String rustDocId;
        try {
            rustDocId = rustDocumentService.ensureUploaded(bidId, tenantId);
            log.info("chatStream: Rust doc ready, docId={}", rustDocId);
        } catch (Exception e) {
            log.warn("chatStream: ensureUploaded failed for bidId={}: {}", bidId, e.getMessage());
            sendErrorAsync(emitter, "文档正在处理中，请稍后再试。若持续出现此提示，请重新上传文件。", bidId);
            return emitter;
        }

        // 3. Build Rust request
        RustChatRequest rustReq = buildRustChatRequest(requestDTO, projectId, bidId, userId);
        log.info("chatStream: connecting to Rust SSE, docId={}", rustDocId);

        // 4. Async: connect to Rust SSE, relay events, save AI response on done
        String threadName = "chat-stream-" + bidId;
        new Thread(TenantContext.wrap(() -> {
            try {
                CompletableFuture<Void> connected = rustApiClient.connectChatStream(
                        rustDocId, rustReq, (eventType, jsonNode) -> {
                            log.info("chatStream event received: type={}, bidId={}", eventType, bidId);
                            try {
                                switch (eventType) {
                                    case "thinking":
                                    case "tool_call":
                                        emitter.send(SseEmitter.event()
                                                .name(eventType)
                                                .data(jsonNode.toString()));
                                        break;
                                    case "answer":
                                        emitter.send(SseEmitter.event()
                                                .name("answer")
                                                .data(jsonNode.toString()));
                                        break;
                                    case "done":
                                        // Extract answer text from ChatResponse JSON
                                        String answer = "";
                                        if (jsonNode.has("answer")) {
                                            answer = jsonNode.get("answer").asText("");
                                        }
                                        // Save AI message to DB
                                        ChatMessage aiMsg = ChatMessage.builder()
                                                .tenantId(tenantId).projectId(projectId).bidId(bidId).userId(userId)
                                                .role("assistant").content(answer)
                                                .createTime(LocalDateTime.now()).build();
                                        chatMessageMapper.insert(aiMsg);
                                        // Forward done event to client
                                        emitter.send(SseEmitter.event()
                                                .name("done")
                                                .data(jsonNode.toString()));
                                        emitter.complete();
                                        log.info("chatStream done: bidId={}, answerLen={}", bidId, answer.length());
                                        break;
                                    case "error":
                                        emitter.send(SseEmitter.event()
                                                .name("error")
                                                .data(jsonNode.toString()));
                                        emitter.complete();
                                        log.warn("chatStream error from Rust: bidId={}, data={}", bidId, jsonNode.toString());
                                        break;
                                }
                            } catch (IOException e) {
                                log.warn("SSE emit failed for bidId={}: {}", bidId, e.getMessage());
                            }
                        });

                log.info("chatStream: waiting for Rust connection, bidId={}", bidId);
                connected.get(15, TimeUnit.SECONDS);
                log.info("chatStream: Rust connected, bidId={}", bidId);

            } catch (Exception e) {
                log.warn("Chat stream failed for bidId={}: {}", bidId, e.getMessage(), e);
                try {
                    emitter.send(SseEmitter.event()
                            .name("error")
                            .data(objectMapper.writeValueAsString(Map.of("message", "AI 审核引擎暂时不可用，请稍后再试。"))));
                    emitter.complete();
                } catch (IOException ex) {
                    emitter.completeWithError(ex);
                }
            }
        }), threadName).start();

        log.info("chatStream: emitter returned, bidId={}", bidId);
        return emitter;
    }

    /** Async error sender — lets Spring initialize the emitter first. */
    private void sendErrorAsync(SseEmitter emitter, String message, Long bidId) {
        new Thread(TenantContext.wrap(() -> {
            try {
                Thread.sleep(50);
                String json = objectMapper.writeValueAsString(Map.of("message", message));
                emitter.send(SseEmitter.event().name("error").data(json));
                emitter.complete();
            } catch (IOException e) {
                emitter.completeWithError(e);
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
            }
        }), "chat-stream-err-" + bidId).start();
    }

    // ── Rust ChatAgent ──────────────────────────────────────────────

    private ChatResponseVO chatViaRust(String rustDocId, Tender tender, ChatRequestDTO requestDTO,
                                        Long projectId, Long bidId, Long userId) {
        try {
            RustChatRequest req = buildRustChatRequest(requestDTO, projectId, bidId, userId);
            RustChatResponse resp = rustApiClient.chatWithDocument(rustDocId, req);

            List<ChatResponseVO.ChatCitationVO> citations = new ArrayList<>();
            // BlockRef → citation
            if (resp.getReferences() != null) {
                for (RustChatResponse.BlockRef ref : resp.getReferences()) {
                    citations.add(ChatResponseVO.ChatCitationVO.builder()
                            .type("block")
                            .blockId(ref.getBlockId()).quote(ref.getQuote())
                            .snippet(ref.getSnippet()).page(ref.getPage())
                            .build());
                }
            }
            // KnowledgeRef → citation
            if (resp.getKnowledgeRefs() != null) {
                for (RustChatResponse.KnowledgeRef ref : resp.getKnowledgeRefs()) {
                    citations.add(ChatResponseVO.ChatCitationVO.builder()
                            .type(ref.getRefType())
                            .title(ref.getTitle()).excerpt(ref.getExcerpt())
                            .sourceUrl(ref.getSourceUrl())
                            .build());
                }
            }

            String answer = resp.getAnswer();
            if (answer == null || answer.isBlank()) {
                answer = "AI 未能生成有效回复，请重新描述问题。";
            }

            return ChatResponseVO.builder()
                    .content(answer).citations(citations)
                    .confidence(resp.getConfidence())
                    .suggestedActions(resp.getSuggestedActions())
                    .build();
        } catch (Exception e) {
            log.warn("Rust chat failed, bidId={}, error={}", bidId, e.getMessage());
            return ChatResponseVO.builder()
                    .content("AI 审核引擎暂时不可用，请稍后再试。")
                    .citations(List.of()).build();
        }
    }

    private RustChatRequest buildRustChatRequest(ChatRequestDTO dto, Long projectId, Long bidId, Long userId) {
        Long tenantId = resolveChatResource(projectId, bidId).tenantId();
        RustChatRequest req = new RustChatRequest();
        req.setUserInput(dto.getContent());
        req.setMaxTurns(12); // 对齐 Rust ChatAgentConfig 默认值

        // 文本选区（含 bbox，对齐 Rust TextSelection）
        if (dto.getSelection() != null) {
            RustChatRequest.RustTextSelection sel = new RustChatRequest.RustTextSelection();
            sel.setText(dto.getSelection().getText());
            sel.setBlockIds(dto.getSelection().getBlockIds());
            sel.setPage(dto.getSelection().getPage());
            if (dto.getSelection().getBbox() != null) {
                RustChatRequest.RustBBox bbox = new RustChatRequest.RustBBox();
                bbox.setX0(dto.getSelection().getBbox().getX0());
                bbox.setTop(dto.getSelection().getBbox().getTop());
                bbox.setX1(dto.getSelection().getBbox().getX1());
                bbox.setBottom(dto.getSelection().getBbox().getBottom());
                sel.setBbox(bbox);
            }
            req.setSelection(sel);
        }

        // 对话历史（最近 6 条）
        LocalDateTime startTime = LocalDateTime.now().minusDays(3);
        List<ChatMessage> history = chatMessageMapper.selectList(
                new LambdaQueryWrapper<ChatMessage>()
                        .eq(ChatMessage::getTenantId, tenantId)
                        .eq(ChatMessage::getProjectId, projectId)
                        .eq(ChatMessage::getBidId, bidId)
                        .eq(ChatMessage::getUserId, userId)
                        .ge(ChatMessage::getCreateTime, startTime)
                        .orderByAsc(ChatMessage::getCreateTime));
        if (history != null && !history.isEmpty()) {
            int start = Math.max(0, history.size() - 6);
            List<RustChatRequest.RustChatMessageDto> rustHistory = new ArrayList<>();
            for (int i = start; i < history.size(); i++) {
                ChatMessage m = history.get(i);
                RustChatRequest.RustChatMessageDto h = new RustChatRequest.RustChatMessageDto();
                h.setRole(m.getRole());
                h.setContent(m.getContent());
                rustHistory.add(h);
            }
            req.setHistory(rustHistory);
        }
        return req;
    }

    // ── 历史 ────────────────────────────────────────────────────────

    @Transactional(readOnly = true)
    public List<ChatMessageVO> getHistory(Long projectId, Long bidId, Integer days) {
        Long tenantId = resolveChatResource(projectId, bidId).tenantId();
        Long rawUserId = BaseContext.getCurrentId();
        final Long userId = rawUserId != null ? rawUserId : 0L;
        Long tenantId = TenantScope.requiredTenantId();

        // 验证标书属于当前租户
        Tender tender = tenderMapper.selectOne(new LambdaQueryWrapper<Tender>()
                .eq(Tender::getId, bidId)
                .eq(Tender::getTenantId, tenantId));
        if (tender == null) {
            return List.of();
        }

        int queryDays = (days != null && days > 0) ? days : 10;
        LocalDateTime startTime = LocalDateTime.now().minusDays(queryDays);
        List<ChatMessage> messages = chatMessageMapper.selectList(
                new LambdaQueryWrapper<ChatMessage>()
                        .eq(ChatMessage::getTenantId, tenantId)
                        .eq(ChatMessage::getProjectId, projectId)
                        .eq(ChatMessage::getBidId, bidId)
                        .eq(ChatMessage::getUserId, userId)
                        .ge(ChatMessage::getCreateTime, startTime)
                        .orderByAsc(ChatMessage::getCreateTime));
        return messages.stream().map(msg -> {
            ChatMessageVO vo = new ChatMessageVO();
            BeanUtils.copyProperties(msg, vo);
            return vo;
        }).collect(Collectors.toList());
    }

    private ChatResource resolveChatResource(Long projectId, Long bidId) {
        Long tenantId = TenantScope.requiredTenantId();
        if (projectId == null || bidId == null) {
            throw TenantScope.resourceNotFound();
        }

        Project project = projectMapper.selectOne(new QueryWrapper<Project>()
                .eq("id", projectId)
                .eq("tenant_id", tenantId));
        if (project == null) {
            throw TenantScope.resourceNotFound();
        }

        Tender tender = tenderMapper.selectOne(new QueryWrapper<Tender>()
                .eq("id", bidId)
                .eq("project_id", projectId)
                .eq("tenant_id", tenantId));
        if (tender == null) {
            throw TenantScope.resourceNotFound();
        }
        return new ChatResource(tenantId, tender);
    }

    private record ChatResource(Long tenantId, Tender tender) {
    }

}
