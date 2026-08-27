package com.ithsd.smart_tender.service.impl;

import org.junit.jupiter.api.Test;

import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;

import static org.junit.jupiter.api.Assertions.*;

/**
 * F3：RunningTaskRegistry 的并发注册表语义（TDD 先行）。
 */
class RunningTaskRegistryTest {

    @Test
    void registerAndContains_roundtrip() {
        RunningTaskRegistry registry = new RunningTaskRegistry();
        assertTrue(registry.register("1:task-a"));
        assertTrue(registry.contains("1:task-a"));
        assertFalse(registry.contains("1:task-b"));
    }

    @Test
    void duplicateRegister_returnsFalseButStillContains() {
        RunningTaskRegistry registry = new RunningTaskRegistry();
        registry.register("1:task-a");
        assertFalse(registry.register("1:task-a"), "重复注册应返回 false（已在运行）");
        assertTrue(registry.contains("1:task-a"));
    }

    @Test
    void remove_clearsKey() {
        RunningTaskRegistry registry = new RunningTaskRegistry();
        registry.register("2:task-x");
        registry.remove("2:task-x");
        assertFalse(registry.contains("2:task-x"));
    }

    @Test
    void removeMissingKey_isNoOp() {
        RunningTaskRegistry registry = new RunningTaskRegistry();
        assertDoesNotThrow(() -> registry.remove("9:never"));
    }

    @Test
    void concurrentRegister_singleWinner() throws InterruptedException {
        RunningTaskRegistry registry = new RunningTaskRegistry();
        int threads = 16;
        CountDownLatch start = new CountDownLatch(1);
        Set<Integer> winners = ConcurrentHashMap.newKeySet();

        Thread[] workers = new Thread[threads];
        for (int i = 0; i < threads; i++) {
            final int idx = i;
            workers[i] = new Thread(() -> {
                try {
                    start.await();
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                    return;
                }
                if (registry.register("1:race-task")) {
                    winners.add(idx);
                }
            });
            workers[i].start();
        }
        start.countDown();
        for (Thread t : workers) {
            t.join(TimeUnit.SECONDS.toMillis(10));
        }

        assertEquals(1, winners.size(), "并发注册同一 key 只能有一个赢家");
        assertTrue(registry.contains("1:race-task"));
    }

    @Test
    void emptyRegistry_containsNothing() {
        RunningTaskRegistry registry = new RunningTaskRegistry();
        assertFalse(registry.contains("1:any"));
        assertEquals(0, registry.size());
    }
}
