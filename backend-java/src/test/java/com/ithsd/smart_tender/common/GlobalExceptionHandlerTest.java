package com.ithsd.smart_tender.common;

import com.ithsd.smart_tender.model.result.Result;
import jakarta.validation.ConstraintViolation;
import jakarta.validation.ConstraintViolationException;
import jakarta.validation.Path;
import org.apache.catalina.connector.ClientAbortException;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.junit.jupiter.MockitoExtension;
import org.springframework.core.MethodParameter;
import org.springframework.validation.BindingResult;
import org.springframework.validation.FieldError;
import org.springframework.web.bind.MethodArgumentNotValidException;
import org.springframework.validation.BindException;
import org.springframework.web.bind.MissingServletRequestParameterException;
import org.springframework.web.multipart.MaxUploadSizeExceededException;
import org.springframework.web.multipart.support.MissingServletRequestPartException;

import java.util.List;
import java.util.Set;

import static org.junit.jupiter.api.Assertions.*;
import static org.mockito.Mockito.*;

@ExtendWith(MockitoExtension.class)
class GlobalExceptionHandlerTest {

    private final GlobalExceptionHandler handler = new GlobalExceptionHandler();

    // ========================================================================
    // BizException
    // ========================================================================

    @Test
    void handleBizException_shouldReturnCustomCodeAndMessage() {
        BizException ex = new BizException(400, "业务异常信息");
        Result<Void> result = handler.handleBizException(ex);
        assertEquals(400, result.getCode());
        assertEquals("业务异常信息", result.getMsg());
    }

    @Test
    void handleBizException_withDefaultConstructor_shouldReturn500WithMessage() {
        BizException ex = new BizException("默认错误消息");
        Result<Void> result = handler.handleBizException(ex);
        assertEquals(500, result.getCode());
        assertEquals("默认错误消息", result.getMsg());
    }

    @Test
    void handleBizException_shouldPropagateCustomCodeAndMessage() {
        BizException ex = new BizException(403, "禁止访问");
        Result<Void> result = handler.handleBizException(ex);
        assertEquals(403, result.getCode());
        assertEquals("禁止访问", result.getMsg());
    }

    // ========================================================================
    // Generic Exception
    // ========================================================================

    @Test
    void handleException_shouldReturn500WithGenericMessage() {
        Exception ex = new RuntimeException("内部错误详情");
        Result<Void> result = handler.handleException(ex);
        assertEquals(500, result.getCode());
        assertEquals("系统繁忙，请稍后重试", result.getMsg());
    }

    @Test
    void handleException_withNestedCause_shouldStillReturn500() {
        Exception ex = new RuntimeException("外层错误", new NullPointerException("空指针"));
        Result<Void> result = handler.handleException(ex);
        assertEquals(500, result.getCode());
        assertEquals("系统繁忙，请稍后重试", result.getMsg());
    }

    @Test
    void handleException_withCustomException_shouldReturn500() {
        Exception ex = new IllegalArgumentException("非法参数");
        Result<Void> result = handler.handleException(ex);
        assertEquals(500, result.getCode());
        assertEquals("系统繁忙，请稍后重试", result.getMsg());
    }

    // ========================================================================
    // MethodArgumentNotValidException
    // ========================================================================

    @Test
    void handleMethodArgumentNotValidException_shouldReturnFieldErrors() {
        // Given: BindingResult with two FieldErrors
        BindingResult bindingResult = mock(BindingResult.class);
        MethodParameter methodParameter = mock(MethodParameter.class);

        FieldError fieldError1 = new FieldError("obj", "name", "名称不能为空");
        FieldError fieldError2 = new FieldError("obj", "age", "年龄必须大于0");
        when(bindingResult.getFieldErrors()).thenReturn(List.of(fieldError1, fieldError2));

        MethodArgumentNotValidException ex =
                new MethodArgumentNotValidException(methodParameter, bindingResult);

        // When
        Result<Void> result = handler.handleMethodArgumentNotValidException(ex);

        // Then
        assertEquals(400, result.getCode());
        assertTrue(result.getMsg().contains("name:名称不能为空"));
        assertTrue(result.getMsg().contains("age:年龄必须大于0"));
    }

    @Test
    void handleMethodArgumentNotValidException_withEmptyErrors_shouldReturnDefaultMessage() {
        // Given: BindingResult with no FieldErrors
        BindingResult bindingResult = mock(BindingResult.class);
        MethodParameter methodParameter = mock(MethodParameter.class);
        when(bindingResult.getFieldErrors()).thenReturn(List.of());

        MethodArgumentNotValidException ex =
                new MethodArgumentNotValidException(methodParameter, bindingResult);

        // When
        Result<Void> result = handler.handleMethodArgumentNotValidException(ex);

        // Then
        assertEquals(400, result.getCode());
        assertEquals("请求参数不合法", result.getMsg());
    }

    @Test
    void handleMethodArgumentNotValidException_withNullDefaultMessage_shouldUseFieldAndNull() {
        // Given: FieldError with null default message
        BindingResult bindingResult = mock(BindingResult.class);
        MethodParameter methodParameter = mock(MethodParameter.class);

        FieldError fieldError = new FieldError("obj", "email", null, false, null, null, null);
        when(bindingResult.getFieldErrors()).thenReturn(List.of(fieldError));

        MethodArgumentNotValidException ex =
                new MethodArgumentNotValidException(methodParameter, bindingResult);

        // When
        Result<Void> result = handler.handleMethodArgumentNotValidException(ex);

        // Then — the collector joins "field:null" since defaultMessage is null
        assertEquals(400, result.getCode());
        assertEquals("email:null", result.getMsg());
    }

    // ========================================================================
    // BindException
    // ========================================================================

    @Test
    void handleBindException_shouldReturnFieldErrorMessages() {
        // Given: BindException with two FieldErrors
        BindException ex = new BindException(new Object(), "target");
        ex.addError(new FieldError("target", "title", "标题不能为空"));
        ex.addError(new FieldError("target", "content", "内容不能为空"));

        // When
        Result<Void> result = handler.handleBindException(ex);

        // Then
        assertEquals(400, result.getCode());
        assertTrue(result.getMsg().contains("标题不能为空"));
        assertTrue(result.getMsg().contains("内容不能为空"));
        assertTrue(result.getMsg().contains(";"));
    }

    @Test
    void handleBindException_withEmptyErrors_shouldReturnDefaultMessage() {
        // Given: BindException with no errors
        BindException ex = new BindException(new Object(), "target");

        // When
        Result<Void> result = handler.handleBindException(ex);

        // Then
        assertEquals(400, result.getCode());
        assertEquals("请求参数不合法", result.getMsg());
    }

    @Test
    void handleBindException_withNullDefaultMessage_shouldIncludeNullInOutput() {
        // Given: FieldError with null default message
        BindException ex = new BindException(new Object(), "target");
        ex.addError(new FieldError("target", "phone", null, false, null, null, null));

        // When
        Result<Void> result = handler.handleBindException(ex);

        // Then
        assertEquals(400, result.getCode());
        // BindException handler uses FieldError::getDefaultMessage directly (not "field:msg" format)
        assertEquals("null", result.getMsg());
    }

    // ========================================================================
    // ConstraintViolationException
    // ========================================================================

    @Test
    void handleConstraintViolationException_shouldReturnViolationMessages() {
        // Given: two mock ConstraintViolations
        ConstraintViolation<?> violation1 = mock(ConstraintViolation.class);
        ConstraintViolation<?> violation2 = mock(ConstraintViolation.class);
        Path path1 = mock(Path.class);
        Path path2 = mock(Path.class);

        when(path1.toString()).thenReturn("createUser.name");
        when(path2.toString()).thenReturn("createUser.age");
        when(violation1.getPropertyPath()).thenReturn(path1);
        when(violation2.getPropertyPath()).thenReturn(path2);
        when(violation1.getMessage()).thenReturn("名称不能为空");
        when(violation2.getMessage()).thenReturn("年龄必须大于0");

        ConstraintViolationException ex =
                new ConstraintViolationException("校验失败", Set.of(violation1, violation2));

        // When
        Result<Void> result = handler.handleConstraintViolationException(ex);

        // Then
        assertEquals(400, result.getCode());
        assertTrue(result.getMsg().contains("createUser.name:名称不能为空"));
        assertTrue(result.getMsg().contains("createUser.age:年龄必须大于0"));
        assertTrue(result.getMsg().contains(";"));
    }

    @Test
    void handleConstraintViolationException_withEmptyViolations_shouldReturnDefaultMessage() {
        // Given: empty violations
        ConstraintViolationException ex =
                new ConstraintViolationException("校验失败", Set.of());

        // When
        Result<Void> result = handler.handleConstraintViolationException(ex);

        // Then
        assertEquals(400, result.getCode());
        assertEquals("请求参数不合法", result.getMsg());
    }

    // ========================================================================
    // MissingServletRequestPartException
    // ========================================================================

    @Test
    void handleMissingServletRequestPartException_shouldReturn400WithPartName() {
        MissingServletRequestPartException ex = new MissingServletRequestPartException("file");
        Result<Void> result = handler.handleMissingServletRequestPartException(ex);
        assertEquals(400, result.getCode());
        assertTrue(result.getMsg().contains("file"));
    }

    // ========================================================================
    // MissingServletRequestParameterException
    // ========================================================================

    @Test
    void handleMissingServletRequestParameterException_shouldReturn400WithParamName() {
        MissingServletRequestParameterException ex =
                new MissingServletRequestParameterException("token", "String");
        Result<Void> result = handler.handleMissingServletRequestParameterException(ex);
        assertEquals(400, result.getCode());
        assertTrue(result.getMsg().contains("token"));
    }

    // ========================================================================
    // MaxUploadSizeExceededException
    // ========================================================================

    @Test
    void handleMaxUploadSizeExceededException_shouldReturn413() {
        MaxUploadSizeExceededException ex = new MaxUploadSizeExceededException(10 * 1024 * 1024L);
        Result<Void> result = handler.handleMaxUploadSizeExceededException(ex);
        assertEquals(413, result.getCode());
        assertEquals("上传文件过大，超过服务器限制", result.getMsg());
    }

    // ========================================================================
    // ClientAbortException
    // ========================================================================

    @Test
    void handleClientAbortException_shouldNotThrow() {
        ClientAbortException ex = new ClientAbortException("连接被客户端重置");
        // 处理器已改为 void：客户端断开时连接不可写，仅记录日志、不返回响应体。
        assertDoesNotThrow(() -> handler.handleClientAbortException(ex));
    }

    // ========================================================================
    // Result structure validation
    // ========================================================================

    @Test
    void allHandlers_shouldReturnResultWithTimestamp() {
        BizException ex = new BizException(400, "test");
        Result<Void> result = handler.handleBizException(ex);

        assertNotNull(result.getCode());
        assertNotNull(result.getMsg());
        assertNotNull(result.getTimestamp());
        assertNull(result.getData());
    }
}
