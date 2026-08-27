package com.ithsd.smart_tender.service.impl;

import com.ithsd.smart_tender.common.BizException;
import com.ithsd.smart_tender.model.dto.rust.RustReviewResponse;
import com.ithsd.smart_tender.model.dto.rust.RustReviewResultResponse;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.*;

/**
 * F2：审核结果轮询的快速失败语义（TDD 先行）。
 *
 * <p>轮询单次结果的处置由纯静态方法 {@code classifyPollResult} 决定，
 * 无需网络/线程池即可完整测试。</p>
 */
class AuditEnginePollDecisionTest {

    // ── 已启动审核：404 = 状态丢失 → 快速失败 ─────────────────────

    @Test
    void nullResultMeansStateLost_failsFast() {
        BizException ex = assertThrows(
                BizException.class,
                () -> AuditEngineServiceImpl.classifyPollResult(null));
        assertEquals(5705, ex.getCode());
        assertTrue(
                ex.getMessage().contains("状态丢失"),
                "应明确说明审核状态丢失: " + ex.getMessage());
    }

    // ── failed 结果透传 Rust 原始错误 ─────────────────────────────

    @Test
    void failedResultCarriesRustError() {
        RustReviewResultResponse failed = new RustReviewResultResponse();
        failed.setStatus("failed");
        failed.setError("审核引擎执行失败: LLM 超时");

        BizException ex = assertThrows(
                BizException.class,
                () -> AuditEngineServiceImpl.classifyPollResult(failed));
        assertEquals(5705, ex.getCode());
        assertTrue(
                ex.getMessage().contains("LLM 超时"),
                "应透传 Rust 原始失败原因: " + ex.getMessage());
    }

    @Test
    void failedResultWithNullErrorStillFails() {
        RustReviewResultResponse failed = new RustReviewResultResponse();
        failed.setStatus("failed");

        BizException ex = assertThrows(
                BizException.class,
                () -> AuditEngineServiceImpl.classifyPollResult(failed));
        assertEquals(5705, ex.getCode());
        assertFalse(ex.getMessage().isBlank());
    }

    // ── completed / partial_failed → 返回结果 ─────────────────────

    @Test
    void completedResultReturnsResponse() {
        RustReviewResultResponse completed = new RustReviewResultResponse();
        completed.setStatus("completed");
        RustReviewResponse response = new RustReviewResponse();
        completed.setResult(response);

        assertSame(response, AuditEngineServiceImpl.classifyPollResult(completed));
    }

    @Test
    void partialFailedWithResultReturnsResponse() {
        RustReviewResultResponse partial = new RustReviewResultResponse();
        partial.setStatus("partial_failed");
        RustReviewResponse response = new RustReviewResponse();
        partial.setResult(response);

        assertSame(response, AuditEngineServiceImpl.classifyPollResult(partial));
    }

    @Test
    void partialFailedWithoutResultFails() {
        RustReviewResultResponse partial = new RustReviewResultResponse();
        partial.setStatus("partial_failed");

        BizException ex = assertThrows(
                BizException.class,
                () -> AuditEngineServiceImpl.classifyPollResult(partial));
        assertEquals(5705, ex.getCode());
    }

    // ── pending → 继续等待 ───────────────────────────────────────

    @Test
    void pendingResultKeepsWaiting() {
        RustReviewResultResponse pending = new RustReviewResultResponse();
        pending.setStatus("pending");

        assertNull(
                AuditEngineServiceImpl.classifyPollResult(pending),
                "pending 表示审核仍在进行，应继续轮询");
    }
}
