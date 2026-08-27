import request from '@/api/request';
import type { BaseResponse } from '@/api/types';
import type {
   CreateUserParams,
   EnterpriseUser,
   UpdateMemberParams,
   UpdateUserParams,
} from '../types';

/** 企业管理模块 API（与后端 EnterpriseUserController 对齐）。 */
export const enterpriseApi = {
   listUsers: (): Promise<BaseResponse<EnterpriseUser[]>> =>
      request.get<unknown, BaseResponse<EnterpriseUser[]>>('/api/enterprise/users'),

   createUser: (data: CreateUserParams): Promise<BaseResponse<EnterpriseUser>> =>
      request.post<unknown, BaseResponse<EnterpriseUser>>('/api/enterprise/users', data),

   updateUser: (userId: string, data: UpdateUserParams): Promise<BaseResponse<unknown>> =>
      request.put<unknown, BaseResponse<unknown>>(`/api/enterprise/users/${userId}`, data),

   updateMember: (
      userId: string,
      data: UpdateMemberParams
   ): Promise<BaseResponse<EnterpriseUser>> =>
      request.patch<unknown, BaseResponse<EnterpriseUser>>(
         `/api/enterprise/users/${userId}`,
         data
      ),

   resetPassword: (userId: string, password: string): Promise<BaseResponse<unknown>> =>
      request.post<unknown, BaseResponse<unknown>>(
         `/api/enterprise/users/${userId}/password`,
         { password }
      ),

   removeMember: (userId: string): Promise<BaseResponse<unknown>> =>
      request.delete<unknown, BaseResponse<unknown>>(`/api/enterprise/users/${userId}`),
};