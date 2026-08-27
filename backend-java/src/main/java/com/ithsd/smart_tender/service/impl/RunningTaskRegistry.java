package com.ithsd.smart_tender.service.impl;

import org.springframework.stereotype.Component;

import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;

/**
 * 当前 JVM 内正在执行的审核任务注册表（key = "tenantId:taskId"）。
 *
 * <p>替代原先 {@code AuditEngineServiceImpl} 内的 private static Set：
 * <ol>
 *   <li>可注入、可单测（孤儿守护依赖它判断任务是否仍有存活线程）；</li>
 *   <li>与租户语义显式绑定，key 格式统一。</li>
 * </ol>
 */
@Component
public class RunningTaskRegistry {

    private final Set<String> running = ConcurrentHashMap.newKeySet();

    /** 注册正在运行的任务；返回 false 表示该 key 已存在（并发启动保护）。 */
    public boolean register(String key) {
        return running.add(key);
    }

    public void remove(String key) {
        running.remove(key);
    }

    public boolean contains(String key) {
        return running.contains(key);
    }

    public int size() {
        return running.size();
    }
}
