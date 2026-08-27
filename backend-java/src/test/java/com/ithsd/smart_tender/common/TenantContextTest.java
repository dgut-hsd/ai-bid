package com.ithsd.smart_tender.common;

import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;

import java.util.concurrent.Callable;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

class TenantContextTest {

    @AfterEach
    void cleanUp() {
        TenantContext.clear();
        BaseContext.removeCurrentId();
    }

    @Test
    void context_shouldExposeLegacyUserIdBridge() {
        TenantRequestContext context = new TenantRequestContext(10001L, 20001L, "ADMIN", 3L, "request-1");

        TenantContext.set(context);

        assertThat(TenantContext.get()).isEqualTo(context);
        assertThat(BaseContext.getCurrentId()).isEqualTo(10001L);
    }

    @Test
    void wrapper_shouldPropagateContextAndRestoreWorkerState() throws Exception {
        TenantRequestContext outer = new TenantRequestContext(10001L, 20001L, "ADMIN", 1L, "outer");
        TenantRequestContext workerState = new TenantRequestContext(10002L, 20002L, "MEMBER", 2L, "worker");
        TenantContext.set(outer);

        Callable<TenantRequestContext> wrapped = TenantContext.wrap(() -> {
            assertThat(TenantContext.get()).isEqualTo(outer);
            assertThat(BaseContext.getCurrentId()).isEqualTo(outer.userId());
            return TenantContext.get();
        });

        TenantContext.set(workerState);
        assertThat(wrapped.call()).isEqualTo(outer);
        assertThat(TenantContext.get()).isEqualTo(workerState);
        assertThat(BaseContext.getCurrentId()).isEqualTo(workerState.userId());
    }

    @Test
    void wrapper_shouldClearContextWhenCapturedContextIsAbsentAndOnFailure() {
        TenantContext.clear();
        Runnable wrapped = TenantContext.wrap((Runnable) () -> {
            assertThat(TenantContext.get()).isNull();
            assertThat(BaseContext.getCurrentId()).isNull();
            throw new IllegalStateException("boom");
        });

        assertThatThrownBy(wrapped::run).isInstanceOf(IllegalStateException.class);
        assertThat(TenantContext.get()).isNull();
        assertThat(BaseContext.getCurrentId()).isNull();
    }
}
