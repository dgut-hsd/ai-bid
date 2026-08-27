package com.ithsd.smart_tender.mapper;

import com.baomidou.mybatisplus.core.mapper.BaseMapper;
import com.baomidou.mybatisplus.extension.plugins.pagination.Page;
import com.ithsd.smart_tender.model.entity.Project;
import org.apache.ibatis.annotations.Mapper;
import org.apache.ibatis.annotations.Param;
import org.apache.ibatis.annotations.Select;

import java.time.LocalDateTime;

@Mapper
public interface ProjectMapper extends BaseMapper<Project> {

    /**
     * 方案甲：审核列表分页查询。状态/文件类型过滤下推到 SQL，保证 selectPage 的 total 与
     * 当前页数据严格一致（修复「先分页再内存过滤导致 total 失配」的问题）。
     *
     * <p>状态口径与 TenderServiceImpl 的 resolveParseStatusFromLatestTask 保持一致
     * （基于项目「最新版本标书」的「最新审核任务」）：
     * <ul>
     *   <li>status=0 待审核：最新版标书尚无审核任务（lt.task_status IS NULL）</li>
     *   <li>status=1 审核中：最新审核任务为 PENDING(0)/PROCESSING(1)</li>
     *   <li>status=2 已完成：最新审核任务 COMPLETED(2)（任务跑完即视为已完成，不再细分通过/需修改）</li>
     *   <li>status=3 审核失败：最新审核任务 FAILED(3)（任务技术性失败）</li>
     * </ul>
     */
    @Select("<script>"
            + "SELECT p.* FROM project p "
            + "LEFT JOIN ( "
            + "  SELECT x.project_id, x.tenant_id, x.file_category, x.task_status "
            + "  FROM ( "
            + "    SELECT pbd.project_id, pbd.tenant_id, pbd.file_category, "
            + "           at.task_status, "
            + "           ROW_NUMBER() OVER (PARTITION BY pbd.project_id, pbd.tenant_id "
            + "             ORDER BY pbd.version DESC, at.create_time DESC, at.id DESC) AS rn "
            + "    FROM bid_document pbd "
            + "    LEFT JOIN audit_task at ON at.bid_id = pbd.id AND at.tenant_id = pbd.tenant_id "
            + "  ) x WHERE x.rn = 1 "
            + ") lt ON lt.project_id = p.id AND lt.tenant_id = p.tenant_id "
            + "WHERE p.tenant_id = #{tenantId} AND p.user_id = #{userId} "
            + "<if test=\"bidName != null and bidName != ''\"> AND p.project_name LIKE CONCAT('%', #{bidName}, '%') </if>"
            + "<if test=\"uploadStartTime != null\"> AND p.create_time &gt;= #{uploadStartTime} </if>"
            + "<if test=\"uploadEndTime != null\"> AND p.create_time &lt;= #{uploadEndTime} </if>"
            + "<choose>"
            + "  <when test=\"fileCategory != null and fileCategory == 'bid'\"> AND lt.file_category = 'bid' </when>"
            + "  <when test=\"fileCategory != null\"> AND (lt.file_category IS NULL OR lt.file_category != 'bid') </when>"
            + "</choose>"
            + "<choose>"
            + "  <when test=\"status != null and status == 0\"> AND lt.task_status IS NULL </when>"
            + "  <when test=\"status != null and status == 1\"> AND lt.task_status IN (0, 1) </when>"
            + "  <when test=\"status != null and status == 2\"> AND lt.task_status = 2 </when>"
            + "  <when test=\"status != null and status == 3\"> AND lt.task_status = 3 </when>"
            + "</choose>"
            + "ORDER BY p.update_time DESC "
            + "</script>")
    Page<Project> selectProjectPageWithStatus(Page<Project> page,
                                               @Param("tenantId") Long tenantId,
                                               @Param("userId") Long userId,
                                               @Param("bidName") String bidName,
                                               @Param("fileCategory") String fileCategory,
                                               @Param("status") Integer status,
                                               @Param("uploadStartTime") LocalDateTime uploadStartTime,
                                               @Param("uploadEndTime") LocalDateTime uploadEndTime);
}