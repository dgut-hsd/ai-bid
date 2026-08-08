package com.ithsd.smart_tender.service;

import com.ithsd.smart_tender.common.TenantAuthException;
import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;

import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.assertj.core.api.Assertions.assertThatCode;

class TenantAuthorizationServiceTest {

    private final TenantAuthorizationService authorization = new TenantAuthorizationService();

    @AfterEach
    void clearContext() {
        TenantContext.clear();
    }

    @Test
    void ownerAndAdminCanReadMembersButViewerCannotChangeMembership() {
        TenantContext.set(new TenantRequestContext(1001L, 2001L, "OWNER", 1L, "owner-request"));
        assertThatCode(() -> authorization.requireTenant(2001L, "tenant.members.role.write"))
                .doesNotThrowAnyException();

        TenantContext.set(new TenantRequestContext(1002L, 2001L, "ADMIN", 1L, "admin-request"));
        assertThatCode(() -> authorization.requireTenant(2001L, "tenant.members.role.write"))
                .doesNotThrowAnyException();

        TenantContext.set(new TenantRequestContext(1003L, 2001L, "VIEWER", 1L, "viewer-request"));
        assertThatThrownBy(() -> authorization.requireTenant(2001L, "tenant.members.role.write"))
                .isInstanceOfSatisfying(TenantAuthException.class, ex -> {
                    org.assertj.core.api.Assertions.assertThat(ex.getStatus()).isEqualTo(403);
                    org.assertj.core.api.Assertions.assertThat(ex.getErrorCode())
                            .isEqualTo("TENANT_ROLE_FORBIDDEN");
                });
    }

    @Test
    void crossTenantPathIsNotFoundEvenForOwner() {
        TenantContext.set(new TenantRequestContext(1001L, 2001L, "OWNER", 1L, "request"));

        assertThatThrownBy(() -> authorization.requireTenant(2002L, "tenant.read"))
                .isInstanceOfSatisfying(TenantAuthException.class, ex -> {
                    org.assertj.core.api.Assertions.assertThat(ex.getStatus()).isEqualTo(404);
                    org.assertj.core.api.Assertions.assertThat(ex.getErrorCode())
                            .isEqualTo("TENANT_NOT_FOUND");
                });
    }
}
