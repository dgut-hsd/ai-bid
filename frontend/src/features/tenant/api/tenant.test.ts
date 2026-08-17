import { beforeEach, describe, expect, it, vi } from 'vitest';

const { get, post } = vi.hoisted(() => ({
  get: vi.fn(),
  post: vi.fn(),
}));

vi.mock('@/api/request', () => ({ default: { get, post } }));

import { tenantApi } from './tenant';

describe('tenantApi', () => {
  beforeEach(() => {
    get.mockReset();
    post.mockReset();
  });

  it('normalizes numeric IDs in switch-tenant auth responses', async () => {
    post.mockResolvedValue({
      code: 200,
      msg: 'success',
      data: {
        token: 'new-token',
        token_type: 'Bearer',
        expires_in: 86400,
        session_version: 2,
        user_info: { user_id: 10001, username: 'alice', realName: 'Alice' },
        current_tenant: {
          tenant_id: 20002,
          name: '第二个租户',
          status: 'ACTIVE',
          role: 'MEMBER',
          permissions: [],
          is_current: true,
        },
        tenants: [],
      },
      timestamp: 1,
    });

    const response = await tenantApi.switchTenant({ tenant_id: '20002' });

    expect(post).toHaveBeenCalledWith('/api/auth/switch-tenant', {
      tenant_id: '20002',
    });
    expect(response.data?.current_tenant?.tenant_id).toBe('20002');
    expect(response.data).not.toHaveProperty('refresh_token');
  });

  it('normalizes numeric IDs in the tenant list', async () => {
    get.mockResolvedValue({
      code: 200,
      msg: 'success',
      data: {
        current_tenant_id: 20001,
        items: [
          {
            tenant_id: 20001,
            name: '第一个租户',
            status: 'ACTIVE',
            role: 'ADMIN',
            permissions: [],
            is_current: true,
          },
        ],
      },
      timestamp: 1,
    });

    const response = await tenantApi.getTenants();

    expect(get).toHaveBeenCalledWith('/api/tenants');
    expect(response.data?.current_tenant_id).toBe('20001');
    expect(response.data?.items[0]?.tenant_id).toBe('20001');
  });
});
