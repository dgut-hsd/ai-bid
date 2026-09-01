import request from '@/api/request';
import { queryOptions } from '@tanstack/react-query';
import type { BaseResponse } from '@/api/types';
import type { AuditStatus, AuditResult, BidDetail } from '../types';
import {
   mapBackendGraphSnapshot,
   type BackendGraphSnapshot,
} from '@/features/bidAudit/utils/mapFinding';

type BackendAuditResult = Omit<AuditResult, 'graphSnapshot'> & {
   graphSnapshot?: BackendGraphSnapshot;
};

export const getAuditStatus = async (taskId: string): Promise<AuditStatus> => {
   const res = await request.get<unknown, BaseResponse<AuditStatus>>(
      `/api/audit-tasks/${taskId}`
   );

   return res.data;
};

export const getAuditResult = async (
   taskId: string,
   params?: { page?: number; size?: number; sinceIssueNo?: string }
): Promise<AuditResult> => {
   const res = await request.get<unknown, BaseResponse<BackendAuditResult>>(
      `/api/audit-tasks/${taskId}/result`,
      {
         params,
      }
   );

   return {
      ...res.data,
      graphSnapshot: res.data.graphSnapshot
         ? mapBackendGraphSnapshot(res.data.graphSnapshot)
         : undefined,
   };
};

export const getBidDetail = async (id: number): Promise<BidDetail> => {
   const res = await request.get<unknown, BaseResponse<BidDetail>>(
      `/api/bid-documents/${id}`
   );

   return res.data;
};

export const auditIssuesListOptions = {
   status: (taskId: string) =>
      queryOptions({
         queryKey: ['auditStatus', taskId],
         queryFn: () => getAuditStatus(taskId),
         enabled: !!taskId,
         refetchInterval: 3000,
      }),

   result: (
      taskId: string,
      params?: { page?: number; size?: number; sinceIssueNo?: string }
   ) =>
      queryOptions({
         queryKey: ['auditResult', taskId, params],
         queryFn: () => getAuditResult(taskId, params),
         enabled: !!taskId,
         staleTime: 5 * 60 * 1000,
      }),

   bidDetail: (id: number) =>
      queryOptions({
         queryKey: ['bidDetail', id],
         queryFn: () => getBidDetail(id),
         enabled: !!id,
         staleTime: 5 * 60 * 1000,
      }),
};
