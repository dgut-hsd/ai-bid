package com.ithsd.smart_tender.common;

import com.ithsd.smart_tender.common.BizException;
import com.ithsd.smart_tender.model.result.Result;
import jakarta.servlet.http.HttpServletResponse;
import jakarta.validation.ConstraintViolationException;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.validation.BindException;
import org.springframework.validation.FieldError;
import org.springframework.web.bind.MethodArgumentNotValidException;
import org.springframework.web.bind.annotation.ExceptionHandler;
import org.springframework.web.bind.annotation.RestControllerAdvice;

import java.util.stream.Collectors;
import java.util.LinkedHashMap;
import java.util.Map;

import org.springframework.web.bind.MissingServletRequestParameterException;
import org.springframework.web.multipart.support.MissingServletRequestPartException;
import org.springframework.web.multipart.MaxUploadSizeExceededException;
import org.apache.catalina.connector.ClientAbortException;

@RestControllerAdvice
public class GlobalExceptionHandler {

    private static final Logger log = LoggerFactory.getLogger(GlobalExceptionHandler.class);

    @ExceptionHandler(MissingServletRequestPartException.class)
    public Result<Void> handleMissingServletRequestPartException(MissingServletRequestPartException ex) {
        log.warn("缺少请求参数: {}", ex.getMessage());
        return Result.error(400, "缺少必要参数: " + ex.getRequestPartName());
    }

    @ExceptionHandler(MissingServletRequestParameterException.class)
    public Result<Void> handleMissingServletRequestParameterException(MissingServletRequestParameterException ex) {
        log.warn("缺少请求参数: {}", ex.getMessage());
        return Result.error(400, "缺少必要参数: " + ex.getParameterName());
    }

    @ExceptionHandler(BizException.class)
    public Result<Void> handleBizException(BizException ex) {
        log.warn("业务异常: {}", ex.getMessage());
        return Result.error(ex.getCode(), ex.getMessage());
    }

    @ExceptionHandler(TenantAuthException.class)
    public Result<Map<String, Object>> handleTenantAuthException(
            TenantAuthException ex,
            HttpServletResponse response
    ) {
        response.setStatus(ex.getStatus());
        Map<String, Object> data = new LinkedHashMap<>();
        data.put("error_code", ex.getErrorCode());
        data.put("request_id", ex.getRequestId());
        if (!ex.getDetails().isEmpty()) {
            data.put("details", ex.getDetails());
        }
        Result<Map<String, Object>> result = Result.error(ex.getStatus(), ex.getMessage());
        result.setData(data);
        return result;
    }

    @ExceptionHandler(MethodArgumentNotValidException.class)
    public Result<Void> handleMethodArgumentNotValidException(MethodArgumentNotValidException ex) {
        String msg = ex.getBindingResult().getFieldErrors().stream()
                .map(item -> item.getField() + ":" + item.getDefaultMessage())
                .collect(Collectors.joining("; "));
        if (msg.isBlank()) {
            msg = "请求参数不合法";
        }
        log.warn("参数校验异常: {}", msg);
        return Result.error(400, msg);
    }

    @ExceptionHandler(BindException.class)
    public Result<Void> handleBindException(BindException ex) {
        String msg = ex.getBindingResult().getFieldErrors().stream()
                .map(FieldError::getDefaultMessage)
                .collect(Collectors.joining("; "));
        if (msg.isBlank()) {
            msg = "请求参数不合法";
        }
        log.warn("绑定异常: {}", msg);
        return Result.error(400, msg);
    }

    @ExceptionHandler(ConstraintViolationException.class)
    public Result<Void> handleConstraintViolationException(ConstraintViolationException ex) {
        String msg = ex.getConstraintViolations().stream()
                .map(item -> item.getPropertyPath() + ":" + item.getMessage())
                .collect(Collectors.joining("; "));
        if (msg.isBlank()) {
            msg = "请求参数不合法";
        }
        log.warn("约束校验异常: {}", msg);
        return Result.error(400, msg);
    }

    @ExceptionHandler(MaxUploadSizeExceededException.class)
    public Result<Void> handleMaxUploadSizeExceededException(MaxUploadSizeExceededException ex) {
        log.warn("上传文件过大: {}", ex.getMessage());
        return Result.error(413, "上传文件过大，超过服务器限制");
    }


    @ExceptionHandler(Exception.class)
    public Result<Void> handleException(Exception ex) {
        log.error("系统异常", ex);
        return Result.error(500, "系统繁忙，请稍后重试");
    }

    @ExceptionHandler(ClientAbortException.class)
    public Result<Void> handleClientAbortException(ClientAbortException ex) {
        log.info("客户端主动断开连接: {}", ex.getMessage());
        return Result.error(499, "客户端已取消请求或连接中断");
    }
}
