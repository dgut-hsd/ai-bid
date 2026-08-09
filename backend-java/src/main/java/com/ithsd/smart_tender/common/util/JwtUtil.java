package com.ithsd.smart_tender.common.util;

import io.jsonwebtoken.Claims;
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
}
