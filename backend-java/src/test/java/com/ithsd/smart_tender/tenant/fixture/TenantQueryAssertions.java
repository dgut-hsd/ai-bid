package com.ithsd.smart_tender.tenant.fixture;

import com.baomidou.mybatisplus.core.MybatisConfiguration;
import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.baomidou.mybatisplus.core.metadata.TableInfoHelper;
import com.ithsd.smart_tender.model.entity.AuditTask;
import com.ithsd.smart_tender.model.entity.Tender;
import org.apache.ibatis.builder.MapperBuilderAssistant;

import static org.assertj.core.api.Assertions.assertThat;

/**
 * 断言查询包装器确实携带了租户过滤条件。
 *
 * <p>仅 mock 返回 null 无法证明 {@code .eq(Entity::getTenantId, tenantId)} 真的写进了
 * 查询——删掉该条件测试照样通过。这里直接校验 WHERE 中出现 tenant_id 列、且租户值已绑定。</p>
 */
public final class TenantQueryAssertions {

    static {
        // LambdaQueryWrapper.getSqlSegment() 需要 MyBatis-Plus 的 lambda 列缓存，
        // 纯 Mockito 单测（无 Spring 上下文）里该缓存为空。这里为被测实体预建缓存。
        initLambdaCache(Tender.class, AuditTask.class);
    }

    private TenantQueryAssertions() {
    }

    public static void assertTenantScoped(LambdaQueryWrapper<?> wrapper, Long expectedTenantId) {
        // 先触发 getSqlSegment() 渲染：MyBatis-Plus 的 paramNameValuePairs 是惰性填充的，
        // 未渲染前为空 Map，直接断言会误报。
        assertThat(wrapper.getSqlSegment())
                .as("查询 WHERE 必须包含 tenant_id 列")
                .contains("tenant_id");
        assertThat(wrapper.getParamNameValuePairs())
                .as("查询必须绑定租户过滤值")
                .containsValue(expectedTenantId);
    }

    private static void initLambdaCache(Class<?>... entityClasses) {
        for (Class<?> entityClass : entityClasses) {
            TableInfoHelper.initTableInfo(
                    new MapperBuilderAssistant(new MybatisConfiguration(), ""), entityClass);
        }
    }
}
