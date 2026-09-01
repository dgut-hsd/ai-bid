package com.ithsd.smart_tender.service;

import com.ithsd.smart_tender.model.dto.CreateAuditTaskRequest;
import com.ithsd.smart_tender.model.dto.rust.RustBlockBBoxResponse;
import com.ithsd.smart_tender.model.vo.AuditTaskCreateVO;
import com.ithsd.smart_tender.model.vo.AuditTaskStatusVO;
import com.ithsd.smart_tender.model.vo.ResultVO;
import org.springframework.web.servlet.mvc.method.annotation.SseEmitter;

import java.util.List;
import java.util.Map;

public interface AuditTaskService {
    AuditTaskCreateVO createTask(CreateAuditTaskRequest request);

    AuditTaskStatusVO getStatus(String taskId);

    AuditTaskStatusVO getStatusByBid(Long bidId);

    ResultVO getResult(String taskId, Integer page, Integer size, String sinceIssueNo);

    SseEmitter subscribeStream(String taskId, String lastEventId);

    List<Long> getAuditIdsByBidIds(List<Long> bidIds);

    Map<String, Long> countByWeek();

    void processAuditResult(String taskId, String responseBody);

    void markTaskProcessing(String taskId);

    void markTaskFailed(String taskId, String errorMessage);

    /** 查询指定 block_id 的 BBox 坐标（代理到 Rust 引擎） */
    List<RustBlockBBoxResponse> getBlockBboxes(String taskId, String blockIds);
}
