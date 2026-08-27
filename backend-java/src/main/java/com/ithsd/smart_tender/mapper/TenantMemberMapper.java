package com.ithsd.smart_tender.mapper;

import com.baomidou.mybatisplus.core.mapper.BaseMapper;
import com.ithsd.smart_tender.model.entity.TenantMember;
import org.apache.ibatis.annotations.Mapper;
import org.apache.ibatis.annotations.Param;
import org.apache.ibatis.annotations.Select;

import java.util.List;

@Mapper
public interface TenantMemberMapper extends BaseMapper<TenantMember> {
    @Select("""
            SELECT id, tenant_id, user_id, role, status, joined_at, invited_by, last_seen_at
              FROM tenant_member
             WHERE user_id = #{userId}
             ORDER BY joined_at ASC, id ASC
            """)
    List<TenantMember> findByUserId(@Param("userId") Long userId);

    @Select("""
            SELECT id, tenant_id, user_id, role, status, joined_at, invited_by, last_seen_at
              FROM tenant_member
             WHERE user_id = #{userId} AND tenant_id = #{tenantId}
             LIMIT 1
            """)
    TenantMember findByUserAndTenantId(
            @Param("userId") Long userId,
            @Param("tenantId") Long tenantId
    );

    @Select("""
            SELECT id, tenant_id, user_id, role, status, joined_at, invited_by, last_seen_at
              FROM tenant_member
             WHERE tenant_id = #{tenantId}
             ORDER BY joined_at ASC, id ASC
            """)
    List<TenantMember> findByTenantId(@Param("tenantId") Long tenantId);

    @Select("""
            SELECT COUNT(*)
              FROM tenant_member
             WHERE tenant_id = #{tenantId} AND status <> 'REMOVED'
            """)
    long countActiveByTenantId(@Param("tenantId") Long tenantId);
}
