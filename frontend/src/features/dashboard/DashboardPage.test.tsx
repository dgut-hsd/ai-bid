import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';
import { MemoryRouter } from 'react-router-dom';
import { Provider } from 'react-redux';
import { configureStore } from '@reduxjs/toolkit';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { App as AntdApp } from 'antd';
import authReducer from '@/store/slices/authSlice';
import { DashboardPage } from './DashboardPage';

function renderDashboard() {
  const store = configureStore({
    reducer: { auth: authReducer },
    preloadedState: {
      auth: {
        token: 'access-token',
        userInfo: { id: '10001', username: 'alice', realName: 'Alice' },
        isAuthenticated: true,
        currentTenantId: null,
        tenantList: [],
      },
    },
  });
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });

  return render(
    <Provider store={store}>
      <QueryClientProvider client={queryClient}>
        <AntdApp>
          <MemoryRouter initialEntries={['/dashboard']}>
            <DashboardPage />
          </MemoryRouter>
        </AntdApp>
      </QueryClientProvider>
    </Provider>
  );
}

describe('DashboardPage without a current tenant', () => {
  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
  });

  it('renders onboarding instead of expected no-tenant error panels', () => {
    renderDashboard();

    expect(screen.getByText('请先选择或创建租户')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '创建或选择租户' })).toBeInTheDocument();
    expect(screen.queryByText('项目列表加载失败')).not.toBeInTheDocument();
    expect(screen.queryByText('问题分布加载失败')).not.toBeInTheDocument();
    expect(screen.queryByText('审核统计加载失败')).not.toBeInTheDocument();
  });
});
