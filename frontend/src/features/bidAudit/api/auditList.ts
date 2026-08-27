import request from '@/api/request';
import {
   queryOptions,
   useMutation,
   useQueryClient,
} from '@tanstack/react-query';
import type { BaseResponse, PageResponse } from '@/api/types';
import type {
   AuditListQueryParams,
   AuditListItem,
   ProjectItem,
} from '../types';

export const getAllAuditList = async (): Promise<AuditListItem[]> => {
   const res = await request.get<unknown, BaseResponse<AuditListItem[]>>(
      '/api/bid-documents/projects'
   );

   if (res.code !== 0 && res.code !== 200) {
      throw new Error(res.msg || '审核项目列表加载失败');
   }

   return res.data || [];
};

export const getAuditListWithParams = async (
   params: AuditListQueryParams
): Promise<PageResponse<ProjectItem>> => {
   const queryParams = {
      page: params.page,
      size: params.size,
      bidName: params.bidName,
      fileCategory: params.fileCategory,
      status: params.status,
      uploadStartTime: params.uploadStartTime,
      uploadEndTime: params.uploadEndTime,
   };

   const res = await request.get<
      unknown,
      BaseResponse<PageResponse<ProjectItem>>
   >('/api/bid-documents', {
      params: queryParams,
   });

   if (res.code !== 0 && res.code !== 200) {
      throw new Error(res.msg || '审核列表加载失败');
   }

   return res.data;
};

export const getProjectVersions = async (
   projectId: number | null
): Promise<ProjectItem[]> => {
   const res = await request.get<unknown, BaseResponse<ProjectItem[]>>(
      `/api/bid-documents/project/${projectId}/versions`
   );

   if (res.code !== 0 && res.code !== 200) {
      throw new Error(res.msg || '项目版本列表加载失败');
   }

   return (
      res.data ?? [
         {
            id: 0,
            fileName: '',
            filePath: '',
            fileSize: 0,
            fileType: '',
            fileCategory: '',
            bidName: '',
            supplierName: '',
            budgetAmount: 0.0,
            pageCount: 0,
            parseStatus: 0,
            uploadUserId: 0,
            uploadTime: '',
            version: 0,
            projectId: 0,
            auditorName: '阿飘',
         },
      ]
   );
};

export const deleteProject = async (id: number): Promise<void> => {
   await request.delete<unknown, BaseResponse<void>>(`/api/projects/${id}`);
};

export const deleteTenderVersion = async (id: number): Promise<void> => {
   await request.delete<unknown, BaseResponse<void>>(`/api/bid-documents/${id}`);
};

export const auditListOptions = {
   list: () =>
      queryOptions({
         queryKey: ['auditList'],
         queryFn: () => getAllAuditList(),
         placeholderData: (previousData) => previousData,
         staleTime: 0,
      }),

   queryList: (params: AuditListQueryParams) =>
      queryOptions({
         queryKey: ['auditListWithParams', params],
         queryFn: () => getAuditListWithParams(params),
         placeholderData: (previousData) => previousData,
         staleTime: 0,
      }),

   detail: (projectId: number | null) =>
      queryOptions({
         queryKey: ['projectVersions', projectId],
         queryFn: () => getProjectVersions(projectId),
         placeholderData: (previousData) => previousData,
         staleTime: 0,
      }),
};

export const useDeleteProject = () => {
   const queryClient = useQueryClient();

   return useMutation({
      mutationFn: (id: number) => deleteProject(id),
      onSuccess: () => {
         queryClient.invalidateQueries({ queryKey: ['auditList'] });
         queryClient.invalidateQueries({ queryKey: ['auditListWithParams'] });
         queryClient.invalidateQueries({ queryKey: ['dashboardList'] });
      },
   });
};

export const useDeleteTenderVersion = () => {
   const queryClient = useQueryClient();

   return useMutation({
      mutationFn: (id: number) => deleteTenderVersion(id),
      onSuccess: () => {
         queryClient.invalidateQueries({ queryKey: ['auditList'] });
         queryClient.invalidateQueries({ queryKey: ['auditListWithParams'] });
         queryClient.invalidateQueries({ queryKey: ['projectVersions'] });
      },
   });
};

export const useDeleteTenderVersion = () => {
   const queryClient = useQueryClient();

   return useMutation({
      mutationFn: (id: number) => deleteTenderVersion(id),
      onSuccess: () => {
         queryClient.invalidateQueries({ queryKey: ['auditList'] });
         queryClient.invalidateQueries({ queryKey: ['auditListWithParams'] });
         queryClient.invalidateQueries({ queryKey: ['projectVersions'] });
      },
   });
};