/** Tenant API calls. All response data is normalized in login/api/session.ts. */
import request from '@/api/request';
import type { BaseResponse } from '@/api/types';
import {
  normalizeAuthResponse,
  normalizeMemberListResponse,
  normalizeResponse,
  normalizeTenantListResponse,
  normalizeTenantResponse,
} from '@/features/login/api/session';
import type {
  ApiResponse,
  AuthSession,
  CreateTenantParams,
  MemberListResponse,
  SwitchTenantParams,
  TenantListResponse,
  TenantSummary,
} from '../types';

export const tenantApi = {
  /** 切换租户 — 返回完整新会话，旧 token 立即失效 */
  switchTenant: async (
    data: SwitchTenantParams
  ): Promise<ApiResponse<AuthSession>> => {
    const response = await request.post<unknown, BaseResponse<unknown>>(
      '/api/auth/switch-tenant',
      data
    );
    return normalizeAuthResponse(response);
  },

  /** 刷新令牌 — 使用当前 access token 的 Bearer Header，无独立 refresh token */
  refresh: async (): Promise<ApiResponse<AuthSession>> => {
    const response = await request.post<unknown, BaseResponse<unknown>>(
      '/api/auth/refresh'
    );
    return normalizeAuthResponse(response);
  },

  /** 获取租户列表（含 current_tenant_id） */
  getTenants: async (): Promise<ApiResponse<TenantListResponse>> => {
    const response = await request.get<unknown, BaseResponse<unknown>>('/api/tenants');
    return normalizeResponse(response, normalizeTenantListResponse);
  },

  /** 创建租户；请求不包含服务端生成的 tenant_id 等字段 */
  createTenant: async (
    data: CreateTenantParams
  ): Promise<ApiResponse<TenantSummary>> => {
    const response = await request.post<unknown, BaseResponse<unknown>>(
      '/api/tenants',
      data
    );
    return normalizeTenantResponse(response);
  },

  /** 获取当前租户 */
  getCurrentTenant: async (): Promise<ApiResponse<TenantSummary>> => {
    const response = await request.get<unknown, BaseResponse<unknown>>(
      '/api/tenants/current'
    );
    return normalizeTenantResponse(response);
  },

  /** 获取租户成员列表 */
  getMembers: async (
    tenantId: string,
    page = 1,
    size = 20
  ): Promise<ApiResponse<MemberListResponse>> => {
    const response = await request.get<unknown, BaseResponse<unknown>>(
      `/api/tenants/${tenantId}/members`,
      { params: { page, size } }
    );
    return normalizeResponse(response, normalizeMemberListResponse);
  },
};
