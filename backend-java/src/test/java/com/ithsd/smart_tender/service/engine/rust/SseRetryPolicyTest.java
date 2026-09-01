package com.ithsd.smart_tender.service.engine.rust;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.*;

/**
 * F4：SSE 重连退避策略（纯逻辑，TDD 先行）。
 */
class SseRetryPolicyTest {

    @Test
    void defaultPolicy_allowsFiveAttempts() {
        SseRetryPolicy policy = SseRetryPolicy.defaults();
        assertTrue(policy.allowRetry(1));
        assertTrue(policy.allowRetry(2));
        assertTrue(policy.allowRetry(3));
        assertTrue(policy.allowRetry(4));
        assertTrue(policy.allowRetry(5));
        assertFalse(policy.allowRetry(6), "第 6 次尝试应被拒绝");
    }

    @Test
    void backoffIsExponentialWithBaseDelay() {
        SseRetryPolicy policy = new SseRetryPolicy(5, 1000, 30_000);
        assertEquals(1000, policy.delayForAttempt(1));
        assertEquals(2000, policy.delayForAttempt(2));
        assertEquals(4000, policy.delayForAttempt(3));
        assertEquals(8000, policy.delayForAttempt(4));
        assertEquals(16_000, policy.delayForAttempt(5));
    }

    @Test
    void backoffIsCappedAtMaxDelay() {
        SseRetryPolicy policy = new SseRetryPolicy(10, 1000, 2000);
        assertEquals(2000, policy.delayForAttempt(3));
        assertEquals(2000, policy.delayForAttempt(10));
    }

    @Test
    void zeroAttemptsPolicy_neverRetries() {
        SseRetryPolicy policy = new SseRetryPolicy(0, 1000, 30_000);
        assertFalse(policy.allowRetry(1));
    }

    @Test
    void invalidAttemptNumber_returnsZeroDelay() {
        SseRetryPolicy policy = SseRetryPolicy.defaults();
        assertEquals(0, policy.delayForAttempt(0));
        assertEquals(0, policy.delayForAttempt(-1));
    }
}
