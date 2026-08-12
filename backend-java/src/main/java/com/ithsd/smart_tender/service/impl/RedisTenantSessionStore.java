package com.ithsd.smart_tender.service.impl;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.ithsd.smart_tender.model.vo.TenantSessionStateVO;
import com.ithsd.smart_tender.service.TenantSessionStore;
import lombok.RequiredArgsConstructor;
import org.springframework.data.redis.core.StringRedisTemplate;
import org.springframework.stereotype.Service;

import java.time.Duration;
import java.util.Optional;

/** Redis implementation of the authoritative per-user tenant session. */
@Service
@RequiredArgsConstructor
public class RedisTenantSessionStore implements TenantSessionStore {

    static final String KEY_PREFIX = "auth:tenant-session:user:";

    private final StringRedisTemplate stringRedisTemplate;
    private final ObjectMapper objectMapper;

    @Override
    public Optional<TenantSessionStateVO> findByUserId(Long userId) {
        String raw = stringRedisTemplate.opsForValue().get(key(userId));
        if (raw == null) {
            return Optional.empty();
        }
        try {
            return Optional.of(objectMapper.readValue(raw, TenantSessionStateVO.class));
        } catch (JsonProcessingException ex) {
            return Optional.empty();
        }
    }

    @Override
    public void save(TenantSessionStateVO session, Duration ttl) {
        try {
            String raw = objectMapper.writeValueAsString(session);
            stringRedisTemplate.opsForValue().set(key(session.getUserId()), raw, ttl);
        } catch (JsonProcessingException ex) {
            throw new IllegalStateException("Unable to persist tenant session", ex);
        }
    }

    @Override
    public void deleteByUserId(Long userId) {
        stringRedisTemplate.delete(key(userId));
    }

    private static String key(Long userId) {
        return KEY_PREFIX + userId;
    }
}
