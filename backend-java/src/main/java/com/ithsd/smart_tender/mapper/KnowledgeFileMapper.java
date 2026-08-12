package com.ithsd.smart_tender.mapper;

import com.baomidou.mybatisplus.core.mapper.BaseMapper;
import com.ithsd.smart_tender.model.entity.KnowledgeFile;
import org.apache.ibatis.annotations.Mapper;
import org.apache.ibatis.annotations.Param;
import org.apache.ibatis.annotations.Select;

@Mapper
public interface KnowledgeFileMapper extends BaseMapper<KnowledgeFile> {

    @Select("""
            SELECT id, tenant_id, file_name, file_path, file_size, file_type, category, tags,
                   description, applicable_scope, status, version, chunk_count,
                   upload_user_id, upload_time, update_time
              FROM knowledge_file
             WHERE id = #{fileId} AND tenant_id = #{tenantId}
             LIMIT 1
            """)
    KnowledgeFile findByIdAndTenantId(
            @Param("fileId") Long fileId,
            @Param("tenantId") Long tenantId
    );
}
