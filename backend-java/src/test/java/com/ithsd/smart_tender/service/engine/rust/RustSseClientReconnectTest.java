package com.ithsd.smart_tender.service.engine.rust;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.config.RustApiProperties;
import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.List;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicInteger;

import static org.junit.jupiter.api.Assertions.*;

/**
 * F4：RustSseClient 断线重连行为（JDK HttpServer 模拟 Rust SSE 端点，TDD 先行）。
 */
class RustSseClientReconnectTest {

    private HttpServer server;
    private RustApiProperties properties;

    private final AtomicInteger connectionCount = new AtomicInteger();
    private final List<String> receivedEvents = new CopyOnWriteArrayList<>();
    /** 每次连接要发送的事件列表；用完连接即关闭（模拟断线）。 */
    private volatile List<List<String>> perConnectionEvents = List.of();
    /** 所有连接用完后是否保持长连接（模拟审核仍进行中）。 */
    private volatile boolean keepLastOpen = false;
    /** 每连接发送事件前的延迟毫秒 */
    private volatile long delayMs = 0;

    @BeforeEach
    void setUp() throws IOException {
        connectionCount.set(0);
        receivedEvents.clear();
        keepLastOpen = false;
        delayMs = 0;

        server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext("/api/v1/review/doc-reconnect/stream", this::handleSse);
        server.start();

        properties = new RustApiProperties();
        properties.setBaseUrl("http://127.0.0.1:" + server.getAddress().getPort());
        properties.setConnectTimeoutMs(5000);
        properties.setReadTimeoutMs(900_000);
        properties.setInternalSecret("test-secret");

        TenantContext.set(new TenantRequestContext(1L, 1L, "admin", 1L, "reconnect-test"));
    }

    @AfterEach
    void tearDown() {
        TenantContext.clear();
        server.stop(0);
    }

    private void handleSse(HttpExchange exchange) throws IOException {
        int conn = connectionCount.incrementAndGet();
        exchange.getResponseHeaders().set("Content-Type", "text/event-stream");
        exchange.sendResponseHeaders(200, 0);
        try (OutputStream out = exchange.getResponseBody()) {
            List<List<String>> plan = perConnectionEvents;
            if (conn <= plan.size()) {
                for (String event : plan.get(conn - 1)) {
                    if (delayMs > 0) {
                        try {
                            Thread.sleep(delayMs);
                        } catch (InterruptedException e) {
                            Thread.currentThread().interrupt();
                            return;
                        }
                    }
                    out.write(event.getBytes(StandardCharsets.UTF_8));
                    out.flush();
                }
                if (!keepLastOpen || conn < plan.size()) {
                    return; // 关闭连接，模拟断线
                }
            } else if (!keepLastOpen) {
                return; // 超出计划且不保持打开 → 立即关闭
            }
            // 保持打开直到客户端断开
            try {
                Thread.sleep(Duration.ofMinutes(1).toMillis());
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
            }
        }
    }

    private static String sseEvent(String type, String json) {
        return "event: " + type + "\ndata: " + json + "\n\n";
    }

    // ── 测试 ──────────────────────────────────────────────────────

    @Test
    void reconnectsAfterStreamDrops_andReceivesLaterEvents() throws Exception {
        perConnectionEvents = List.of(
                List.of(sseEvent("phase", "{\"phase\":\"execute\"}")),          // 连接1：发完即断
                List.of(sseEvent("phase", "{\"phase\":\"merge\"}"),
                        sseEvent("done", "{\"total\":1}")));                    // 连接2：正常完成
        AtomicBoolean stop = new AtomicBoolean(false);

        RustSseClient client = new RustSseClient(properties);
        client.connectWithReconnect(
                "doc-reconnect",
                (type, data) -> {
                    receivedEvents.add(type + ":" + new ObjectMapper().convertValue(data, JsonNode.class).toString());
                    if ("done".equals(type)) {
                        stop.set(true);
                    }
                },
                stop::get,
                new SseRetryPolicy(5, 10, 100));

        waitUntil(() -> receivedEvents.stream().anyMatch(e -> e.startsWith("done:")));

        assertTrue(connectionCount.get() >= 2,
                "断线后应重连，实际连接次数=" + connectionCount.get());
        assertTrue(receivedEvents.stream().anyMatch(e -> e.startsWith("phase:")),
                "应收到断线前的事件: " + receivedEvents);
        assertTrue(receivedEvents.stream().anyMatch(e -> e.startsWith("done:")),
                "应收到重连后的事件: " + receivedEvents);
    }

    @Test
    void stopsRetryingAfterDoneEvent() throws Exception {
        perConnectionEvents = List.of(
                List.of(sseEvent("done", "{\"total\":1}"))); // 完成即断（模拟 Rust 完成清理）
        AtomicBoolean stop = new AtomicBoolean(false);

        RustSseClient client = new RustSseClient(properties);
        client.connectWithReconnect(
                "doc-reconnect",
                (type, data) -> {
                    receivedEvents.add(type);
                    if ("done".equals(type)) {
                        stop.set(true);
                    }
                },
                stop::get,
                new SseRetryPolicy(5, 10, 100));

        waitUntil(() -> receivedEvents.contains("done"));
        Thread.sleep(200); // 留出可能的误重连时间

        assertEquals(1, connectionCount.get(), "done 之后不应再重连");
    }

    @Test
    void givesUpAfterMaxAttempts() throws Exception {
        perConnectionEvents = List.of(); // 任何连接都不发事件
        keepLastOpen = false;
        AtomicBoolean stop = new AtomicBoolean(false);

        RustSseClient client = new RustSseClient(properties);
        client.connectWithReconnect(
                "doc-reconnect",
                (type, data) -> receivedEvents.add(type),
                stop::get,
                new SseRetryPolicy(3, 10, 100));

        waitUntil(() -> connectionCount.get() >= 3);
        Thread.sleep(200);

        assertEquals(3, connectionCount.get(), "达到最大尝试次数后应停止");
    }

    @Test
    void externalStopSignal_stopsReconnectLoop() throws Exception {
        perConnectionEvents = List.of(
                List.of(sseEvent("phase", "{\"phase\":\"execute\"}")));
        AtomicBoolean stop = new AtomicBoolean(false);

        RustSseClient client = new RustSseClient(properties);
        client.connectWithReconnect(
                "doc-reconnect",
                (type, data) -> stop.set(true), // 收到任何事件后外部叫停
                stop::get,
                new SseRetryPolicy(10, 10, 100));

        waitUntil(stop::get);
        Thread.sleep(200);

        assertEquals(1, connectionCount.get(), "外部停止信号应终止重连");
    }

    private static void waitUntil(java.util.function.BooleanSupplier condition) throws InterruptedException {
        long deadline = System.currentTimeMillis() + 10_000;
        while (System.currentTimeMillis() < deadline) {
            if (condition.getAsBoolean()) {
                return;
            }
            Thread.sleep(50);
        }
        fail("等待条件超时（10s）");
    }
}
