package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.baomidou.mybatisplus.extension.plugins.pagination.Page;
import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.ithsd.smart_tender.mapper.AuditTaskMapper;
import com.ithsd.smart_tender.mapper.TraceEventBlockMapper;
import com.ithsd.smart_tender.mapper.TraceEventMapper;
import com.ithsd.smart_tender.mapper.TraceSessionMapper;
import com.ithsd.smart_tender.model.entity.AuditTask;
import com.ithsd.smart_tender.model.entity.TraceEventBlock;
import com.ithsd.smart_tender.model.entity.TraceEventEntity;
import com.ithsd.smart_tender.model.entity.TraceSession;
import com.ithsd.smart_tender.model.result.PageResult;
import com.ithsd.smart_tender.model.vo.TraceEventVO;
import com.ithsd.smart_tender.model.vo.TraceSessionDetailVO;
import com.ithsd.smart_tender.model.vo.TraceSessionVO;
import com.ithsd.smart_tender.service.TraceService;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Propagation;
import org.springframework.transaction.annotation.Transactional;

import java.time.LocalDateTime;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;

/**
 * 审查追溯服务实现 — Session 生命周期管理 + 事件持久化 + 查询。
 *
 * <p>使用 REQUIRES_NEW 事务传播：SSE 回调线程独立于主审核事务，
 * 每条 trace 事件立即提交，避免丢失。</p>
 */
@Service
public class TraceServiceImpl implements TraceService {

    private static final Logger log = LoggerFactory.getLogger(TraceServiceImpl.class);

    private final TraceSessionMapper sessionMapper;
    private final TraceEventMapper eventMapper;
    private final TraceEventBlockMapper blockMapper;
    private final AuditTaskMapper auditTaskMapper;
    private final ObjectMapper jsonMapper = new ObjectMapper();

    public TraceServiceImpl(
            TraceSessionMapper sessionMapper,
            TraceEventMapper eventMapper,
            TraceEventBlockMapper blockMapper,
            AuditTaskMapper auditTaskMapper) {
        this.sessionMapper = sessionMapper;
        this.eventMapper = eventMapper;
        this.blockMapper = blockMapper;
        this.auditTaskMapper = auditTaskMapper;
    }

    // ── 摄入 ──────────────────────────────────────────────────

    @Override
    @Transactional(propagation = Propagation.REQUIRES_NEW)
    public void ingestTraceEvent(String taskId, String docId, TraceEventVO vo) {
        if (vo.getAgentName() == null || vo.getClauseId() == null) {
            // 没有 clause 关联的 trace 事件（如全局 turn_start）暂不建立 session
            return;
        }

        // 1. 查找或创建 session
        TraceSession session = findOrCreateSession(taskId, docId, vo);

        // 2. 插入 trace_event
        String eventId = UUID.randomUUID().toString();
        TraceEventEntity event = new TraceEventEntity();
        event.setEventId(eventId);
        event.setSessionId(session.getId());
        event.setAgentName(vo.getAgentName());
        event.setEventType(vo.getEventType());
        event.setTurn(vo.getTurn());
        event.setTimestamp(parseTimestamp(vo.getTimestamp()));
        event.setClauseId(vo.getClauseId());
        event.setRiskId(vo.getRiskId());
        event.setSummary(vo.getSummary() != null ? vo.getSummary() : "");
        event.setPayload(toJsonString(vo.getPayload()));
        event.setCreatedAt(LocalDateTime.now());
        eventMapper.insert(event);

        // 3. 更新 session 统计
        session.setEventCount(session.getEventCount() + 1);
        if (session.getTotalTurns() < vo.getTurn()) {
            session.setTotalTurns(vo.getTurn());
        }
        if ("tool_call".equals(vo.getEventType())) {
            session.setTotalToolCalls(session.getTotalToolCalls() + 1);
        }
        if ("tool_result".equals(vo.getEventType())) {
            session.setTotalSearchCalls(session.getTotalSearchCalls() + 1);
        }
        if ("output_finding".equals(vo.getEventType())) {
            extractFinding(session, vo);
        }
        if ("agent_bus_send".equals(vo.getEventType())) {
            session.setTotalToolCalls(session.getTotalToolCalls() + 1);
        }
        sessionMapper.updateById(session);

        // 4. 写入 block_ids 关联（预留：当前 Rust SSE 不携带 block_ids）
        // 未来从 payload.block_ids 或 vo 扩展字段提取后写入 trace_event_blocks
        log.debug("Trace ingested: session={} event={} type={} turn={} clause={}",
                session.getId(), eventId, vo.getEventType(), vo.getTurn(), vo.getClauseId());
    }

    @Override
    @Transactional(propagation = Propagation.REQUIRES_NEW)
    public void markSessionsCompleted(String taskId) {
        List<TraceSession> running = sessionMapper.selectList(
                new LambdaQueryWrapper<TraceSession>()
                        .eq(TraceSession::getTaskId, taskId)
                        .eq(TraceSession::getStatus, "running"));
        LocalDateTime now = LocalDateTime.now();
        for (TraceSession s : running) {
            s.setStatus("completed");
            s.setFinishedAt(now);
            sessionMapper.updateById(s);
        }
        log.info("Trace sessions completed: taskId={}, count={}", taskId, running.size());
    }

    // ── 查询 ──────────────────────────────────────────────────

    @Override
    @Transactional(readOnly = true)
    public PageResult listByTaskId(String taskId, String agent, String severity,
                                                    int page, int size) {
        // 验证审核任务属于当前租户
        Long tenantId = TenantScope.requiredTenantId();
        AuditTask task = auditTaskMapper.selectOne(new LambdaQueryWrapper<AuditTask>()
                .eq(AuditTask::getTaskId, taskId)
                .eq(AuditTask::getTenantId, tenantId));
        if (task == null) {
            return new PageResult(0L, List.of());
        }

        LambdaQueryWrapper<TraceSession> q = new LambdaQueryWrapper<TraceSession>()
                .eq(TraceSession::getTaskId, taskId);
        if (agent != null && !agent.isBlank()) {
            q.eq(TraceSession::getAgentName, agent);
        }
        if (severity != null && !severity.isBlank()) {
            q.eq(TraceSession::getSeverity, severity);
        }
        q.orderByDesc(TraceSession::getStartedAt);

        Page<TraceSession> mpPage = sessionMapper.selectPage(new Page<>(page, size), q);
        List<TraceSessionVO> vos = new ArrayList<>();
        for (TraceSession s : mpPage.getRecords()) {
            vos.add(toSessionVO(s));
        }
        return new PageResult(mpPage.getTotal(), vos);
    }

    @Override
    @Transactional(readOnly = true)
    public TraceSessionDetailVO getSessionDetail(String sessionId) {
        TraceSession session = sessionMapper.selectById(sessionId);
        if (session == null) {
            return null;
        }

        // 验证 session 关联的审核任务属于当前租户
        Long tenantId = TenantScope.requiredTenantId();
        if (session.getTaskId() != null) {
            AuditTask task = auditTaskMapper.selectOne(new LambdaQueryWrapper<AuditTask>()
                    .eq(AuditTask::getTaskId, session.getTaskId())
                    .eq(AuditTask::getTenantId, tenantId));
            if (task == null) {
                return null;
            }
        }

        List<TraceEventEntity> entities = eventMapper.selectList(
                new LambdaQueryWrapper<TraceEventEntity>()
                        .eq(TraceEventEntity::getSessionId, sessionId)
                        .orderByAsc(TraceEventEntity::getTurn)
                        .orderByAsc(TraceEventEntity::getId));

        List<TraceEventVO> events = new ArrayList<>();
        for (TraceEventEntity e : entities) {
            TraceEventVO vo = new TraceEventVO();
            vo.setEventId(e.getEventId());
            vo.setSessionId(e.getSessionId());
            vo.setEventType(e.getEventType());
            vo.setAgentName(e.getAgentName());
            vo.setTurn(e.getTurn());
            vo.setClauseId(e.getClauseId());
            vo.setRiskId(e.getRiskId());
            vo.setSummary(e.getSummary());
            vo.setPayload(parseJson(e.getPayload()));
            vo.setTimestamp(e.getTimestamp() != null ? e.getTimestamp().toString() : null);
            events.add(vo);
        }

        TraceSessionDetailVO detail = new TraceSessionDetailVO();
        detail.setSession(toSessionVO(session));
        detail.setEvents(events);
        return detail;
    }

    // ── 内部辅助 ──────────────────────────────────────────────

    /** 按 (task_id, agent_name, clause_id) 查 session，不存在则创建。 */
    private TraceSession findOrCreateSession(String taskId, String docId, TraceEventVO vo) {
        List<TraceSession> existing = sessionMapper.selectList(
                new LambdaQueryWrapper<TraceSession>()
                        .eq(TraceSession::getTaskId, taskId)
                        .eq(TraceSession::getAgentName, vo.getAgentName())
                        .eq(TraceSession::getClauseId, vo.getClauseId())
                        .last("LIMIT 1"));
        if (!existing.isEmpty()) {
            return existing.get(0);
        }

        TraceSession session = new TraceSession();
        session.setId(UUID.randomUUID().toString());
        session.setTaskId(taskId);
        session.setDocId(docId);
        session.setAgentName(vo.getAgentName());
        session.setClauseId(vo.getClauseId());
        session.setInitialTier("L2");
        session.setFinalTier("L2");
        session.setTierEscalated(0);
        session.setStatus("running");
        session.setTotalTurns(0);
        session.setTotalToolCalls(0);
        session.setTotalSearchCalls(0);
        session.setEventCount(0);
        session.setStartedAt(LocalDateTime.now());
        sessionMapper.insert(session);
        return session;
    }

    /** 从 output_finding 的 payload 中提取 risk_id / severity / confidence。 */
    private void extractFinding(TraceSession session, TraceEventVO vo) {
        JsonNode payload = vo.getPayload();
        if (payload == null) return;

        if (payload.has("risk_id")) {
            session.setRiskId(payload.get("risk_id").asText());
            vo.setRiskId(session.getRiskId());  // 回填到 VO
        }
        if (payload.has("severity")) {
            session.setSeverity(payload.get("severity").asText());
        }
        if (payload.has("confidence")) {
            session.setConfidence(payload.get("confidence").asDouble());
        }
    }

    private TraceSessionVO toSessionVO(TraceSession s) {
        TraceSessionVO vo = new TraceSessionVO();
        vo.setSessionId(s.getId());
        vo.setAgentName(s.getAgentName());
        vo.setClauseId(s.getClauseId());
        vo.setRiskId(s.getRiskId());
        vo.setSeverity(s.getSeverity());
        vo.setConfidence(s.getConfidence());
        vo.setInitialTier(s.getInitialTier());
        vo.setFinalTier(s.getFinalTier());
        vo.setTotalTurns(s.getTotalTurns());
        vo.setEventCount(s.getEventCount());
        vo.setStatus(s.getStatus());
        vo.setStartedAt(s.getStartedAt());
        vo.setFinishedAt(s.getFinishedAt());
        // summary: 取 risk_id + severity 组合作为卡片摘要
        if (s.getRiskId() != null) {
            vo.setSummary((s.getSeverity() != null ? s.getSeverity() : "") + " | " + s.getRiskId());
        }
        return vo;
    }

    private LocalDateTime parseTimestamp(String ts) {
        if (ts == null || ts.isBlank()) return LocalDateTime.now();
        try {
            return LocalDateTime.parse(ts.replace("Z", ""));
        } catch (Exception e) {
            return LocalDateTime.now();
        }
    }

    private String toJsonString(JsonNode node) {
        if (node == null) return null;
        try {
            return jsonMapper.writeValueAsString(node);
        } catch (JsonProcessingException e) {
            return null;
        }
    }

    private JsonNode parseJson(String json) {
        if (json == null || json.isBlank()) return null;
        try {
            return jsonMapper.readTree(json);
        } catch (JsonProcessingException e) {
            return null;
        }
    }
}
