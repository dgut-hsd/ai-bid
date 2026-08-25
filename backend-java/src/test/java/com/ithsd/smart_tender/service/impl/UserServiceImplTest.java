package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.common.BizException;
import com.ithsd.smart_tender.common.util.MD5Util;
import com.ithsd.smart_tender.mapper.UserMapper;
import com.ithsd.smart_tender.model.dto.UserLoginDTO;
import com.ithsd.smart_tender.model.dto.UserRegisterDTO;
import com.ithsd.smart_tender.model.entity.User;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.ArgumentCaptor;
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

    // ────────────── Register tests ──────────────

    @Test
    void register_Success_ShouldInsertUserWhenDataIsValid() {
        try (MockedStatic<MD5Util> md5 = mockStatic(MD5Util.class)) {
            // Arrange
            String rawPassword = "password123";
            String encryptedPassword = "encryptedHash123";

            UserRegisterDTO dto = new UserRegisterDTO();
            dto.setUsername("newuser");
            dto.setPassword(rawPassword);
            dto.setRealName("New User");
            dto.setEmail("new@test.com");
            dto.setPhone("13900139000");

            when(userMapper.selectOne(any())).thenReturn(null);
            md5.when(() -> MD5Util.encrypt(rawPassword)).thenReturn(encryptedPassword);

            // Act
            userService.register(dto);

            // Assert
            ArgumentCaptor<User> userCaptor = ArgumentCaptor.forClass(User.class);
            verify(userMapper, times(2)).selectOne(any());
            verify(userMapper).insert(userCaptor.capture());

            User capturedUser = userCaptor.getValue();
            assertNotNull(capturedUser);
            assertEquals("newuser", capturedUser.getUsername());
            assertEquals(encryptedPassword, capturedUser.getPassword());
            assertEquals("New User", capturedUser.getRealName());
            assertEquals("new@test.com", capturedUser.getEmail());
            assertEquals("13900139000", capturedUser.getPhone());
            assertEquals(1, capturedUser.getStatus());
            assertNotNull(capturedUser.getCreateTime());
            assertNotNull(capturedUser.getUpdateTime());

            md5.verify(() -> MD5Util.encrypt(rawPassword));
        }
    }

    @Test
    void register_DuplicateUsername_ShouldThrowBizException() {
        // Arrange
        UserRegisterDTO dto = new UserRegisterDTO();
        dto.setUsername("existinguser");
        dto.setPassword("password123");
        dto.setPhone("13900139000");

        User existingUser = User.builder().id(1L).username("existinguser").build();
        when(userMapper.selectOne(any())).thenReturn(existingUser);

        // Act & Assert
        BizException exception = assertThrows(BizException.class,
                () -> userService.register(dto));
        assertEquals("用户名已存在", exception.getMessage());

        verify(userMapper).selectOne(any());
        verify(userMapper, never()).insert(any());
    }

    @Test
    void register_DuplicatePhone_ShouldThrowBizException() {
        // Arrange
        UserRegisterDTO dto = new UserRegisterDTO();
        dto.setUsername("newuser");
        dto.setPassword("password123");
        dto.setPhone("13900139000");

        User existingPhoneUser = User.builder()
                .id(2L)
                .phone("13900139000")
                .build();

        // First selectOne (username check) returns null, second (phone check) returns existing
        when(userMapper.selectOne(any()))
                .thenReturn(null)
                .thenReturn(existingPhoneUser);

        // Act & Assert
        BizException exception = assertThrows(BizException.class,
                () -> userService.register(dto));
        assertEquals("手机号已存在", exception.getMessage());

        verify(userMapper, times(2)).selectOne(any());
        verify(userMapper, never()).insert(any());
    }

    @Test
    void register_PasswordShouldBeEncryptedBeforeInsert() {
        try (MockedStatic<MD5Util> md5 = mockStatic(MD5Util.class)) {
            // Arrange
            String rawPassword = "mySecretPassword";

            UserRegisterDTO dto = new UserRegisterDTO();
            dto.setUsername("user");
            dto.setPassword(rawPassword);
            dto.setPhone("13900139000");

            when(userMapper.selectOne(any())).thenReturn(null);
            md5.when(() -> MD5Util.encrypt(rawPassword)).thenReturn("encryptedValue_xyz");

            // Act
            userService.register(dto);

            // Assert — verify encryption was invoked with the raw password
            md5.verify(() -> MD5Util.encrypt(rawPassword), times(1));

            // Assert — verify the encrypted value is what gets stored
            ArgumentCaptor<User> captor = ArgumentCaptor.forClass(User.class);
            verify(userMapper).insert(captor.capture());
            assertEquals("encryptedValue_xyz", captor.getValue().getPassword());
        }
    }

}
