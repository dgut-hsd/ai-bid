package com.ithsd.smart_tender.config;

import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.service.engine.rust.InternalRequestSigner;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;
import org.springframework.scheduling.concurrent.ThreadPoolTaskExecutor;

import java.net.URI;
import java.net.http.HttpRequest;
import java.util.concurrent.Future;

import static org.assertj.core.api.Assertions.assertThat;

class AsyncConfigTenantPropagationTest {

    @AfterEach
    void clearContext() {
        TenantContext.clear();
    }

    @Test
    void auditExecutorCarriesSignerContextAndRestoresWorkerState() throws Exception {
        RustApiProperties properties = new RustApiProperties();
        properties.setInternalSecret("test-secret");
        InternalRequestSigner signer = new InternalRequestSigner(properties);
        ThreadPoolTaskExecutor executor = (ThreadPoolTaskExecutor) new AsyncConfig()
                .auditTaskExecutor(1, 1, 10, 60);

        try {
            TenantContext.set(new TenantRequestContext(10001L, 20001L, "OWNER", 1L, "request-a"));

            Future<String> signedTenant = executor.submit(() -> signer.sign(
                    HttpRequest.newBuilder(URI.create("http://rust.test/api/v1/documents")),
                    "GET",
                    URI.create("http://rust.test/api/v1/documents"),
                    new byte[0]
            ).build().headers().firstValue(InternalRequestSigner.TENANT_HEADER).orElseThrow());

            assertThat(signedTenant.get()).isEqualTo("20001");

            TenantContext.clear();
            Future<TenantRequestContext> leakedContext = executor.submit(TenantContext::get);
            assertThat(leakedContext.get()).isNull();
        } finally {
            executor.shutdown();
        }
    }
}
