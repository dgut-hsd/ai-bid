package com.ithsd.smart_tender.service.engine.rust;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.PropertyNamingStrategies;
import com.ithsd.smart_tender.config.RustApiProperties;
import com.ithsd.smart_tender.common.BizException;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.model.dto.rust.*;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.stereotype.Component;

import java.io.IOException;
import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;

/**
 * Rust 审核引擎 HTTP API 客户端。
 *
 * <p>封装所有对 Rust axum 服务（端口 3001）的调用。
 * 使用 JDK 内置 HttpClient（HTTP/1.1），避免引入额外依赖。</p>
 *
 * <h3>错误码</h3>
 * <ul>
 *   <li>5701 — Rust 连接失败</li>
 *   <li>5702 — Rust 返回错误</li>
 *   <li>5703 — 文档上传失败</li>
 *   <li>5704 — 文档未找到</li>
 *   <li>5705 — 审核执行失败</li>
 *   <li>5706 — 对话执行失败</li>
 *   <li>5707 — 搜索执行失败</li>
 * </ul>
 */
@Component
public class RustApiClient {

    private static final Logger log = LoggerFactory.getLogger(RustApiClient.class);
    private static final String MULTIPART_BOUNDARY = "----RustApiClientBoundary" + UUID.randomUUID().toString().replace("-", "");

    private final RustApiProperties properties;
    private final HttpClient httpClient;
    private final ObjectMapper objectMapper;
    private final InternalRequestSigner requestSigner;

    public RustApiClient(RustApiProperties properties) {
        this.properties = properties;
        this.requestSigner = new InternalRequestSigner(properties);
        this.objectMapper = new ObjectMapper()
                .setPropertyNamingStrategy(PropertyNamingStrategies.SNAKE_CASE);
        this.httpClient = HttpClient.newBuilder()
                .version(HttpClient.Version.HTTP_1_1)
                .connectTimeout(Duration.ofMillis(properties.getConnectTimeoutMs()))
                .build();
    }

    // ── 文档上传 ───────────────────────────────────────────────────

    /**
     * 上传文件到 Rust 处理管线。
     *
     * @param filePath 本地文件路径
     * @param filename 原始文件名（用于扩展名推断）
     * @return 处理结果（含 document_id）
     */
    public RustProcessResponse uploadDocument(Path filePath, String filename) {
        try {
            byte[] fileBytes = Files.readAllBytes(filePath);
            byte[] body = buildMultipartBody(
                    filename,
                    fileBytes,
                    properties.getDesensitizationMode());

            URI uri = URI.create(properties.apiUrl("/api/v1/documents"));
            HttpRequest request = signedRequestBuilder("POST", uri, body)
                    .timeout(Duration.ofMillis(properties.getReadTimeoutMs()))
                    .header("Content-Type", "multipart/form-data; boundary=" + MULTIPART_BOUNDARY)
                    .POST(HttpRequest.BodyPublishers.ofByteArray(body))
                    .build();

            log.info("Rust upload: file={}, size={} bytes", filename, fileBytes.length);
            HttpResponse<String> response = httpClient.send(request, HttpResponse.BodyHandlers.ofString());

            if (response.statusCode() == 200) {
                RustProcessResponse result = objectMapper.readValue(response.body(), RustProcessResponse.class);
                log.info("Rust upload ok: docId={}, chunks={}", result.getDocumentId(), result.getTotalChunks());
                return result;
            }

            throw new BizException(5703, "文档上传失败: HTTP " + response.statusCode() + " — " + truncate(response.body()));
        } catch (BizException e) {
            throw e;
        } catch (IOException e) {
            throw new BizException(5701, "Rust 连接失败 (upload): " + e.getMessage());
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new BizException(5701, "Rust 调用被中断 (upload)");
        }
    }

    // ── 文档查询 ───────────────────────────────────────────────────

    /**
     * 查询 Rust 侧文档状态。
     *
     * @return null 表示文档不存在（404）
     */
    public RustDocumentInfo getDocument(String documentId) {
        try {
            URI uri = URI.create(properties.apiUrl("/api/v1/documents/" + documentId));
            HttpRequest request = signedRequestBuilder("GET", uri, new byte[0])
                    .timeout(Duration.ofMillis(properties.getReadTimeoutMs()))
                    .GET()
                    .build();

            HttpResponse<String> response = httpClient.send(request, HttpResponse.BodyHandlers.ofString());

            if (response.statusCode() == 200) {
                return objectMapper.readValue(response.body(), RustDocumentInfo.class);
            }
            if (response.statusCode() == 404) {
                return null;
            }

            throw new BizException(5702, "查询文档失败: HTTP " + response.statusCode());
        } catch (BizException e) {
            throw e;
        } catch (IOException e) {
            throw new BizException(5701, "Rust 连接失败 (getDocument): " + e.getMessage());
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new BizException(5701, "Rust 调用被中断 (getDocument)");
        }
    }

    // ── 审核 ──────────────────────────────────────────────────────

    /**
     * 启动异步 Multi-Agent 合规审核（立即返回 202）。
     *
     * <p>审核在 Rust 后台 Tokio task 中执行，通过 SSE 实时推送进度，
     * 完成后调用 {@link #getReviewResult(String)} 获取结果。</p>
     */
    public RustReviewAcceptedResponse startReview(String documentId, RustReviewRequest reviewReq) {
        try {
            byte[] body = objectMapper.writeValueAsString(reviewReq).getBytes(StandardCharsets.UTF_8);

            URI uri = URI.create(properties.apiUrl("/api/v1/documents/" + documentId + "/review"));
            HttpRequest request = signedRequestBuilder("POST", uri, body)
                    .timeout(Duration.ofMillis(properties.getConnectTimeoutMs()))  // 仅连接超时，不设读超时
                    .header("Content-Type", "application/json")
                    .POST(HttpRequest.BodyPublishers.ofByteArray(body))
                    .build();

            log.info("Rust review start (async): docId={}, maxClauses={}", documentId, reviewReq.getMaxClauses());
            HttpResponse<String> response = httpClient.send(request, HttpResponse.BodyHandlers.ofString());

            if (response.statusCode() == 202) {
                RustReviewAcceptedResponse result = objectMapper.readValue(response.body(), RustReviewAcceptedResponse.class);
                log.info("Rust review accepted: docId={}, status={}", documentId, result.getStatus());
                return result;
            }
            if (response.statusCode() == 409) {
                RustReviewAcceptedResponse result = objectMapper.readValue(response.body(), RustReviewAcceptedResponse.class);
                log.warn("Rust review conflict: docId={}, msg={}", documentId, result.getMessage());
                return result;  // 调用方自行判断 isConflict()
            }
            if (response.statusCode() == 404) {
                throw new BizException(5704, "文档未在 Rust 中找到: " + documentId);
            }

            throw new BizException(5705, "启动审核失败: HTTP " + response.statusCode() + " — " + truncate(response.body()));
        } catch (BizException e) {
            throw e;
        } catch (IOException e) {
            String msg = e.getMessage();
            if (msg == null || msg.isEmpty()) {
                msg = e.getClass().getSimpleName();
            }
            throw new BizException(5701, "Rust 连接失败 (startReview): " + msg
                + " [url=" + properties.apiUrl("/api/v1/documents/" + documentId + "/review") + "]");
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new BizException(5701, "Rust 调用被中断 (startReview)");
        }
    }

    /**
     * 查询异步审核的最终结果。
     *
     * <p>对应 Rust {@code GET /api/v1/review/:doc_id/result}。</p>
     *
     * @return 结果对象，status 为 "completed" / "pending" / "failed"
     * @throws BizException 5701 连接失败；5705 查询失败
     */
    public RustReviewResultResponse getReviewResult(String documentId) {
        try {
            URI uri = URI.create(properties.apiUrl("/api/v1/review/" + documentId + "/result"));
            HttpRequest request = signedRequestBuilder("GET", uri, new byte[0])
                    .timeout(Duration.ofMillis(properties.getReadTimeoutMs()))
                    .GET()
                    .build();

            log.debug("Rust getReviewResult: docId={}", documentId);
            HttpResponse<String> response = httpClient.send(request, HttpResponse.BodyHandlers.ofString());

            if (response.statusCode() == 200) {
                return objectMapper.readValue(response.body(), RustReviewResultResponse.class);
            }
            if (response.statusCode() == 404) {
                return null;  // 无审查记录，调用方处理
            }

            throw new BizException(5705, "查询审核结果失败: HTTP " + response.statusCode() + " — " + truncate(response.body()));
        } catch (BizException e) {
            throw e;
        } catch (IOException e) {
            throw new BizException(5701, "Rust 连接失败 (getReviewResult): " + e.getMessage());
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new BizException(5701, "Rust 调用被中断 (getReviewResult)");
        }
    }

    // ── 对话 ──────────────────────────────────────────────────────

    /**
     * 与文档对话（ChatAgent）。
     */
    public RustChatResponse chatWithDocument(String documentId, RustChatRequest chatReq) {
        try {
            byte[] body = objectMapper.writeValueAsString(chatReq).getBytes(StandardCharsets.UTF_8);

            URI uri = URI.create(properties.apiUrl("/api/v1/documents/" + documentId + "/chat"));
            HttpRequest request = signedRequestBuilder("POST", uri, body)
                    .timeout(Duration.ofMillis(properties.getReadTimeoutMs()))
                    .header("Content-Type", "application/json")
                    .POST(HttpRequest.BodyPublishers.ofByteArray(body))
                    .build();

            log.info("Rust chat: docId={}, inputLen={}", documentId, chatReq.getUserInput().length());
            HttpResponse<String> response = httpClient.send(request, HttpResponse.BodyHandlers.ofString());

            if (response.statusCode() == 200) {
                return objectMapper.readValue(response.body(), RustChatResponse.class);
            }
            if (response.statusCode() == 404) {
                throw new BizException(5704, "文档未在 Rust 中找到: " + documentId);
            }

            throw new BizException(5706, "对话执行失败: HTTP " + response.statusCode() + " — " + truncate(response.body()));
        } catch (BizException e) {
            throw e;
        } catch (IOException e) {
            throw new BizException(5701, "Rust 连接失败 (chat): " + e.getMessage());
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new BizException(5701, "Rust 调用被中断 (chat)");
        }
    }

    /**
     * 连接 Rust Chat SSE 流，解析事件并回调。
     *
     * <p>POST 到 Rust /api/v1/documents/:docId/chat/stream，
     * 读取 SSE 事件流，对每个事件调用 consumer。</p>
     *
     * @param docId   Rust 文档 ID
     * @param chatReq 聊天请求体
     * @param onEvent 事件回调 (eventType, dataJson)
     * @return CompletableFuture，连接建立时完成；失败则异常完成
     */
    public java.util.concurrent.CompletableFuture<Void> connectChatStream(
            String docId, RustChatRequest chatReq,
            java.util.function.BiConsumer<String, com.fasterxml.jackson.databind.JsonNode> onEvent) {
        java.util.concurrent.CompletableFuture<Void> connectedFuture =
            new java.util.concurrent.CompletableFuture<>();

        java.util.concurrent.Executor executor = java.util.concurrent.Executors.newCachedThreadPool(r -> {
            Thread t = new Thread(r, "rust-chat-sse");
            t.setDaemon(true);
            return t;
        });

        Runnable task = TenantContext.wrap((Runnable) () -> {
            try {
                byte[] body = objectMapper.writeValueAsString(chatReq).getBytes(StandardCharsets.UTF_8);
                URI uri = URI.create(properties.apiUrl("/api/v1/documents/" + docId + "/chat/stream"));
                String url = uri.toString();
                log.info("Rust chat SSE connecting: {}", url);

                HttpRequest request = signedRequestBuilder("POST", uri, body)
                        .timeout(Duration.ofMillis(properties.getReadTimeoutMs()))
                        .header("Content-Type", "application/json")
                        .header("Accept", "text/event-stream")
                        .POST(HttpRequest.BodyPublishers.ofByteArray(body))
                        .build();

                HttpResponse<java.io.InputStream> response = httpClient.send(
                        request, HttpResponse.BodyHandlers.ofInputStream());

                if (response.statusCode() != 200) {
                    log.warn("Rust chat SSE HTTP {}: {}", response.statusCode(), docId);
                    connectedFuture.completeExceptionally(
                        new BizException(5706, "Chat SSE HTTP " + response.statusCode()));
                    return;
                }

                log.info("Rust chat SSE connected: docId={}", docId);
                connectedFuture.complete(null);

                try (java.io.BufferedReader reader = new java.io.BufferedReader(
                        new java.io.InputStreamReader(response.body(), StandardCharsets.UTF_8))) {

                    String currentEvent = "message";
                    StringBuilder dataBuffer = new StringBuilder();

                    String line;
                    while ((line = reader.readLine()) != null) {
                        if (line.startsWith("event:")) {
                            currentEvent = line.substring(6).trim();
                        } else if (line.startsWith("data:")) {
                            dataBuffer.append(line.substring(5).trim());
                        } else if (line.isEmpty() && dataBuffer.length() > 0) {
                            String json = dataBuffer.toString();
                            try {
                                com.fasterxml.jackson.databind.JsonNode node =
                                    objectMapper.readTree(json);
                                onEvent.accept(currentEvent, node);
                            } catch (Exception e) {
                                log.debug("Chat SSE parse failed: {}", e.getMessage());
                            }
                            dataBuffer = new StringBuilder();
                            currentEvent = "message";
                        } else if (!line.startsWith(":") && !line.isEmpty()) {
                            dataBuffer.append(line.trim());
                        }
                    }
                }

                log.info("Rust chat SSE stream ended: docId={}", docId);
            } catch (Exception e) {
                log.warn("Rust chat SSE failed for docId={}: {}", docId, e.getMessage());
                connectedFuture.completeExceptionally(e);
            }
        });
        executor.execute(task);

        return connectedFuture;
    }

    // ── 搜索 ──────────────────────────────────────────────────────

    /**
     * 语义搜索文档内容。
     */
    public RustSearchResponse searchDocument(String documentId, RustSearchRequest searchReq) {
        try {
            byte[] body = objectMapper.writeValueAsString(searchReq).getBytes(StandardCharsets.UTF_8);

            URI uri = URI.create(properties.apiUrl("/api/v1/documents/" + documentId + "/search"));
            HttpRequest request = signedRequestBuilder("POST", uri, body)
                    .timeout(Duration.ofMillis(properties.getReadTimeoutMs()))
                    .header("Content-Type", "application/json")
                    .POST(HttpRequest.BodyPublishers.ofByteArray(body))
                    .build();

            log.debug("Rust search: docId={}, queries={}", documentId, searchReq.getQueries());
            HttpResponse<String> response = httpClient.send(request, HttpResponse.BodyHandlers.ofString());

            if (response.statusCode() == 200) {
                return objectMapper.readValue(response.body(), RustSearchResponse.class);
            }
            if (response.statusCode() == 404) {
                throw new BizException(5704, "文档未在 Rust 中找到: " + documentId);
            }

            throw new BizException(5707, "搜索执行失败: HTTP " + response.statusCode() + " — " + truncate(response.body()));
        } catch (BizException e) {
            throw e;
        } catch (IOException e) {
            throw new BizException(5701, "Rust 连接失败 (search): " + e.getMessage());
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new BizException(5701, "Rust 调用被中断 (search)");
        }
    }

    // ── Block BBox 查询 ─────────────────────────────────────────────

    /**
     * 查询指定 block_id 的 BBox 坐标。
     *
     * @param documentId Rust 侧文档 UUID
     * @param blockIds   逗号分隔的 block_id 列表
     * @return BBox 坐标列表
     */
    public List<RustBlockBBoxResponse> getBlockBboxes(String documentId, String blockIds) {
        try {
            String encodedIds = URLEncoder.encode(blockIds, StandardCharsets.UTF_8);
            URI uri = URI.create(properties.apiUrl(
                    "/api/v1/documents/" + documentId + "/blocks?ids=" + encodedIds));
            HttpRequest request = signedRequestBuilder("GET", uri, new byte[0])
                    .timeout(Duration.ofMillis(properties.getReadTimeoutMs()))
                    .GET()
                    .build();

            log.debug("Rust getBlockBboxes: docId={}, ids={}", documentId, blockIds);
            HttpResponse<String> response = httpClient.send(request, HttpResponse.BodyHandlers.ofString());

            if (response.statusCode() == 200) {
                return objectMapper.readValue(
                        response.body(),
                        objectMapper.getTypeFactory()
                                .constructCollectionType(List.class, RustBlockBBoxResponse.class));
            }

            String errorBody = response.body();
            log.warn("Rust getBlockBboxes failed: status={}, body={}", response.statusCode(), errorBody);
            return List.of();
        } catch (BizException e) {
            throw e;
        } catch (IOException e) {
            throw new BizException(5701, "Rust 连接失败 (blocks): " + e.getMessage());
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new BizException(5701, "Rust 调用被中断 (blocks)");
        }
    }

    // ── 健康检查 ──────────────────────────────────────────────────

    /**
     * 检查 Rust 服务是否可用。
     */
    public boolean healthCheck() {
        try {
            HttpRequest request = HttpRequest.newBuilder()
                    .uri(URI.create(properties.apiUrl("/health")))
                    .timeout(Duration.ofMillis(properties.getConnectTimeoutMs()))
                    .GET()
                    .build();

            HttpResponse<String> response = httpClient.send(request, HttpResponse.BodyHandlers.ofString());
            return response.statusCode() == 200;
        } catch (Exception e) {
            log.warn("Rust health check failed: {}", e.getMessage());
            return false;
        }
    }

    // ── 私有工具方法 ──────────────────────────────────────────────

    private HttpRequest.Builder signedRequestBuilder(String method, URI uri, byte[] body) {
        return requestSigner.sign(HttpRequest.newBuilder().uri(uri), method, uri, body);
    }

    /**
     * 构建 multipart/form-data 请求体。
     */
    private byte[] buildMultipartBody(
            String filename,
            byte[] fileBytes,
            String desensitizationMode
    ) throws IOException {
        List<byte[]> parts = new ArrayList<>();

        String modePart = "--" + MULTIPART_BOUNDARY + "\r\n"
                + "Content-Disposition: form-data; name=\"desensitize_mode\"\r\n\r\n"
                + (desensitizationMode == null ? "low" : desensitizationMode)
                + "\r\n";
        parts.add(modePart.getBytes(StandardCharsets.UTF_8));

        // File part
        String partHeader = "--" + MULTIPART_BOUNDARY + "\r\n"
                + "Content-Disposition: form-data; name=\"file\"; filename=\"" + filename + "\"\r\n"
                + "Content-Type: application/octet-stream\r\n\r\n";
        parts.add(partHeader.getBytes(StandardCharsets.UTF_8));
        parts.add(fileBytes);

        // Closing boundary
        String closing = "\r\n--" + MULTIPART_BOUNDARY + "--\r\n";
        parts.add(closing.getBytes(StandardCharsets.UTF_8));

        // Concatenate
        int totalLen = parts.stream().mapToInt(b -> b.length).sum();
        byte[] result = new byte[totalLen];
        int offset = 0;
        for (byte[] part : parts) {
            System.arraycopy(part, 0, result, offset, part.length);
            offset += part.length;
        }
        return result;
    }

    private static String truncate(String text) {
        if (text == null) return "null";
        if (text.length() <= 200) return text;
        return text.substring(0, 200) + "...";
    }
}
