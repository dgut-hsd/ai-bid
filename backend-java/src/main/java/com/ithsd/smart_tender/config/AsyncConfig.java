package com.ithsd.smart_tender.config;

import com.ithsd.smart_tender.common.TenantContext;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;
import org.springframework.scheduling.concurrent.ThreadPoolTaskExecutor;

import java.util.concurrent.Executor;
import java.util.concurrent.ThreadPoolExecutor;

@Configuration
public class AsyncConfig {
    private static final Logger log = LoggerFactory.getLogger(AsyncConfig.class);

    @Bean("auditTaskExecutor")
    public Executor auditTaskExecutor(
            @Value("${audit.async.core-pool-size:2}") Integer corePoolSize,
            @Value("${audit.async.max-pool-size:4}") Integer maxPoolSize,
            @Value("${audit.async.queue-capacity:100}") Integer queueCapacity,
            @Value("${audit.async.keep-alive-seconds:60}") Integer keepAliveSeconds
    ) {
        ThreadPoolTaskExecutor executor = new ThreadPoolTaskExecutor();
        executor.setCorePoolSize(corePoolSize);
        executor.setMaxPoolSize(maxPoolSize);
        executor.setQueueCapacity(queueCapacity);
        executor.setKeepAliveSeconds(keepAliveSeconds);
        executor.setThreadNamePrefix("audit-async-");
        executor.setWaitForTasksToCompleteOnShutdown(true);
        executor.setAwaitTerminationSeconds(10);
        // 提交任务时在调用方线程捕获 TenantContext 快照，并在工作线程上安装。
        // 否则 Java→Rust 的内部请求签名（InternalRequestSigner）拿不到租户身份，
        // 会直接抛 "TenantContext is required for internal Rust requests"。
        executor.setTaskDecorator(TenantContext::wrap);
        executor.setRejectedExecutionHandler((r, e) -> {
            log.warn("audit executor rejected task, active={}, queueSize={}", e.getActiveCount(), e.getQueue().size());
            new ThreadPoolExecutor.CallerRunsPolicy().rejectedExecution(r, e);
        });
        executor.initialize();
        return executor;
    }
}
