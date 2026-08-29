package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.common.BaseContext;
import com.ithsd.smart_tender.common.BizException;
import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.mapper.AuditIssueMapper;
import com.ithsd.smart_tender.mapper.AuditTaskMapper;
import com.ithsd.smart_tender.mapper.KnowledgeFileMapper;
import com.ithsd.smart_tender.mapper.TenderMapper;
import com.ithsd.smart_tender.model.entity.AuditIssue;
import com.ithsd.smart_tender.model.dto.CreateAuditTaskRequest;
import com.ithsd.smart_tender.model.entity.AuditTask;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.model.enums.AuditStageEnum;
import com.ithsd.smart_tender.model.enums.AuditTaskStatusEnum;
import com.ithsd.smart_tender.model.enums.SseEventTypeEnum;
import com.ithsd.smart_tender.model.vo.AuditTaskCreateVO;
import com.ithsd.smart_tender.model.vo.AuditTaskStatusVO;
import com.ithsd.smart_tender.model.vo.IssueVO;
import com.ithsd.smart_tender.model.vo.ResultVO;
import com.ithsd.smart_tender.model.vo.SummaryVO;
import com.ithsd.smart_tender.service.AuditTaskService;
import com.ithsd.smart_tender.service.TenderService;
import com.ithsd.smart_tender.service.engine.queue.AuditTaskDispatcher;
import com.ithsd.smart_tender.service.engine.queue.AuditTaskEnvelope;
import com.ithsd.smart_tender.service.engine.rust.RustApiClient;
import com.ithsd.smart_tender.model.dto.rust.RustBlockBBoxResponse;
import com.ithsd.smart_tender.model.dto.rust.RustReviewResponse;
import com.ithsd.smart_tender.model.dto.rust.RustReviewResultResponse;
import com.ithsd.smart_tender.model.dto.rust.RustRiskFinding;
import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.PropertyNamingStrategies;
import com.ithsd.smart_tender.sse.AuditTaskEventService;
import com.ithsd.smart_tender.sse.ReplaySseEvent;
import com.ithsd.smart_tender.sse.SseHub;
import lombok.extern.slf4j.Slf4j;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;
import org.springframework.transaction.support.TransactionSynchronization;
import org.springframework.transaction.support.TransactionSynchronizationManager;
import org.springframework.util.StringUtils;
import org.springframework.web.servlet.mvc.method.annotation.SseEmitter;

import java.time.DayOfWeek;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.format.DateTimeFormatter;
import java.time.temporal.TemporalAdjusters;
import java.util.*;
import java.util.stream.Collectors;

/**
 * 审核任务管理 — 委托 Rust Multi-Agent 引擎执行。
 * <p>getResult() 优先从 Rust 内存取，Rust 重启后回退到 audit_issue 表。</p>
 */
@Service
@Slf4j
public class AuditTaskServiceImpl implements AuditTaskService {

    private final AuditTaskMapper auditTaskMapper;
    private final AuditIssueMapper auditIssueMapper;
    private final TenderService tenderService;
    private final AuditTaskDispatcher taskDispatcher;
    private final AuditTaskEventService eventService;
    private final SseHub sseHub;
    private final TenderMapper tenderMapper;
    private final KnowledgeFileMapper knowledgeFileMapper;
    private final RustApiClient rustApiClient;

    /** DB 回退反序列化 bbox JSON 用；与 RustApiClient / AuditEngineServiceImpl 同为 SNAKE_CASE */
    private static final ObjectMapper JSON = new ObjectMapper()
            .setPropertyNamingStrategy(PropertyNamingStrategies.SNAKE_CASE);

    public AuditTaskServiceImpl(
            AuditTaskMapper auditTaskMapper, AuditIssueMapper auditIssueMapper,
            TenderService tenderService,
            AuditTaskDispatcher taskDispatcher, AuditTaskEventService eventService,
            SseHub sseHub, TenderMapper tenderMapper, KnowledgeFileMapper knowledgeFileMapper,
            RustApiClient rustApiClient
    ) {
        this.auditTaskMapper = auditTaskMapper;
        this.auditIssueMapper = auditIssueMapper;
        this.tenderService = tenderService;
        this.taskDispatcher = taskDispatcher;
        this.eventService = eventService;
        this.sseHub = sseHub;
        this.tenderMapper = tenderMapper;
        this.knowledgeFileMapper = knowledgeFileMapper;
        this.rustApiClient = rustApiClient;
    }

    @Override
    @Transactional
    public AuditTaskCreateVO createTask(CreateAuditTaskRequest request) {
        Long tenantId = TenantScope.requiredTenantId();
        Tender tender = tenderMapper.selectOne(new LambdaQueryWrapper<Tender>()
                .eq(Tender::getId, request.getBidId())
                .eq(Tender::getTenantId, tenantId));
        if (tender == null) {
            throw TenantScope.resourceNotFound();
        }
        // 去重：同一标书若已有「进行中/待调度」的审核任务，直接复用该任务，
        // 避免前端重复点击 / 重复提交制造出多个并发任务，进而在 Rust 侧撞
        // 「该文档已有进行中的审核任务」冲突而被快速失败。
        AuditTask inProgress = findInProgressTaskByBidId(request.getBidId(), tenantId);
        if (inProgress != null) {
            log.info("重复提交已拦截：同一标书已有进行中的审核任务，复用 taskId={}, bidId={}",
                    inProgress.getTaskId(), request.getBidId());
            return new AuditTaskCreateVO(inProgress.getTaskId());
        }
        LocalDateTime now = LocalDateTime.now();
        AuditTask entity = AuditTask.builder()
                .tenantId(tenantId)
                .taskId(buildTaskId())
                .bidId(request.getBidId())
                .taskStatus(AuditTaskStatusEnum.PENDING.getCode())
                .stage(AuditStageEnum.UPLOADING.name())
                .progress(0)
                .enabledChecks(request.getEnabledAgents() != null ? request.getEnabledAgents() : new ArrayList<>())
                .failedStages(new ArrayList<>())
                .createTime(now)
                .updatedAt(now)
                .build();
        Long auditUserId = BaseContext.getCurrentId();
        if (auditUserId == null) {
            auditUserId = tender.getUploadUserId();
        }
        entity.setAuditUserId(auditUserId);
        auditTaskMapper.insert(entity);

        // 必须在当前事务提交后再 dispatch，否则 @Async 线程查不到刚插入的 task
        final String taskId = entity.getTaskId();
        final AuditTaskEnvelope envelope = AuditTaskEnvelope.capture(taskId);
        TransactionSynchronizationManager.registerSynchronization(new TransactionSynchronization() {
            @Override
            public void afterCommit() {
                taskDispatcher.dispatch(envelope);
            }
        });
        return new AuditTaskCreateVO(entity.getTaskId());
    }

    @Override
    @Transactional(readOnly = true)
    public AuditTaskStatusVO getStatus(String taskId) {
        AuditTask task = loadTask(taskId);
        return buildStatusVO(task);
    }

    @Override
    @Transactional(readOnly = true)
    public AuditTaskStatusVO getStatusByBid(Long bidId) {
        Tender tender = tenderMapper.selectOne(new LambdaQueryWrapper<Tender>()
                .eq(Tender::getId, bidId)
                .eq(Tender::getTenantId, TenantScope.requiredTenantId()));
        if (tender == null) {
            throw TenantScope.resourceNotFound();
        }
        AuditTask task = findLatestTaskByBidId(bidId);
        if (task == null) {
            // 该文档从未发起过审核：返回空任务标识，前端据此显示「准备审核」
            AuditTaskStatusVO vo = new AuditTaskStatusVO();
            vo.setStatus("pending");
            vo.setStage(null);
            vo.setProgress(0);
            vo.setIssueCount(0);
            vo.setFailedStages(List.of());
            fillCounts(vo);
            return vo;
        }
        // 复用 loadTask 的归属校验：任务创建者或标书上传者
        Long currentUserId = BaseContext.getCurrentId();
        if (currentUserId != null && !isTaskOwner(task, currentUserId)) {
            throw new BizException(403, "无权访问该任务");
        }
        return buildStatusVO(task);
    }

    private AuditTaskStatusVO buildStatusVO(AuditTask task) {
        AuditTaskStatusVO vo = new AuditTaskStatusVO();
        vo.setTaskId(task.getTaskId());
        vo.setStatus(AuditTaskStatusEnum.fromCode(task.getTaskStatus()).getValue());
        vo.setStage(task.getStage());
        vo.setProgress(task.getProgress());
        vo.setIssueCount(0); // 不再从 DB 读 issue count，前端从 /result 获取
        vo.setFailedStages(task.getFailedStages() == null ? List.of() : task.getFailedStages());
        fillCounts(vo);
        return vo;
    }

    private void fillCounts(AuditTaskStatusVO vo) {
        Long tenantId = TenantScope.requiredTenantId();
        vo.setTotalFileCount(auditTaskMapper.selectCount(new LambdaQueryWrapper<AuditTask>()
                .eq(AuditTask::getTenantId, tenantId)));
        vo.setPendingFileCount(defLong(auditTaskMapper.selectCount(new LambdaQueryWrapper<AuditTask>()
                .eq(AuditTask::getTenantId, tenantId)
                .eq(AuditTask::getTaskStatus, AuditTaskStatusEnum.PENDING.getCode()))));
        vo.setProcessingFileCount(defLong(auditTaskMapper.selectCount(new LambdaQueryWrapper<AuditTask>()
                .eq(AuditTask::getTenantId, tenantId)
                .eq(AuditTask::getTaskStatus, AuditTaskStatusEnum.PROCESSING.getCode()))));
        vo.setFailedFileCount(defLong(auditTaskMapper.selectCount(new LambdaQueryWrapper<AuditTask>()
                .eq(AuditTask::getTenantId, tenantId)
                .eq(AuditTask::getTaskStatus, AuditTaskStatusEnum.FAILED.getCode()))));
    }

    private AuditTask findLatestTaskByBidId(Long bidId) {
        return auditTaskMapper.selectOne(new LambdaQueryWrapper<AuditTask>()
                .eq(AuditTask::getBidId, bidId)
                .eq(AuditTask::getTenantId, TenantScope.requiredTenantId())
                .orderByDesc(AuditTask::getCreateTime)
                .last("LIMIT 1"));
    }

    /**
     * 查找同一标书下「待调度(PENDING)/进行中(PROCESSING)」的审核任务。
     * 用于 {@link #createTask} 去重：存在则复用，不再新建。
     */
    private AuditTask findInProgressTaskByBidId(Long bidId, Long tenantId) {
        return auditTaskMapper.selectOne(new LambdaQueryWrapper<AuditTask>()
                .eq(AuditTask::getBidId, bidId)
                .eq(AuditTask::getTenantId, tenantId)
                .in(AuditTask::getTaskStatus,
                        AuditTaskStatusEnum.PENDING.getCode(),
                        AuditTaskStatusEnum.PROCESSING.getCode())
                .orderByDesc(AuditTask::getCreateTime)
                .last("LIMIT 1"));
    }

    @Override
    @Transactional(readOnly = true)
    public ResultVO getResult(String taskId, Integer page, Integer size, String sinceIssueNo) {
        AuditTask task = loadTask(taskId);
        boolean completed = AuditTaskStatusEnum.COMPLETED.getCode().equals(task.getTaskStatus());

        // Rust 内存结果仅对「已完成」任务有意义；进行中任务直接走 DB 增量结果（P2）
        RustReviewResponse review = null;
        if (completed) {
            Tender tender = tenderMapper.selectOne(new LambdaQueryWrapper<Tender>()
                    .eq(Tender::getId, task.getBidId())
                    .eq(Tender::getTenantId, TenantScope.requiredTenantId()));
            if (tender != null && StringUtils.hasText(tender.getRustDocumentId())) {
                try {
                    RustReviewResultResponse rustResult =
                        rustApiClient.getReviewResult(tender.getRustDocumentId());
                    if (rustResult != null && rustResult.isCompleted()) {
                        review = rustResult.getResult();
                    }
                } catch (Exception e) {
                    log.info("getResult: Rust unavailable (may have restarted), falling back to DB: {}",
                            e.getMessage());
                }
            }
        }

        // 构建 ResultVO
        ResultVO vo = new ResultVO();
        vo.setTaskId(task.getTaskId());

        List<RustRiskFinding> allFindings;
        if (review != null && review.getFindings() != null) {
            // Rust 内存中有结果 → 直接使用
            allFindings = review.getFindings().stream()
                    .filter(f -> !f.shouldSkip()).collect(Collectors.toList());
        } else {
            // P2: 统一从 audit_issue 读（含进行中的增量结果 + 完成后的最终结果）
            List<AuditIssue> dbIssues = auditIssueMapper.selectList(
                    new LambdaQueryWrapper<AuditIssue>()
                            .eq(AuditIssue::getAuditId, task.getId())
                            .eq(AuditIssue::getTenantId, task.getTenantId()));
            allFindings = dbIssues.stream().map(i -> {
                RustRiskFinding f = new RustRiskFinding();
                f.setRiskId(i.getRiskId() != null ? i.getRiskId() : i.getIssueNo());
                f.setRiskType(i.getCategory());
                f.setSeverity(i.getSeverity());
                f.setCritical(Boolean.TRUE.equals(i.getIsCritical()));
                f.setCriticalReason(i.getCriticalReason());
                f.setReason(i.getDescription());
                f.setSuggestion(i.getSuggestion());
                f.setPageNumber(i.getPageNumber());
                f.setSourceQuote(i.getContext());
                if (i.getReference() != null) {
                    f.setLegalBasis(java.util.List.of(i.getReference().split("; ")));
                }
                f.setSectionPath(i.getSectionName() != null
                        ? java.util.List.of(i.getSectionName().split(" > ")) : null);
                f.setConfidence(i.getConfidence() != null ? i.getConfidence().floatValue() : 0f);
                f.setBlockIds(parseStringList(i.getBlockIds()));
                f.setHighlightRects(parseHighlightRects(i.getHighlightRects()));
                return f;
            }).collect(Collectors.toList());
            log.info("getResult: {} findings from DB for taskId={} (completed={})", allFindings.size(), taskId, completed);
        }

        vo.setAuditResult(!completed ? "pending" : (allFindings.isEmpty() ? "pass" : "revise"));

        // 4 级统计
        SummaryVO summary = buildSummary(allFindings);
        vo.setSummary(summary);

        // RoutingSummary (only when review came from Rust)
        if (review != null && review.getRoutingSummary() != null) {
            SummaryVO routingSummary = new SummaryVO();
            routingSummary.setTotalClauses(review.getRoutingSummary().getTotalClauses());
            routingSummary.setAgentClauseCounts(review.getRoutingSummary().getAgentClauseCounts());
            routingSummary.setLegalVerifyCount(review.getRoutingSummary().getLegalVerifyCount());
            routingSummary.setBlindSpotFindings(review.getRoutingSummary().getBlindSpotFindings());
            vo.setRoutingSummary(routingSummary);
        }

        // GraphSnapshot (only when review came from Rust)
        if (review != null && review.getGraphSnapshot() != null) {
            vo.setGraphSnapshot(review.getGraphSnapshot());
        }

        // Issues → 映射为前端兼容的 IssueVO
        List<IssueVO> issues = allFindings.stream()
                .map(this::toIssueVO)
                .collect(Collectors.toList());
        vo.setIssues(issues);

        return vo;
    }

    @Override
    @Transactional(readOnly = true)
    public SseEmitter subscribeStream(String taskId, String lastEventId) {
        AuditTaskStatusVO statusVO = getStatus(taskId);
        SseEmitter emitter = sseHub.subscribe(taskId);
        if (StringUtils.hasText(lastEventId)) {
            replay(taskId, lastEventId, emitter);
        } else {
            try {
                sseHub.emitToEmitter(emitter, SseEventTypeEnum.PROGRESS, statusVO, null);
            } catch (Exception ignored) {}
        }
        return emitter;
    }

    @Override
    public List<Long> getAuditIdsByBidIds(List<Long> bidIds) {
        if (bidIds.isEmpty()) return List.of();
        LambdaQueryWrapper<AuditTask> qw = new LambdaQueryWrapper<AuditTask>()
                .select(AuditTask::getId)
                .in(AuditTask::getBidId, bidIds)
                .eq(AuditTask::getTenantId, TenantScope.requiredTenantId());
        return auditTaskMapper.selectObjs(qw).stream()
                .filter(Objects::nonNull).filter(obj -> obj instanceof Long)
                .map(obj -> (Long) obj).toList();
    }

    @Override
    public Map<String, Long> countByWeek() {
        Long userId = BaseContext.getCurrentId();
        Map<String, Long> result = new LinkedHashMap<>();
        result.put("Monday", 0L); result.put("Tuesday", 0L); result.put("Wednesday", 0L);
        result.put("Thursday", 0L); result.put("Friday", 0L); result.put("Saturday", 0L);
        result.put("Sunday", 0L);
        if (userId == null) return result;

        List<Long> bidIds = tenderService.getBidIdsByUserId(userId);
        if (bidIds.isEmpty()) return result;

        List<Map<String, Object>> counts = auditTaskMapper.countByWeek(
                TenantScope.requiredTenantId(), bidIds);
        LocalDate today = LocalDate.now();
        LocalDate monday = today.with(TemporalAdjusters.previousOrSame(DayOfWeek.MONDAY));
        DateTimeFormatter fmt = DateTimeFormatter.ofPattern("yyyy-MM-dd");
        Map<String, String> dateToDay = new HashMap<>();
        for (int i = 0; i < 7; i++) {
            LocalDate d = monday.plusDays(i);
            String day = d.getDayOfWeek().name();
            day = day.substring(0, 1).toUpperCase() + day.substring(1).toLowerCase();
            dateToDay.put(d.format(fmt), day);
        }
        for (Map<String, Object> map : counts) {
            String date = (String) map.get("day_date");
            Long count = ((Number) map.get("count")).longValue();
            String dayName = dateToDay.get(date);
            if (dayName != null) result.put(dayName, count);
        }
        return result;
    }

    @Override
    @Transactional
    public void markTaskProcessing(String taskId) {
        AuditTask task;
        try { task = loadTask(taskId); } catch (TenantAuthException e) {
            throw e;
        } catch (Exception e) {
            log.warn("markTaskProcessing: task not found taskId={}", taskId); return;
        }
        if (AuditTaskStatusEnum.COMPLETED.getCode().equals(task.getTaskStatus())
                || AuditTaskStatusEnum.FAILED.getCode().equals(task.getTaskStatus())) return;
        task.setTaskStatus(AuditTaskStatusEnum.PROCESSING.getCode());
        task.setStage(AuditStageEnum.UPLOADING.name());
        if (task.getProgress() == null || task.getProgress() < 5) task.setProgress(5);
        if (task.getStartTime() == null) task.setStartTime(LocalDateTime.now());
        task.setUpdatedAt(LocalDateTime.now());
        auditTaskMapper.update(task, new LambdaQueryWrapper<AuditTask>()
                .eq(AuditTask::getId, task.getId())
                .eq(AuditTask::getTenantId, TenantScope.requiredTenantId()));
        emitSafe(taskId, SseEventTypeEnum.PROGRESS, getStatus(taskId));
    }

    @Override
    @Transactional
    public void markTaskFailed(String taskId, String errorMessage) {
        AuditTask task;
        try { task = loadTask(taskId); } catch (TenantAuthException e) {
            throw e;
        } catch (Exception e) {
            log.warn("markTaskFailed: task not found taskId={}", taskId); return;
        }
        if (AuditTaskStatusEnum.COMPLETED.getCode().equals(task.getTaskStatus())) return;
        task.setTaskStatus(AuditTaskStatusEnum.FAILED.getCode());
        task.setErrorMsg(StringUtils.hasText(errorMessage) ? errorMessage : "触发审核失败");
        if (task.getStage() == null) task.setStage(AuditStageEnum.UPLOADING.name());
        if (task.getProgress() == null) task.setProgress(0);
        task.setEndTime(LocalDateTime.now());
        task.setUpdatedAt(LocalDateTime.now());
        auditTaskMapper.update(task, new LambdaQueryWrapper<AuditTask>()
                .eq(AuditTask::getId, task.getId())
                .eq(AuditTask::getTenantId, TenantScope.requiredTenantId()));
        emitSafe(taskId, SseEventTypeEnum.PROGRESS, getStatus(taskId));
    }

    @Override
    @Transactional
    public void processAuditResult(String taskId, String responseBody) {
        // 旧 RAG 回调路径已废弃 — Rust 引擎直接在 AuditEngineServiceImpl 中处理结果
        log.warn("processAuditResult called (deprecated path), taskId={}", taskId);
    }

    // ── 私有方法 ────────────────────────────────────────────────────

    private void replay(String taskId, String lastEventId, SseEmitter emitter) {
        for (ReplaySseEvent ev : eventService.replay(taskId, lastEventId)) {
            try { sseHub.emitToEmitter(emitter, ev.getEventType(), ev.getData(), ev.getEventId()); }
            catch (Exception ignored) { break; }
        }
    }

    private void emitSafe(String taskId, SseEventTypeEnum eventType, Object payload) {
        try {
            String eventId = eventService.persist(taskId, eventType, payload);
            sseHub.emit(taskId, eventType, payload, eventId);
        } catch (Exception ex) {
            log.warn("emit sse failed: taskId={}, event={}", taskId, eventType.getEventName(), ex);
        }
    }

    private AuditTask loadTask(String taskId) {
        AuditTask task = auditTaskMapper.selectOne(
                new LambdaQueryWrapper<AuditTask>()
                        .eq(AuditTask::getTaskId, taskId)
                        .eq(AuditTask::getTenantId, TenantScope.requiredTenantId()));
        if (task == null) {
            throw TenantScope.resourceNotFound();
        }
        // 验证资源归属：只有任务创建者或标书上传者才能访问
        Long currentUserId = BaseContext.getCurrentId();
        if (currentUserId != null && !isTaskOwner(task, currentUserId)) {
            throw new BizException(403, "无权访问该任务");
        }
        return task;
    }

    /**
     * 判断当前用户是否为任务所有者（任务创建者 或 关联标书的上传者）。
     */
    private boolean isTaskOwner(AuditTask task, Long userId) {
        if (userId.equals(task.getAuditUserId())) {
            return true;
        }
        if (task.getBidId() != null) {
            Tender tender = tenderMapper.selectOne(new LambdaQueryWrapper<Tender>()
                    .eq(Tender::getId, task.getBidId())
                    .eq(Tender::getTenantId, TenantScope.requiredTenantId()));
            if (tender != null && userId.equals(tender.getUploadUserId())) {
                return true;
            }
        }
        return false;
    }

    private String buildTaskId() {
        String random = UUID.randomUUID().toString().replace("-", "").substring(0, 8);
        return "task_" + System.currentTimeMillis() + "_" + random;
    }

    // ── 映射 ────────────────────────────────────────────────────────

    private static List<String> parseStringList(String json) {
        if (!StringUtils.hasText(json)) return new ArrayList<>();
        try {
            return JSON.readValue(json, new TypeReference<List<String>>() {});
        } catch (Exception e) {
            log.warn("parse blockIds JSON failed: {}", e.getMessage());
            return new ArrayList<>();
        }
    }

    private static List<RustRiskFinding.RustHighlightRect> parseHighlightRects(String json) {
        if (!StringUtils.hasText(json)) return new ArrayList<>();
        try {
            return JSON.readValue(json, new TypeReference<List<RustRiskFinding.RustHighlightRect>>() {});
        } catch (Exception e) {
            log.warn("parse highlightRects JSON failed: {}", e.getMessage());
            return new ArrayList<>();
        }
    }

    /**
     * 从 RustRiskFinding 直接映射为前端 IssueVO（不再经过 DB Entity）。
     */
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

        // 定位
        IssueVO.LocationVO loc = new IssueVO.LocationVO();
        if (f.getPageNumber() != null) loc.setPageNumber(f.getPageNumber());
        if (f.getSectionPath() != null && !f.getSectionPath().isEmpty())
            loc.setSectionName(String.join(" > ", f.getSectionPath()));
        loc.setContext(f.getContext() != null ? f.getContext() : f.getSourceQuote());
        vo.setLocation(loc);

        // 法规引用
        if (f.getLegalBasis() != null && !f.getLegalBasis().isEmpty())
            vo.setReference(String.join("; ", f.getLegalBasis()));

        // 锚定
        if (f.getPageNumber() != null) {
            vo.setAnchorPage(f.getPageNumber());
        }
        if (f.getSectionPath() != null && !f.getSectionPath().isEmpty()) {
            vo.setAnchorSection(String.join(" > ", f.getSectionPath()));
        }

        // ── Rust 新增字段 ──
        vo.setNoRisk(f.isNoRisk());
        vo.setInitialTier(f.getInitialTier());
        vo.setFinalTier(f.getFinalTier());
        vo.setTierEscalated(f.isTierEscalated());
        vo.setTruncated(f.isTruncated());
        vo.setClauseIds(f.getClauseIds());
        vo.setBlockIds(f.getBlockIds());
        // 词级精确高亮矩形（非空时前端跳过段落级 block 与文本层收敛，直接渲染）
        if (f.getHighlightRects() != null && !f.getHighlightRects().isEmpty()) {
            List<IssueVO.HighlightRectVO> rects = f.getHighlightRects().stream().map(r -> {
                IssueVO.HighlightRectVO rv = new IssueVO.HighlightRectVO();
                rv.setPage(r.getPage());
                rv.setX0(r.getX0());
                rv.setTop(r.getTop());
                rv.setX1(r.getX1());
                rv.setBottom(r.getBottom());
                rv.setPageWidth(r.getPageWidth());
                return rv;
            }).collect(Collectors.toList());
            vo.setHighlightRects(rects);
        }
        vo.setAgent(f.getAgent());

        // Citations
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

        // SuggestedAgent
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

    /**
     * 从 findings 直接构建 4 级统计 SummaryVO。
     */
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

    private Long defLong(Long v) { return v == null ? 0L : v; }

    @Override
    public List<RustBlockBBoxResponse> getBlockBboxes(String taskId, String blockIds) {
        AuditTask task = loadTask(taskId);
        Tender tender = tenderMapper.selectOne(new LambdaQueryWrapper<Tender>()
                .eq(Tender::getId, task.getBidId())
                .eq(Tender::getTenantId, TenantScope.requiredTenantId()));
        if (tender == null || !StringUtils.hasText(tender.getRustDocumentId())) {
            log.warn("getBlockBboxes: tender not found or no rustDocumentId, taskId={}, bidId={}",
                    taskId, task.getBidId());
            return List.of();
        }
        log.info("getBlockBboxes: taskId={}, rustDocId={}, blockIds={}",
                taskId, tender.getRustDocumentId(), blockIds);
        List<RustBlockBBoxResponse> result = rustApiClient.getBlockBboxes(
                tender.getRustDocumentId(), blockIds);
        log.info("getBlockBboxes: returned {} bbox entries", result.size());
        return result;
    }
}
