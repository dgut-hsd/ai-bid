package com.ithsd.smart_tender.sse;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.ithsd.smart_tender.mapper.AuditTaskEventMapper;
import com.ithsd.smart_tender.model.entity.AuditTaskEvent;
import com.ithsd.smart_tender.model.enums.SseEventTypeEnum;
import com.ithsd.smart_tender.service.impl.TenantScope;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.stereotype.Service;
import org.springframework.util.StringUtils;

import java.time.LocalDateTime;
import java.util.ArrayList;
import java.util.List;

@Service
public class AuditTaskEventService {
    private static final Logger log = LoggerFactory.getLogger(AuditTaskEventService.class);
    private static final ObjectMapper OBJECT_MAPPER = new ObjectMapper();
    private final AuditTaskEventMapper eventMapper;
    private final AuditSseProperties sseProperties;

    public AuditTaskEventService(AuditTaskEventMapper eventMapper, AuditSseProperties sseProperties) {
        this.eventMapper = eventMapper;
        this.sseProperties = sseProperties;
    }

    public String persist(String taskId, SseEventTypeEnum eventType, Object payload) {
        if (!StringUtils.hasText(taskId) || eventType == null || payload == null) {
            return null;
        }
        Long tenantId = TenantScope.requiredTenantId();
        try {
            AuditTaskEvent event = new AuditTaskEvent();
            event.setTenantId(tenantId);
            event.setTaskId(taskId);
            event.setEventType(eventType.getEventName());
            event.setEventData(OBJECT_MAPPER.writeValueAsString(payload));
            event.setCreatedAt(LocalDateTime.now());
            eventMapper.insert(event);
            return String.valueOf(event.getId());
        } catch (RuntimeException | JsonProcessingException ex) {
            log.warn("persist task event failed, taskId={}, eventType={}", taskId, eventType.getEventName(), ex);
            return null;
        }
    }

    public List<ReplaySseEvent> replay(String taskId, String lastEventId) {
        List<ReplaySseEvent> events = new ArrayList<>();
        if (!StringUtils.hasText(taskId)) {
            return events;
        }
        long startId = parseLastEventId(lastEventId);
        int limit = Math.max(1, sseProperties.getReplayMaxEvents());
        Long tenantId = TenantScope.requiredTenantId();

        LambdaQueryWrapper<AuditTaskEvent> qw = new LambdaQueryWrapper<AuditTaskEvent>()
                .eq(AuditTaskEvent::getTaskId, taskId)
                .eq(AuditTaskEvent::getTenantId, tenantId)
                .gt(AuditTaskEvent::getId, startId)
                .orderByAsc(AuditTaskEvent::getId)
                .last("LIMIT " + limit);
        List<AuditTaskEvent> entities = eventMapper.selectList(qw);

        for (AuditTaskEvent entity : entities) {
            SseEventTypeEnum eventType = SseEventTypeEnum.fromEventName(entity.getEventType());
            if (eventType == null) {
                continue;
            }
            try {
                ReplaySseEvent replayEvent = new ReplaySseEvent();
                replayEvent.setEventId(String.valueOf(entity.getId()));
                replayEvent.setEventType(eventType);
                replayEvent.setData(OBJECT_MAPPER.readTree(entity.getEventData()));
                events.add(replayEvent);
            } catch (JsonProcessingException ex) {
                log.warn("parse replay event failed, eventId={}", entity.getId(), ex);
            }
        }
        return events;
    }

    private long parseLastEventId(String lastEventId) {
        if (!StringUtils.hasText(lastEventId)) {
            return 0L;
        }
        try {
            return Long.parseLong(lastEventId);
        } catch (NumberFormatException ex) {
            return 0L;
        }
    }
}
