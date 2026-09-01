package com.ithsd.smart_tender.common.util;

import org.junit.jupiter.api.Test;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

class PasswordServiceTest {

    private final PasswordService passwordService = new PasswordService();

    @Test
    void encodeProducesBcryptHash() {
        String hash = passwordService.encode("s3cret-password");
        assertThat(hash).startsWith("$2");
        assertThat(passwordService.requiresRehash(hash)).isFalse();
    }

    @Test
    void matchesRoundTripsBcryptHash() {
        String hash = passwordService.encode("s3cret-password");
        assertThat(passwordService.matches("s3cret-password", hash)).isTrue();
        assertThat(passwordService.matches("wrong-password", hash)).isFalse();
    }

    @Test
    void matchesLegacyMd5AndRequiresRehash() {
        // MD5("123456") 的历史种子哈希
        String legacy = "e10adc3949ba59abbe56e057f20f883e";
        assertThat(passwordService.matches("123456", legacy)).isTrue();
        assertThat(passwordService.matches("000000", legacy)).isFalse();
        assertThat(passwordService.requiresRehash(legacy)).isTrue();
    }

    @Test
    void matchesHandlesNullInputsGracefully() {
        assertThat(passwordService.matches("x", null)).isFalse();
        assertThat(passwordService.matches(null, "$2a$10$abcdefghijklmnopqrstuv")).isFalse();
    }

    @Test
    void encodeRejectsBlankPassword() {
        assertThatThrownBy(() -> passwordService.encode("")).isInstanceOf(IllegalArgumentException.class);
        assertThatThrownBy(() -> passwordService.encode(null)).isInstanceOf(IllegalArgumentException.class);
    }
}