/**
 * 多租户 API 层
 *
 * 接口路径严格对齐飞书《前端多租户交接文档》。
 * 所有租户接口使用 snake_case，与旧业务接口（camelCase）区分。
 */
import request from '@/api/request';
import type { BaseResponse } from '@/api/types';
import {
  normalizeAuthResponse,
  normalizeResponse,
  normalizeTenantListResponse,
} from '@/features/login/api/session';
import type {
  ApiResponse,
  AuthSession,
  TenantListResponse,
  TenantSummary,
  CreateTenantParams,
  MemberListResponse,
  SwitchTenantParams,
} from '../types';

export const tenantApi = {
  // ─── 认证相关 ──────────────────────────────────────────────────────

  /** 切换租户 — 切完整体替换登录会话，旧 token 失效 */
  switchTenant: async (data: SwitchTenantParams): Promise<ApiResponse<AuthSession>> => {
    const response = await request.post<unknown, BaseResponse<unknown>>(
      '/api/auth/switch-tenant',
      data
    );
    return normalizeAuthResponse(response);
  },

  /** 刷新令牌 — Bearer，无请求体，返回新 AuthSession */
  refresh: (): Promise<BaseResponse<AuthSession>> => {
    return request.post('/api/auth/refresh', {});
  },

  // ─── 租户管理 ──────────────────────────────────────────────────────

  /** 获取租户列表（含 current_tenant_id） */
  getTenants: async (): Promise<ApiResponse<TenantListResponse>> => {
    const response = await request.get<unknown, BaseResponse<unknown>>('/api/tenants');
    return normalizeResponse(response, normalizeTenantListResponse);
  },

  /** 创建租户 */
  createTenant: (data: CreateTenantParams): Promise<BaseResponse<TenantSummary>> => {
    return request.post('/api/tenants', data);
  },

  /** 获取当前租户 */
  getCurrentTenant: (): Promise<BaseResponse<TenantSummary>> => {
    return request.get('/api/tenants/current');
  },

  /** 获取租户成员列表 */
  getMembers: (
    tenantId: string,
    page = 1,
    size = 20
  ): Promise<BaseResponse<MemberListResponse>> => {
    return request.get(`/api/tenants/${tenantId}/members`, {
      params: { page, size },
    });
  },
};
