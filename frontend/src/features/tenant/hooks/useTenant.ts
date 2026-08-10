/**
 * Tenant list and session switching.
 *
 * A successful switch replaces the complete auth session before clearing all
 * React Query state. The subsequent reload is intentional: it closes any
 * active SSE readers that belong to the old token/session.
 */
import { useEffect } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useDispatch, useSelector } from 'react-redux';
import { App } from 'antd';
import { tenantApi } from '../api/tenant';
import {
  setAuthSession,
  setCurrentTenantId,
  setTenantList,
} from '@/store/slices/authSlice';
import type { RootState } from '@/store';
import type { SwitchTenantParams } from '../types';

function errorMessage(error: unknown, fallback: string): string {
  if (typeof error === 'object' && error !== null) {
    const record = error as {
      response?: { data?: { msg?: string } };
      message?: string;
    };
    return record.response?.data?.msg || record.message || fallback;
  }
  return fallback;
}

export const useTenant = () => {
  const dispatch = useDispatch();
  const queryClient = useQueryClient();
  const { message } = App.useApp();
  const { currentTenantId, tenantList, isAuthenticated, token } = useSelector(
    (state: RootState) => state.auth
  );

  const tenantsQuery = useQuery({
    queryKey: ['tenants', token],
    queryFn: async () => {
      const response = await tenantApi.getTenants();
      if (response.code === 200 && response.data) {
        return response.data;
      }
      throw new Error(response.msg || '获取租户列表失败');
    },
    enabled: isAuthenticated && Boolean(token),
    staleTime: 5 * 60 * 1000,
  });

  // Keep server list synchronization in the observer lifecycle. A tenant
  // switch clears this query; an in-flight old response must not dispatch its
  // previous current_tenant_id back into the new session.
  useEffect(() => {
    if (!tenantsQuery.data) return;
    dispatch(setTenantList(tenantsQuery.data.items));
    dispatch(setCurrentTenantId(tenantsQuery.data.current_tenant_id));
  }, [dispatch, tenantsQuery.data]);

  const switchMutation = useMutation({
    mutationFn: (params: SwitchTenantParams) => tenantApi.switchTenant(params),
    onSuccess: (response) => {
      if (response.code === 200 && response.data) {
        dispatch(setAuthSession({ session: response.data }));
        // Clear every cached query because existing keys may not all encode
        // tenant identity. This also removes stale dashboard/SSE snapshots.
        queryClient.clear();
        message.success('租户切换成功，正在刷新…');
        window.setTimeout(() => {
          window.location.reload();
        }, 500);
      } else {
        message.error(response.msg || '租户切换失败');
      }
    },
    onError: (error: unknown) => {
      message.error(errorMessage(error, '租户切换失败'));
    },
  });

  const currentTenant =
    tenantList.find((tenant) => tenant.tenant_id === currentTenantId) ||
    tenantsQuery.data?.items.find((tenant) => tenant.tenant_id === currentTenantId);

  return {
    tenantList,
    currentTenant,
    currentTenantId,
    isLoading: tenantsQuery.isLoading && tenantList.length === 0,
    isSwitching: switchMutation.isPending,
    switchTenant: (tenantId: string) =>
      switchMutation.mutate({ tenant_id: tenantId }),
    refetchTenants: tenantsQuery.refetch,
  };
};
