import { store } from '@/store';
import { logout } from '@/store/slices/authSlice';
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
let isRefreshing = false;
let refreshPromise: Promise<string> | null = null;

function getRefreshToken(): string | null {
  return localStorage.getItem('refreshToken') || sessionStorage.getItem('refreshToken');
}

function getCurrentStorage(): Storage {
  return localStorage.getItem('token') ? localStorage : sessionStorage;
}

function doRefresh(): Promise<string> {
  if (refreshPromise) return refreshPromise;
  isRefreshing = true;
  refreshPromise = (async () => {
    const refreshToken = getRefreshToken();
    if (!refreshToken) throw new Error('no refresh token');
    const resp = await axios.post(
      `${import.meta.env.VITE_API_BASE_URL}/api/auth/refresh`,
      {},
      { headers: { Authorization: `Bearer ${refreshToken}` } }
    );
    if (resp.data?.code !== 200 || !resp.data?.data) {
      throw new Error('refresh failed');
    }
    const newToken: string = resp.data.data.token;
    const storage = getCurrentStorage();
    storage.setItem('token', newToken);
    if (resp.data.data.refresh_token) {
      storage.setItem('refreshToken', resp.data.data.refresh_token);
    }
    return newToken;
  })();
  refreshPromise
    .catch(() => { /* 错误由调用方处理 */ })
    .finally(() => {
      isRefreshing = false;
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
  async (error: AxiosError<{ code?: number; msg?: string; error_code?: string }>) => {
    const status = error.response?.status;
    const errorCode = error.response?.data?.error_code;
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
        } catch (refreshError) {
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
