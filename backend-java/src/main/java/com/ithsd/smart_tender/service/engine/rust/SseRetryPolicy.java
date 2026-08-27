package com.ithsd.smart_tender.service.engine.rust;

/**
 * Java→Rust SSE 断线重连策略（纯逻辑，便于单测）。
 *
 * <p>第 n 次重连前的退避延迟 = min(maxDelay, baseDelay × 2^(n-1))，
 * 最多允许 maxAttempts 次尝试（含首次连接）。</p>
 */
public final class SseRetryPolicy {

    private final int maxAttempts;
    private final long baseDelayMs;
    private final long maxDelayMs;

    public SseRetryPolicy(int maxAttempts, long baseDelayMs, long maxDelayMs) {
        if (maxAttempts < 0) {
            throw new IllegalArgumentException("maxAttempts must be >= 0");
        }
        if (baseDelayMs < 0 || maxDelayMs < 0) {
            throw new IllegalArgumentException("delay values must be >= 0");
        }
        this.maxAttempts = maxAttempts;
        this.baseDelayMs = baseDelayMs;
        this.maxDelayMs = maxDelayMs;
    }

    /** 默认策略：最多 5 次尝试，退避 1s→2s→4s→8s→16s。 */
    public static SseRetryPolicy defaults() {
        return new SseRetryPolicy(5, 1000, 30_000);
    }

    /** 第 attempt 次尝试是否被允许（1-based）。 */
    public boolean allowRetry(int attempt) {
        return attempt >= 1 && attempt <= maxAttempts;
    }

    /** 第 attempt 次尝试失败后、下一次尝试前的退避毫秒数（1-based）。 */
    public long delayForAttempt(int attempt) {
        if (attempt < 1) {
            return 0;
        }
        long delay = baseDelayMs * (1L << Math.min(attempt - 1, 30));
        return Math.min(delay, maxDelayMs);
    }
}
