import request from '@/api/request';
import { queryOptions } from '@tanstack/react-query';
import type { BaseResponse } from '@/api/types';
import type {
   AuditStatus,
   AuditResult,
   CreateTaskParams,
   BidDetail,
} from '../types';

export const createTask = async (
   params: CreateTaskParams
): Promise<{ taskId: string }> => {
   const res = await request.post<unknown, BaseResponse<{ taskId: string }>>(
      '/api/audit-tasks',
      params
   );
   
   return res.data;
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

/** 所有 SSE 事件类型（与 Java SseEventTypeEnum 对齐） */
export type SseEventType =
   | 'progress'
   | 'issue'
   | 'issues'
   | 'complete'
   | 'agent_progress'
   | 'trace'
   | 'phase'
   | 'stats'
   | 'finding_added'
   | 'finding_updated'
   | 'finding_removed'
   | 'call_log';

/** SSE 断线重连参数（对齐 frontend/docs/设计.md "连接断开 → 自动重连"） */
const SSE_MAX_RETRIES = 5;
const SSE_BASE_DELAY_MS = 1000; // 指数退避：1s → 2s → 4s → 8s → 16s

export const connectStream = async (
   taskId: string,
   lastEventId: string,
   onMessage: (type: SseEventType, data: unknown) => void,
   onComplete: () => void,
   onError: (err: Error) => void
) => {
   // ── 单次 SSE 连接（由外层重试循环调用） ───────────────────
   const doConnect = async (): Promise<boolean> => {
      // 每次重连从 localStorage 读最新 lastEventId（前一次连接可能已写入了更新的 id）
      let currentLastId = lastEventId;
      try {
         const saved = localStorage.getItem(`auditLastEvent:${taskId}`);
         if (saved) currentLastId = saved;
      } catch { /* ignore */ }

      const token =
         localStorage.getItem('token') || sessionStorage.getItem('token');
      const baseUrl = import.meta.env.VITE_API_BASE_URL || '';
      const url = `${baseUrl}/api/audit-tasks/${taskId}/stream`;

      const response = await fetch(url, {
         headers: {
            Authorization: token ? `Bearer ${token}` : '',
            'Last-Event-ID': currentLastId,
            Accept: 'text/event-stream',
         },
      });

      if (!response.ok) throw new Error(`SSE 连接失败: ${response.status}`);
      if (!response.body) throw new Error('浏览器不支持 Stream');

      const reader = response.body.getReader();
      const decoder = new TextDecoder('utf-8');

      let buffer = '';

      const knownTypes = new Set([
         'issue', 'issues', 'progress', 'agent_progress',
         'trace', 'phase', 'stats', 'finding_added',
         'finding_updated', 'finding_removed', 'call_log',
      ]);

      while (true) {
         const { done, value } = await reader.read();
         if (done) break;

         buffer += decoder.decode(value, { stream: true });
         const lines = buffer.split('\n');
         buffer = lines.pop() || '';

         let currentEvent = 'message';
         let lastId = '';

         for (const line of lines) {
            const trimmedLine = line.trim();
            if (!trimmedLine) {
               currentEvent = 'message';
               continue;
            }

            if (trimmedLine.startsWith('id:')) {
               lastId = trimmedLine.slice(3).trim();
               try {
                  localStorage.setItem(`auditLastEvent:${taskId}`, lastId);
               } catch { /* ignore */ }
               continue;
            }

            if (trimmedLine.startsWith('event:')) {
               currentEvent = trimmedLine.slice(6).trim();
               continue;
            }

            if (trimmedLine.startsWith('data:')) {
               const dataStr = trimmedLine.slice(5).trim();
               try {
                  const parsed = JSON.parse(dataStr);

                  if (currentEvent === 'complete' || parsed.complete === true) {
                     onComplete();
                     return true; // 正常结束，外层不重试
                  }

                  if (knownTypes.has(currentEvent)) {
                     onMessage(currentEvent as SseEventType, parsed);
                  } else if (currentEvent === 'message' && parsed.event) {
                     const innerType = parsed.event as string;
                     if (knownTypes.has(innerType)) {
                        onMessage(innerType as SseEventType, parsed.data ?? parsed);
                     }
                  }
               } catch {
                  console.warn('[SSE] 数据解析失败:', dataStr);
               }
               currentEvent = 'message';
            }
         }
      }
      // read() done: true 但未收到 complete 事件 — 视为正常结束
      onComplete();
      return true;
   };

   // ── 重试循环（指数退避，最多 5 次） ──────────────────────
   let retries = 0;
   while (retries <= SSE_MAX_RETRIES) {
      try {
         const normalEnd = await doConnect();
         if (normalEnd) return; // 正常结束，停止重试
      } catch (error) {
         retries++;
         if (retries > SSE_MAX_RETRIES) {
            onError(error as Error);
            return;
         }
         const delay = Math.min(
            SSE_BASE_DELAY_MS * (2 ** (retries - 1)),
            30_000,
         );
         console.warn(
            `[SSE] 连接断开，${delay}ms 后重连 (${retries}/${SSE_MAX_RETRIES})…`,
         );
         await new Promise((r) => setTimeout(r, delay));
      }
   }
};

export const auditDetailOptions = {
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
