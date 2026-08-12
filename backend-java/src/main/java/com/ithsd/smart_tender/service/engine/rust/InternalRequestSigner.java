package com.ithsd.smart_tender.service.engine.rust;

import com.ithsd.smart_tender.common.TenantContext;
import com.ithsd.smart_tender.common.TenantRequestContext;
import com.ithsd.smart_tender.config.RustApiProperties;

import javax.crypto.Mac;
import javax.crypto.spec.SecretKeySpec;
import java.net.URI;
import java.net.http.HttpRequest;
import java.nio.charset.StandardCharsets;
import java.security.GeneralSecurityException;
import java.security.MessageDigest;
import java.time.Clock;
import java.util.HexFormat;
import java.util.Locale;
import java.util.Objects;
import java.util.UUID;

/**
 * Adds the authenticated Java-to-Rust request envelope.
 *
 * <p>The canonical request is deliberately made from the exact request
 * method/path and bytes that are sent on the wire. Rust uses the same format
 * before it dispatches any {@code /api/v1} handler.</p>
 */
public final class InternalRequestSigner {

    public static final String SIGNATURE_VERSION = "v1";
    public static final String TENANT_HEADER = "X-Tenant-Id";
    public static final String USER_HEADER = "X-User-Id";
    public static final String REQUEST_HEADER = "X-Request-Id";
    public static final String TIMESTAMP_HEADER = "X-Internal-Timestamp";
    public static final String SIGNATURE_HEADER = "X-Internal-Signature";

    private final RustApiProperties properties;
    private final Clock clock;

    public InternalRequestSigner(RustApiProperties properties) {
        this(properties, Clock.systemUTC());
    }

    InternalRequestSigner(RustApiProperties properties, Clock clock) {
        this.properties = Objects.requireNonNull(properties, "properties");
        this.clock = Objects.requireNonNull(clock, "clock");
    }

    /**
     * Signs a request using the current {@link TenantContext}.
     *
     * @throws IllegalStateException when tenant context or the shared secret
     *                              is absent; no partially signed request is
     *                              returned in that case
     */
    public HttpRequest.Builder sign(
            HttpRequest.Builder builder,
            String method,
            URI uri,
            byte[] body
    ) {
        Objects.requireNonNull(builder, "builder");
        Objects.requireNonNull(method, "method");
        Objects.requireNonNull(uri, "uri");
        Objects.requireNonNull(body, "body");

        TenantRequestContext context = TenantContext.get();
        if (context == null
                || context.tenantId() == null
                || context.tenantId() <= 0
                || context.userId() == null
                || context.userId() <= 0
                || context.requestId() == null
                || context.requestId().isBlank()) {
            throw new IllegalStateException("TenantContext is required for internal Rust requests");
        }

        String secret = properties.getInternalSecret();
        if (secret == null || secret.isBlank()) {
            throw new IllegalStateException("Rust internal request secret is not configured");
        }

        String tenantId = Long.toString(context.tenantId());
        String userId = Long.toString(context.userId());
        String requestId = requireHeaderValue(
                "request id",
                context.requestId() + "." + UUID.randomUUID());
        String normalizedMethod = method.toUpperCase(Locale.ROOT);
        String pathAndQuery = pathAndQuery(uri);
        String timestamp = Long.toString(clock.instant().getEpochSecond());
        String bodySha256 = sha256(body);
        String canonicalRequest = String.join(
                "\n",
                SIGNATURE_VERSION,
                normalizedMethod,
                pathAndQuery,
                timestamp,
                tenantId,
                userId,
                requestId,
                bodySha256
        );
        String signature = hmacSha256(secret, canonicalRequest);

        return builder
                .header(TENANT_HEADER, tenantId)
                .header(USER_HEADER, userId)
                .header(REQUEST_HEADER, requestId)
                .header(TIMESTAMP_HEADER, timestamp)
                .header(SIGNATURE_HEADER, SIGNATURE_VERSION + "=" + signature);
    }

    static String pathAndQuery(URI uri) {
        String path = uri.getRawPath();
        if (path == null || path.isEmpty()) {
            path = "/";
        }
        String query = uri.getRawQuery();
        return query == null || query.isEmpty() ? path : path + "?" + query;
    }

    private static String requireHeaderValue(String field, String value) {
        if (value.isBlank() || value.indexOf('\r') >= 0 || value.indexOf('\n') >= 0) {
            throw new IllegalStateException("Invalid internal request " + field);
        }
        return value;
    }

    private static String sha256(byte[] body) {
        try {
            return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(body));
        } catch (GeneralSecurityException e) {
            throw new IllegalStateException("SHA-256 is unavailable", e);
        }
    }

    private static String hmacSha256(String secret, String canonicalRequest) {
        try {
            Mac mac = Mac.getInstance("HmacSHA256");
            mac.init(new SecretKeySpec(secret.getBytes(StandardCharsets.UTF_8), "HmacSHA256"));
            return HexFormat.of().formatHex(mac.doFinal(canonicalRequest.getBytes(StandardCharsets.UTF_8)));
        } catch (GeneralSecurityException e) {
            throw new IllegalStateException("HmacSHA256 is unavailable", e);
        }
    }
}
