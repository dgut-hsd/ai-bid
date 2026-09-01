package com.ithsd.smart_tender.common.util;

import org.springframework.security.crypto.bcrypt.BCryptPasswordEncoder;
import org.springframework.stereotype.Component;

/**
 * 密码哈希服务（S2）。
 *
 * <p>统一密码的生成与校验，并将历史无盐 MD5 哈希在下次成功登录时透明升级为 BCrypt。
 * BCrypt 哈希以 {@code $2a$}/{@code $2b$}/{@code $2y$} 开头，历史哈希是 32 位小写十六进制，
 * 因此可按前缀区分两种格式，无需额外迁移字段。</p>
 */
@Component
public class PasswordService {

    private static final int DEFAULT_STRENGTH = 10;

    private final BCryptPasswordEncoder encoder = new BCryptPasswordEncoder(DEFAULT_STRENGTH);

    /** 生成新的密码哈希（BCrypt）。 */
    public String encode(String rawPassword) {
        if (rawPassword == null || rawPassword.isEmpty()) {
            throw new IllegalArgumentException("password must not be empty");
        }
        return encoder.encode(rawPassword);
    }

    /** 校验明文密码是否匹配存储的哈希，同时兼容历史 MD5 哈希。 */
    public boolean matches(String rawPassword, String storedHash) {
        if (rawPassword == null || storedHash == null) {
            return false;
        }
        if (isBcrypt(storedHash)) {
            try {
                return encoder.matches(rawPassword, storedHash);
            } catch (IllegalArgumentException malformed) {
                // 非法 bcrypt 串回退到历史 MD5 比较，避免校验逻辑被格式问题击穿。
                return legacyMd5Matches(rawPassword, storedHash);
            }
        }
        return legacyMd5Matches(rawPassword, storedHash);
    }

    /** 存储哈希是否为历史 MD5，需要升级为 BCrypt。 */
    public boolean requiresRehash(String storedHash) {
        return storedHash != null && !isBcrypt(storedHash);
    }

    private boolean legacyMd5Matches(String rawPassword, String storedHash) {
        return MD5Util.encrypt(rawPassword).equalsIgnoreCase(storedHash);
    }

    private boolean isBcrypt(String hash) {
        return hash.startsWith("$2a$") || hash.startsWith("$2b$") || hash.startsWith("$2y$");
    }
}