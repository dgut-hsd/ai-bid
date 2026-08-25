import request from '@/api/request';
import type { BaseResponse } from '@/api/types';
import type { AuthResponse, LoginParams, RegisterParams } from '../types';
import { normalizeAuthResponse } from './session';

export const loginApi = {
   login: async (data: LoginParams): Promise<AuthResponse> => {
      const response = await request.post<unknown, BaseResponse<unknown>>(
         '/api/auth/login',
         data
      );
      return normalizeAuthResponse(response);
   },

   logout: (): Promise<BaseResponse<unknown>> => {
      return request.post('/api/auth/logout');
   },

   /**
    * 刷新登录态：必须用 refreshToken 作为凭证，而非可能已经过期的 access token。
    * 这里显式设置 Authorization（请求拦截器会尊重调用方已设置的 Authorization，不再覆盖为 access token），
    * 否则后端 jjwt 拒绝过期 token → 刷新永远失败、用户被强制登出。
    */
   refresh: async (): Promise<AuthResponse> => {
      const refreshToken =
         localStorage.getItem('refreshToken') || sessionStorage.getItem('refreshToken');
      if (!refreshToken) throw new Error('no refresh token');
      const response = await request.post<unknown, BaseResponse<unknown>>(
         '/api/auth/refresh',
         {},
         { headers: { Authorization: `Bearer ${refreshToken}` } }
      );
      return normalizeAuthResponse(response);
   },

   register: (data: RegisterParams): Promise<BaseResponse<unknown>> => {
      return request.post('/api/auth/register', data);
   },

    /** 用户本人修改密码：校验旧密码，设置新密码，旧会话失效。 */
    changePassword: (data: {
      old_password: string;
      new_password: string;
    }): Promise<BaseResponse<unknown>> => {
      return request.post('/api/auth/change-password', data);
    },
};
