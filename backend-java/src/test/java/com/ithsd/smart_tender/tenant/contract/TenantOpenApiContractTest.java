package com.ithsd.smart_tender.tenant.contract;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import java.util.stream.StreamSupport;

import static org.assertj.core.api.Assertions.assertThat;

class TenantOpenApiContractTest {

    private static final ObjectMapper JSON = new ObjectMapper();

    private static final Map<String, List<String>> ERROR_CODES = Map.of(
            "400", List.of("REQUEST_INVALID", "TENANT_REQUIRED"),
            "401", List.of(
                    "AUTH_REQUIRED",
                    "AUTH_INVALID",
                    "TENANT_SESSION_STALE",
                    "INTERNAL_SIGNATURE_MISSING",
                    "INTERNAL_SIGNATURE_INVALID",
                    "INTERNAL_SIGNATURE_EXPIRED",
                    "INTERNAL_REQUEST_REPLAYED"
            ),
            "403", List.of(
                    "TENANT_ROLE_FORBIDDEN",
                    "TENANT_OWNER_REQUIRED",
                    "TENANT_CONTEXT_INVALID",
                    "INTERNAL_TENANT_MISMATCH"
            ),
            "404", List.of(
                    "TENANT_NOT_FOUND",
                    "RESOURCE_NOT_FOUND",
                    "TENANT_MEMBER_NOT_FOUND",
                    "TENANT_INVITATION_NOT_FOUND"
            ),
            "409", List.of(
                    "TENANT_ID_MISMATCH",
                    "TENANT_STATE_INVALID",
                    "TENANT_LAST_OWNER",
                    "TENANT_MEMBER_EXISTS",
                    "TENANT_INVITATION_STATE_INVALID",
                    "TENANT_EVENT_REPLAY_GAP"
            ),
            "410", List.of("TENANT_INVITATION_EXPIRED"),
            "429", List.of("TENANT_QUOTA_EXCEEDED"),
            "503", List.of("TENANT_MIGRATION_NOT_READY")
    );

    private static final List<String> SSE_EVENTS = List.of(
            "progress",
            "issue",
            "complete",
            "agent_progress",
            "trace",
            "phase",
            "stats",
            "finding_added",
            "finding_updated",
            "finding_removed",
            "error"
    );

    @Test
    void t0OpenApi_hasStableVersionAndSecurityScheme() throws IOException {
        JsonNode spec = openApi();

        assertThat(spec.path("openapi").asText()).isEqualTo("3.0.3");
        assertThat(spec.path("info").path("version").asText()).isEqualTo("tenant-v1");
        assertThat(spec.path("x-contract-version").asText()).isEqualTo("tenant-v1");
        assertThat(spec.path("x-schema-version").asInt()).isEqualTo(1);

        JsonNode bearerAuth = spec.path("components").path("securitySchemes").path("BearerAuth");
        assertThat(bearerAuth.path("type").asText()).isEqualTo("http");
        assertThat(bearerAuth.path("scheme").asText()).isEqualTo("bearer");
        assertThat(bearerAuth.path("bearerFormat").asText()).isEqualTo("JWT");
    }

    @Test
    void t0OpenApi_protectedTenantOperationsRequireBearerAuth() throws IOException {
        JsonNode paths = openApi().path("paths");
        Map<String, List<String>> protectedOperations = Map.ofEntries(
                Map.entry("/api/auth/refresh", List.of("post")),
                Map.entry("/api/tenants", List.of("get", "post")),
                Map.entry("/api/tenants/current", List.of("get")),
                Map.entry("/api/tenants/{tenantId}", List.of("patch", "delete")),
                Map.entry("/api/tenants/{tenantId}/switch", List.of("post")),
                Map.entry("/api/tenants/{tenantId}/transfer-owner", List.of("post")),
                Map.entry("/api/tenants/{tenantId}/disable", List.of("post")),
                Map.entry("/api/tenants/{tenantId}/restore", List.of("post")),
                Map.entry("/api/tenants/{tenantId}/members", List.of("get")),
                Map.entry("/api/tenants/{tenantId}/members/{userId}", List.of("patch", "delete")),
                Map.entry("/api/tenants/{tenantId}/invitations", List.of("get", "post")),
                Map.entry("/api/tenants/{tenantId}/invitations/{invitationId}", List.of("delete")),
                Map.entry("/api/tenant-invitations/{token}/accept", List.of("post")),
                Map.entry("/api/audit-tasks/{taskId}/stream", List.of("get"))
        );

        protectedOperations.forEach((path, methods) -> methods.forEach(method -> {
            JsonNode operation = paths.path(path).path(method);
            assertThat(operation.isObject()).as(path + " " + method).isTrue();
            assertThat(operation.path("security").toString())
                    .as(path + " " + method)
                    .contains("BearerAuth");
        }));

        assertThat(paths.path("/api/auth/login").path("post").path("security").isMissingNode())
                .isTrue();
    }

    @Test
    void t0OpenApi_freezesTenantIdAndPaginationParameterShapes() throws IOException {
        JsonNode parameters = openApi().path("components").path("parameters");

        JsonNode tenantId = parameters.path("TenantId");
        assertThat(tenantId.path("name").asText()).isEqualTo("tenantId");
        assertThat(tenantId.path("in").asText()).isEqualTo("path");
        assertThat(tenantId.path("required").asBoolean()).isTrue();
        assertThat(tenantId.path("schema").path("type").asText()).isEqualTo("string");
        assertThat(tenantId.path("schema").path("pattern").asText()).isEqualTo("^[0-9]+$");

        JsonNode page = parameters.path("Page").path("schema");
        assertThat(page.path("type").asText()).isEqualTo("integer");
        assertThat(page.path("minimum").asInt()).isEqualTo(1);
        assertThat(page.path("default").asInt()).isEqualTo(1);

        JsonNode size = parameters.path("Size").path("schema");
        assertThat(size.path("type").asText()).isEqualTo("integer");
        assertThat(size.path("minimum").asInt()).isEqualTo(1);
        assertThat(size.path("maximum").asInt()).isEqualTo(100);
        assertThat(size.path("default").asInt()).isEqualTo(20);
    }

    @Test
    void t0OpenApi_responseAndEventSchemas_keepTenantFieldsStable() throws IOException {
        JsonNode schemas = openApi().path("components").path("schemas");

        assertThat(textArray(schemas.path("Result").path("required")))
                .containsExactly("code", "msg", "timestamp");
        assertThat(textArray(schemas.path("ErrorData").path("required")))
                .containsExactly("error_code", "request_id");
        assertThat(textArray(schemas.path("TenantSummary").path("required")))
                .containsExactly(
                        "tenant_id",
                        "tenant_code",
                        "name",
                        "status",
                        "role",
                        "permissions",
                        "is_current"
                );
        assertThat(textArray(schemas.path("EventEnvelope").path("required")))
                .containsExactly(
                        "schema_version",
                        "event_id",
                        "event",
                        "tenant_id",
                        "task_id",
                        "occurred_at",
                        "data"
                );
        assertThat(schemas.path("EventEnvelope").path("properties").path("schema_version")
                .path("enum").get(0).asInt()).isEqualTo(1);
        assertThat(schemas.path("EventEnvelope").path("properties").path("occurred_at")
                .path("format").asText()).isEqualTo("date-time");
    }

    @Test
    void t0OpenApi_publishesStableErrorCodeCatalog() throws IOException {
        JsonNode errorCodes = openApi().path("x-error-codes");

        ERROR_CODES.forEach((httpCode, expectedCodes) ->
                assertThat(textArray(errorCodes.path(httpCode)))
                        .as("HTTP " + httpCode)
                        .containsExactlyElementsOf(expectedCodes)
        );
    }

    @Test
    void t0OpenApi_sseExample_hasMatchingIdsAndTenantEnvelope() throws IOException {
        JsonNode spec = openApi();
        JsonNode sse = spec.path("x-sse");
        assertThat(sse.path("path").asText()).isEqualTo("/api/audit-tasks/{taskId}/stream");
        assertThat(textArray(sse.path("required_headers"))).containsExactly("Authorization");
        assertThat(textArray(sse.path("optional_headers"))).containsExactly("Last-Event-ID");
        assertThat(sse.path("event_schema").path("$ref").asText())
                .isEqualTo("#/components/schemas/EventEnvelope");
        assertThat(sse.path("heartbeat_comment_seconds").asInt()).isEqualTo(15);
        assertThat(sse.path("reconnect_retry_ms").asInt()).isEqualTo(3000);
        assertThat(textArray(sse.path("events"))).containsExactlyElementsOf(SSE_EVENTS);

        String example = spec.path("paths")
                .path("/api/audit-tasks/{taskId}/stream")
                .path("get")
                .path("responses")
                .path("200")
                .path("content")
                .path("text/event-stream")
                .path("example")
                .asText();
        assertThat(example).contains("id: 1002", "event: progress", "data: ");

        String dataLine = example.substring(example.indexOf("data: ") + "data: ".length()).trim();
        JsonNode event = JSON.readTree(dataLine);
        assertThat(event.path("schema_version").asInt()).isEqualTo(1);
        assertThat(event.path("event_id").asText()).isEqualTo("1002");
        assertThat(event.path("event").asText()).isEqualTo("progress");
        assertThat(event.path("tenant_id").asText()).isEqualTo("20001");
        assertThat(event.path("task_id").asText()).isNotBlank();
        assertThat(event.path("occurred_at").asText()).endsWith("Z");
        assertThat(event.path("data").isObject()).isTrue();
    }

    @Test
    void t0OpenApi_internalJavaRustContract_freezesHeadersSigningAndReplayWindow() throws IOException {
        JsonNode internal = openApi().path("x-internal-java-rust");

        assertThat(textArray(internal.path("required_headers"))).containsExactly(
                "X-Tenant-Id",
                "X-User-Id",
                "X-Request-Id",
                "X-Internal-Timestamp",
                "X-Internal-Signature"
        );
        assertThat(internal.path("algorithm").asText()).isEqualTo("HMAC-SHA256");
        assertThat(internal.path("signature_format").asText())
                .isEqualTo("v1=<lowercase hexadecimal digest>");
        assertThat(internal.path("timestamp_window_seconds").asInt()).isEqualTo(300);
        assertThat(internal.path("replay_ttl_seconds").asInt()).isEqualTo(600);
        assertThat(textArray(internal.path("canonical_lines"))).containsExactly(
                "v1",
                "{METHOD}",
                "{path_with_query}",
                "{X-Internal-Timestamp}",
                "{X-Tenant-Id}",
                "{X-User-Id}",
                "{X-Request-Id}",
                "{lowercase_hex(SHA-256(body_bytes))}"
        );
    }

    private static JsonNode openApi() throws IOException {
        List<Path> candidates = List.of(
                Path.of("docs", "tenant-openapi.json"),
                Path.of("backend-java", "docs", "tenant-openapi.json"),
                Path.of("..", "backend-java", "docs", "tenant-openapi.json")
        );
        for (Path candidate : candidates) {
            if (Files.isRegularFile(candidate)) {
                return JSON.readTree(Files.readString(candidate, StandardCharsets.UTF_8));
            }
        }
        throw new IOException("Unable to locate backend-java/docs/tenant-openapi.json");
    }

    private static List<String> textArray(JsonNode node) {
        return StreamSupport.stream(node.spliterator(), false)
                .map(JsonNode::asText)
                .toList();
    }
}
