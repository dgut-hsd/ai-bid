package com.ithsd.smart_tender.mapper;

import com.baomidou.mybatisplus.core.mapper.BaseMapper;
import com.ithsd.smart_tender.model.entity.AuditIssue;
import org.apache.ibatis.annotations.Insert;
import org.apache.ibatis.annotations.Mapper;
import org.apache.ibatis.annotations.Param;
import org.apache.ibatis.annotations.Select;

import java.util.List;
import java.util.Map;

@Mapper
public interface AuditIssueMapper extends BaseMapper<AuditIssue> {

    /**
     * 按天统计指定审核任务在当前月给出的问题数量。
     * @param auditIds 审核任务 ID 列表（非空）
     * @return 每行含 day_num（当月第几日，1..31）与 count（问题数）
     */
    @Select("""
        <script>
        SELECT
            DAY(create_time) AS day_num,
            COUNT(*) AS count
        FROM audit_issue
        WHERE audit_id IN
        <foreach item="item" collection="auditIds" open="(" separator="," close=")">
            #{item}
        </foreach>
        AND create_time IS NOT NULL
        AND DATE_FORMAT(create_time, '%Y-%m') = DATE_FORMAT(CURDATE(), '%Y-%m')
        GROUP BY day_num
        ORDER BY day_num
        </script>
        """)
    @org.apache.ibatis.annotations.ResultType(java.util.Map.class)
    List<Map<String, Object>> countIssuesByDayCurrentMonth(@Param("auditIds") List<Long> auditIds);

    /**
     * 增量 upsert：按 (tenant_id, audit_id, risk_id) 唯一键去重。
     * 用于审核进行中把 Rust finding 增量落库（P2），完成时再由 completeTaskFromReview 全量对账。
     */
    @Insert("""
        INSERT INTO audit_issue
            (tenant_id, audit_id, issue_no, risk_id, severity, is_critical, critical_reason,
             category, description, suggestion, page_number, section_name, context, reference, create_time)
        VALUES
            (#{tenantId}, #{auditId}, #{issueNo}, #{riskId}, #{severity}, #{isCritical}, #{criticalReason},
             #{category}, #{description}, #{suggestion}, #{pageNumber}, #{sectionName}, #{context}, #{reference}, #{createTime})
        ON DUPLICATE KEY UPDATE
            issue_no = VALUES(issue_no),
            severity = VALUES(severity),
            is_critical = VALUES(is_critical),
            critical_reason = VALUES(critical_reason),
            category = VALUES(category),
            description = VALUES(description),
            suggestion = VALUES(suggestion),
            page_number = VALUES(page_number),
            section_name = VALUES(section_name),
            context = VALUES(context),
            reference = VALUES(reference)
        """)
    int upsert(AuditIssue issue);
}