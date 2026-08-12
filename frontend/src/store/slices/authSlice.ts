import { createSlice } from '@reduxjs/toolkit';

export interface UserInfo {
   /** 用户 ID（后端可能返回数字或字符串 UUID，统一保留原值） */
   id: number | string;
   username: string;
   realName: string;
}

export interface AuthState {
   token: string | null;
   userInfo: UserInfo | null;
   isAuthenticated: boolean;
   /** 当前租户 ID（字符串，按飞书交接文档要求） */
   currentTenantId: string | null;
   /** 租户列表 */
   tenantList: import('@/features/tenant/types').TenantSummary[];
   /** 刷新令牌 */
   refreshToken: string | null;
}

const STORAGE_KEYS = {
   token: 'token',
   userInfo: 'userInfo',
   tenantId: 'tenantId',
   refreshToken: 'refreshToken',
} as const;

const clearAllStorage = () => {
   localStorage.removeItem(STORAGE_KEYS.token);
   localStorage.removeItem(STORAGE_KEYS.userInfo);
   localStorage.removeItem(STORAGE_KEYS.tenantId);
   localStorage.removeItem(STORAGE_KEYS.refreshToken);
   sessionStorage.removeItem(STORAGE_KEYS.token);
   sessionStorage.removeItem(STORAGE_KEYS.userInfo);
   sessionStorage.removeItem(STORAGE_KEYS.tenantId);
   sessionStorage.removeItem(STORAGE_KEYS.refreshToken);
};

const getStored = (key: string) =>
   localStorage.getItem(key) || sessionStorage.getItem(key);

/** 安全解析 localStorage 中的 JSON，无效数据返回 null 并清理 */
const safeJsonParse = (raw: string | null): unknown | null => {
   if (!raw || raw === 'undefined' || raw === 'null') return null;
   try {
      return JSON.parse(raw);
   } catch {
      return null;
   }
};

const token = getStored(STORAGE_KEYS.token);
const userInfoStr = getStored(STORAGE_KEYS.userInfo);
const storedTenantId = getStored(STORAGE_KEYS.tenantId);
const storedRefreshToken = getStored(STORAGE_KEYS.refreshToken);

const initialState: AuthState = {
   token: token && token !== 'undefined' ? token : null,
   userInfo: safeJsonParse(userInfoStr) as UserInfo | null,
   isAuthenticated: !!token && token !== 'undefined',
   currentTenantId: storedTenantId && storedTenantId !== 'undefined' ? storedTenantId : null,
   tenantList: [],
   refreshToken: storedRefreshToken && storedRefreshToken !== 'undefined' ? storedRefreshToken : null,
};

const persistSession = (
   token: string,
   userInfo: UserInfo,
   rememberMe?: boolean,
   tenantId?: string,
   refreshToken?: string
) => {
   clearAllStorage();
   const store = rememberMe ? localStorage : sessionStorage;
   store.setItem(STORAGE_KEYS.token, token);
   if (userInfo) store.setItem(STORAGE_KEYS.userInfo, JSON.stringify(userInfo));
   if (tenantId) store.setItem(STORAGE_KEYS.tenantId, tenantId);
   if (refreshToken) store.setItem(STORAGE_KEYS.refreshToken, refreshToken);
};

const authSlice = createSlice({
   name: 'auth',
   initialState,
   reducers: {
      setCredentials: (
         state,
         action: {
            payload: {
               token: string;
               userInfo: UserInfo;
               rememberMe?: boolean;
               tenantId?: string | number;
               refreshToken?: string;
            };
         }
      ) => {
         const { token, userInfo, rememberMe, tenantId, refreshToken } =
            action.payload;
         state.token = token;
         state.userInfo = userInfo;
         state.isAuthenticated = true;
         if (tenantId != null) state.currentTenantId = String(tenantId);
         if (refreshToken) state.refreshToken = refreshToken;

         persistSession(token, userInfo, rememberMe, tenantId != null ? String(tenantId) : undefined, refreshToken);
      },
      /** 切换租户 — 整体替换登录会话，清旧缓存 */
      switchTenant: (
         state,
         action: {
            payload: {
               token: string;
               refreshToken?: string;
               tenantId: string | number;
               userInfo: UserInfo;
            };
         }
      ) => {
         const { token, refreshToken, tenantId, userInfo } = action.payload;
         // 先清旧缓存（飞书要求：切租户成功后整体替换登录会话，清掉旧租户缓存和 SSE 连接）
         clearAllStorage();

         state.token = token;
         state.refreshToken = refreshToken || null;
         state.currentTenantId = String(tenantId);
         state.userInfo = userInfo;
         state.isAuthenticated = true;
         state.tenantList = []; // 清旧租户列表，由 UI 重新拉取

         // 新会话写入 localStorage（切租户不涉及 rememberMe，默认持久）
         localStorage.setItem(STORAGE_KEYS.token, token);
         localStorage.setItem(STORAGE_KEYS.userInfo, JSON.stringify(userInfo));
         localStorage.setItem(STORAGE_KEYS.tenantId, String(tenantId));
         if (refreshToken) localStorage.setItem(STORAGE_KEYS.refreshToken, refreshToken);
      },
      /** 设置租户列表（UI 拉取后存入 store） */
      setTenantList: (
         state,
         action: {
            payload: import('@/features/tenant/types').TenantSummary[];
         }
      ) => {
         state.tenantList = action.payload;
      },
      /** 仅更新当前租户 ID（Mock 模式 / 不涉及 token 切换时使用） */
      setCurrentTenantId: (
         state,
         action: { payload: string | number }
      ) => {
         state.currentTenantId = String(action.payload);
         localStorage.setItem(STORAGE_KEYS.tenantId, String(action.payload));
      },
      logout: (state) => {
         state.token = null;
         state.userInfo = null;
         state.isAuthenticated = false;
         state.currentTenantId = null;
         state.tenantList = [];
         state.refreshToken = null;
         clearAllStorage();
      },
      restoreAuth: (state) => {
         const token = getStored(STORAGE_KEYS.token);
         const userInfoStr = getStored(STORAGE_KEYS.userInfo);
         const tenantId = getStored(STORAGE_KEYS.tenantId);
         const refreshToken = getStored(STORAGE_KEYS.refreshToken);

         if (token && token !== 'undefined' && userInfoStr) {
            const userInfo = safeJsonParse(userInfoStr) as UserInfo | null;
            if (userInfo) {
               state.token = token;
               state.userInfo = userInfo;
               state.isAuthenticated = true;
               state.currentTenantId = tenantId && tenantId !== 'undefined' ? tenantId : null;
               state.refreshToken = refreshToken && refreshToken !== 'undefined' ? refreshToken : null;
            } else {
               clearAllStorage();
            }
         }
      },
   },
});

export const {
   setCredentials,
   switchTenant,
   setTenantList,
   setCurrentTenantId,
   logout,
   restoreAuth,
} = authSlice.actions;
export default authSlice.reducer;
