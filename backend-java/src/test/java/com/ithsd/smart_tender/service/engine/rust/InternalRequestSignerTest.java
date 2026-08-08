package com.ithsd.smart_tender.service.engine.rust;

import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.config.RustApiProperties;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;

import javax.crypto.Mac;
import javax.crypto.spec.SecretKeySpec;
import java.net.URI;
import java.net.http.HttpRequest;
import java.nio.charset.StandardCharsets;
import java.time.Clock;
import java.time.Instant;
import java.time.ZoneOffset;
import java.util.HexFormat;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

class InternalRequestSignerTest {

    private static final String SECRET = "java-rust-test-secret";
    private static final Instant FIXED_TIME = Instant.ofEpochSecond(1_786_019_460L);

    @AfterEach
    void clearContext() {
        TenantContext.clear();
    }

    @Test
    void signsExactBodyAndRequestIdentity() throws Exception {
        RustApiProperties properties = new RustApiProperties();
        properties.setInternalSecret(SECRET);
        InternalRequestSigner signer = new InternalRequestSigner(
                properties,
                Clock.fixed(FIXED_TIME, ZoneOffset.UTC));
        TenantContext.set(new TenantRequestContext(10001L, 20001L, "ADMIN", 1L, "request-1"));

        byte[] body = "{\"hello\":\"世界\"}".getBytes(StandardCharsets.UTF_8);
        URI uri = URI.create("http://127.0.0.1:3001/api/v1/documents?mode=full");
        HttpRequest request = signer.sign(
                        HttpRequest.newBuilder().uri(uri),
                        "POST",
                        uri,
                        body)
                .POST(HttpRequest.BodyPublishers.ofByteArray(body))
                .build();
        HttpRequest secondRequest = signer.sign(
                        HttpRequest.newBuilder().uri(uri),
                        "POST",
                        uri,
                        body)
                .POST(HttpRequest.BodyPublishers.ofByteArray(body))
                .build();

        String bodySha256 = HexFormat.of().formatHex(
                java.security.MessageDigest.getInstance("SHA-256").digest(body));
        String requestId = header(request, InternalRequestSigner.REQUEST_HEADER);
        String secondRequestId = header(secondRequest, InternalRequestSigner.REQUEST_HEADER);
        String canonical = String.join(
                "\n",
                "v1",
                "POST",
                "/api/v1/documents?mode=full",
                "1786019460",
                "20001",
                "10001",
                requestId,
                bodySha256);
        Mac mac = Mac.getInstance("HmacSHA256");
        mac.init(new SecretKeySpec(SECRET.getBytes(StandardCharsets.UTF_8), "HmacSHA256"));

        assertThat(header(request, InternalRequestSigner.TENANT_HEADER)).isEqualTo("20001");
        assertThat(header(request, InternalRequestSigner.USER_HEADER)).isEqualTo("10001");
        assertThat(requestId).startsWith("request-1.");
        assertThat(secondRequestId).startsWith("request-1.");
        assertThat(secondRequestId).isNotEqualTo(requestId);
        assertThat(header(request, InternalRequestSigner.TIMESTAMP_HEADER)).isEqualTo("1786019460");
        assertThat(header(request, InternalRequestSigner.SIGNATURE_HEADER))
                .isEqualTo("v1=" + HexFormat.of().formatHex(mac.doFinal(canonical.getBytes(StandardCharsets.UTF_8))));
    }

    @Test
    void refusesToSignWithoutTenantContext() {
        RustApiProperties properties = new RustApiProperties();
        properties.setInternalSecret(SECRET);
        InternalRequestSigner signer = new InternalRequestSigner(properties);
        URI uri = URI.create("http://127.0.0.1:3001/api/v1/documents");

        assertThatThrownBy(() -> signer.sign(
                HttpRequest.newBuilder().uri(uri), "GET", uri, new byte[0]))
                .isInstanceOf(IllegalStateException.class)
                .hasMessageContaining("TenantContext");
    }

    @Test
    void refusesToSignWithoutSecret() {
        RustApiProperties properties = new RustApiProperties();
        InternalRequestSigner signer = new InternalRequestSigner(properties);
        TenantContext.set(new TenantRequestContext(10001L, 20001L, "ADMIN", 1L, "request-1"));
        URI uri = URI.create("http://127.0.0.1:3001/api/v1/documents");

        assertThatThrownBy(() -> signer.sign(
                HttpRequest.newBuilder().uri(uri), "GET", uri, new byte[0]))
                .isInstanceOf(IllegalStateException.class)
                .hasMessageContaining("secret");
    }

    private static String header(HttpRequest request, String name) {
        return request.headers().firstValue(name).orElseThrow();
    }
}
