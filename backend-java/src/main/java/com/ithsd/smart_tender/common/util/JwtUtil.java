package com.ithsd.smart_tender.common.util;

import io.jsonwebtoken.Claims;
import io.jsonwebtoken.ExpiredJwtException;
import io.jsonwebtoken.Jwts;
import io.jsonwebtoken.SignatureAlgorithm;
import java.nio.charset.StandardCharsets;
import java.util.Date;
import java.util.LinkedHashMap;
import java.util.Map;

public class JwtUtil {
    public static String createJWT(String secretKey, long ttlMillis, Map<String, Object> claims) {
        long expMillis = System.currentTimeMillis() + ttlMillis;
        Date exp = new Date(expMillis);
        Map<String, Object> safeClaims = new LinkedHashMap<>();
        if (claims != null) {
            safeClaims.putAll(claims);
        }
        return Jwts.builder()
                .setClaims(safeClaims)
                .setIssuedAt(new Date())
                .setExpiration(exp)
                .signWith(SignatureAlgorithm.HS256, secretKey.getBytes(StandardCharsets.UTF_8))
                .compact();
    }

    public static Claims parseJWT(String secretKey, String token) {
        return Jwts.parser()
                .setSigningKey(secretKey.getBytes(StandardCharsets.UTF_8))
                .parseClaimsJws(token)
                .getBody();
    }

    /**
     * 与 {@link #parseJWT} 相同，但容忍「签名有效、仅 exp 过期」的 token。
     * <p>
     * 签名错误、格式错误等仍会向上抛异常（不会被吞掉），只有 {@link ExpiredJwtException}
     * 被转为可用的 {@link Claims}。是否据此续期由调用方结合 Redis 会话存活状态决定——
     * 会话 TTL 即续期窗口上限，杜绝“任意旧 token 永不过期”。
     */
    public static Claims parseJWTPermissive(String secretKey, String token) {
        try {
            return parseJWT(secretKey, token);
        } catch (ExpiredJwtException ex) {
            return ex.getClaims();
        }
    }
}
