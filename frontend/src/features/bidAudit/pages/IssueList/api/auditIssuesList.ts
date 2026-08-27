import request from '@/api/request';
import { queryOptions } from '@tanstack/react-query';
import type { BaseResponse } from '@/api/types';
import type { AuditStatus, AuditResult, BidDetail } from '../types';

export const getAuditStatus = async (taskId: string): Promise<AuditStatus> => {
   const res = await request.get<unknown, BaseResponse<AuditStatus>>(
      `/api/audit-tasks/${taskId}`
   );

   return res.data;
};

/** 按标书(bid)取服务端裁决的「当前任务」状态；无任务时 taskId=null。 */
export const getAuditStatusByBid = async (
   bidId: number
): Promise<AuditStatus> => {
   const res = await request.get<unknown, BaseResponse<AuditStatus>>(
      `/api/audit-tasks/by-bid/${bidId}`
   );

   return res.data;
};

export const getAuditResult = async (
   taskId: string,
   params?: { page?: number; size?: number; sinceIssueNo?: string }
): Promise<AuditResult> => {
   const res = await request.get<unknown, BaseResponse<AuditResult>>(
      `/api/audit-tasks/${taskId}/result`,
      {
         params,
      }
   );

   return res.data;
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

   statusByBid: (bidId: number) =>
      queryOptions({
         queryKey: ['auditStatusByBid', bidId],
         queryFn: () => getAuditStatusByBid(bidId),
         enabled: !!bidId && !Number.isNaN(bidId),
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
