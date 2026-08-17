import { createSlice } from '@reduxjs/toolkit';
import { normalizeAuthSession, normalizeTenantSummary } from '@/features/login/api/session';
import type { AuthSession, TenantSummary } from '@/features/tenant/types';

export interface UserInfo {
   /** 用户 ID（统一按字符串保存，避免 Java BIGINT 在 JS 中丢失精度） */
   id: string;
   username: string;
   realName: string;
}

export interface AuthState {
   token: string | null;
   userInfo: UserInfo | null;
   isAuthenticated: boolean;
   /** 当前租户 ID（字符串，按租户 API 契约保存） */
   currentTenantId: string | null;
   /** 当前用户可见的租户列表 */
   tenantList: TenantSummary[];
}

const STORAGE_KEYS = {
   token: 'token',
   userInfo: 'userInfo',
   tenantId: 'tenantId',
   tenantList: 'tenantList',
   authSession: 'authSession',
   // Remove this legacy key when clearing old sessions; the backend does not
   // issue a separate refresh token.
   legacyRefreshToken: 'refreshToken',
} as const;

function getActiveStorage(): Storage | null {
   if (
      localStorage.getItem(STORAGE_KEYS.token) ||
      localStorage.getItem(STORAGE_KEYS.authSession)
   ) {
      return localStorage;
   }
   if (
      sessionStorage.getItem(STORAGE_KEYS.token) ||
      sessionStorage.getItem(STORAGE_KEYS.authSession)
   ) {
      return sessionStorage;
   }
   return null;
}

export function getStoredCurrentTenantId(): string | null {
   return getActiveStorage()?.getItem(STORAGE_KEYS.tenantId) ?? null;
}

const clearAllStorage = () => {
   localStorage.removeItem(STORAGE_KEYS.token);
   localStorage.removeItem(STORAGE_KEYS.userInfo);
   localStorage.removeItem(STORAGE_KEYS.tenantId);
   localStorage.removeItem(STORAGE_KEYS.tenantList);
   localStorage.removeItem(STORAGE_KEYS.authSession);
   localStorage.removeItem(STORAGE_KEYS.legacyRefreshToken);
   sessionStorage.removeItem(STORAGE_KEYS.token);
   sessionStorage.removeItem(STORAGE_KEYS.userInfo);
   sessionStorage.removeItem(STORAGE_KEYS.tenantId);
   sessionStorage.removeItem(STORAGE_KEYS.tenantList);
   sessionStorage.removeItem(STORAGE_KEYS.authSession);
   sessionStorage.removeItem(STORAGE_KEYS.legacyRefreshToken);
};

function userInfoFromSession(session: AuthSession): UserInfo {
   return {
      id: session.user_info.user_id,
      username: session.user_info.username,
      realName: session.user_info.real_name,
   };
}

function stateFromSession(session: AuthSession): AuthState {
   return {
      token: session.token,
      userInfo: userInfoFromSession(session),
      isAuthenticated: true,
      currentTenantId: session.current_tenant?.tenant_id ?? null,
      tenantList: session.tenants,
   };
}

function parseStoredUserInfo(value: string | null): UserInfo | null {
   if (!value) return null;
   try {
      const parsed: unknown = JSON.parse(value);
      if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
         return null;
      }
      const record = parsed as Record<string, unknown>;
      const id = record.id;
      if (
         (typeof id !== 'string' && typeof id !== 'number') ||
         typeof record.username !== 'string' ||
         typeof record.realName !== 'string'
      ) {
         return null;
      }
      return { id: String(id), username: record.username, realName: record.realName };
   } catch {
      return null;
   }
}

function parseStoredTenantList(value: string | null): TenantSummary[] {
   if (!value) return [];
   try {
      const parsed: unknown = JSON.parse(value);
      return Array.isArray(parsed) ? parsed.map(normalizeTenantSummary) : [];
   } catch {
      return [];
   }
}

function readStoredSession(): AuthSession | null {
   const storage = getActiveStorage();
   const raw = storage?.getItem(STORAGE_KEYS.authSession);
   if (!raw) return null;
   try {
      const parsed: unknown = JSON.parse(raw);
      return normalizeAuthSession(parsed);
   } catch {
      return null;
   }
}

function readInitialState(): AuthState {
   const session = readStoredSession();
   if (session) return stateFromSession(session);

   const storage = getActiveStorage();
   const token = storage?.getItem(STORAGE_KEYS.token) ?? null;
   const userInfo = parseStoredUserInfo(storage?.getItem(STORAGE_KEYS.userInfo) ?? null);
   if (!token || !userInfo) {
      return {
         token: null,
         userInfo: null,
         isAuthenticated: false,
         currentTenantId: null,
         tenantList: [],
      };
   }

   return {
      token,
      userInfo,
      isAuthenticated: true,
      currentTenantId: storage?.getItem(STORAGE_KEYS.tenantId) ?? null,
      tenantList: parseStoredTenantList(storage?.getItem(STORAGE_KEYS.tenantList) ?? null),
   };
}

const initialState: AuthState = readInitialState();

function persistSession(session: AuthSession, rememberMe: boolean): void {
   clearAllStorage();
   const storage = rememberMe ? localStorage : sessionStorage;
   const userInfo = userInfoFromSession(session);
   storage.setItem(STORAGE_KEYS.token, session.token);
   storage.setItem(STORAGE_KEYS.userInfo, JSON.stringify(userInfo));
   storage.setItem(STORAGE_KEYS.tenantList, JSON.stringify(session.tenants));
   storage.setItem(STORAGE_KEYS.authSession, JSON.stringify(session));
   if (session.current_tenant) {
      storage.setItem(STORAGE_KEYS.tenantId, session.current_tenant.tenant_id);
   }
}

function persistLegacyCredentials(
   token: string,
   userInfo: UserInfo,
   rememberMe: boolean,
   tenantId: string | null,
   tenantList: TenantSummary[]
): void {
   clearAllStorage();
   const storage = rememberMe ? localStorage : sessionStorage;
   storage.setItem(STORAGE_KEYS.token, token);
   storage.setItem(STORAGE_KEYS.userInfo, JSON.stringify(userInfo));
   storage.setItem(STORAGE_KEYS.tenantList, JSON.stringify(tenantList));
   if (tenantId) storage.setItem(STORAGE_KEYS.tenantId, tenantId);
}

function applySession(state: AuthState, session: AuthSession): void {
   const next = stateFromSession(session);
   state.token = next.token;
   state.userInfo = next.userInfo;
   state.isAuthenticated = next.isAuthenticated;
   state.currentTenantId = next.currentTenantId;
   state.tenantList = next.tenantList;
}

const authSlice = createSlice({
   name: 'auth',
   initialState,
   reducers: {
      setAuthSession: (
         state,
         action: { payload: { session: AuthSession; rememberMe?: boolean } }
      ) => {
         const { session, rememberMe } = action.payload;
         applySession(state, session);
         persistSession(
            session,
            rememberMe ?? Boolean(localStorage.getItem(STORAGE_KEYS.token))
         );
      },
      /**
       * Legacy credential action kept for callers outside the auth flow. New
       * login/refresh/switch code must dispatch setAuthSession instead.
       */
      setCredentials: (
         state,
         action: {
            payload: {
               token: string;
               userInfo: UserInfo;
               rememberMe?: boolean;
               tenantId?: string | null;
               tenantList?: TenantSummary[];
            };
         }
      ) => {
         const { token, userInfo, rememberMe, tenantId, tenantList } = action.payload;
         state.token = token;
         state.userInfo = userInfo;
         state.isAuthenticated = true;
         state.currentTenantId = tenantId ?? null;
         state.tenantList = tenantList ?? [];
         persistLegacyCredentials(
            token,
            userInfo,
            rememberMe ?? false,
            tenantId ?? null,
            tenantList ?? []
         );
      },
      setTenantList: (state, action: { payload: TenantSummary[] }) => {
         state.tenantList = action.payload;
         const storage = getActiveStorage();
         if (storage) storage.setItem(STORAGE_KEYS.tenantList, JSON.stringify(action.payload));
      },
      setCurrentTenantId: (state, action: { payload: string | null }) => {
         state.currentTenantId = action.payload;
         const storage = getActiveStorage();
         if (!storage) return;
         if (action.payload) {
            storage.setItem(STORAGE_KEYS.tenantId, action.payload);
         } else {
            storage.removeItem(STORAGE_KEYS.tenantId);
         }
      },
      logout: (state) => {
         state.token = null;
         state.userInfo = null;
         state.isAuthenticated = false;
         state.currentTenantId = null;
         state.tenantList = [];
         clearAllStorage();
      },
      restoreAuth: (state) => {
         const session = readStoredSession();
         if (session) {
            applySession(state, session);
            return;
         }

         const storage = getActiveStorage();
         const token = storage?.getItem(STORAGE_KEYS.token);
         const userInfo = parseStoredUserInfo(storage?.getItem(STORAGE_KEYS.userInfo) ?? null);
         if (token && userInfo) {
            state.token = token;
            state.userInfo = userInfo;
            state.isAuthenticated = true;
            state.currentTenantId = storage?.getItem(STORAGE_KEYS.tenantId) ?? null;
            state.tenantList = parseStoredTenantList(
               storage?.getItem(STORAGE_KEYS.tenantList) ?? null
            );
            return;
         }

         clearAllStorage();
      },
   },
});

export const {
   setAuthSession,
   setCredentials,
   setTenantList,
   setCurrentTenantId,
   logout,
   restoreAuth,
} = authSlice.actions;
export default authSlice.reducer;
