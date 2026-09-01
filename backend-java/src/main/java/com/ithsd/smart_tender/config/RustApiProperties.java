package com.ithsd.smart_tender.config;

import org.springframework.boot.context.properties.ConfigurationProperties;
import org.springframework.stereotype.Component;

/**
 * Rust 审核引擎 HTTP API 配置。
 *
 * <p>对应 application.yml 中的 {@code rust.api} 前缀。</p>
 */
@Component
@ConfigurationProperties(prefix = "rust.api")
public class RustApiProperties {

    /** Rust HTTP 服务地址，默认 {@code http://127.0.0.1:3001} */
    private String baseUrl = "http://127.0.0.1:3001";

    /** 连接超时（毫秒），默认 5000 */
    private int connectTimeoutMs = 5000;

    /** 读取超时（毫秒），默认 300000（5 分钟，审核管线可能很慢） */
    private int readTimeoutMs = 300_000;

    /** 启动时是否校验 Rust 健康检查，默认 true */
    private boolean healthCheckEnabled = true;

    /** 异步审核等待超时（分钟），默认 60；可由环境变量 RUST_REVIEW_TIMEOUT_MINUTES 覆盖 */
    private int reviewTimeoutMinutes = 60;

    /**
     * 启动审核（POST /review）请求的整体超时（毫秒），默认 30000。
     * 独立于 connectTimeoutMs：审核启动虽然返回 202 很快，但引擎忙碌时
     * 可能超过 5s，不能拿连接超时当请求超时。
     */
    private int reviewStartTimeoutMs = 30_000;

    /** 招标文件云端审核前脱敏模式。当前生产默认 low，可显式设为 off。 */
    private String desensitizationMode = "low";

    /** Java → Rust 内部请求签名密钥；未配置时内部请求必须拒绝发送。 */
    private String internalSecret;

    // ── getters / setters ──────────────────────────────────────────

    public String getBaseUrl() {
        return baseUrl;
    }

    public void setBaseUrl(String baseUrl) {
        this.baseUrl = baseUrl != null ? baseUrl.replaceAll("/+$", "") : null;
    }

    public int getConnectTimeoutMs() {
        return connectTimeoutMs;
    }

    public void setConnectTimeoutMs(int connectTimeoutMs) {
        this.connectTimeoutMs = connectTimeoutMs;
    }

    public int getReadTimeoutMs() {
        return readTimeoutMs;
    }

    public void setReadTimeoutMs(int readTimeoutMs) {
        this.readTimeoutMs = readTimeoutMs;
    }

    public boolean isHealthCheckEnabled() {
        return healthCheckEnabled;
    }

    public void setHealthCheckEnabled(boolean healthCheckEnabled) {
        this.healthCheckEnabled = healthCheckEnabled;
    }

    public int getReviewTimeoutMinutes() {
        return reviewTimeoutMinutes;
    }

    public void setReviewTimeoutMinutes(int reviewTimeoutMinutes) {
        this.reviewTimeoutMinutes = reviewTimeoutMinutes;
    }

    public int getReviewStartTimeoutMs() {
        return reviewStartTimeoutMs;
    }

    public void setReviewStartTimeoutMs(int reviewStartTimeoutMs) {
        this.reviewStartTimeoutMs = reviewStartTimeoutMs;
    }

    public String getDesensitizationMode() {
        return desensitizationMode;
    }

    public void setDesensitizationMode(String desensitizationMode) {
        this.desensitizationMode = desensitizationMode;
    }

    public String getInternalSecret() {
        return internalSecret;
    }

    public void setInternalSecret(String internalSecret) {
        this.internalSecret = internalSecret;
    }

    // ── 派生方法 ──────────────────────────────────────────────────

    /** 构建完整 API URL，自动去除尾部斜杠。 */
    public String apiUrl(String path) {
        String normalized = path.startsWith("/") ? path : "/" + path;
        return baseUrl + normalized;
    }
}
