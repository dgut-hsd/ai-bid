package com.ithsd.smart_tender.mapper;

import org.apache.ibatis.annotations.Param;
import org.apache.ibatis.annotations.Select;
import org.junit.jupiter.api.Test;

import java.lang.annotation.Annotation;
import java.lang.reflect.Method;

import static org.assertj.core.api.Assertions.assertThat;

class TenderMapperTenantScopeTest {

    @Test
    void selectTenderProjects_requiresTenantAndScopesEveryTenantResourceSubquery() throws Exception {
        Method method = TenderMapper.class.getMethod(
                "selectTenderProjects", Long.class, Long.class);
        Annotation[] params = method.getParameterAnnotations()[1];
        assertThat(params).anyMatch(annotation ->
                annotation instanceof Param && "tenantId".equals(((Param) annotation).value()));

        String sql = String.join(" ", method.getAnnotation(Select.class).value());
        assertThat(sql)
                .contains("t1.tenant_id = #{tenantId}")
                .contains("t2.tenant_id = #{tenantId}")
                .contains("t3.tenant_id = #{tenantId}")
                .contains("t4.tenant_id = #{tenantId}")
                .contains("t5.tenant_id = #{tenantId}")
                .contains("t6.tenant_id = #{tenantId}")
                .contains("t7.tenant_id = #{tenantId}")
                .contains("at.tenant_id = #{tenantId}")
                .contains("t1.upload_user_id = #{userId}");
    }
}
