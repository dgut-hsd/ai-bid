import { describe, expect, it } from 'vitest';
import { normalizeAuthSession } from './session';

describe('normalizeAuthSession', () => {
  it('normalizes the backend UserLoginVO shape without inventing a refresh token', () => {
    const session = normalizeAuthSession({
      token: 'access-token',
      token_type: 'Bearer',
      expires_in: 86400,
      session_version: 3,
      user_info: {
        user_id: 10001,
        username: 'alice',
        realName: 'Alice',
      },
      current_tenant: {
        tenant_id: 20001,
        tenant_code: 'acme-bid',
        name: 'Acme 招标团队',
        status: 'ACTIVE',
        role: 'ADMIN',
        permissions: ['tenant.read'],
        is_current: true,
      },
      tenants: [
        {
          tenant_id: 20001,
          tenant_code: 'acme-bid',
          name: 'Acme 招标团队',
          status: 'ACTIVE',
          role: 'ADMIN',
          permissions: ['tenant.read'],
          is_current: true,
        },
      ],
    });

    expect(session).toEqual({
      token: 'access-token',
      token_type: 'Bearer',
      expires_in: 86400,
      session_version: 3,
      user_info: {
        user_id: '10001',
        username: 'alice',
        real_name: 'Alice',
      },
      current_tenant: {
        tenant_id: '20001',
        tenant_code: 'acme-bid',
        name: 'Acme 招标团队',
        status: 'ACTIVE',
        role: 'ADMIN',
        permissions: ['tenant.read'],
        is_current: true,
      },
      tenants: [
        {
          tenant_id: '20001',
          tenant_code: 'acme-bid',
          name: 'Acme 招标团队',
          status: 'ACTIVE',
          role: 'ADMIN',
          permissions: ['tenant.read'],
          is_current: true,
        },
      ],
    });
    expect(session).not.toHaveProperty('refresh_token');
  });

  it('preserves an authenticated session when the backend has no current tenant', () => {
    const session = normalizeAuthSession({
      token: 'access-token',
      token_type: 'Bearer',
      expires_in: 86400,
      session_version: 4,
      user_info: {
        user_id: '10001',
        username: 'alice',
        real_name: 'Alice',
      },
      current_tenant: null,
      tenants: [],
    });

    expect(session.current_tenant).toBeNull();
    expect(session.tenants).toEqual([]);
  });

  it('rejects a response that does not contain the required nested user info', () => {
    expect(() =>
      normalizeAuthSession({
        token: 'access-token',
        token_type: 'Bearer',
        expires_in: 86400,
        session_version: 3,
        current_tenant: null,
        tenants: [],
      })
    ).toThrow('user_info');
  });
});
