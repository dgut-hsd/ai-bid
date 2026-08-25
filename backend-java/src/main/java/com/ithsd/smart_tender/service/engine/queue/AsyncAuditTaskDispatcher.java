package com.ithsd.smart_tender.service.engine.queue;

import com.ithsd.smart_tender.service.AuditEngineService;
import org.springframework.boot.autoconfigure.condition.ConditionalOnProperty;
import org.springframework.stereotype.Component;

@Component
@ConditionalOnProperty(prefix = "audit.queue", name = "mode", havingValue = "async", matchIfMissing = true)
public class AsyncAuditTaskDispatcher implements AuditTaskDispatcher {
    private final AuditEngineService auditEngineService;

    public AsyncAuditTaskDispatcher(AuditEngineService auditEngineService) {
        this.auditEngineService = auditEngineService;
    }

    @Override
    public void dispatch(AuditTaskEnvelope envelope) {
        auditEngineService.start(envelope);
    }
}
