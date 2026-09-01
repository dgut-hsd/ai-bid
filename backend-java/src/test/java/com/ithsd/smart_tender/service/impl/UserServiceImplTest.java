package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.common.util.MD5Util;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.dto.UserLoginDTO;
import com.ithsd.smart_tender.model.entity.User;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.InjectMocks;
import org.mockito.Mock;
import org.mockito.MockedStatic;
import org.mockito.junit.jupiter.MockitoExtension;

import static org.junit.jupiter.api.Assertions.*;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.*;

@ExtendWith(MockitoExtension.class)
class UserServiceImplTest {

    @Mock
    private UserMapper userMapper;

    @InjectMocks
    private UserServiceImpl userService;

    // ────────────── Login tests ──────────────

    @Test
    void login_Success_ShouldReturnUserWhenCredentialsAreValid() {
        try (MockedStatic<MD5Util> md5 = mockStatic(MD5Util.class)) {
            // Arrange
            String phone = "13800138000";
            String rawPassword = "correctPassword";
            String encryptedPassword = "encryptedHash";

            UserLoginDTO dto = new UserLoginDTO();
            dto.setUsername(phone);
            dto.setPassword(rawPassword);

            User mockUser = User.builder()
                    .id(1L)
                    .username("testuser")
                    .password(encryptedPassword)
                    .realName("Test User")
                    .email("test@example.com")
                    .phone(phone)
                    .status(1)
                    .build();

            when(userMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(mockUser);
            md5.when(() -> MD5Util.encrypt(rawPassword)).thenReturn(encryptedPassword);

            // Act
            User result = userService.login(dto);

            // Assert
            assertNotNull(result);
            assertEquals(1L, result.getId());
            assertEquals("testuser", result.getUsername());
            assertEquals("Test User", result.getRealName());
            assertEquals("test@example.com", result.getEmail());
            assertEquals(phone, result.getPhone());
            assertEquals(encryptedPassword, result.getPassword());
            assertEquals(1, result.getStatus());

            verify(userMapper).selectOne(any(LambdaQueryWrapper.class));
            md5.verify(() -> MD5Util.encrypt(rawPassword));
        }
    }

    @Test
    void login_UserNotFound_ShouldThrowWhenPhoneDoesNotExist() {
        // Arrange
        UserLoginDTO dto = new UserLoginDTO();
        dto.setUsername("nonexistentPhone");
        dto.setPassword("anyPassword");

        when(userMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(null);

        // Act & Assert
        RuntimeException exception = assertThrows(RuntimeException.class,
                () -> userService.login(dto));
        assertEquals("账号或密码错误", exception.getMessage());

        verify(userMapper).selectOne(any(LambdaQueryWrapper.class));
    }

    @Test
    void login_WrongPassword_ShouldThrowWhenPasswordDoesNotMatch() {
        try (MockedStatic<MD5Util> md5 = mockStatic(MD5Util.class)) {
            // Arrange
            UserLoginDTO dto = new UserLoginDTO();
            dto.setUsername("13800138000");
            dto.setPassword("wrongPassword");

            User mockUser = User.builder()
                    .id(1L)
                    .password("storedEncryptedPassword")
                    .phone("13800138000")
                    .status(1)
                    .build();

            when(userMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(mockUser);
            md5.when(() -> MD5Util.encrypt("wrongPassword")).thenReturn("differentEncryptedHash");

            // Act & Assert
            RuntimeException exception = assertThrows(RuntimeException.class,
                    () -> userService.login(dto));
            assertEquals("账号或密码错误", exception.getMessage());

            verify(userMapper).selectOne(any(LambdaQueryWrapper.class));
            md5.verify(() -> MD5Util.encrypt("wrongPassword"));
        }
    }

    @Test
    void login_UserDisabled_ShouldThrowWhenStatusIsZero() {
        try (MockedStatic<MD5Util> md5 = mockStatic(MD5Util.class)) {
            // Arrange
            String rawPassword = "password";
            String encryptedPassword = "encryptedHash";

            UserLoginDTO dto = new UserLoginDTO();
            dto.setUsername("13800138000");
            dto.setPassword(rawPassword);

            User mockUser = User.builder()
                    .id(1L)
                    .password(encryptedPassword)
                    .phone("13800138000")
                    .status(0)   // disabled account
                    .build();

            when(userMapper.selectOne(any(LambdaQueryWrapper.class))).thenReturn(mockUser);
            md5.when(() -> MD5Util.encrypt(rawPassword)).thenReturn(encryptedPassword);

            // Act & Assert
            RuntimeException exception = assertThrows(RuntimeException.class,
                    () -> userService.login(dto));
            assertEquals("账户已被禁用", exception.getMessage());

            verify(userMapper).selectOne(any(LambdaQueryWrapper.class));
        }
    }
}