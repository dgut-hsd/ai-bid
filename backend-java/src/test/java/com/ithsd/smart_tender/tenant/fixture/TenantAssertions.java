package com.ithsd.smart_tender.tenant.fixture;

import org.assertj.core.api.Assertions;

import java.util.Collection;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;

/**
 * Assertions shared by contract-spec tests and future endpoint tests.
 */
public final class TenantAssertions {

    private TenantAssertions() {
    }

    public static void assertSameTenant(
            TenantFixture.Resource parent,
            TenantFixture.Resource child
    ) {
        assertThat(parent.tenantId()).isEqualTo(child.tenantId());
        assertThat(child.parentId()).isEqualTo(parent.id());
    }

    public static void assertTenantScoped(
            String expectedTenantId,
            TenantFixture.Resource resource
    ) {
        assertThat(resource.tenantId()).isEqualTo(expectedTenantId);
    }

    public static void assertTenantScoped(
            String expectedTenantId,
            Collection<? extends TenantFixture.Resource> resources
    ) {
        assertThat(resources)
                .allSatisfy(resource -> assertThat(resource.tenantId()).isEqualTo(expectedTenantId));
    }

    public static void assertCrossTenant(
            TenantFixture.Tenant requester,
            TenantFixture.Resource target
    ) {
        assertThat(target.tenantId()).isNotEqualTo(requester.id());
    }

    public static void assertTenantPath(
            String tenantId,
            String path
    ) {
        assertThat(path).contains("/" + tenantId + "/");
    }

    public static void assertSseEvent(
            TenantFixture.Tenant tenant,
            TenantFixture.SseEvent event
    ) {
        assertThat(event.schemaVersion()).isEqualTo(1);
        assertThat(event.eventId()).isNotBlank();
        assertThat(event.event()).isNotBlank();
        assertThat(event.tenantId()).isEqualTo(tenant.id());
        assertThat(event.taskId()).isNotBlank();
        assertThat(event.occurredAt()).endsWith("Z");
        assertThat(event.data()).isNotEmpty();
    }

    public static void assertQueueMessage(
            TenantFixture.Tenant tenant,
            TenantFixture.QueueMessage message,
            String expectedActorUserId
    ) {
        assertThat(message.schemaVersion()).isEqualTo(1);
        assertThat(message.tenantId()).isEqualTo(tenant.id());
        assertThat(message.taskId()).isNotBlank();
        assertThat(message.actorUserId()).isEqualTo(expectedActorUserId);
        assertThat(message.requestId()).isNotBlank();
    }

    public static void assertInternalRequest(
            TenantFixture.Tenant tenant,
            TenantFixture.InternalRequest request
    ) {
        assertThat(request.tenantId()).isEqualTo(tenant.id());
        assertThat(request.userId()).isNotBlank();
        assertThat(request.requestId()).isNotBlank();
        assertThat(request.bodySha256()).matches("[0-9a-f]{64}");
        assertThat(request.canonicalRequest()).doesNotEndWith("\n");
        assertThat(request.canonicalRequest()).contains("\n" + tenant.id() + "\n");
        assertThat(request.replayKey()).isEqualTo(tenant.id() + ":" + request.requestId());
        assertThat(request.headers()).containsEntry("X-Tenant-Id", tenant.id());
        assertThat(request.headers()).containsEntry("X-User-Id", request.userId());
        assertThat(request.headers()).containsEntry("X-Request-Id", request.requestId());
        assertThat(request.headers()).containsEntry("X-Internal-Timestamp", request.timestamp());
        assertThat(request.headers()).containsEntry("X-Internal-Signature", request.signature());
        assertThat(request.signature()).matches("v1=[0-9a-f]{64}");
    }

    public static void assertErrorShape(
            Map<String, Object> data,
            String expectedErrorCode,
            String expectedRequestId
    ) {
        assertThat(data).containsEntry("error_code", expectedErrorCode);
        assertThat(data).containsEntry("request_id", expectedRequestId);
    }
}
