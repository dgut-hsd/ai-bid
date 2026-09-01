package com.ithsd.smart_tender.controller;

import com.ithsd.smart_tender.model.result.Result;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;

import java.time.LocalDateTime;
import java.time.format.DateTimeFormatter;
import java.util.LinkedHashMap;
import java.util.Map;

/**
 * 存活探针：供 deploy/update.sh 冒烟测试判断后端新版本是否真正就绪。
 *
 * <p>此前冒烟 URL {@code /api/health} 命中「无此路由」，被全局异常处理器包装成
 * HTTP 200 + {@code code:404}，导致 update.sh 只看 HTTP 状态码时「假通过」。
 * 这里给出真实端点，且 update.sh 已改为校验响应体 {@code code==200}。</p>
 */
@RestController
@RequestMapping("/api")
public class HealthController {

    @GetMapping("/health")
    public Result<Map<String, Object>> health() {
        Map<String, Object> data = new LinkedHashMap<>();
        data.put("status", "UP");
        data.put("service", "smart-tender-backend");
        data.put("time", LocalDateTime.now().format(DateTimeFormatter.ISO_LOCAL_DATE_TIME));
        return Result.success(data);
    }
}