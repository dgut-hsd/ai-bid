package com.ithsd.smart_tender.tenant.contract;

import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;

import static org.assertj.core.api.Assertions.assertThat;

class TenantDatabaseSafetyContractTest {

    @Test
    void applicationProfiles_disableSqlInitialization() throws IOException {
        String application = resource("application.yml");
        String production = resource("application-prod.yml");

        assertThat(application).contains("  sql:\n    init:\n      mode: never");
        assertThat(production).contains("spring:\n  sql:\n    init:\n      mode: never");
        assertThat(application).doesNotContain("schema-locations");
    }

    @Test
    void destructiveLegacyScripts_areNotStartupInputs() throws IOException {
        String application = resource("application.yml");
        String production = resource("application-prod.yml");

        assertThat(resource("audit_task_event.sql"))
                .containsPattern("(?im)^\\s*DROP\\s+TABLE\\b");
        assertThat(resource("trace_schema.sql"))
                .containsPattern("(?im)^\\s*DROP\\s+TABLE\\b");
        assertThat(application).doesNotContain("audit_task_event.sql", "trace_schema.sql");
        assertThat(production).doesNotContain("audit_task_event.sql", "trace_schema.sql");
    }

    private static String resource(String name) throws IOException {
        try (InputStream input = TenantDatabaseSafetyContractTest.class
                .getClassLoader()
                .getResourceAsStream(name)) {
            if (input == null) {
                throw new IOException("Missing classpath resource: " + name);
            }
            return new String(input.readAllBytes(), StandardCharsets.UTF_8)
                    .replace("\r\n", "\n");
        }
    }
}
