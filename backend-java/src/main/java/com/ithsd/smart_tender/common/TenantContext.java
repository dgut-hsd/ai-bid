package com.ithsd.smart_tender.common;

import java.util.Objects;
import java.util.concurrent.Callable;

/**
 * Request context holder. The only supported propagation mechanism is an
 * explicit wrapper (see {@link #wrap(Runnable)}); callers must never rely on
 * a raw ThreadLocal crossing an executor boundary.
 */
public final class TenantContext {

    private static final ThreadLocal<TenantRequestContext> CURRENT = new ThreadLocal<>();

    private TenantContext() {
    }

    public static void set(TenantRequestContext context) {
        Objects.requireNonNull(context, "context");
        CURRENT.set(context);
        BaseContext.setCurrentId(context.userId());
    }

    public static TenantRequestContext get() {
        return CURRENT.get();
    }

    public static TenantRequestContext current() {
        return get();
    }

    public static void clear() {
        CURRENT.remove();
        BaseContext.removeCurrentId();
    }

    public static Runnable wrap(Runnable task) {
        Objects.requireNonNull(task, "task");
        TenantRequestContext captured = get();
        Long capturedLegacyUserId = BaseContext.getCurrentId();
        return () -> {
            TenantRequestContext previous = get();
            Long previousLegacyUserId = BaseContext.getCurrentId();
            install(captured, capturedLegacyUserId);
            try {
                task.run();
            } finally {
                install(previous, previousLegacyUserId);
            }
        };
    }

    public static <T> Callable<T> wrap(Callable<T> task) {
        Objects.requireNonNull(task, "task");
        TenantRequestContext captured = get();
        Long capturedLegacyUserId = BaseContext.getCurrentId();
        return () -> {
            TenantRequestContext previous = get();
            Long previousLegacyUserId = BaseContext.getCurrentId();
            install(captured, capturedLegacyUserId);
            try {
                return task.call();
            } finally {
                install(previous, previousLegacyUserId);
            }
        };
    }

    private static void install(TenantRequestContext context, Long legacyUserId) {
        if (context == null) {
            CURRENT.remove();
            if (legacyUserId == null) {
                BaseContext.removeCurrentId();
            } else {
                BaseContext.setCurrentId(legacyUserId);
            }
            return;
        }
        CURRENT.set(context);
        BaseContext.setCurrentId(context.userId());
    }
}
