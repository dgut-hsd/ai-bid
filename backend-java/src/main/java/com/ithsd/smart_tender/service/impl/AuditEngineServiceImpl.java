package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.ithsd.smart_tender.config.RustApiProperties;
import com.ithsd.smart_tender.mapper.AuditIssueMapper;
import com.ithsd.smart_tender.mapper.AuditTaskMapper;
import com.ithsd.smart_tender.model.entity.AuditIssue;
import com.ithsd.smart_tender.model.entity.AuditTask;
import com.ithsd.smart_tender.model.enums.AuditStageEnum;
import com.ithsd.smart_tender.model.enums.AuditTaskStatusEnum;
import com.ithsd.smart_tender.model.enums.SseEventTypeEnum;
import com.ithsd.smart_tender.model.vo.AuditCompleteVO;
import com.ithsd.smart_tender.model.vo.AuditTaskStatusVO;
import com.ithsd.smart_tender.model.vo.IssueVO;
import com.ithsd.smart_tender.model.vo.SummaryVO;
import com.ithsd.smart_tender.service.AuditEngineService;
import com.ithsd.smart_tender.service.TraceService;
import com.ithsd.smart_tender.service.engine.rust.RustApiClient;
import com.ithsd.smart_tender.service.engine.rust.RustDocumentService;
import com.ithsd.smart_tender.service.engine.rust.RustSseClient;
import com.ithsd.smart_tender.model.dto.rust.RustReviewAcceptedResponse;
import com.ithsd.smart_tender.model.dto.rust.RustReviewRequest;
import com.ithsd.smart_tender.model.dto.rust.RustReviewResponse;
import com.ithsd.smart_tender.model.dto.rust.RustReviewResultResponse;
import com.ithsd.smart_tender.model.dto.rust.RustRiskFinding;
import com.ithsd.smart_tender.sse.AuditTaskEventService;
import com.ithsd.smart_tender.sse.SseHub;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.scheduling.annotation.Async;
import org.springframework.stereotype.Service;
import org.springframework.util.StringUtils;

import java.time.LocalDateTime;
import java.util.ArrayList;
import java.util.List;
import java.util.Set;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ConcurrentHashMap;
import java.util.stream.Collectors;

/**
 * 审核引擎 — 委托 Rust Multi-Agent 引擎执行审核。
 * <p>整个管线（extract → chunk → embed → review）在 Rust 侧同步完成，
 * Java 仅负责：上传文件 → 调用审核 → 映射结果 → 发射 SSE。</p>
 */
@Service
public class AuditEngineServiceImpl implements AuditEngineService {

    private static final Set<String> RUNNING_TASKS = ConcurrentHashMap.newKeySet();
    private static final Logger log = LoggerFactory.getLogger(AuditEngineServiceImpl.class);

    private final AuditTaskMapper auditTaskMapper;
    private final AuditIssueMapper auditIssueMapper;
    private final SseHub sseHub;
    private final AuditTaskEventService eventService;
    private final RustApiClient rustApiClient;
    private final RustDocumentService rustDocumentService;
    private final RustSseClient rustSseClient;
    private final RustApiProperties rustApiProperties;
    private final TraceService traceService;
    private final ObjectMapper objectMapper = new ObjectMapper()
            .setPropertyNamingStrategy(
                com.fasterxml.jackson.databind.PropertyNamingStrategies.SNAKE_CASE);

    public AuditEngineServiceImpl(
            AuditTaskMapper auditTaskMapper,
            AuditIssueMapper auditIssueMapper,
            SseHub sseHub,
            AuditTaskEventService eventService,
            RustApiClient rustApiClient,
            RustDocumentService rustDocumentService,
            RustSseClient rustSseClient,
            RustApiProperties rustApiProperties,
            TraceService traceService
    ) {
        this.auditTaskMapper = auditTaskMapper;
        this.auditIssueMapper = auditIssueMapper;
        this.sseHub = sseHub;
        this.eventService = eventService;
        this.rustApiClient = rustApiClient;
        this.rustDocumentService = rustDocumentService;
        this.rustSseClient = rustSseClient;
        this.rustApiProperties = rustApiProperties;
        this.traceService = traceService;
    }

    @Override
    @Async("auditTaskExecutor")
    public void start(String taskId) {
        log.info("audit async task started: taskId={}, thread={}", taskId, Thread.currentThread().getName());
        if (!RUNNING_TASKS.add(taskId)) {
            log.warn("audit task skipped due to concurrent start, taskId={}", taskId);
            return;
        }
        try {
            runEngine(taskId);
        } finally {
            RUNNING_TASKS.remove(taskId);
        }
    }

    @Override
    public boolean recover(String taskId) {
        AuditTask task = loadTask(taskId);
        if (task == null) {
            return false;
        }
        if (AuditTaskStatusEnum.COMPLETED.getCode().equals(task.getTaskStatus())) {
            return true;
        }

        String rustDocId = rustDocumentService.getCachedDocumentId(task.getBidId());
        if (!StringUtils.hasText(rustDocId)) {
            log.warn("recover: no cached Rust document id, taskId={}", taskId);
            return false;
        }

        RustReviewResultResponse result;
        try {
            result = rustApiClient.getReviewResult(rustDocId);
        } catch (Exception e) {
            log.warn("recover: Rust result unavailable, taskId={}, {}", taskId, e.getMessage());
            return false;
        }
        if (result != null && result.isCompleted() && result.getResult() != null) {
            log.info("recover: completed Rust result found, taskId={}, docId={}",
                    taskId, rustDocId);
            completeTaskFromReview(task, result.getResult());
            return true;
        }
        if (result != null && result.isFailed()) {
            failTask(task, "Rust 审核失败: " + result.getError());
        }
        return false;
    }

    // ── 主流程 ──────────────────────────────────────────────────────

    private AuditTask loadTask(String taskId) {
        return auditTaskMapper.selectOne(
                new LambdaQueryWrapper<AuditTask>().eq(AuditTask::getTaskId, taskId));
    }

    private void runEngine(String taskId) {
        AuditTask task = loadTask(taskId);
        if (task == null) {
            log.warn("audit task not found, taskId={}", taskId);
            sseHub.close(taskId);
            return;
        }
        if (!AuditTaskStatusEnum.PENDING.getCode().equals(task.getTaskStatus())) {
            log.info("audit task already started or finished, taskId={}, status={}", taskId, task.getTaskStatus());
            sseHub.close(taskId);
            return;
        }

        try {
            // Stage 1: 上传文件到 Rust（幂等）
            log.info("═══ [审核 Stage 1/4] 开始上传文件到 Rust 引擎: taskId={}, bidId={} ═══", taskId, task.getBidId());
            updateStage(task, AuditStageEnum.UPLOADING, 10);
            String rustDocId;
            try {
                rustDocId = rustDocumentService.ensureUploaded(task.getBidId());
                log.info("═══ [审核 Stage 1/4] 文件上传完成 → rustDocId={} ═══", rustDocId);
            } catch (Exception ex) {
                log.error("❌ [审核 Stage 1/4] Rust 上传失败: taskId={}, bidId={} — {}", taskId, task.getBidId(), ex.getMessage(), ex);
                failTask(task, "文件上传 Rust 失败: " + ex.getMessage());
                return;
            }

            // Stage 2: 异步调用 Rust Multi-Agent 审核（SSE 实时推送 + 异步结果获取）
            log.info("═══ [审核 Stage 2/4] 开始异步调用 Rust Multi-Agent 审核引擎: rustDocId={} ═══", rustDocId);
            updateStage(task, AuditStageEnum.REVIEWING, 30);
            RustReviewRequest reviewReq = new RustReviewRequest();
            reviewReq.setMaxClauses(200);
            if (task.getEnabledChecks() != null && !task.getEnabledChecks().isEmpty()) {
                reviewReq.setEnabledAgents(task.getEnabledChecks());
            }

            // 信号量：SSE 回调 → 主流程 await
            CompletableFuture<Void> reviewDoneSignal = new CompletableFuture<>();
            CompletableFuture<String> reviewErrorSignal = new CompletableFuture<>();

            // 启动 Rust SSE 实时推送 relay（在调用 Rust POST /review 之前连接，
            // 确保不丢失早期事件）
            CompletableFuture<Void> sseRelay = rustSseClient.connect(rustDocId, (eventType, data) -> {
                try {
                    switch (eventType) {
                        case "agent_progress" -> {
                            com.ithsd.smart_tender.model.vo.AgentProgressVO vo =
                                objectMapper.convertValue(data, com.ithsd.smart_tender.model.vo.AgentProgressVO.class);
                            emitTransient(taskId, SseEventTypeEnum.AGENT_PROGRESS, vo);
                        }
                        case "trace" -> {
                            com.ithsd.smart_tender.model.vo.TraceEventVO vo =
                                objectMapper.convertValue(data, com.ithsd.smart_tender.model.vo.TraceEventVO.class);
                            // TraceService 已持久化完整轨迹；不再重复写 audit_task_event。
                            emitTransient(taskId, SseEventTypeEnum.TRACE, vo);
                            try {
                                traceService.ingestTraceEvent(taskId, rustDocId, vo);
                            } catch (Exception e) {
                                log.warn("Trace ingest failed: clause={} turn={}", vo.getClauseId(), vo.getTurn(), e);
                            }
                        }
                        case "phase" -> {
                            com.ithsd.smart_tender.model.vo.PhaseVO vo =
                                objectMapper.convertValue(data, com.ithsd.smart_tender.model.vo.PhaseVO.class);
                            emitSafe(taskId, SseEventTypeEnum.PHASE, vo);
                            String phase = data.has("phase") ? data.get("phase").asText() : "";
                            int progress = "execute".equals(phase) ? 35 : "merge".equals(phase) ? 45 : "legal_verify".equals(phase) ? 55 : 60;
                            // 使用无乐观锁的单调推进 SQL。不要在多个异步回调中共享并修改 task.version。
                            CompletableFuture.runAsync(() -> {
                                try {
                                    auditTaskMapper.advanceReviewProgress(
                                            taskId, task.getTenantId(), progress, LocalDateTime.now());
                                } catch (Exception e) {
                                    log.warn("advanceReviewProgress async failed: {}", e.getMessage());
                                }
                            });
                        }
                        case "stats" -> {
                            emitSafe(taskId, SseEventTypeEnum.STATS, data);
                        }
                        case "finding_added" -> {
                            // 1. 透传原始数据给前端的 liveFindings（增量更新）
                            emitTransient(taskId, SseEventTypeEnum.FINDING_ADDED, data);
                            // 2. 同时映射为 ISSUE（兼容 AnalysisList 实时显示）
                            try {
                                RustRiskFinding rf = objectMapper.convertValue(data, RustRiskFinding.class);
                                if (!rf.shouldSkip()) {
                                    emitTransient(taskId, SseEventTypeEnum.ISSUE, toIssueVO(rf));
                                }
                            } catch (Exception ignored) {
                                log.debug("SSE finding_added map failed: {}", ignored.getMessage());
                            }
                        }
                        case "finding_removed" -> {
                            // 去重合并时移除 → 前端从 liveFindings 中删除
                            emitTransient(taskId, SseEventTypeEnum.FINDING_REMOVED, data);
                        }
                        case "finding_updated" -> {
                            // 字段变更（降级/辩论） → 前端就地更新 liveFindings
                            emitTransient(taskId, SseEventTypeEnum.FINDING_UPDATED, data);
                        }
                        case "done" -> {
                            log.info("Rust SSE done received: docId={}", rustDocId);
                            // 标记所有 running session 为 completed
                            try {
                                traceService.markSessionsCompleted(taskId);
                            } catch (Exception e) {
                                log.warn("Trace markSessionsCompleted failed: taskId={}", taskId, e);
                            }
                            reviewDoneSignal.complete(null);
                        }
                        case "error" -> {
                            String msg = data.has("message") ? data.get("message").asText() : "审核引擎未知错误";
                            // 兼容旧 Rust：事件通道落后只代表实时进度丢帧，最终结果仍可通过
                            // GET /result 获取，不能把它升级成审核失败。
                            if (msg.contains("SSE lagged") || msg.contains("events were dropped")) {
                                log.warn("Rust SSE progress events dropped; continuing with result polling: docId={}", rustDocId);
                            } else {
                                log.error("Rust SSE error received: docId={}, msg={}", rustDocId, msg);
                                reviewErrorSignal.complete(msg);
                            }
                        }
                        case "stream_lagged" -> {
                            long dropped = data.has("dropped") ? data.get("dropped").asLong() : -1L;
                            log.warn("Rust SSE consumer lagged; {} progress events dropped, review continues: docId={}",
                                    dropped, rustDocId);
                        }
                        default -> {
                            log.debug("Rust SSE unknown event: {}", eventType);
                        }
                    }
                } catch (Exception e) {
                    log.debug("SSE relay event process failed: {}", e.getMessage());
                }
            });

            // 等待 SSE 连接就绪（避免丢失早期事件）
            waitForSseConnection(sseRelay);

            // 启动异步审核（202 Accepted）
            RustReviewAcceptedResponse accepted;
            try {
                accepted = rustApiClient.startReview(rustDocId, reviewReq);
            } catch (Exception ex) {
                log.error("❌ [审核 Stage 2/4] Rust 审核启动失败: taskId={}, rustDocId={} — {}", taskId, rustDocId, ex.getMessage(), ex);
                failTask(task, "Rust 审核启动失败: " + ex.getMessage());
                return;
            }
            if (accepted.isConflict()) {
                log.warn("Rust review conflict, retrying with wait: docId={}", rustDocId);
                try { Thread.sleep(3000); } catch (InterruptedException ignored) {}
                try {
                    accepted = rustApiClient.startReview(rustDocId, reviewReq);
                } catch (Exception ex) {
                    log.error("❌ [审核 Stage 2/4] Rust 审核重试失败: taskId={}, rustDocId={} — {}", taskId, rustDocId, ex.getMessage(), ex);
                    failTask(task, "Rust 审核启动失败（重试）: " + ex.getMessage());
                    return;
                }
                if (accepted.isConflict()) {
                    log.error("❌ [审核 Stage 2/4] Rust 审核仍冲突: taskId={}, rustDocId={}", taskId, rustDocId);
                    failTask(task, "该文档已有进行中的审核任务，请稍后重试");
                    return;
                }
            }
            log.info("═══ [审核 Stage 2/4] Rust 异步审核已提交，等待完成... ═══");

            // 等待审核完成（SSE "done" 信号 或 "error" 信号）
            RustReviewResponse reviewResp = awaitReviewResult(rustDocId, reviewDoneSignal, reviewErrorSignal);
            if (reviewResp == null) {
                log.error("❌ [审核 Stage 2/4] Rust 审核结果获取失败: taskId={}, rustDocId={}", taskId, rustDocId);
                failTask(task, "Rust 审核结果获取失败，请重试或执行任务恢复");
                return;
            }
            log.info("═══ [审核 Stage 2/4] Rust 审核引擎返回: findings={} ═══",
                reviewResp.getFindings() != null ? reviewResp.getFindings().size() : 0);

            completeTaskFromReview(task, reviewResp);

        } catch (Exception ex) {
            log.error("❌ [审核] 未预期的异常导致审核失败: taskId={} — {}", taskId, ex.getMessage(), ex);
            failTask(task, crop(ex.getMessage()));
        } finally {
            sseHub.close(taskId);
        }
    }

    /**
     * 将 Rust 最终结果幂等写入 Java 数据库。正常审核与孤儿任务恢复共用，
     * 避免两条路径出现状态或字段映射差异。
     */
    private synchronized void completeTaskFromReview(
            AuditTask originalTask,
            RustReviewResponse reviewResp) {
        AuditTask task = loadTask(originalTask.getTaskId());
        if (task == null) {
            throw new IllegalStateException("审核任务不存在: " + originalTask.getTaskId());
        }
        if (AuditTaskStatusEnum.COMPLETED.getCode().equals(task.getTaskStatus())) {
            log.info("completeTaskFromReview skipped: task already completed, taskId={}", task.getTaskId());
            return;
        }

        log.info("═══ [审核 Stage 3/4] findings → SSE + DB: taskId={} ═══", task.getTaskId());
        auditTaskMapper.advanceReviewProgress(
                task.getTaskId(), task.getTenantId(), 70, LocalDateTime.now());

        List<RustRiskFinding> activeFindings = new ArrayList<>();
        // 先删除旧数据，确保恢复操作可安全重试。
        auditIssueMapper.delete(new LambdaQueryWrapper<AuditIssue>()
                .eq(AuditIssue::getAuditId, task.getId()));

        if (reviewResp.getFindings() != null) {
            int seq = 0;
            for (RustRiskFinding finding : reviewResp.getFindings()) {
                if (finding.shouldSkip()) continue;
                activeFindings.add(finding);
                try {
                    AuditIssue issue = AuditIssue.builder()
                            .auditId(task.getId())
                            .issueNo("ISSUE-" + (finding.getRiskId() != null
                                    ? finding.getRiskId() : String.valueOf(++seq)))
                            .severity(finding.mappedSeverity())
                            .isCritical(finding.isCritical())
                            .criticalReason(finding.getCriticalReason())
                            .category(finding.getRiskType())
                            .description(finding.getReason() != null
                                    ? finding.getReason() : finding.getRiskType())
                            .suggestion(finding.getSuggestion())
                            .pageNumber(finding.getPageNumber())
                            .sectionName(finding.getSectionPath() != null
                                    ? String.join(" > ", finding.getSectionPath()) : null)
                            .context(finding.getContext() != null
                                    ? finding.getContext() : finding.getSourceQuote())
                            .reference(finding.getLegalBasis() != null
                                    && !finding.getLegalBasis().isEmpty()
                                    ? String.join("; ", finding.getLegalBasis()) : null)
                            .createTime(LocalDateTime.now())
                            .build();
                    auditIssueMapper.insert(issue);
                } catch (Exception e) {
                    log.warn("Failed to persist finding {}: {}", finding.getRiskId(), e.getMessage());
                }
            }
        }

        LocalDateTime completedAt = LocalDateTime.now();
        auditTaskMapper.markCompleted(task.getTaskId(), task.getTenantId(), completedAt);
        task.setTaskStatus(AuditTaskStatusEnum.COMPLETED.getCode());
        task.setStage(AuditStageEnum.SUMMARY.name());
        task.setProgress(100);
        task.setEndTime(completedAt);
        task.setUpdatedAt(completedAt);
        emitSafe(task.getTaskId(), SseEventTypeEnum.COMPLETE,
                toCompleteVO(task, activeFindings, reviewResp));
        log.info("═══ [审核 Stage 4/4] ✅ 审核完成: taskId={}, findings={} (high={}, medium={}, low={}, info={}) ═══",
                task.getTaskId(), activeFindings.size(),
                activeFindings.stream().filter(i -> "high".equals(i.mappedSeverity())).count(),
                activeFindings.stream().filter(i -> "medium".equals(i.mappedSeverity())).count(),
                activeFindings.stream().filter(i -> "low".equals(i.mappedSeverity())).count(),
                activeFindings.stream().filter(i -> "info".equals(i.mappedSeverity())).count());
    }

    // ── 映射：RustRiskFinding → IssueVO（SSE 推送用） ────────────────

    private IssueVO toIssueVO(RustRiskFinding f) {
        IssueVO vo = new IssueVO();
        vo.setIssueNo("ISSUE-" + (f.getRiskId() != null ? f.getRiskId() : "?"));
        vo.setRiskId(f.getRiskId());
        vo.setSeverity(f.mappedSeverity());
        vo.setIsCritical(f.isCritical());
        vo.setCriticalReason(f.getCriticalReason());
        vo.setCategory(f.getRiskType());
        vo.setAgentName(f.getAgent());
        vo.setDescription(f.getReason() != null ? f.getReason() : f.getRiskType());
        vo.setSuggestion(f.getSuggestion());
        vo.setSourceQuote(f.getSourceQuote());
        vo.setLegalBasis(f.getLegalBasis());
        vo.setCaseRefs(f.getCaseRefs());
        vo.setConfidence(f.getConfidence() > 0 ? f.getConfidence() : null);

        IssueVO.LocationVO loc = new IssueVO.LocationVO();
        if (f.getPageNumber() != null) loc.setPageNumber(f.getPageNumber());
        if (f.getSectionPath() != null && !f.getSectionPath().isEmpty())
            loc.setSectionName(String.join(" > ", f.getSectionPath()));
        loc.setContext(f.getContext() != null ? f.getContext() : f.getSourceQuote());
        vo.setLocation(loc);

        if (f.getLegalBasis() != null && !f.getLegalBasis().isEmpty())
            vo.setReference(String.join("; ", f.getLegalBasis()));

        if (f.getPageNumber() != null) {
            vo.setAnchorPage(f.getPageNumber());
        }
        if (f.getSectionPath() != null && !f.getSectionPath().isEmpty()) {
            vo.setAnchorSection(String.join(" > ", f.getSectionPath()));
        }

        vo.setNoRisk(f.isNoRisk());
        vo.setInitialTier(f.getInitialTier());
        vo.setFinalTier(f.getFinalTier());
        vo.setTierEscalated(f.isTierEscalated());
        vo.setTruncated(f.isTruncated());
        vo.setClauseIds(f.getClauseIds());
        vo.setBlockIds(f.getBlockIds());
        vo.setAgent(f.getAgent());

        if (f.getCitations() != null && !f.getCitations().isEmpty()) {
            List<IssueVO.CitationVO> citations = f.getCitations().stream().map(c -> {
                IssueVO.CitationVO cv = new IssueVO.CitationVO();
                cv.setTitle(c.getTitle());
                cv.setUrl(c.getUrl());
                cv.setSiteName(c.getSiteName());
                return cv;
            }).collect(Collectors.toList());
            vo.setCitations(citations);
        }

        if (f.getSuggestedAgent() != null) {
            IssueVO.SuggestedAgentVO sa = new IssueVO.SuggestedAgentVO();
            sa.setAgentName(f.getSuggestedAgent().getAgentName());
            sa.setAgentPrompt(f.getSuggestedAgent().getAgentPrompt());
            sa.setSectionKeywords(f.getSuggestedAgent().getSectionKeywords());
            sa.setReason(f.getSuggestedAgent().getReason());
            vo.setSuggestedAgent(sa);
        }

        return vo;
    }

    // ── 状态更新 & SSE ──────────────────────────────────────────────

    private void updateStage(AuditTask task, AuditStageEnum stage, int progress) {
        task.setTaskStatus(AuditTaskStatusEnum.PROCESSING.getCode());
        task.setStage(stage.name());
        task.setProgress(progress);
        if (task.getStartTime() == null) task.setStartTime(LocalDateTime.now());
        task.setUpdatedAt(LocalDateTime.now());
        auditTaskMapper.update(task, new LambdaQueryWrapper<AuditTask>()
                .eq(AuditTask::getId, task.getId())
                .eq(AuditTask::getTenantId, task.getTenantId()));
        emitSafe(task.getTaskId(), SseEventTypeEnum.PROGRESS, toStatusVO(task));
    }

    private void failTask(AuditTask task, String errorMsg) {
        LocalDateTime failedAt = LocalDateTime.now();
        String safeError = crop(errorMsg);
        auditTaskMapper.markFailed(task.getTaskId(), task.getTenantId(), safeError, failedAt);
        task.setTaskStatus(AuditTaskStatusEnum.FAILED.getCode());
        task.setErrorMsg(safeError);
        task.setEndTime(failedAt);
        task.setUpdatedAt(failedAt);
        emitSafe(task.getTaskId(), SseEventTypeEnum.COMPLETE, toCompleteVO(task, List.of(), null));
    }

    private void emitSafe(String taskId, SseEventTypeEnum eventType, Object payload) {
        try {
            String eventId = eventService.persist(taskId, eventType, payload);
            sseHub.emit(taskId, eventType, payload, eventId);
        } catch (Exception ex) {
            log.warn("emit sse failed: taskId={}, event={}", taskId, eventType.getEventName(), ex);
        }
    }

    /**
     * 高频实时事件不写 audit_task_event。Trace 已有专用表，finding 最终会写
     * audit_issue；重复持久化会放大数据库压力并拖慢 Rust SSE 消费。
     */
    private void emitTransient(String taskId, SseEventTypeEnum eventType, Object payload) {
        try {
            sseHub.emit(taskId, eventType, payload);
        } catch (Exception ex) {
            log.warn("emit transient sse failed: taskId={}, event={}",
                    taskId, eventType.getEventName(), ex);
        }
    }

    private String crop(String value) {
        if (!StringUtils.hasText(value)) return "引擎执行失败";
        return value.length() <= 1000 ? value : value.substring(0, 1000);
    }

    // ── 异步审核等待 ──────────────────────────────────────────────────

    private void waitForSseConnection(CompletableFuture<Void> sseRelay) {
        try {
            sseRelay.get(15, java.util.concurrent.TimeUnit.SECONDS);
        } catch (Exception e) {
            log.warn("Rust SSE connect timeout, continuing without real-time events: {}", e.getMessage());
        }
    }

    private RustReviewResponse awaitReviewResult(
            String rustDocId,
            CompletableFuture<Void> reviewDoneSignal,
            CompletableFuture<String> reviewErrorSignal) {
        long timeoutMs = java.util.concurrent.TimeUnit.MINUTES.toMillis(
                rustApiProperties.getReviewTimeoutMinutes());
        long deadline = System.currentTimeMillis() + timeoutMs;
        boolean doneLogged = false;

        // Rust 的 GET /result 是最终事实来源；SSE 只负责低延迟通知和界面进度。
        // 即使进度事件丢帧或连接中断，也持续轮询最终结果，避免制造孤儿任务。
        while (System.currentTimeMillis() < deadline) {
            if (reviewDoneSignal.isDone() && !doneLogged) {
                log.info("Rust review SSE done received; confirming via result endpoint: docId={}", rustDocId);
                doneLogged = true;
            }

            try {
                RustReviewResultResponse result = rustApiClient.getReviewResult(rustDocId);
                if (result != null && result.isCompleted()) {
                    return result.getResult();
                }
                if (result != null && result.isFailed()) {
                    log.error("Rust review failed: docId={}, error={}", rustDocId, result.getError());
                    return null;
                }
            } catch (Exception e) {
                log.warn("Rust result poll failed, will retry: docId={}, {}", rustDocId, e.getMessage());
            }

            if (reviewErrorSignal.isDone()) {
                try {
                    log.error("Rust review engine error via SSE: docId={}, {}",
                            rustDocId, reviewErrorSignal.get());
                } catch (Exception ignored) {
                    log.error("Rust review engine error via SSE: docId={}", rustDocId);
                }
                return null;
            }

            try {
                Thread.sleep(reviewDoneSignal.isDone() ? 500L : 3000L);
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                log.warn("Rust review wait interrupted: docId={}", rustDocId);
                return null;
            }
        }

        log.error("Rust review timed out after {}min: docId={}",
                rustApiProperties.getReviewTimeoutMinutes(), rustDocId);
        return null;
    }

    // ── VO 构建 ─────────────────────────────────────────────────────

    private AuditTaskStatusVO toStatusVO(AuditTask task) {
        AuditTaskStatusVO vo = new AuditTaskStatusVO();
        vo.setTaskId(task.getTaskId());
        vo.setStatus(AuditTaskStatusEnum.fromCode(task.getTaskStatus()).getValue());
        vo.setStage(task.getStage());
        vo.setProgress(task.getProgress());
        vo.setIssueCount(0);
        vo.setFailedStages(task.getFailedStages() == null ? List.of() : task.getFailedStages());
        return vo;
    }

    private AuditCompleteVO toCompleteVO(AuditTask task, List<RustRiskFinding> findings,
                                          RustReviewResponse reviewResp) {
        AuditCompleteVO vo = new AuditCompleteVO();
        vo.setTaskId(task.getTaskId());
        vo.setStatus(AuditTaskStatusEnum.fromCode(task.getTaskStatus()).getValue());
        vo.setAuditResult(findings.isEmpty() ? "pass" : "revise");
        vo.setIssueCount(findings.size());
        vo.setFailedStages(task.getFailedStages() == null ? List.of() : task.getFailedStages());
        vo.setSummary(buildSummary(findings));
        if (reviewResp != null && reviewResp.getRoutingSummary() != null) {
            vo.setRoutingSummary(buildRoutingSummary(reviewResp));
        }
        if (reviewResp != null && reviewResp.getGraphSnapshot() != null) {
            vo.setGraphSnapshot(reviewResp.getGraphSnapshot());
        }
        return vo;
    }

    private SummaryVO buildRoutingSummary(RustReviewResponse reviewResp) {
        SummaryVO s = new SummaryVO();
        if (reviewResp.getRoutingSummary() == null) return s;
        s.setTotalClauses(reviewResp.getRoutingSummary().getTotalClauses());
        s.setAgentClauseCounts(reviewResp.getRoutingSummary().getAgentClauseCounts());
        s.setLegalVerifyCount(reviewResp.getRoutingSummary().getLegalVerifyCount());
        s.setBlindSpotFindings(reviewResp.getRoutingSummary().getBlindSpotFindings());
        return s;
    }

    private SummaryVO buildSummary(List<RustRiskFinding> findings) {
        SummaryVO s = new SummaryVO();
        int critical = 0, high = 0, medium = 0, low = 0, info = 0;
        for (RustRiskFinding f : findings) {
            if (f.isCritical()) {
                critical++;
            }
            String sev = f.mappedSeverity();
            switch (sev) {
                case "high" -> high++;
                case "medium" -> medium++;
                case "low" -> low++;
                default -> info++;
            }
        }
        s.setTotalIssues(findings.size());
        s.setCritical(critical);
        s.setHigh(high);
        s.setMedium(medium);
        s.setLow(low);
        s.setInfo(info);
        return s;
    }
}
