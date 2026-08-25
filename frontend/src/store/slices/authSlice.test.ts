import { describe, it, expect, beforeEach, vi } from 'vitest';
import authReducer, { setCredentials, logout, restoreAuth } from './authSlice';
import type { AuthState, UserInfo } from './authSlice';

describe('authSlice', () => {
  const mockUser: UserInfo = { id: '1', username: 'testuser', realName: '测试用户' };
  const mockToken = 'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.mock-token';

  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
  });

  // ─── initialState ──────────────────────────────────────────────

  describe('initialState', () => {
    it('should have null token, null userInfo, and isAuthenticated false', () => {
      const state = authReducer(undefined, { type: 'init' });
      expect(state.token).toBeNull();
      expect(state.userInfo).toBeNull();
      expect(state.isAuthenticated).toBe(false);
    });
  });

  // ─── setCredentials ────────────────────────────────────────────

  describe('setCredentials', () => {
    it('should store token and userInfo in state and mark as authenticated', () => {
      const state = authReducer(
        undefined,
        setCredentials({ token: mockToken, userInfo: mockUser }),
      );
      expect(state.token).toBe(mockToken);
      expect(state.userInfo).toEqual(mockUser);
      expect(state.isAuthenticated).toBe(true);
    });

    it('should persist to localStorage when rememberMe is true', () => {
      authReducer(
        undefined,
        setCredentials({ token: mockToken, userInfo: mockUser, rememberMe: true }),
      );
      expect(localStorage.getItem('token')).toBe(mockToken);
      expect(localStorage.getItem('userInfo')).toBe(JSON.stringify(mockUser));
      expect(sessionStorage.getItem('token')).toBeNull();
      expect(sessionStorage.getItem('userInfo')).toBeNull();
    });

    it('should persist to sessionStorage when rememberMe is false', () => {
      authReducer(
        undefined,
        setCredentials({ token: mockToken, userInfo: mockUser, rememberMe: false }),
      );
      expect(sessionStorage.getItem('token')).toBe(mockToken);
      expect(sessionStorage.getItem('userInfo')).toBe(JSON.stringify(mockUser));
      expect(localStorage.getItem('token')).toBeNull();
      expect(localStorage.getItem('userInfo')).toBeNull();
    });

    it('should default to sessionStorage when rememberMe is not provided', () => {
      authReducer(
        undefined,
        setCredentials({ token: mockToken, userInfo: mockUser }),
      );
      expect(sessionStorage.getItem('token')).toBe(mockToken);
      expect(sessionStorage.getItem('userInfo')).toBe(JSON.stringify(mockUser));
      expect(localStorage.getItem('token')).toBeNull();
    });

    it('should clear the opposite storage before writing new credentials', () => {
      // Pre-populate sessionStorage (simulate a previous login without rememberMe)
      sessionStorage.setItem('token', 'old-session-token');
      sessionStorage.setItem('userInfo', JSON.stringify({ id: 99, username: 'old', realName: 'Old' }));

      // Now login with rememberMe=true — the old sessionStorage data must be cleared
      authReducer(
        undefined,
        setCredentials({ token: mockToken, userInfo: mockUser, rememberMe: true }),
      );

      expect(sessionStorage.getItem('token')).toBeNull();
      expect(sessionStorage.getItem('userInfo')).toBeNull();
      expect(localStorage.getItem('token')).toBe(mockToken);
    });
  });

  // ─── logout ────────────────────────────────────────────────────

  describe('logout', () => {
    it('should reset state to unauthenticated', () => {
      const prevState: AuthState = {
        token: mockToken,
        userInfo: mockUser,
        isAuthenticated: true,
        currentTenantId: null,
        tenantList: [],
      };
      const state = authReducer(prevState, logout());
      expect(state.token).toBeNull();
      expect(state.userInfo).toBeNull();
      expect(state.isAuthenticated).toBe(false);
    });

    it('should remove credentials from both localStorage and sessionStorage', () => {
      // Populate both storages
      localStorage.setItem('token', mockToken);
      localStorage.setItem('userInfo', JSON.stringify(mockUser));
      sessionStorage.setItem('token', mockToken);
      sessionStorage.setItem('userInfo', JSON.stringify(mockUser));

      authReducer(
        {
          token: mockToken,
          userInfo: mockUser,
          isAuthenticated: true,
          currentTenantId: null,
          tenantList: [],
        },
        logout(),
      );

      expect(localStorage.getItem('token')).toBeNull();
      expect(localStorage.getItem('userInfo')).toBeNull();
      expect(sessionStorage.getItem('token')).toBeNull();
      expect(sessionStorage.getItem('userInfo')).toBeNull();
    });
  });

  // ─── restoreAuth ───────────────────────────────────────────────

  describe('restoreAuth', () => {
    it('should restore authentication from localStorage', () => {
      localStorage.setItem('token', mockToken);
      localStorage.setItem('userInfo', JSON.stringify(mockUser));

      const state = authReducer(
        {
          token: null,
          userInfo: null,
          isAuthenticated: false,
          currentTenantId: null,
          tenantList: [],
        },
        restoreAuth(),
      );
      expect(state.token).toBe(mockToken);
      expect(state.userInfo).toEqual(mockUser);
      expect(state.isAuthenticated).toBe(true);
    });

    it('should fall back to sessionStorage when localStorage is empty', () => {
      sessionStorage.setItem('token', mockToken);
      sessionStorage.setItem('userInfo', JSON.stringify(mockUser));

      const state = authReducer(
        {
          token: null,
          userInfo: null,
          isAuthenticated: false,
          currentTenantId: null,
          tenantList: [],
        },
        restoreAuth(),
      );
      expect(state.token).toBe(mockToken);
      expect(state.userInfo).toEqual(mockUser);
      expect(state.isAuthenticated).toBe(true);
    });

    it('should keep current state when both storages are empty', () => {
      const currentState: AuthState = {
        token: null,
        userInfo: null,
        isAuthenticated: false,
        currentTenantId: null,
        tenantList: [],
      };
      const state = authReducer(currentState, restoreAuth());
      expect(state).toEqual(currentState);
    });

    it('should clear corrupted data from storage and leave state unchanged when stored userInfo is invalid JSON', () => {
      vi.spyOn(console, 'error').mockImplementation(() => {});
      localStorage.setItem('token', mockToken);
      localStorage.setItem('userInfo', 'not-valid-json');

      const currentState: AuthState = {
        token: null,
        userInfo: null,
        isAuthenticated: false,
        currentTenantId: null,
        tenantList: [],
      };
      const state = authReducer(currentState, restoreAuth());

      expect(state.token).toBeNull();
      expect(state.userInfo).toBeNull();
      expect(state.isAuthenticated).toBe(false);
      // Corrupted data should be purged from storage
      expect(localStorage.getItem('token')).toBeNull();
      expect(localStorage.getItem('userInfo')).toBeNull();
    });
  });
});
