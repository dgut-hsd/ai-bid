import request from '@/api/request';
import type { BaseResponse } from '@/api/types';
import type { EnterpriseUser } from '@/features/enterprise/types';
import type { CreatePlatformTenantParams, PlatformTenant, PlatformTenantPage } from '../types';

export interface PlatformTenantQuery {
   page?: number;
   size?: number;
   keyword?: string;
   status?: string;
}

/** 系统管理模块 API（与后端 PlatformTenantController 对齐）。 */
export const platformApi = {
   listTenants: (
      params: PlatformTenantQuery = {}
   ): Promise<BaseResponse<PlatformTenantPage>> =>
      request.get<unknown, BaseResponse<PlatformTenantPage>>('/api/platform/tenants', {
         params,
      }),

   createTenant: (
      data: CreatePlatformTenantParams
   ): Promise<BaseResponse<PlatformTenant>> =>
      request.post<unknown, BaseResponse<PlatformTenant>>('/api/platform/tenants', data),

   listTenantMembers: (tenantId: string): Promise<BaseResponse<EnterpriseUser[]>> =>
      request.get<unknown, BaseResponse<EnterpriseUser[]>>(
         `/api/platform/tenants/${tenantId}/members`
      ),

   transferOwner: (
      tenantId: string,
      targetUserId: string
   ): Promise<BaseResponse<PlatformTenant>> =>
      request.post<unknown, BaseResponse<PlatformTenant>>(
         `/api/platform/tenants/${tenantId}/transfer-owner`,
         { target_user_id: targetUserId }
      ),

   disableTenant: (tenantId: string): Promise<BaseResponse<PlatformTenant>> =>
      request.post<unknown, BaseResponse<PlatformTenant>>(
         `/api/platform/tenants/${tenantId}/disable`
      ),

   restoreTenant: (tenantId: string): Promise<BaseResponse<PlatformTenant>> =>
      request.post<unknown, BaseResponse<PlatformTenant>>(
         `/api/platform/tenants/${tenantId}/restore`
      ),

   deleteTenant: (tenantId: string): Promise<BaseResponse<PlatformTenant>> =>
      request.delete<unknown, BaseResponse<PlatformTenant>>(
         `/api/platform/tenants/${tenantId}`
      ),
};