import { beforeEach, describe, expect, it } from 'vitest';
import authReducer, {
  restoreAuth,
  setAuthSession,
} from './authSlice';
import type { AuthState } from './authSlice';
import { normalizeAuthSession } from '@/features/login/api/session';

const session = normalizeAuthSession({
  token: 'access-token',
  token_type: 'Bearer',
  expires_in: 86400,
  session_version: 3,
  user_info: {
    user_id: 10001,
    username: 'alice',
    real_name: 'Alice',
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

const noTenantSession = normalizeAuthSession({
  token: 'no-tenant-token',
  token_type: 'Bearer',
  expires_in: 86400,
  session_version: 4,
  user_info: {
    user_id: 10001,
    username: 'alice',
    real_name: 'Alice',
  },
  current_tenant: null,
  tenants: [],
});

const emptyState: AuthState = {
  token: null,
  userInfo: null,
  isAuthenticated: false,
  currentTenantId: null,
  tenantList: [],
};

describe('auth session persistence', () => {
  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
  });

  it('persists the normalized user, current tenant, and tenant list at login', () => {
    const state = authReducer(
      emptyState,
      setAuthSession({ session, rememberMe: true })
    );

    expect(state.token).toBe('access-token');
    expect(state.userInfo).toEqual({
      id: '10001',
      username: 'alice',
      realName: 'Alice',
    });
    expect(state.currentTenantId).toBe('20001');
    expect(state.tenantList).toHaveLength(1);
    expect(JSON.parse(localStorage.getItem('authSession') || '{}')).toEqual(session);
    expect(sessionStorage.getItem('authSession')).toBeNull();
  });

  it('clears a previous tenant when the authenticated session has no current tenant', () => {
    const loggedIn = authReducer(
      emptyState,
      setAuthSession({ session, rememberMe: false })
    );
    const state = authReducer(
      loggedIn,
      setAuthSession({ session: noTenantSession, rememberMe: false })
    );

    expect(state.token).toBe('no-tenant-token');
    expect(state.currentTenantId).toBeNull();
    expect(state.tenantList).toEqual([]);
    expect(sessionStorage.getItem('tenantId')).toBeNull();
  });

  it('restores the complete normalized session instead of only the access token', () => {
    localStorage.setItem('authSession', JSON.stringify(session));

    const state = authReducer(emptyState, restoreAuth());

    expect(state.isAuthenticated).toBe(true);
    expect(state.userInfo?.id).toBe('10001');
    expect(state.currentTenantId).toBe('20001');
    expect(state.tenantList[0]?.name).toBe('Acme 招标团队');
  });
});
