package com.ithsd.smart_tender.mapper;

import com.baomidou.mybatisplus.core.mapper.BaseMapper;
import com.ithsd.smart_tender.model.entity.Tender;
import com.ithsd.smart_tender.model.vo.TenderProjectVO;
import org.apache.ibatis.annotations.Mapper;
import org.apache.ibatis.annotations.Param;
import org.apache.ibatis.annotations.Select;

import java.util.List;

@Mapper
public interface TenderMapper extends BaseMapper<Tender> {

    /**
     * 查询所有标书项目聚合信息
     */
    @Select("""
        SELECT
            t1.project_id AS projectId,
            MAX(t1.version) AS latestVersion,
            (SELECT t2.bid_name FROM bid_document t2 WHERE t2.project_id = t1.project_id AND t2.tenant_id = #{tenantId} ORDER BY t2.version ASC LIMIT 1) AS projectName,
            (SELECT t3.upload_time FROM bid_document t3 WHERE t3.project_id = t1.project_id AND t3.tenant_id = #{tenantId} ORDER BY t3.version ASC LIMIT 1) AS createTime,
            (SELECT t5.supplier_name FROM bid_document t5 WHERE t5.project_id = t1.project_id AND t5.tenant_id = #{tenantId} ORDER BY t5.version ASC LIMIT 1) AS supplierName,
            (SELECT t6.file_category FROM bid_document t6 WHERE t6.project_id = t1.project_id AND t6.tenant_id = #{tenantId} ORDER BY t6.version ASC LIMIT 1) AS fileCategory,
            (SELECT u.real_name FROM sys_user u WHERE u.id = (SELECT t4.upload_user_id FROM bid_document t4 WHERE t4.project_id = t1.project_id AND t4.tenant_id = #{tenantId} ORDER BY t4.version ASC LIMIT 1)) AS creatorName,
            (SELECT au.real_name
             FROM audit_task at
             LEFT JOIN sys_user au ON at.audit_user_id = au.id
             WHERE at.bid_id = (SELECT t7.id FROM bid_document t7 WHERE t7.project_id = t1.project_id AND t7.tenant_id = #{tenantId} ORDER BY t7.version DESC LIMIT 1)
             AND at.tenant_id = #{tenantId}
             LIMIT 1) AS auditorName
        FROM
            bid_document t1
        WHERE
            t1.project_id IS NOT NULL
            AND t1.upload_user_id = #{userId}
            AND t1.tenant_id = #{tenantId}
        GROUP BY
            t1.project_id
        ORDER BY
            createTime DESC
    """)
    List<TenderProjectVO> selectTenderProjects(
            @Param("userId") Long userId,
            @Param("tenantId") Long tenantId);
}
