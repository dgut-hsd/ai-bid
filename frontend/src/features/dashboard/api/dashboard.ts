import request from '@/api/request';
import type { BaseResponse } from '@/api/types';
import type {
   AuditCount,
   IssueChartItem,
   DistributionResponse,
   ProjectItem,
   ProjectParams,
   AuditCountItem,
} from '../types';
import { mutationOptions, queryOptions } from '@tanstack/react-query';
import { getStoredCurrentTenantId } from '@/store/slices/authSlice';

const WEEKDAY_LABEL_MAP: Record<string, string> = {
   Monday: '周一',
   Tuesday: '周二',
   Wednesday: '周三',
   Thursday: '周四',
   Friday: '周五',
   Saturday: '周六',
   Sunday: '周日',
};

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

   return [
      { name: '合规性', value: data?.budget || 0 },
      { name: '法律法规', value: data?.legal || 0 },
      { name: '采购需求', value: data?.demand || 0 },
   ];
};

export const getAuditCount = async (): Promise<AuditCountItem[]> => {
   const res = await request.get<unknown, BaseResponse<AuditCount>>(
      '/api/audit-tasks/count-audit'
   );

   const data = res.data;

   if (!data) return [];

   return Object.entries(data).map(([key, value]) => ({
      name: WEEKDAY_LABEL_MAP[key] ?? key,
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
   auditCount: (tenantId?: string | null) => {
      const resolvedTenantId =
         tenantId === undefined ? getStoredCurrentTenantId() : tenantId;
      return queryOptions({
         queryKey: ['auditCount', resolvedTenantId],
         queryFn: () => getAuditCount(),
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
