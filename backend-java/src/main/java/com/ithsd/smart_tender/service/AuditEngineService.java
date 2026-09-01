package com.ithsd.smart_tender.service;

import com.ithsd.smart_tender.service.engine.queue.AuditTaskEnvelope;

public interface AuditEngineService {
    void start(AuditTaskEnvelope envelope);

    /**
     * 从 Rust 已完成的结果恢复 Java 侧孤儿任务，不重新执行模型审核。
     *
     * <p>返回枚举而非布尔，供孤儿守护区分「仍可等」与「应判死」。</p>
     */
    RecoverOutcome recover(String taskId);

    /**
     * 将孤儿任务标记为失败并推送 SSE 完成事件（幂等）。
     * 仅处理 PENDING / PROCESSING 状态的任务，已完成/已失败的任务不动。
     */
    void failOrphan(String taskId);

    /** 任务对账结果。 */
    enum RecoverOutcome {
        /** 已从 Rust 结果恢复完成（或本已 COMPLETED） */
        COMPLETED,
        /** Rust 侧已失败，任务已标记 FAILED */
        FAILED,
        /** Rust 审核仍在进行，应继续等待 */
        STILL_RUNNING,
        /** Rust 暂不可达（连接失败），下次守护再试 */
        UNREACHABLE,
        /** Rust 无该审核任何记录（引擎重启后状态蒸发），应判失败 */
        STATE_LOST,
        /** 任务不存在（已被删除/租户不符） */
        NOT_FOUND
    }
}
