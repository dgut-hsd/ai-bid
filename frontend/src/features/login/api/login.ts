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

   /** Refresh uses the current access token as a Bearer credential. */
   refresh: async (): Promise<AuthResponse> => {
      const response = await request.post<unknown, BaseResponse<unknown>>(
         '/api/auth/refresh',
         {}
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
