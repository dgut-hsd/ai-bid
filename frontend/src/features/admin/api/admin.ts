import request from '@/api/request';
import type { BaseResponse } from '@/api/types';
import type { AdminUser, CreateUserParams } from '../types';

/** 系统管理模块 API（与后端 AdminUserController 对齐）。 */
export const adminApi = {
  listUsers: (): Promise<BaseResponse<AdminUser[]>> => {
    return request.get('/api/admin/users');
  },

  createUser: (data: CreateUserParams): Promise<BaseResponse<AdminUser>> => {
    return request.post('/api/admin/users', data);
  },

  updateUser: (
    userId: string,
    data: { username?: string; real_name?: string }
  ): Promise<BaseResponse<unknown>> => {
    return request.put(`/api/admin/users/${userId}`, data);
  },

  resetPassword: (userId: string, password: string): Promise<BaseResponse<unknown>> => {
    return request.post(`/api/admin/users/${userId}/password`, { password });
  },

  removeMember: (userId: string): Promise<BaseResponse<unknown>> => {
    return request.delete(`/api/admin/users/${userId}`);
  },
};