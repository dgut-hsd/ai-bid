package com.ithsd.smart_tender.config;

import org.springframework.boot.context.properties.ConfigurationProperties;
import org.springframework.stereotype.Component;

/**
 * JWT settings are bound from configuration so application code never owns a
 * production signing secret or token lifetime.
 */
@Component
@ConfigurationProperties(prefix = "security.jwt")
public class JwtProperties {

    private String secret;
    private long ttlMillis = 86_400_000L;
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

    public boolean isAcceptLegacyUserId() {
        return acceptLegacyUserId;
    }

    public void setAcceptLegacyUserId(boolean acceptLegacyUserId) {
        this.acceptLegacyUserId = acceptLegacyUserId;
    }
}
