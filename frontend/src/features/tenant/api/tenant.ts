/**
 * 多租户 API 层
 *
 * 接口路径严格对齐飞书《前端多租户交接文档》。
 * 所有租户接口使用 snake_case，与旧业务接口（camelCase）区分。
 */
import request from '@/api/request';
import type { BaseResponse } from '@/api/types';
import type {
  AuthSession,
  TenantListResponse,
  TenantSummary,
  CreateTenantParams,
  MemberListResponse,
  SwitchTenantParams,
} from '../types';

// ─── 是否启用 Mock（PR#6 合并前为 true，联调时改 false） ─────────────
export const USE_MOCK = true;

export const tenantApi = {
  // ─── 认证相关 ──────────────────────────────────────────────────────

  /** 切换租户 — 切完整体替换登录会话，旧 token 失效 */
  switchTenant: (data: SwitchTenantParams): Promise<BaseResponse<AuthSession>> => {
    if (USE_MOCK) return mockSwitchTenant(data);
    return request.post('/api/auth/switch-tenant', data);
  },

  /** 刷新令牌 — Bearer，无请求体，返回新 AuthSession */
  refresh: (): Promise<BaseResponse<AuthSession>> => {
    if (USE_MOCK) return mockRefresh();
    return request.post('/api/auth/refresh', {});
  },

  // ─── 租户管理 ──────────────────────────────────────────────────────

  /** 获取租户列表（含 current_tenant_id） */
  getTenants: (): Promise<BaseResponse<TenantListResponse>> => {
    if (USE_MOCK) return mockGetTenants();
    return request.get('/api/tenants');
  },

  /** 创建租户 */
  createTenant: (data: CreateTenantParams): Promise<BaseResponse<TenantSummary>> => {
    if (USE_MOCK) return mockCreateTenant(data);
    return request.post('/api/tenants', data);
  },

  /** 获取当前租户 */
  getCurrentTenant: (): Promise<BaseResponse<TenantSummary>> => {
    if (USE_MOCK) return mockGetCurrentTenant();
    return request.get('/api/tenants/current');
  },

  /** 获取租户成员列表 */
  getMembers: (
    tenantId: string,
    page = 1,
    size = 20
  ): Promise<BaseResponse<MemberListResponse>> => {
    if (USE_MOCK) return mockGetMembers(tenantId, page, size);
    return request.get(`/api/tenants/${tenantId}/members`, {
      params: { page, size },
    });
  },
};

// ─── Mock 数据（PR#6 合并后删除或设 USE_MOCK=false） ──────────────────

const MOCK_TOKEN = 'mock-access-token';
const MOCK_REFRESH_TOKEN = 'mock-refresh-token';

const mockTenants: import('../types').TenantSummary[] = [
  { tenant_id: 't_001', name: '默认租户', role: 'owner', created_at: '2026-08-01T10:00:00Z' },
  { tenant_id: 't_002', name: '测试租户', role: 'member', created_at: '2026-08-03T14:30:00Z' },
];

function delay(ms = 300) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function mockSwitchTenant(data: SwitchTenantParams): Promise<BaseResponse<AuthSession>> {
  await delay();
  const target = mockTenants.find((t) => t.tenant_id === data.tenant_id);
  if (!target) {
    return { code: 404, msg: 'TENANT_NOT_FOUND', data: {} as AuthSession, timestamp: Date.now() };
  }
  return {
    code: 200,
    msg: 'success',
    data: {
      token: `${MOCK_TOKEN}-${data.tenant_id}`,
      refresh_token: `${MOCK_REFRESH_TOKEN}-${data.tenant_id}`,
      tenant_id: data.tenant_id,
      user_id: 'u_001',
      username: 'admin',
      real_name: '管理员',
    },
    timestamp: Date.now(),
  };
}

async function mockRefresh(): Promise<BaseResponse<AuthSession>> {
  await delay();
  return {
    code: 200,
    msg: 'success',
    data: {
      token: `${MOCK_TOKEN}-refreshed`,
      refresh_token: `${MOCK_REFRESH_TOKEN}-refreshed`,
      tenant_id: 't_001',
      user_id: 'u_001',
      username: 'admin',
      real_name: '管理员',
    },
    timestamp: Date.now(),
  };
}

async function mockGetTenants(): Promise<BaseResponse<TenantListResponse>> {
  await delay();
  return {
    code: 200,
    msg: 'success',
    data: {
      current_tenant_id: 't_001',
      items: [...mockTenants], // 返回新数组引用，确保 React Query 能感知变化
    },
    timestamp: Date.now(),
  };
}

async function mockCreateTenant(
  data: CreateTenantParams
): Promise<BaseResponse<TenantSummary>> {
  await delay();
  const newTenant: import('../types').TenantSummary = {
    tenant_id: `t_${Date.now()}`,
    name: data.name,
    role: 'owner',
    created_at: new Date().toISOString(),
  };
  mockTenants.push(newTenant);
  return { code: 200, msg: 'success', data: newTenant, timestamp: Date.now() };
}

async function mockGetCurrentTenant(): Promise<BaseResponse<TenantSummary>> {
  await delay();
  return { code: 200, msg: 'success', data: mockTenants[0], timestamp: Date.now() };
}

async function mockGetMembers(
  _tenantId: string,
  page: number,
  size: number
): Promise<BaseResponse<MemberListResponse>> {
  await delay();
  const allMembers: import('../types').TenantMember[] = [
    { user_id: 'u_001', username: 'admin', real_name: '管理员', role: 'owner', joined_at: '2026-08-01T10:00:00Z' },
    { user_id: 'u_002', username: 'reviewer1', real_name: '张三', role: 'member', joined_at: '2026-08-02T09:00:00Z' },
    { user_id: 'u_003', username: 'reviewer2', real_name: '李四', role: 'member', joined_at: '2026-08-04T16:00:00Z' },
  ];
  const start = (page - 1) * size;
  const items = allMembers.slice(start, start + size);
  return {
    code: 200,
    msg: 'success',
    data: { page, size, total: allMembers.length, items },
    timestamp: Date.now(),
  };
}
