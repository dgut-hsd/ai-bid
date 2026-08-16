package com.ithsd.smart_tender;

import org.junit.jupiter.api.Test;
import org.springframework.boot.test.context.SpringBootTest;
import org.springframework.test.context.TestPropertySource;

@SpringBootTest
// prod 配置要求显式注入 JWT / 内部签名密钥（无硬编码回退），测试上下文注入占位值即可加载。
@TestPropertySource(properties = {
        "ST_JWT_SECRET=test-jwt-secret-for-context-loading-only",
        "RUST_API_INTERNAL_SECRET=test-internal-secret-for-context-loading-only"
})
class SmartTenderApplicationTests {

    @Test
    void contextLoads() {
    }

}
