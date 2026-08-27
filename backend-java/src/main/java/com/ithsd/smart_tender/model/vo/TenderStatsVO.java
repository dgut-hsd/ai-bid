package com.ithsd.smart_tender.model.vo;

import lombok.AllArgsConstructor;
import lombok.Builder;
import lombok.Data;
import lombok.NoArgsConstructor;

import java.io.Serializable;

/**
 * 标书审核状态统计（首页顶部状态 Tab 角标）。
 *
 * <p>状态口径与 ProjectMapper#selectProjectPageWithStatus 保持一致
 * （基于项目「最新版本标书」的「最新审核任务」）：
 * <ul>
 *   <li>pendingCount    待审核：最新版标书尚无审核任务</li>
 *   <li>processingCount 审核中：最新审核任务为 PENDING(0)/PROCESSING(1)</li>
 *   <li>completedCount  已完成：最新审核任务 COMPLETED(2)</li>
 *   <li>failedCount     审核失败：最新审核任务 FAILED(3)</li>
 * </ul>
 */
@Data
@Builder
@NoArgsConstructor
@AllArgsConstructor
public class TenderStatsVO implements Serializable {
    private Long allCount;        // 全部
    private Long pendingCount;    // 待审核（status=0）
    private Long processingCount; // 审核中（status=1）
    private Long completedCount;  // 已完成（status=2）
    private Long failedCount;     // 审核失败（status=3）
}