package com.ithsd.smart_tender.mapper;

import com.baomidou.mybatisplus.core.mapper.BaseMapper;
import com.ithsd.smart_tender.model.entity.AuditTask;
import org.apache.ibatis.annotations.Mapper;
import org.apache.ibatis.annotations.Select;
import org.apache.ibatis.annotations.Param;
import org.apache.ibatis.annotations.Update;

import java.time.LocalDateTime;
import java.util.List;
import java.util.Map;

@Mapper
public interface AuditTaskMapper extends BaseMapper<AuditTask> {

    /**
     * 按天统计本周指定 bidId 的审核任务数量
     * @param bidIds 投标ID列表
     * @return 列表元素为 Map，每个 Map 包含 day_date(日期) 和 count(数量) 两个键
     */
    @Select("""
        <script>
        SELECT
            DATE_FORMAT(create_time, '%Y-%m-%d') AS day_date,
            COUNT(*) AS count
        FROM audit_task
        WHERE
            tenant_id = #{tenantId}
            <if test="bidIds != null and bidIds.size() > 0">
                AND bid_id IN
                <foreach item="item" collection="bidIds" open="(" separator="," close=")">
                    #{item}
                </foreach>
            </if>
            AND create_time IS NOT NULL
            AND YEARWEEK(DATE_FORMAT(create_time, '%Y-%m-%d'), 1) = YEARWEEK(CURDATE(), 1)
        GROUP BY day_date
        ORDER BY day_date
        </script>
        """)
    @org.apache.ibatis.annotations.ResultType(java.util.Map.class)
    List<Map<String, Object>> countByWeek(
            @Param("tenantId") Long tenantId,
            @Param("bidIds") List<Long> bidIds);

    /**
     * 审核阶段事件只做单调进度推进，不参与实体乐观锁，避免多个 SSE 回调
     * 共享同一个 AuditTask 对象时互相覆盖 version。
     */
    @Update("""
        UPDATE audit_task
        SET task_status = 1,
            stage = 'REVIEWING',
            progress = GREATEST(COALESCE(progress, 0), #{progress}),
            updated_at = #{updatedAt}
        WHERE task_id = #{taskId}
          AND tenant_id = #{tenantId}
          AND task_status IN (0, 1)
        """)
    int advanceReviewProgress(
            @Param("taskId") String taskId,
            @Param("tenantId") Long tenantId,
            @Param("progress") int progress,
            @Param("updatedAt") LocalDateTime updatedAt);

    @Update("""
        UPDATE audit_task
        SET task_status = 3,
            error_msg = #{errorMsg},
            end_time = #{endTime},
            updated_at = #{endTime},
            version = version + 1
        WHERE task_id = #{taskId}
          AND tenant_id = #{tenantId}
          AND task_status <> 2
        """)
    int markFailed(
            @Param("taskId") String taskId,
            @Param("tenantId") Long tenantId,
            @Param("errorMsg") String errorMsg,
            @Param("endTime") LocalDateTime endTime);

    @Update("""
        UPDATE audit_task
        SET task_status = 2,
            stage = 'SUMMARY',
            progress = 100,
            error_msg = NULL,
            failed_stages = #{failedStages},
            end_time = #{endTime},
            updated_at = #{endTime},
            version = version + 1
        WHERE task_id = #{taskId}
          AND tenant_id = #{tenantId}
          AND task_status <> 2
        """)
    int markCompleted(
            @Param("taskId") String taskId,
            @Param("tenantId") Long tenantId,
            @Param("endTime") LocalDateTime endTime,
            @Param("failedStages") String failedStages);
}
