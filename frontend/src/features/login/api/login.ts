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
};
