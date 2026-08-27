package com.ithsd.smart_tender.service.engine.rust;

import com.ithsd.smart_tender.common.BizException;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.config.RustApiProperties;
import com.ithsd.smart_tender.model.dto.rust.RustReviewAcceptedResponse;
import com.ithsd.smart_tender.model.dto.rust.RustReviewRequest;
import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;

import static org.junit.jupiter.api.Assertions.*;

/**
 * F5：startReview 的请求超时语义（TDD 先行）。
 *
 * <p>用本地 HttpServer 模拟 Rust POST /review：验证请求超时来自独立的
 * reviewStartTimeoutMs，而不是 5s 的 connectTimeoutMs。</p>
 */
class RustApiClientStartReviewTimeoutTest {

    private HttpServer server;
    private RustApiProperties properties;
    /** 服务端处理 POST /review 前的人为延迟毫秒 */
    private volatile long serverDelayMs = 0;

    @BeforeEach
    void setUp() throws IOException {
        server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext("/api/v1/documents/doc-timeout/review", this::handleReview);
        server.start();

        properties = new RustApiProperties();
        properties.setBaseUrl("http://127.0.0.1:" + server.getAddress().getPort());
        properties.setConnectTimeoutMs(1000);   // 连接超时 1s
        properties.setReadTimeoutMs(300_000);
        properties.setInternalSecret("test-secret");

        TenantContext.set(new TenantRequestContext(1L, 1L, "admin", 1L, "timeout-test"));
    }

    @AfterEach
    void tearDown() {
        TenantContext.clear();
        server.stop(0);
    }

    private void handleReview(HttpExchange exchange) throws IOException {
        if (serverDelayMs > 0) {
            try {
                Thread.sleep(serverDelayMs);
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                return;
            }
        }
        String body = "{\"status\":\"accepted\",\"document_id\":\"doc-timeout\",\"message\":\"ok\"}";
        byte[] bytes = body.getBytes(StandardCharsets.UTF_8);
        exchange.getResponseHeaders().set("Content-Type", "application/json");
        exchange.sendResponseHeaders(202, bytes.length);
        exchange.getResponseBody().write(bytes);
        exchange.close();
    }

    @Test
    void startReviewSucceeds_whenServerRespondsSlowerThanConnectTimeout() {
        // 旧实现：整个请求超时 = connectTimeoutMs(1s)，服务端 3s 才响应 → 必然失败
        // 新实现：请求超时 = reviewStartTimeoutMs(10s) → 成功
        serverDelayMs = 3000;
        properties.setReviewStartTimeoutMs(10_000);

        RustApiClient client = new RustApiClient(properties);
        RustReviewAcceptedResponse response = client.startReview(
                "doc-timeout", new RustReviewRequest());

        assertNotNull(response);
        assertEquals("accepted", response.getStatus());
    }

    @Test
    void startReviewFails_whenServerExceedsStartTimeout() {
        serverDelayMs = 4000;
        properties.setReviewStartTimeoutMs(1500);

        RustApiClient client = new RustApiClient(properties);
        BizException ex = assertThrows(
                BizException.class,
                () -> client.startReview("doc-timeout", new RustReviewRequest()));
        assertEquals(5701, ex.getCode(), "超时应按连接失败(5701)归类");
        assertTrue(ex.getMessage().contains("doc-timeout"),
                "错误信息应包含文档标识: " + ex.getMessage());
    }

    @Test
    void startReviewDefaultTimeout_isAtLeastTenSeconds() {
        RustApiProperties defaults = new RustApiProperties();
        assertTrue(defaults.getReviewStartTimeoutMs() >= 10_000,
                "默认启动超时应 ≥10s，避免把慢启动误判为失败: "
                        + defaults.getReviewStartTimeoutMs());
    }
}
