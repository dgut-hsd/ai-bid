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

/**
 * Rust SSE 客户端 — 连接 Rust 的 GET /api/v1/review/:docId/stream 端点，
 * 解析 SSE 事件并回调 consumer。
 *
 * <p>用于 Java 在调用 Rust POST /review 的同时，通过此客户端接收
 * 实时审查进度事件，转发到 Java SseHub。</p>
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

    /**
     * 连接 Rust SSE 流，解析事件并回调。
     *
     * <p>返回的 CompletableFuture 在 HTTP 连接建立（200 OK）时完成，
     * 调用方可等待此 Future 后再触发 POST /review，确保不丢早期事件。</p>
     *
     * @param docId    Rust 文档 ID
     * @param onEvent  事件回调 (eventType, data)
     * @return CompletableFuture，连接建立时完成；若连接失败则异常完成
     */
    public CompletableFuture<Void> connect(String docId, BiConsumer<String, JsonNode> onEvent) {
        CompletableFuture<Void> connectedFuture = new CompletableFuture<>();

        Runnable task = TenantContext.wrap((Runnable) () -> {
            try {
                URI uri = URI.create(properties.apiUrl("/api/v1/review/" + docId + "/stream"));
                String url = uri.toString();
                log.info("Rust SSE connecting: {}", url);

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
                    connectedFuture.completeExceptionally(
                        new RuntimeException("Rust SSE HTTP " + response.statusCode()));
                    return;
                }

                // HTTP 200 → 连接已建立，通知调用方可以开始 POST /review
                log.info("Rust SSE connected: docId={}", docId);
                connectedFuture.complete(null);

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
            } catch (Exception e) {
                log.warn("Rust SSE connect failed for docId={}: {}", docId, e.getMessage());
                connectedFuture.completeExceptionally(e);
            }
        });
        SSE_EXECUTOR.execute(task);

        return connectedFuture;
    }
}
