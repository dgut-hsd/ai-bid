package com.ithsd.smart_tender.service.engine.queue;

import org.springframework.boot.context.properties.ConfigurationProperties;
import org.springframework.stereotype.Component;

@Component
@ConfigurationProperties(prefix = "audit.queue")
public class AuditQueueProperties {
    private String mode = "async";
    private String streamKey = "queue:audit:tasks";
    private String consumerGroup = "audit-task-workers";
    private String consumerNamePrefix = "worker";
    private Integer batchSize = 5;
    private Integer blockMs = 1000;
    private Integer pollDelayMs = 500;
    private Integer maxRetry = 3;
    private String dlqStreamKey = "queue:audit:tasks:dlq";
    private String dlqListKey = "queue:audit:tasks:list-dlq";

    public String getMode() {
        return mode;
    }

    public void setMode(String mode) {
        this.mode = mode;
    }

    public String getStreamKey() {
        return streamKey;
    }

    public void setStreamKey(String streamKey) {
        this.streamKey = streamKey;
    }

    public String getConsumerGroup() {
        return consumerGroup;
    }

    public void setConsumerGroup(String consumerGroup) {
        this.consumerGroup = consumerGroup;
    }

    public String getConsumerNamePrefix() {
        return consumerNamePrefix;
    }

    public void setConsumerNamePrefix(String consumerNamePrefix) {
        this.consumerNamePrefix = consumerNamePrefix;
    }

    public Integer getBatchSize() {
        return batchSize;
    }

    public void setBatchSize(Integer batchSize) {
        this.batchSize = batchSize;
    }

    public Integer getBlockMs() {
        return blockMs;
    }

    public void setBlockMs(Integer blockMs) {
        this.blockMs = blockMs;
    }

    public Integer getPollDelayMs() {
        return pollDelayMs;
    }

    public void setPollDelayMs(Integer pollDelayMs) {
        this.pollDelayMs = pollDelayMs;
    }

    public Integer getMaxRetry() {
        return maxRetry;
    }

    public void setMaxRetry(Integer maxRetry) {
        this.maxRetry = maxRetry;
    }

    public String getDlqStreamKey() {
        return dlqStreamKey;
    }

    public void setDlqStreamKey(String dlqStreamKey) {
        this.dlqStreamKey = dlqStreamKey;
    }

    public String getDlqListKey() {
        return dlqListKey;
    }

    public void setDlqListKey(String dlqListKey) {
        this.dlqListKey = dlqListKey;
    }
}
