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
import { tenantApi } from '../api/tenant';
import { switchTenant as switchTenantAction, setTenantList } from '@/store/slices/authSlice';
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
           // 后端 sessionResponse 真实返回结构（与前端 AuthSession 类型声明不同）：
           //   token（顶层）
           //   user_info { user_id, username, realName }  —— 嵌套，user_id 为数字 Long
           //   current_tenant { tenant_id, name, ... }    —— 嵌套，tenant_id 为数字 Long
           //   tenants[]
           //   ❌ 没有顶层 tenant_id  ❌ 没有 refresh_token
           // 故不能按顶层/驼峰读取。这里统一按真实结构提取并保留兼容回退。
           const raw = resp.data as any;
           const userInfoRaw = raw.userInfo ?? raw.user_info ?? {};
           const currentTenantRaw = raw.currentTenant ?? raw.current_tenant ?? {};

           const tenantId =
              currentTenantRaw.tenant_id ??
              currentTenantRaw.tenantId ??
              raw.tenant_id ??
              undefined;

           // 真实模式：dispatch switchTenant（清旧缓存+写新会话）→ 刷新页面断 SSE
           dispatch(
              switchTenantAction({
                 token: raw.token,
                 // 后端 switch-tenant 响应不含 refresh_token，传 undefined 由 reducer 保留原值
                 refreshToken: raw.refresh_token ?? raw.refreshToken,
                 tenantId,
                 userInfo: {
                    // 后端 user_id 是数字 Long，统一保留原值避免被 Number() 转成 0
                    id: userInfoRaw.user_id ?? userInfoRaw.userId,
                    username: userInfoRaw.username,
                    realName:
                       userInfoRaw.realName ??
                       userInfoRaw.real_name ??
                       userInfoRaw.username,
                 },
              })
           );
        // 不再整页 reload：沙箱/预览 iframe 会拦截 reload 并抛出
        // "Permissions policy violation: unload is not allowed"，导致刷新不生效。
        // 改用 React Query 失效重拉，所有查询（含 dashboard）会用新 token 重新请求。
        queryClient.invalidateQueries();
        message.success('租户切换成功，正在刷新…');
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
