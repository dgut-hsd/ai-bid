package com.ithsd.smart_tender.tenant.contract;

import com.ithsd.smart_tender.tenant.fixture.TenantAssertions;
import com.ithsd.smart_tender.tenant.fixture.TenantFixture;
import com.ithsd.smart_tender.tenant.fixture.TenantSecurityMatrix;
import org.junit.jupiter.api.Test;

import static org.assertj.core.api.Assertions.assertThat;

class TenantIsolationFixtureTest {

    private final TenantFixture fixture = TenantFixture.defaults();

    @Test
    void fixture_separatesTenantIdsAndResourceIds() {
        assertThat(fixture.tenantA().id()).isNotEqualTo(fixture.tenantB().id());
        assertThat(fixture.resource(fixture.tenantA(), "document-01").id())
                .isEqualTo(fixture.resource(fixture.tenantB(), "document-01").id());

        TenantAssertions.assertTenantScoped(
                fixture.tenantA().id(),
                fixture.resource(fixture.tenantA(), "document-01")
        );
        TenantAssertions.assertTenantScoped(
                fixture.tenantB().id(),
                fixture.resource(fixture.tenantB(), "document-01")
        );
        TenantAssertions.assertCrossTenant(
                fixture.tenantA(),
                fixture.resource(fixture.tenantB(), "document-01")
        );
    }

    @Test
    void fixture_keepsPaginationAndSearchResultsTenantScoped() {
        TenantFixture.Page<TenantFixture.Resource> pageA = fixture.pageFor(fixture.tenantA());
        TenantFixture.Page<TenantFixture.Resource> pageB = fixture.pageFor(fixture.tenantB());

        assertThat(pageA.page()).isEqualTo(1);
        assertThat(pageA.size()).isEqualTo(20);
        assertThat(pageA.query()).isEqualTo("smart campus");
        TenantAssertions.assertTenantScoped(fixture.tenantA().id(), pageA.items());
        TenantAssertions.assertTenantScoped(fixture.tenantB().id(), pageB.items());
    }

    @Test
    void fixture_keepsParentAndChildResourcesInTheSameTenant() {
        TenantAssertions.assertSameTenant(
                fixture.resource(fixture.tenantA(), "project-01"),
                fixture.resource(fixture.tenantA(), "document-01")
        );
        TenantAssertions.assertSameTenant(
                fixture.resource(fixture.tenantB(), "project-01"),
                fixture.resource(fixture.tenantB(), "document-01")
        );
        TenantAssertions.assertCrossTenant(
                fixture.tenantA(),
                fixture.resource(fixture.tenantB(), "project-01")
        );
    }

    @Test
    void fixture_tenantPrefixesDownloadPreviewAndStoragePaths() {
        TenantFixture.DownloadPreview downloadPreviewA =
                fixture.downloadPreviewFor(fixture.tenantA());
        TenantFixture.DownloadPreview downloadPreviewB =
                fixture.downloadPreviewFor(fixture.tenantB());

        TenantAssertions.assertTenantPath(
                fixture.tenantA().id(),
                fixture.resource(fixture.tenantA(), "document-01").storagePath()
        );
        assertThat(downloadPreviewA.tenantId()).isEqualTo(fixture.tenantA().id());
        assertThat(downloadPreviewB.tenantId()).isEqualTo(fixture.tenantB().id());
        assertThat(downloadPreviewA.resourceId()).isEqualTo(downloadPreviewB.resourceId());
        TenantAssertions.assertTenantPath(
                fixture.tenantB().id(),
                fixture.resource(fixture.tenantB(), "document-01").storagePath()
        );
    }

    @Test
    void fixture_propagatesTenantContextThroughSseAndQueue() {
        TenantAssertions.assertSseEvent(
                fixture.tenantA(),
                fixture.eventFor(fixture.tenantA())
        );
        TenantAssertions.assertQueueMessage(
                fixture.tenantA(),
                fixture.queueMessageFor(fixture.tenantA()),
                fixture.actorUserId()
        );
        assertThat(fixture.eventFor(fixture.tenantA()).tenantId())
                .isNotEqualTo(fixture.eventFor(fixture.tenantB()).tenantId());
        assertThat(fixture.queueMessageFor(fixture.tenantA()).requestId())
                .isEqualTo(fixture.queueMessageFor(fixture.tenantB()).requestId());
    }

    @Test
    void fixture_modelsTenantScopedRustHeadersAndReplayKeys() {
        TenantFixture.InternalRequest requestA = fixture.internalRequestFor(fixture.tenantA());
        TenantFixture.InternalRequest requestB = fixture.internalRequestFor(fixture.tenantB());

        TenantAssertions.assertInternalRequest(fixture.tenantA(), requestA);
        TenantAssertions.assertInternalRequest(fixture.tenantB(), requestB);
        assertThat(requestA.replayKey()).isNotEqualTo(requestB.replayKey());
        assertThat(requestA.canonicalRequest()).isNotEqualTo(requestB.canonicalRequest());
        assertThat(requestA.headers().keySet()).containsExactlyInAnyOrder(
                "X-Tenant-Id",
                "X-User-Id",
                "X-Request-Id",
                "X-Internal-Timestamp",
                "X-Internal-Signature"
        );
    }

    @Test
    void negativeMatrix_isExplicitAboutFutureActivationPoints() {
        assertThat(TenantSecurityMatrix.cases()).hasSize(10);
        assertThat(TenantSecurityMatrix.cases())
                .extracting(TenantSecurityMatrix.NegativeCase::expectedErrorCode)
                .contains("RESOURCE_NOT_FOUND", "TENANT_CONTEXT_INVALID", "INTERNAL_TENANT_MISMATCH");
        assertThat(java.util.Set.of(
                TenantSecurityMatrix.Activation.T3_RESOURCE_AND_PARENT_CHILD,
                TenantSecurityMatrix.Activation.T4_DOWNLOAD_AND_PREVIEW,
                TenantSecurityMatrix.Activation.T5_SSE_REPLAY,
                TenantSecurityMatrix.Activation.T6_QUEUE_AND_OUTBOX,
                TenantSecurityMatrix.Activation.T7_RUST_INTERNAL_HEADERS,
                TenantSecurityMatrix.Activation.T8_REGRESSION_SWEEP
        )).containsExactlyInAnyOrderElementsOf(
                java.util.Set.copyOf(
                        TenantSecurityMatrix.cases().stream()
                                .map(TenantSecurityMatrix.NegativeCase::activation)
                                .toList()
                )
        );
    }
}
