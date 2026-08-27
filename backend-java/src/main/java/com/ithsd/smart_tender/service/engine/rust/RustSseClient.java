package com.ithsd.smart_tender.service.engine.rust;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.config.RustApiProperties;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.stereotype.Component;

import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.Executors;
import java.util.function.BiConsumer;
import java.util.function.BooleanSupplier;

/**
 * Rust SSE 客户端 — 连接 Rust 的 GET /api/v1/review/:docId/stream 端点，
 * 解析 SSE 事件并回调 consumer。
 *
 * <p>用于 Java 在调用 Rust POST /review 的同时，通过此客户端接收
 * 实时审查进度事件，转发到 Java SseHub。</p>
 *
 * <p>自「审核稳定性修复」起支持断线重连：{@link #connectWithReconnect}
 * 在流意外中断后按退避策略重连，直到收到 done/error 或外部叫停。</p>
 */
@Component
public class RustSseClient {

    private static final Logger log = LoggerFactory.getLogger(RustSseClient.class);

    private final RustApiProperties properties;
    private final HttpClient httpClient;
    private final ObjectMapper objectMapper;
    private final InternalRequestSigner requestSigner;

    public RustSseClient(RustApiProperties properties) {
        this.properties = properties;
        this.requestSigner = new InternalRequestSigner(properties);
        this.objectMapper = new ObjectMapper()
                .setPropertyNamingStrategy(
                    com.fasterxml.jackson.databind.PropertyNamingStrategies.SNAKE_CASE);
        this.httpClient = HttpClient.newBuilder()
                .version(HttpClient.Version.HTTP_1_1)
                .connectTimeout(Duration.ofMillis(properties.getConnectTimeoutMs()))
                .build();
    }

    /** SSE 专用线程池，避免阻塞 ForkJoinPool。 */
    private static final java.util.concurrent.Executor SSE_EXECUTOR =
        Executors.newCachedThreadPool(r -> {
            Thread t = new Thread(r, "rust-sse-client");
            t.setDaemon(true);
            return t;
        });

    /** 单次连接结果。 */
    private enum ConnectResult { OK, HTTP_ERROR, CONNECT_ERROR }

    /**
     * 连接 Rust SSE 流，解析事件并回调（一次性，无重连）。
     *
     * <p>返回的 CompletableFuture 在 HTTP 连接建立（200 OK）时完成，
     * 调用方可等待此 Future 后再触发 POST /review，确保不丢早期事件。</p>
     *
     * @param docId    Rust 文档 ID
     * @param onEvent  事件回调 (eventType, data)
     * @return CompletableFuture，连接建立时完成；若连接失败则异常完成
     */
    public CompletableFuture<Void> connect(String docId, BiConsumer<String, JsonNode> onEvent) {
        return connectWithReconnect(docId, onEvent, () -> true, new SseRetryPolicy(1, 0, 0));
    }

    /**
     * 连接 Rust SSE 流并支持断线重连。
     *
     * <p>语义：
     * <ul>
     *   <li>首次成功建立连接（HTTP 200）时完成返回的 Future；</li>
     *   <li>流中断/连接失败后，按 {@code policy} 退避重连；</li>
     *   <li>{@code shouldStop} 返回 true（如已收到 done/error、任务已结束）时停止；</li>
     *   <li>达到最大尝试次数仍失败 → Future 异常完成。</li>
     * </ul>
     *
     * @param docId     Rust 文档 ID
     * @param onEvent   事件回调 (eventType, data)
     * @param shouldStop 是否停止重连（外部叫停）
     * @param policy    重连退避策略
     */
    public CompletableFuture<Void> connectWithReconnect(
            String docId,
            BiConsumer<String, JsonNode> onEvent,
            BooleanSupplier shouldStop,
            SseRetryPolicy policy) {
        CompletableFuture<Void> connectedFuture = new CompletableFuture<>();

        Runnable task = TenantContext.wrap((Runnable) () -> {
            int attempt = 0;
            boolean everConnected = false;
            try {
                while (true) {
                    attempt++;
                    if (!policy.allowRetry(attempt)) {
                        break;
                    }
                    ConnectResult result = connectOnce(docId, onEvent);
                    if (result == ConnectResult.OK && !everConnected) {
                        everConnected = true;
                        connectedFuture.complete(null);
                    }
                    if (shouldStop.getAsBoolean()) {
                        break;
                    }
                    if (!policy.allowRetry(attempt + 1)) {
                        break;
                    }
                    try {
                        Thread.sleep(policy.delayForAttempt(attempt));
                    } catch (InterruptedException e) {
                        Thread.currentThread().interrupt();
                        break;
                    }
                }
            } finally {
                if (!everConnected && !connectedFuture.isDone()) {
                    connectedFuture.completeExceptionally(
                            new RuntimeException("Rust SSE 连接失败，已尝试 " + attempt + " 次"));
                }
            }
        });
        SSE_EXECUTOR.execute(task);

        return connectedFuture;
    }

    /**
     * 建立一次连接并读取流直到结束。
     *
     * @return OK=连接成功且流正常结束；HTTP_ERROR=非 200；CONNECT_ERROR=网络异常
     */
    private ConnectResult connectOnce(String docId, BiConsumer<String, JsonNode> onEvent) {
        try {
            URI uri = URI.create(properties.apiUrl("/api/v1/review/" + docId + "/stream"));
            log.info("Rust SSE connecting: {}", uri);

            HttpRequest request = requestSigner.sign(
                        HttpRequest.newBuilder().uri(uri),
                        "GET",
                        uri,
                        new byte[0])
                    .timeout(Duration.ofMillis(properties.getReadTimeoutMs()))
                    .header("Accept", "text/event-stream")
                    .GET()
                    .build();

            HttpResponse<java.io.InputStream> response = httpClient.send(
                    request, HttpResponse.BodyHandlers.ofInputStream());

            if (response.statusCode() != 200) {
                log.warn("Rust SSE returned HTTP {}: {}", response.statusCode(), docId);
                return ConnectResult.HTTP_ERROR;
            }

            log.info("Rust SSE connected: docId={}", docId);

            try (BufferedReader reader = new BufferedReader(
                    new InputStreamReader(response.body()))) {

                String currentEvent = null;
                StringBuilder dataBuffer = new StringBuilder();

                String line;
                while ((line = reader.readLine()) != null) {
                    if (line.startsWith("event:")) {
                        currentEvent = line.substring(6).trim();
                    } else if (line.startsWith("data:")) {
                        dataBuffer.append(line.substring(5).trim());
                    } else if (line.isEmpty() && currentEvent != null) {
                        // 空行 = 事件结束
                        String json = dataBuffer.toString();
                        if (!json.isEmpty()) {
                            try {
                                JsonNode node = objectMapper.readTree(json);
                                // 从 JSON 中提取 event 字段（用于 done/message 类型）
                                String eventType = currentEvent;
                                if ("message".equals(eventType) && node.has("event")) {
                                    eventType = node.get("event").asText();
                                }
                                onEvent.accept(eventType, node);
                            } catch (Exception e) {
                                log.debug("Rust SSE parse failed: {}", e.getMessage());
                            }
                        }
                        dataBuffer = new StringBuilder();
                    } else if (!line.startsWith(":")) {
                        // 续行（多行 data）
                        dataBuffer.append(line.trim());
                    }
                }
            }

            log.info("Rust SSE stream ended: docId={}", docId);
            return ConnectResult.OK;
        } catch (Exception e) {
            log.warn("Rust SSE connect failed for docId={}: {}", docId, e.getMessage());
            return ConnectResult.CONNECT_ERROR;
        }
    }
}
