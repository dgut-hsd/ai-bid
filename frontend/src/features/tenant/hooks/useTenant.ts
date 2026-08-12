/**
 * 租户管理 Hook
 *
 * 封装租户列表拉取 + 切换租户逻辑。
 * 切租户流程（飞书交接文档）：调 switch-tenant → 拿新 AuthSession →
 * dispatch switchTenant（清旧缓存+写新会话）→ 刷新页面（断 SSE+清 React 状态）。
 */
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { useDispatch, useSelector } from 'react-redux';
import { App } from 'antd';
import { tenantApi, USE_MOCK } from '../api/tenant';
import { switchTenant as switchTenantAction, setTenantList, setCurrentTenantId } from '@/store/slices/authSlice';
import type { RootState } from '@/store';
import type { SwitchTenantParams } from '../types';

export const useTenant = () => {
  const dispatch = useDispatch();
  const queryClient = useQueryClient();
  const { message } = App.useApp();
  const { currentTenantId, tenantList, isAuthenticated } = useSelector(
    (state: RootState) => state.auth
  );

  // ── 拉取租户列表 ──────────────────────────────────────────────────
  const tenantsQuery = useQuery({
    queryKey: ['tenants'],
    queryFn: async () => {
      const resp = await tenantApi.getTenants();
      if (resp.code === 200 && resp.data) {
        dispatch(setTenantList(resp.data.items));
        return resp.data;
      }
      throw new Error(resp.msg || '获取租户列表失败');
    },
    enabled: isAuthenticated,
    staleTime: 5 * 60 * 1000, // 5 分钟不重复拉
  });

  // ── 切换租户 ──────────────────────────────────────────────────────
  const switchMutation = useMutation({
    mutationFn: (params: SwitchTenantParams) => tenantApi.switchTenant(params),
    onSuccess: (resp) => {
      if (resp.code === 200 && resp.data) {
        const session = resp.data;

        if (USE_MOCK) {
          // Mock 模式：只更新 currentTenantId，不动真实 token（避免覆盖登录态导致 401）
          dispatch(setCurrentTenantId(session.tenant_id));
          message.success(`已切换到租户（Mock 模式）`);
          // 不 reload，只更新 UI 状态
        } else {
          // 真实模式：dispatch switchTenant（清旧缓存+写新会话）→ 刷新页面断 SSE
          dispatch(
            switchTenantAction({
              token: session.token,
              refreshToken: session.refresh_token,
              tenantId: session.tenant_id,
              userInfo: {
                // 保留 user_id 原值，避免 UUID 经 Number() 变成 0
                id: session.user_id,
                username: session.username,
                realName: session.real_name || session.username,
              },
            })
          );
          queryClient.clear();
          message.success('租户切换成功，正在刷新…');
          setTimeout(() => {
            window.location.reload();
          }, 500);
        }
      } else {
        message.error(resp.msg || '租户切换失败');
      }
    },
    onError: (error: any) => {
      const errMsg =
        error?.response?.data?.msg || error?.message || '租户切换失败';
      message.error(errMsg);
    },
  });

  // ── 当前租户名（从列表里找，避免额外请求） ────────────────────────
  const currentTenant =
    tenantList.find((t) => t.tenant_id === currentTenantId) ||
    tenantsQuery.data?.items.find((t) => t.tenant_id === currentTenantId);

  return {
    tenantList,
    currentTenant,
    currentTenantId,
    isLoading: tenantsQuery.isLoading,
    isSwitching: switchMutation.isPending,
    switchTenant: (tenantId: string) =>
      switchMutation.mutate({ tenant_id: tenantId }),
    refetchTenants: tenantsQuery.refetch,
  };
};
