import request from '@/api/request';
import type { BaseResponse } from '@/api/types';
import type {
   IssueChartItem,
   DistributionResponse,
   ProjectItem,
   ProjectParams,
   DailyIssueCountItem,
} from '../types';
import { mutationOptions, queryOptions } from '@tanstack/react-query';
import { getStoredCurrentTenantId } from '@/store/slices/authSlice';

export const getDashboardList = async (): Promise<ProjectItem[]> => {
   const res = await request.get<
      unknown,
      BaseResponse<ProjectItem[]>
   >('/api/projects');

   if (res.code !== 0 && res.code !== 200) {
      throw new Error(res.msg || '项目列表加载失败');
   }

   return res.data || [];
};

export const createProject = async (
   params: ProjectParams
): Promise<ProjectItem> => {
   const res = await request.post<unknown, BaseResponse<ProjectItem>>(
      '/api/projects',
      params
   );

   if (res.code !== 0 && res.code !== 200) {
      throw new Error(res.msg || '项目创建失败');
   }

   return res.data;
};

export const deleteProject = async (id: number): Promise<void> => {
   await request.delete<unknown, BaseResponse<void>>(`/api/projects/${id}`);
};

export const updateProject = async (
   params: ProjectParams
): Promise<ProjectItem> => {
   const res = await request.put<unknown, BaseResponse<ProjectItem>>(
      '/api/projects',
      params
   );

   return res.data;
};

export const getIssueDistribution = async (): Promise<IssueChartItem[]> => {
   const res = await request.get<unknown, BaseResponse<DistributionResponse>>(
      '/api/audit-issues/count-issue'
   );

   const data = res.data;
   if (!data || typeof data !== 'object') return [];

   // category 列实际存的是 Rust 引擎 risk_type（"地域歧视"/"品牌指定"/… 等中文标签），
   // 直接透传后端 map 的 key/value，不再硬编码 budget/legal/demand 三个旧分类。
   return Object.entries(data)
      .map(([name, value]) => ({ name, value: Number(value) || 0 }))
      .filter((item) => item.value > 0)
      .sort((a, b) => b.value - a.value);
};

export const getDailyIssueCount = async (): Promise<DailyIssueCountItem[]> => {
   const res = await request.get<unknown, BaseResponse<Record<string, number>>>(
      '/api/audit-issues/count-by-day'
   );

   const data = res.data;

   if (!data || typeof data !== 'object') return [];

   // 后端返回 Map<当月第几日, 问题数>，直接透传
   return Object.entries(data).map(([day, value]) => ({
      name: day,
      count: Number(value) || 0,
   }));
};

export const dashboardOptions = {
   list: (tenantId?: string | null) => {
      const resolvedTenantId =
         tenantId === undefined ? getStoredCurrentTenantId() : tenantId;
      return queryOptions({
         queryKey: ['dashboardList', resolvedTenantId],
         queryFn: () => getDashboardList(),
         enabled: Boolean(resolvedTenantId),
         placeholderData: (previousData) => previousData,
         staleTime: 0,
      });
   },
   issueDistribution: (tenantId?: string | null) => {
      const resolvedTenantId =
         tenantId === undefined ? getStoredCurrentTenantId() : tenantId;
      return queryOptions({
         queryKey: ['issueDistribution', resolvedTenantId],
         queryFn: () => getIssueDistribution(),
         enabled: Boolean(resolvedTenantId),
         placeholderData: (previousData) => previousData,
         staleTime: 0,
      });
   },
   dailyIssues: (tenantId?: string | null) => {
      const resolvedTenantId =
         tenantId === undefined ? getStoredCurrentTenantId() : tenantId;
      return queryOptions({
         queryKey: ['dailyIssues', resolvedTenantId],
         queryFn: () => getDailyIssueCount(),
         enabled: Boolean(resolvedTenantId),
         placeholderData: (previousData) => previousData,
         staleTime: 0,
      });
   },
};

export const dashboardMutations = {
   create: () =>
      mutationOptions({
         mutationFn: (params: ProjectParams) => createProject(params),
      }),
   update: () =>
      mutationOptions({
         mutationFn: (params: ProjectParams) => updateProject(params),
      }),
   delete: () =>
      mutationOptions({
         mutationFn: (id: number) => deleteProject(id),
      }),
};