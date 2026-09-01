package com.ithsd.smart_tender.config;

import org.springframework.boot.context.properties.ConfigurationProperties;
import org.springframework.stereotype.Component;

import jakarta.annotation.PostConstruct;
import java.util.Set;

/**
 * JWT settings are bound from configuration so application code never owns a
 * production signing secret or token lifetime.
 */
@Component
@ConfigurationProperties(prefix = "security.jwt")
public class JwtProperties {

    private String secret;
    private long ttlMillis = 86_400_000L;
    /** 会话（Redis）存活时长，也作为 access token 过期后的续期窗口上限。默认 7 天。 */
    private long sessionTtlMillis = 604_800_000L;
    private boolean acceptLegacyUserId;

    public String getSecret() {
        return secret;
    }

    public void setSecret(String secret) {
        this.secret = secret;
    }

    public long getTtlMillis() {
        return ttlMillis;
    }

    public void setTtlMillis(long ttlMillis) {
        this.ttlMillis = ttlMillis;
    }

    public long getSessionTtlMillis() {
        return sessionTtlMillis;
    }

    public void setSessionTtlMillis(long sessionTtlMillis) {
        this.sessionTtlMillis = sessionTtlMillis;
    }

    public boolean isAcceptLegacyUserId() {
        return acceptLegacyUserId;
    }

    public void setAcceptLegacyUserId(boolean acceptLegacyUserId) {
        this.acceptLegacyUserId = acceptLegacyUserId;
    }

    /**
     * S1：启动期强校验 JWT 签名密钥，拒绝空值、过短、以及已知的默认/已泄露弱密钥。
     * 该校验让任何遗漏配置或误用弱密钥的部署在启动时即失败，而不是静默运行在可伪造 token 的状态。
     */
    @PostConstruct
    public void validate() {
        if (secret == null || secret.isBlank()) {
            throw new IllegalStateException(
                    "security.jwt.secret (ST_JWT_SECRET) 未配置，拒绝启动（生产环境不允许默认 JWT 密钥）");
        }
        if (secret.length() < 32) {
            throw new IllegalStateException(
                    "security.jwt.secret 长度必须 >= 32 字符，拒绝启动（HS256 需要至少 256 位密钥）");
        }
        Set<String> blocked = Set.of(
                "local-test-only-jwt-secret-change-me",
                "5f71d068faffaa7e5cac07c803f4673fe64c37fbb6c395e7c02747873ba9225d",
                "dev-internal-secret-key-2024",
                "jwt-secret",
                "change-me",
                "changeme",
                "123456",
                "secret");
        String lowered = secret.toLowerCase();
        if (blocked.contains(secret) || lowered.contains("change-me") || lowered.contains("change_me")) {
            throw new IllegalStateException(
                    "security.jwt.secret 使用了默认/已泄露的弱密钥，拒绝启动");
        }
    }
}
