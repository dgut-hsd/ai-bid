package com.ithsd.smart_tender.config;

import com.baomidou.mybatisplus.core.handlers.MetaObjectHandler;
import com.ithsd.smart_tender.common.BaseContext;
import org.apache.ibatis.reflection.MetaObject;
import org.springframework.stereotype.Component;

import java.time.LocalDateTime;

@Component
public class MyMetaObjectHandler implements MetaObjectHandler {

    @Override
    public void insertFill(MetaObject metaObject) {
        LocalDateTime now = LocalDateTime.now();
        Long currentId = BaseContext.getCurrentId();

        // 用 setFieldValByName 而非 strictInsertFill：
        // 字段不存在于实体时(如 User 没有 uploadUserId)会静默跳过，不会抛异常。
        this.setFieldValByName("createTime", now, metaObject);
        this.setFieldValByName("updateTime", now, metaObject);
        this.setFieldValByName("uploadTime", now, metaObject);

        // 操作人字段：仅当存在该字段 且 有登录上下文时才填充，避免注册等无登录场景写入 null。
        if (currentId != null) {
            this.setFieldValByName("uploadUserId", currentId, metaObject);
            this.setFieldValByName("updateUser", currentId, metaObject);
        }
    }

    @Override
    public void updateFill(MetaObject metaObject) {
        LocalDateTime now = LocalDateTime.now();
        Long currentId = BaseContext.getCurrentId();

        this.setFieldValByName("updateTime", now, metaObject);
        if (currentId != null) {
            this.setFieldValByName("updateUser", currentId, metaObject);
        }
    }
}