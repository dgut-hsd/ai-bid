package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.mapper.AuditTaskMapper;
import com.ithsd.smart_tender.model.entity.AuditTask;
import com.ithsd.smart_tender.model.enums.AuditTaskStatusEnum;
import com.ithsd.smart_tender.service.AuditEngineService;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.scheduling.annotation.Scheduled;
import org.springframework.stereotype.Component;

import java.time.LocalDateTime;
import java.util.List;
import java.util.UUID;

/**
 * 孤儿任务守护：周期性对账滞留的 PENDING/PROCESSING 审核任务。
 *
 * <p>触发条件（两个都要满足）：
 * <ol>
 *   <li>{@code updated_at} 早于 now - staleAfter（长时间无进展）；</li>
 *   <li>不在 {@link RunningTaskRegistry} 中（当前 JVM 没有存活线程在推进它）。</li>
 * </ol>
 *
 * <p>处置：
 * <ul>
 *   <li>PENDING → 直接 {@code failOrphan}（从未开始执行，无法恢复）；</li>
 *   <li>PROCESSING → 先 {@code recover()}：Rust 有结果则恢复完成；
 *       Rust 状态丢失 → 判失败；Rust 仍在跑/不可达 → 留给下一轮。</li>
 * </ul>
 *
 * <p>全程以任务自身的租户上下文执行，结束即清理，避免跨租户串扰。</p>
 */
@Component
public class OrphanAuditTaskSweeper {

    private static final Logger log = LoggerFactory.getLogger(OrphanAuditTaskSweeper.class);

    private final AuditTaskMapper auditTaskMapper;
    private final AuditEngineService auditEngineService;
    private final RunningTaskRegistry runningTaskRegistry;
    private final boolean enabled;
    private final long staleAfterMs;

    public OrphanAuditTaskSweeper(
            AuditTaskMapper auditTaskMapper,
            AuditEngineService auditEngineService,
            RunningTaskRegistry runningTaskRegistry,
            @Value("${audit.orphan.enabled:true}") boolean enabled,
            @Value("${audit.orphan.stale-after-ms:180000}") long staleAfterMs
    ) {
        this.auditTaskMapper = auditTaskMapper;
        this.auditEngineService = auditEngineService;
        this.runningTaskRegistry = runningTaskRegistry;
        this.enabled = enabled;
        this.staleAfterMs = staleAfterMs;
    }

    @Scheduled(fixedDelayString = "${audit.orphan.sweep-interval-ms:60000}")
    public void sweep() {
        if (!enabled) {
            return;
        }
        LocalDateTime cutoff = LocalDateTime.now().minusNanos(staleAfterMs * 1_000_000L);
        List<AuditTask> candidates;
        try {
            candidates = auditTaskMapper.selectList(
                    new LambdaQueryWrapper<AuditTask>()
                            .in(AuditTask::getTaskStatus,
                                    AuditTaskStatusEnum.PENDING.getCode(),
                                    AuditTaskStatusEnum.PROCESSING.getCode())
                            .lt(AuditTask::getUpdatedAt, cutoff));
        } catch (Exception e) {
            log.warn("orphan sweep: query failed, {}", e.getMessage());
            return;
        }
        if (candidates.isEmpty()) {
            return;
        }

        for (AuditTask task : candidates) {
            String runningKey = task.getTenantId() + ":" + task.getTaskId();
            if (runningTaskRegistry.contains(runningKey)) {
                continue;
            }
            processWithTenantContext(task);
        }
    }

    private void processWithTenantContext(AuditTask task) {
        // 守护线程无请求上下文；按任务自身的租户构造一次性上下文。
        TenantRequestContext context = new TenantRequestContext(
                task.getAuditUserId() != null && task.getAuditUserId() > 0
                        ? task.getAuditUserId() : 1L,
                task.getTenantId(),
                "system",
                1L,
                "orphan-sweep-" + UUID.randomUUID());
        TenantContext.set(context);
        try {
            process(task);
        } catch (Exception e) {
            log.warn("orphan sweep: process failed, taskId={}, {}", task.getTaskId(), e.getMessage());
        } finally {
            TenantContext.clear();
        }
    }

    private void process(AuditTask task) {
        String taskId = task.getTaskId();
        Integer status = task.getTaskStatus();
        if (AuditTaskStatusEnum.PENDING.getCode().equals(status)) {
            log.warn("orphan sweep: PENDING task never started, taskId={}", taskId);
            auditEngineService.failOrphan(taskId);
            return;
        }

        AuditEngineService.RecoverOutcome outcome = auditEngineService.recover(taskId);
        log.info("orphan sweep: taskId={}, outcome={}", taskId, outcome);
        switch (outcome) {
            case STATE_LOST, NOT_FOUND -> auditEngineService.failOrphan(taskId);
            case COMPLETED, FAILED, STILL_RUNNING, UNREACHABLE -> {
                // 已完成/已失败：无需动作；仍在跑/不可达：留给下一轮守护。
            }
        }
    }
}
