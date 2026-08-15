import { act, renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { App as AntdApp } from 'antd';
import { Provider } from 'react-redux';
import { configureStore } from '@reduxjs/toolkit';
import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
import authReducer from '@/store/slices/authSlice';
import { normalizeAuthSession } from '@/features/login/api/session';
import { useTenant } from './useTenant';

const { getTenants, switchTenant } = vi.hoisted(() => ({
  getTenants: vi.fn(),
  switchTenant: vi.fn(),
}));

vi.mock('../api/tenant', () => ({
  tenantApi: { getTenants, switchTenant },
}));

const firstSession = normalizeAuthSession({
  token: 'first-token',
  token_type: 'Bearer',
  expires_in: 86400,
  session_version: 1,
  user_info: { user_id: 10001, username: 'alice', real_name: 'Alice' },
  current_tenant: {
    tenant_id: 20001,
    name: '第一个租户',
    status: 'ACTIVE',
    role: 'ADMIN',
    permissions: [],
    is_current: true,
  },
  tenants: [
    {
      tenant_id: 20001,
      name: '第一个租户',
      status: 'ACTIVE',
      role: 'ADMIN',
      permissions: [],
      is_current: true,
    },
  ],
});

const secondSession = normalizeAuthSession({
  token: 'second-token',
  token_type: 'Bearer',
  expires_in: 86400,
  session_version: 2,
  user_info: { user_id: 10001, username: 'alice', real_name: 'Alice' },
  current_tenant: {
    tenant_id: 20002,
    name: '第二个租户',
    status: 'ACTIVE',
    role: 'MEMBER',
    permissions: [],
    is_current: true,
  },
  tenants: [
    {
      tenant_id: 20002,
      name: '第二个租户',
      status: 'ACTIVE',
      role: 'MEMBER',
      permissions: [],
      is_current: true,
    },
  ],
});

function createWrapper(queryClient: QueryClient) {
  const store = configureStore({
    reducer: { auth: authReducer },
    preloadedState: {
      auth: {
        token: firstSession.token,
        userInfo: { id: '10001', username: 'alice', realName: 'Alice' },
        isAuthenticated: true,
        currentTenantId: '20001',
        tenantList: firstSession.tenants,
      },
    },
  });
  return {
    store,
    Wrapper: ({ children }: { children: React.ReactNode }) => (
      <Provider store={store}>
        <QueryClientProvider client={queryClient}>
          <AntdApp>{children}</AntdApp>
        </QueryClientProvider>
      </Provider>
    ),
  };
}

describe('useTenant', () => {
  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
    getTenants.mockResolvedValueOnce({
      code: 200,
      msg: 'success',
      data: { current_tenant_id: '20001', items: firstSession.tenants },
      timestamp: 1,
    });
    getTenants.mockResolvedValue({
      code: 200,
      msg: 'success',
      data: { current_tenant_id: '20002', items: secondSession.tenants },
      timestamp: 1,
    });
    switchTenant.mockResolvedValue({
      code: 200,
      msg: 'success',
      data: secondSession,
      timestamp: 1,
    });
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('replaces the session and clears tenant-scoped query data after switching', async () => {
    localStorage.setItem('token', firstSession.token);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    queryClient.setQueryData(['dashboardList', '20001'], ['old tenant data']);
    queryClient.setQueryData(['auditCount', '20001'], ['old tenant stats']);
    const { store, Wrapper } = createWrapper(queryClient);
    const { result } = renderHook(() => useTenant(), { wrapper: Wrapper });

    act(() => {
      result.current.switchTenant('20002');
    });

    await waitFor(() => {
      expect(switchTenant).toHaveBeenCalledWith({ tenant_id: '20002' });
    });

    expect(store.getState().auth.token).toBe('second-token');
    expect(store.getState().auth.currentTenantId).toBe('20002');
    expect(store.getState().auth.tenantList[0]?.name).toBe('第二个租户');
    expect(queryClient.getQueryData(['dashboardList', '20001'])).toBeUndefined();
    expect(queryClient.getQueryData(['auditCount', '20001'])).toBeUndefined();
    expect(localStorage.getItem('token')).toBe('second-token');
  });
});
