import { store } from '@/store';
import { logout, setAuthSession } from '@/store/slices/authSlice';
import { normalizeAuthSession } from '@/features/login/api/session';
import type { BaseResponse } from './types';
import axios from 'axios';
import type { AxiosError, AxiosRequestConfig } from 'axios';

const request = axios.create({
  baseURL: import.meta.env.VITE_API_BASE_URL,
  timeout: 30000,
});

let isLoggingOut = false;

// ── token refresh 并发控制 ───────────────────────────────────────────
// 多个请求同时 401 时，只有第一个真正去 refresh，其余 await 同一个 pending promise；
// refresh 成功后所有排队请求用新 token 重放，避免误触发 logout 踢用户下线。
let refreshPromise: Promise<string> | null = null;

function getAccessToken(): string | null {
  return localStorage.getItem('token') || sessionStorage.getItem('token');
}

export function extractErrorCode(value: unknown): string | undefined {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return undefined;
  }
  const record = value as Record<string, unknown>;
  if (typeof record.error_code === 'string') return record.error_code;
  if (
    typeof record.data === 'object' &&
    record.data !== null &&
    !Array.isArray(record.data)
  ) {
    const data = record.data as Record<string, unknown>;
    if (typeof data.error_code === 'string') return data.error_code;
  }
  return undefined;
}

function doRefresh(): Promise<string> {
  if (refreshPromise) return refreshPromise;
  refreshPromise = (async () => {
    const accessToken = getAccessToken();
    if (!accessToken) throw new Error('no access token');
    const resp = await axios.post<BaseResponse<unknown>>(
      `${import.meta.env.VITE_API_BASE_URL}/api/auth/refresh`,
      {},
      { headers: { Authorization: `Bearer ${accessToken}` } }
    );
    if (
      resp.data.code !== 200 ||
      resp.data.data === null ||
      resp.data.data === undefined
    ) {
      throw new Error('refresh failed');
    }
    const session = normalizeAuthSession(resp.data.data);
    store.dispatch(setAuthSession({ session }));
    return session.token;
  })();
  refreshPromise
    .catch(() => { /* 错误由调用方处理 */ })
    .finally(() => {
      refreshPromise = null;
    });
  return refreshPromise;
}

function forceLogout() {
  if (isLoggingOut) return;
  isLoggingOut = true;
  console.error('登录过期，请重新登录');
  store.dispatch(logout());
  window.location.href = '/login';
  setTimeout(() => { isLoggingOut = false; }, 500);
}

// 请求拦截器：自动注入 Token
request.interceptors.request.use((config) => {
  const token =
    localStorage.getItem('token') || sessionStorage.getItem('token');

  if (token && config.headers) {
    config.headers.Authorization = `Bearer ${token}`;
  }

  return config;
});

// 响应拦截器：按飞书《前端多租户交接文档》错误码细分处理
request.interceptors.response.use(
  (response) => {
    if (response.config.responseType === 'blob') {
      return response.data;
    }
    return response.data;
  },
  async (error: AxiosError<{ code?: number; msg?: string; error_code?: string; data?: unknown }>) => {
    const status = error.response?.status;
    const errorCode = extractErrorCode(error.response?.data);
    const errorMsg = error.response?.data?.msg;
    const originalConfig = error.config as AxiosRequestConfig | undefined;

    // ── 401：认证相关，按 error_code 细分 ──────────────────────────
    if (status === 401) {
      // TENANT_SESSION_STALE：租户会话已过期，不重放，直接清登录引导重新切租户
      if (errorCode === 'TENANT_SESSION_STALE') {
        console.warn('租户会话已过期，请重新切换租户');
        forceLogout();
        return Promise.reject(error);
      }

      // AUTH_REQUIRED / AUTH_INVALID：尝试刷新一次（并发请求复用同一个 refresh）
      if (errorCode === 'AUTH_REQUIRED' || errorCode === 'AUTH_INVALID') {
        try {
          const newToken = await doRefresh();
          if (originalConfig?.headers) {
            originalConfig.headers.Authorization = `Bearer ${newToken}`;
          }
          return request(originalConfig!);
        } catch {
          forceLogout();
          return Promise.reject(error);
        }
      }

      // 其他 401（无 error_code 或未知）：默认清登录
      forceLogout();
      return Promise.reject(error);
    }

    // ── 403：权限相关，保留登录状态，提示无权限 ─────────────────────
    if (status === 403 && errorCode === 'TENANT_ROLE_FORBIDDEN') {
      console.warn('当前租户角色无权限执行此操作');
      // 不 logout，不跳转，由 UI 层提示用户
      return Promise.reject(error);
    }

    // ── 400：TENANT_REQUIRED — 引导选择或创建租户 ──────────────────
    if (status === 400 && errorCode === 'TENANT_REQUIRED') {
      console.warn('需要先选择或创建租户');
      // 不自行补 tenant_id，由 UI 层引导
      return Promise.reject(error);
    }

    // ── 404：TENANT_NOT_FOUND — 刷新租户列表 ────────────────────────
    if (status === 404 && errorCode === 'TENANT_NOT_FOUND') {
      console.warn('租户不存在，请刷新租户列表');
      return Promise.reject(error);
    }

    // ── 其他错误 ─────────────────────────────────────────────────────
    console.error(errorMsg || '网络请求错误');
    return Promise.reject(error);
  }
);

export default request;
