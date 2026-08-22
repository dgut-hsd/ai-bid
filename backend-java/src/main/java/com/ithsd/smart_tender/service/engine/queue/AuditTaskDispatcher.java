package com.ithsd.smart_tender.service.engine.queue;

public interface AuditTaskDispatcher {
    void dispatch(AuditTaskEnvelope envelope);
}
