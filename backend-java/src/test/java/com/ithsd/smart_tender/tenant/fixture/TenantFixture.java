package com.ithsd.smart_tender.tenant.fixture;

import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Set;

/**
 * Reusable two-tenant fixture for contract and isolation tests.
 *
 * <p>The fixture deliberately models data rather than calling an endpoint. That
 * keeps the T0 tests runnable before the T1-T8 implementation slices land.</p>
 */
public final class TenantFixture {

    public static final String TENANT_A_ID = "20001";
    public static final String TENANT_B_ID = "20002";
    public static final String ACTOR_USER_ID = "10001";
    public static final String REQUEST_ID = "8b9c0e7f-9d8b-4f86-b77d-9a3d4c2f5001";
    public static final String OCCURRED_AT = "2026-08-06T13:01:00.123Z";

    public record Tenant(String id, String code, String role, Set<String> permissions) {
        public Tenant {
            Objects.requireNonNull(id);
            Objects.requireNonNull(code);
            Objects.requireNonNull(role);
            permissions = Set.copyOf(permissions);
        }
    }

    public record Resource(
            String id,
            String kind,
            String tenantId,
            String parentId,
            String storagePath
    ) {
    }

    public record Page<T>(
            String tenantId,
            String query,
            int page,
            int size,
            long total,
            List<T> items
    ) {
        public Page {
            items = List.copyOf(items);
        }
    }

    public record DownloadPreview(
            String resourceId,
            String tenantId,
            String downloadPath,
            String previewPath
    ) {
    }

    public record SseEvent(
            int schemaVersion,
            String eventId,
            String event,
            String tenantId,
            String taskId,
            String occurredAt,
            Map<String, Object> data
    ) {
        public SseEvent {
            data = Map.copyOf(data);
        }
    }

    public record QueueMessage(
            int schemaVersion,
            String tenantId,
            String taskId,
            String actorUserId,
            String requestId
    ) {
    }

    public record InternalRequest(
            String method,
            String pathWithQuery,
            String bodySha256,
            String timestamp,
            String tenantId,
            String userId,
            String requestId,
            String signature,
            String replayKey,
            String canonicalRequest,
            Map<String, String> headers
    ) {
        public InternalRequest {
            headers = Map.copyOf(headers);
        }
    }

    private final String actorUserId;
    private final Tenant tenantA;
    private final Tenant tenantB;
    private final Map<String, Resource> resources;
    private final Map<String, Page<Resource>> pages;
    private final Map<String, DownloadPreview> downloadPreviews;
    private final Map<String, SseEvent> events;
    private final Map<String, QueueMessage> queueMessages;
    private final Map<String, InternalRequest> internalRequests;

    private TenantFixture(
            String actorUserId,
            Tenant tenantA,
            Tenant tenantB,
            Map<String, Resource> resources,
            Map<String, Page<Resource>> pages,
            Map<String, DownloadPreview> downloadPreviews,
            Map<String, SseEvent> events,
            Map<String, QueueMessage> queueMessages,
            Map<String, InternalRequest> internalRequests
    ) {
        this.actorUserId = actorUserId;
        this.tenantA = tenantA;
        this.tenantB = tenantB;
        this.resources = Map.copyOf(resources);
        this.pages = Map.copyOf(pages);
        this.downloadPreviews = Map.copyOf(downloadPreviews);
        this.events = Map.copyOf(events);
        this.queueMessages = Map.copyOf(queueMessages);
        this.internalRequests = Map.copyOf(internalRequests);
    }

    public static Builder builder() {
        return new Builder();
    }

    public static TenantFixture defaults() {
        return builder().build();
    }

    public String actorUserId() {
        return actorUserId;
    }

    public Tenant tenantA() {
        return tenantA;
    }

    public Tenant tenantB() {
        return tenantB;
    }

    public Resource resource(Tenant tenant, String resourceId) {
        return resources.get(resourceKey(tenant.id(), resourceId));
    }

    private static String resourceKey(String tenantId, String resourceId) {
        return tenantId + ":" + resourceId;
    }

    public Page<Resource> pageFor(Tenant tenant) {
        return pages.get(tenant.id());
    }

    public DownloadPreview downloadPreviewFor(Tenant tenant) {
        return downloadPreviews.get(tenant.id());
    }

    public SseEvent eventFor(Tenant tenant) {
        return events.get(tenant.id());
    }

    public QueueMessage queueMessageFor(Tenant tenant) {
        return queueMessages.get(tenant.id());
    }

    public InternalRequest internalRequestFor(Tenant tenant) {
        return internalRequests.get(tenant.id());
    }

    public static final class Builder {
        private String actorUserId = ACTOR_USER_ID;
        private Tenant tenantA = new Tenant(
                TENANT_A_ID,
                "acme-bid",
                "ADMIN",
                Set.of("tenant.read", "tender.write", "audit.start", "audit.report.read")
        );
        private Tenant tenantB = new Tenant(
                TENANT_B_ID,
                "solo-10001",
                "OWNER",
                Set.of("tenant.read", "tenant.delete", "audit.report.read")
        );

        public Builder actorUserId(String actorUserId) {
            this.actorUserId = Objects.requireNonNull(actorUserId);
            return this;
        }

        public Builder tenantA(Tenant tenantA) {
            this.tenantA = Objects.requireNonNull(tenantA);
            return this;
        }

        public Builder tenantB(Tenant tenantB) {
            this.tenantB = Objects.requireNonNull(tenantB);
            return this;
        }

        public TenantFixture build() {
            Resource projectA = resource(
                    "project-01",
                    "project",
                    tenantA,
                    null
            );
            Resource documentA = resource(
                    "document-01",
                    "document",
                    tenantA,
                    projectA.id()
            );
            Resource projectB = resource(
                    "project-01",
                    "project",
                    tenantB,
                    null
            );
            Resource documentB = resource(
                    "document-01",
                    "document",
                    tenantB,
                    projectB.id()
            );

            Map<String, Resource> resources = Map.of(
                    resourceKey(tenantA.id(), projectA.id()), projectA,
                    resourceKey(tenantA.id(), documentA.id()), documentA,
                    resourceKey(tenantB.id(), projectB.id()), projectB,
                    resourceKey(tenantB.id(), documentB.id()), documentB
            );
            Map<String, Page<Resource>> pages = Map.of(
                    tenantA.id(), new Page<>(
                            tenantA.id(),
                            "smart campus",
                            1,
                            20,
                            1,
                            List.of(documentA)
                    ),
                    tenantB.id(), new Page<>(
                            tenantB.id(),
                            "smart campus",
                            1,
                            20,
                            1,
                            List.of(documentB)
                    )
            );
            Map<String, DownloadPreview> downloadPreviews = Map.of(
                    tenantA.id(), downloadPreview(documentA),
                    tenantB.id(), downloadPreview(documentB)
            );
            Map<String, SseEvent> events = Map.of(
                    tenantA.id(), event(tenantA, "task-a", "1002"),
                    tenantB.id(), event(tenantB, "task-b", "2002")
            );
            Map<String, QueueMessage> queueMessages = Map.of(
                    tenantA.id(), new QueueMessage(1, tenantA.id(), "task-a", actorUserId, REQUEST_ID),
                    tenantB.id(), new QueueMessage(1, tenantB.id(), "task-b", actorUserId, REQUEST_ID)
            );
            Map<String, InternalRequest> internalRequests = Map.of(
                    tenantA.id(), internalRequest(tenantA),
                    tenantB.id(), internalRequest(tenantB)
            );

            return new TenantFixture(
                    actorUserId,
                    tenantA,
                    tenantB,
                    resources,
                    pages,
                    downloadPreviews,
                    events,
                    queueMessages,
                    internalRequests
            );
        }

        private Resource resource(String id, String kind, Tenant tenant, String parentId) {
            return new Resource(
                    id,
                    kind,
                    tenant.id(),
                    parentId,
                    "storage/tenants/" + tenant.id() + "/tenders/2026-08-06/" + id + ".pdf"
            );
        }

        private DownloadPreview downloadPreview(Resource resource) {
            return new DownloadPreview(
                    resource.id(),
                    resource.tenantId(),
                    "/api/resources/" + resource.id() + "/download",
                    "/api/resources/" + resource.id() + "/preview"
            );
        }

        private SseEvent event(Tenant tenant, String taskId, String eventId) {
            return new SseEvent(
                    1,
                    eventId,
                    "progress",
                    tenant.id(),
                    taskId,
                    OCCURRED_AT,
                    Map.of("stage", "legal", "percent", 42, "message", "reviewing")
            );
        }

        private InternalRequest internalRequest(Tenant tenant) {
            String timestamp = "1786019460";
            String bodySha256 = "a".repeat(64);
            String signature = "v1=" + "b".repeat(64);
            String canonicalRequest = String.join(
                    "\n",
                    "v1",
                    "POST",
                    "/api/v1/review/document-01/stream?mode=full",
                    timestamp,
                    tenant.id(),
                    actorUserId,
                    REQUEST_ID,
                    bodySha256
            );
            Map<String, String> headers = Map.of(
                    "X-Tenant-Id", tenant.id(),
                    "X-User-Id", actorUserId,
                    "X-Request-Id", REQUEST_ID,
                    "X-Internal-Timestamp", timestamp,
                    "X-Internal-Signature", signature
            );
            return new InternalRequest(
                    "POST",
                    "/api/v1/review/document-01/stream?mode=full",
                    bodySha256,
                    timestamp,
                    tenant.id(),
                    actorUserId,
                    REQUEST_ID,
                    signature,
                    tenant.id() + ":" + REQUEST_ID,
                    canonicalRequest,
                    headers
            );
        }
    }
}
