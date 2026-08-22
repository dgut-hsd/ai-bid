import { beforeEach, describe, expect, it, vi } from 'vitest';

const { post } = vi.hoisted(() => ({ post: vi.fn() }));

vi.mock('@/api/request', () => ({ default: { post } }));

import { loginApi } from './login';

const rawSession = {
  token: 'access-token',
  token_type: 'Bearer',
  expires_in: 86400,
  session_version: 3,
  user_info: { user_id: 10001, username: 'alice', realName: 'Alice' },
  current_tenant: null,
  tenants: [],
};

describe('loginApi', () => {
  beforeEach(() => {
    post.mockReset();
    post.mockResolvedValue({
      code: 200,
      msg: 'success',
      data: rawSession,
      timestamp: 1,
    });
  });

  it('sends the contract username and normalizes the nested login response', async () => {
    const response = await loginApi.login({ username: 'alice', password: 'secret' });

    expect(post).toHaveBeenCalledWith('/api/auth/login', {
      username: 'alice',
      password: 'secret',
    });
    expect(response.data?.user_info.user_id).toBe('10001');
    expect(response.data?.current_tenant).toBeNull();
  });

  it('refreshes with the current Bearer session and does not require a refresh token', async () => {
    const response = await loginApi.refresh();

    expect(post).toHaveBeenCalledWith('/api/auth/refresh', {});
    expect(response.data?.token).toBe('access-token');
  });
});
