package com.ithsd.smart_tender.service;

import com.ithsd.smart_tender.model.vo.TenantSessionStateVO;

import java.time.Duration;
import java.util.Optional;

public interface TenantSessionStore {
    Optional<TenantSessionStateVO> findByUserId(Long userId);

    void save(TenantSessionStateVO session, Duration ttl);

    void deleteByUserId(Long userId);
}
