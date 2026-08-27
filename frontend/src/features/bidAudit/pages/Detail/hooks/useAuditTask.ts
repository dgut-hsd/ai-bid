import { useState, useEffect, useCallback, useRef } from 'react';
import { useMutation } from '@tanstack/react-query';
import {
   createTask,
   connectStream,
   getAuditResult,
   getAuditStatus,
   getAuditStatusByBid,
} from '../api/auditDetail';
import type {
  AuditIssue, AgentProgress, TraceEvent,
  PhaseEvent, StatsEvent, AuditStatus,
  FindingAddedEvent, FindingUpdatedEvent, FindingRemovedEvent,
} from '@/types/audit';
import { ensureAuditIssue, mapFindingAddedEvent } from '../../../utils/mapFinding';

/** 连续轮询失败上限，超过则停止并清 stale 数据 */
const MAX_POLL_FAILURES = 5;

/**
 * 按 riskId upsert 去重：已存在则整条替换（保留最新），不存在则追加。
 * 后端 Java 会双发 finding_added + issue，两路都要走这里避免流式阶段出现重复卡。
 */
function upsertIssues(prev: AuditIssue[], mapped: AuditIssue): AuditIssue[] {
  const exists = prev.some((i) => i.riskId === mapped.riskId);
  if (exists) {
    return prev.map((i) => (i.riskId === mapped.riskId ? { ...mapped } : i));
  }
  return [...prev, mapped];
}

/**
 * 后端 GET /result 返回的 IssueVO 漏设 anchorQuote（Java toIssueVO 从未 setAnchorQuote），
 * 导致审核完成后点击高亮退化成整面。这里兜底：anchorQuote 缺失时回退到 sourceQuote / description。
 */
function withAnchorFallback(i: AuditIssue): AuditIssue {
  return {
    ...i,
    anchorQuote: i.anchorQuote || i.sourceQuote || i.description,
  };
}

type StoredAuditTaskState = {
   taskId?: string;
   startedAt?: number;
};

export const useAuditTask = (bidId?: number) => {
   const [progress, setProgress] = useState(0);
   const [currentStage, setCurrentStage] = useState('准备开始...');
   const [issues, setIssues] = useState<AuditIssue[]>([]);
   const [isComplete, setIsComplete] = useState(false);
   const [error, setError] = useState<string | null>(null);
   const storageKey =
      typeof bidId === 'number' && !Number.isNaN(bidId)
         ? `auditTask:${bidId}`
         : null;
   const [taskId, setTaskId] = useState<string | null>(() => {
      if (!storageKey) return null;
      try {
         const raw = localStorage.getItem(storageKey);
         if (!raw) return null;
         const parsed = JSON.parse(raw) as StoredAuditTaskState;
         return parsed.taskId ?? null;
      } catch {
         return null;
      }
   });
   const [hydrated, setHydrated] = useState(false);
   const [shouldConnectStream, setShouldConnectStream] = useState(false);
   const [hasStartedAudit, setHasStartedAudit] = useState(false);
   const [lastStartAt, setLastStartAt] = useState(0);
   const [auditStartedAt, setAuditStartedAt] = useState<number>(() => {
      if (!storageKey) return 0;
      try {
         const raw = localStorage.getItem(storageKey);
         if (!raw) return 0;
         const parsed = JSON.parse(raw) as StoredAuditTaskState;
         const startedAt = Number(parsed.startedAt || 0);
         return Number.isFinite(startedAt) && startedAt > 0 ? startedAt : 0;
      } catch {
         return 0;
      }
   });
   const [elapsedSeconds, setElapsedSeconds] = useState(0);
   const [agentProgresses, setAgentProgresses] = useState<Map<string, AgentProgress>>(new Map());
   const [liveFeedEvents, setLiveFeedEvents] = useState<TraceEvent[]>([]);
   const [phaseEvent, setPhaseEvent] = useState<PhaseEvent | null>(null);
   const [phaseHistory, setPhaseHistory] = useState<PhaseEvent[]>([]);
   const [statsEvent, setStatsEvent] = useState<StatsEvent | null>(null);
   const [liveFindings, setLiveFindings] = useState<FindingAddedEvent[]>([]);
   const pollFailRef = useRef(0);
   const updateFinalElapsed = useCallback(() => {
      if (auditStartedAt <= 0) return;
      const finalSeconds = Math.max(0, Math.floor((Date.now() - auditStartedAt) / 1000));
      setElapsedSeconds((prev) => Math.max(prev, finalSeconds));
   }, [auditStartedAt]);

   const { mutate: startAudit, isPending: isStarting } = useMutation({
      mutationFn: (payload: { bidId: number; webSearchEnabled?: boolean; forceRefresh?: boolean }) =>
         createTask({
            bidId: payload.bidId,
            forceRefresh: !!payload.forceRefresh,
         }),
      onMutate: () => {
         const now = Date.now();
         setHasStartedAudit(true);
         setLastStartAt(now);
         setAuditStartedAt(now);
         setElapsedSeconds(0);
         setIsComplete(false);
         setIssues([]);
         setProgress(0);
         setError(null);
         setAgentProgresses(new Map());
         setLiveFeedEvents([]);
         setPhaseEvent(null);
         setPhaseHistory([]);
         setStatsEvent(null);
         setLiveFindings([]);
         setCurrentStage('正在创建审核任务...');
      },

      onSuccess: (data) => {
         if (!data?.taskId) {
            console.error('API 异常：后端未返回 taskId', data);
            setError('任务创建响应异常');
            return;
         }
         const startedAt = Date.now();
         if (storageKey) {
            try {
               localStorage.setItem(storageKey, JSON.stringify({ taskId: data.taskId, startedAt }));
            } catch (storageError) {
               console.error('[AuditTask] taskId 持久化失败:', storageError);
            }
         }
         setAuditStartedAt((prev) => prev || startedAt);
         setShouldConnectStream(true);
         setTaskId(data.taskId);
         setHydrated(true);
         setProgress(0);
         setIssues([]);
         setIsComplete(false);
         setError(null);
         setCurrentStage('任务已创建，等待流式数据...');
      },
      
      onError: (err: Error) => {
         console.error('[AuditTask] 任务创建失败:', err);
         setError(err.message || '任务创建失败');
         setCurrentStage('任务创建失败');
         setHasStartedAudit(false);
      },
   });

   useEffect(() => {
      let cancelled = false;

      const hydrate = async () => {
         const withinStartWindow =
            lastStartAt > 0 && Date.now() - lastStartAt < 30_000;
         if (withinStartWindow) {
            setHydrated(true);
            return;
         }
         if (isStarting) {
            setHydrated(true);
            return;
         }
         if (shouldConnectStream) {
            setHydrated(true);
            return;
         }

         try {
            // P1: 服务端裁决当前任务（优先 bidId），localStorage 仅作缓存提示。
            let status: AuditStatus | null = null;
            if (typeof bidId === 'number' && !Number.isNaN(bidId)) {
               status = await getAuditStatusByBid(bidId);
               if (cancelled) return;
               if (status?.taskId) {
                  setTaskId(status.taskId);
                  if (storageKey) {
                     try {
                        localStorage.setItem(storageKey, JSON.stringify({
                           taskId: status.taskId,
                           startedAt: Date.now(),
                        }));
                     } catch { /* ignore */ }
                  }
               } else {
                  // 该文档从未发起审核 → 准备审核
                  setTaskId(null);
                  setIssues([]);
                  setProgress(0);
                  setCurrentStage('准备开始审核...');
                  setIsComplete(false);
                  setShouldConnectStream(false);
                  setHasStartedAudit(false);
                  setError(null);
                  setHydrated(true);
                  return;
               }
            } else if (taskId) {
               status = await getAuditStatus(taskId);
               if (cancelled) return;
            } else {
               setHydrated(true);
               return;
            }

            const currentTaskId = status?.taskId ?? taskId;
            const completed = status?.status === 'completed';
            setIsComplete(completed);
            if (completed && currentTaskId) {
               const result = await getAuditResult(currentTaskId, { page: 1, size: 200 });
               if (cancelled) return;
               setIssues((result.issues || []).map(withAnchorFallback));
               updateFinalElapsed();
               setProgress(100);
               setCurrentStage('审核完成');
               setShouldConnectStream(false);
               setHasStartedAudit(false);
            } else if (status?.status === 'failed') {
               // 失败也保留 taskId 供排查，不再清 localStorage 指针
               setIssues([]);
               setProgress(0);
               setCurrentStage('审核失败');
               setIsComplete(false);
               setShouldConnectStream(false);
               setHasStartedAudit(false);
               setError('审核任务执行失败，请点击重新审核');
            } else if (currentTaskId) {
               setAuditStartedAt((prev) => prev || Date.now());
               setProgress(status?.progress || 0);
               setCurrentStage(status?.stage || '审核进行中...');
               setIsComplete(false);
               setShouldConnectStream(true);
               setHasStartedAudit(true);
               // P2: 拉取已落库的增量 findings，刷新/重连后立即看到进行中的结果
               getAuditResult(currentTaskId, { page: 1, size: 200 })
                  .then((result) => {
                     if (!cancelled) setIssues((result.issues || []).map(withAnchorFallback));
                  })
                  .catch(() => { /* 忽略：SSE 仍会兜底 */ });
            }
            if (status?.status !== 'failed') {
               setError(null);
            }
         } catch {
            // P1: 网络/5xx 不再清除 localStorage 指针、不销毁 taskId，保留可恢复性；
            // 后续 3s 轮询(syncStatus)会继续重试；真正 401/404 由下次按 bid 裁决兜底。
            setShouldConnectStream(false);
            setHasStartedAudit(false);
         } finally {
            if (!cancelled) setHydrated(true);
         }
      };

      setHydrated(false);
      hydrate();

      return () => {
         cancelled = true;
      };
   }, [taskId, storageKey, shouldConnectStream, isStarting, lastStartAt, updateFinalElapsed, bidId]);

   useEffect(() => {
      if (!taskId || isComplete || !hydrated || !shouldConnectStream) return;

      let isMounted = true;
      const controller = new AbortController(); // 卸载时 abort SSE，避免后台重连泄漏

      const lastEventId = (
         (typeof window !== 'undefined'
            && window.localStorage.getItem(`auditLastEvent:${taskId}`))
         || ''
      );
      connectStream(
         taskId,
         lastEventId,
         (type, payload) => {
            if (!isMounted) return;

            // 根据后端协议：event: issues / event: issue
            if (type === 'issues' || type === 'issue') {
               const items = Array.isArray(payload) ? payload : [payload];
               const mapped = items.map((item) =>
                  ensureAuditIssue(item)
               );
               // 去重：Java 双发 finding_added+issue，避免同一 risk 重复出现
               setIssues((prev) => mapped.reduce((acc, m) => upsertIssues(acc, m), prev));
            }
            // 根据后端协议：event: progress
            else if (type === 'progress') {
               const progressPayload = payload as {
                  progress?: number;
                  stage?: string;
               };
               setProgress(progressPayload.progress || 0);
               if (progressPayload.stage) setCurrentStage(progressPayload.stage);
            }
            // SSE §17.1: Agent 进度
            else if (type === 'agent_progress') {
               const ap = payload as AgentProgress;
               setAgentProgresses(prev => {
                  const next = new Map(prev);
                  next.set(ap.agent_id, ap);
                  return next;
               });
               setCurrentStage('Multi-Agent 并行审查中...');
            }
            // SSE §17.1: 实时动态
            else if (type === 'trace') {
               setLiveFeedEvents(prev => [...prev.slice(-100), payload as TraceEvent]);
            }
            // SSE §17.1: 管线阶段切换
            else if (type === 'phase') {
               const pe = payload as PhaseEvent;
               setPhaseEvent(pe);
               setPhaseHistory(prev => [...prev, pe]);
               setCurrentStage(pe.message);
            }
            // SSE §17.1: 阶段统计快照
            else if (type === 'stats') {
               setStatsEvent(payload as StatsEvent);
            }
            // SSE §17.1: 实时发现（Java 双发为 issue，此处合并进 issues 并去重，打通流式结果）
            else if (type === 'finding_added') {
               const fe = payload as FindingAddedEvent;
               setLiveFindings(prev => [...prev, fe]);
               const mapped = mapFindingAddedEvent(fe);
               setIssues(prev => upsertIssues(prev, mapped));
            }
            // SSE §17.1: finding 被更新
            else if (type === 'finding_updated') {
               const fe = payload as FindingUpdatedEvent;
               setLiveFindings(prev => prev.map(f =>
                  f.risk_id === fe.risk_id
                     ? { ...f, ...fe.changes.reduce((acc, c) => ({ ...acc, [c.field]: c.new_value }), {}) as Partial<FindingAddedEvent> }
                     : f
               ));
               setIssues(prev => prev.map(i => {
                 if (i.riskId !== fe.risk_id) return i;
                 const fieldMap: Record<string, string> = {
                   'severity': 'severity',
                   'confidence': 'confidence',
                   'reason': 'description',
                 };
                 const patch: Partial<AuditIssue> = {};
                 fe.changes.forEach(c => {
                   const mappedKey = fieldMap[c.field];
                   if (mappedKey) {
                     (patch as Record<string, unknown>)[mappedKey] = c.new_value;
                   }
                 });
                 return { ...i, ...patch };
               }));
            }
            // SSE §17.1: finding 被移除
            else if (type === 'finding_removed') {
               const fr = payload as FindingRemovedEvent;
               setLiveFindings(prev => prev.filter(f => f.risk_id !== fr.risk_id));
               setIssues(prev => prev.filter(i => i.riskId !== fr.risk_id));
            }
         },
        // onComplete —— SSE 流闭合（可能由 Java emit complete、可能由网络断开）。
        // 收到流结束立即主动查 status / result 并切完成态，避免 Rust 已完成但前端一直卡在"等待结果确认…"。
        // 下方仍保留每 3 秒 syncStatus 轮询作为第二重兜底（同步进度 + 检测完成/失败），二者并存不冲突。
         () => {
            if (!isMounted) return;
            (async () => {
               try {
                  const status = await getAuditStatus(taskId);
                  if (!isMounted) return;
                  const completed = status.status === 'completed';
                  if (completed) {
                     const result = await getAuditResult(taskId, { page: 1, size: 200 });
                     if (!isMounted) return;
                     setIssues((result.issues || []).map(withAnchorFallback));
                     updateFinalElapsed();
                     setProgress(100);
                     setIsComplete(true);
                     setCurrentStage('审核完成');
                     setShouldConnectStream(false);
                     setHasStartedAudit(false);
                     setError(null);
                     return;
                  }
                  if (status.status === 'failed') {
                     setError('审核任务执行失败，请点击重新审核');
                     setShouldConnectStream(false);
                     setHasStartedAudit(false);
                     if (storageKey) {
                        try { localStorage.removeItem(storageKey); } catch { /* ignore */ }
                     }
                     setTaskId(null);
                     return;
                  }
                  // 未完成就关流（理论上不会发生，落到这里保留原行为）
                  setCurrentStage('审核流已结束，等待结果确认...');
               } catch {
                  setCurrentStage('审核流已结束，等待结果确认...');
               }
            })();
         },
         // onError
         (err) => {
            if (!isMounted) return;
            console.error('[AuditTask] SSE 异常:', err);
            setError('实时数据连接中断，请刷新页面');
         },
         controller.signal
      );

      return () => {
         isMounted = false;
         controller.abort(); // 组件卸载 → 立即终止 SSE 连接与重连循环
      };
   }, [taskId, isComplete, hydrated, shouldConnectStream]);

   useEffect(() => {
      if (!taskId || isComplete || !shouldConnectStream) return;

      let stopped = false;

      const syncStatus = async () => {
         try {
            const status = await getAuditStatus(taskId);
            if (stopped) return;
            // 防御：后端异常/响应缺失 data 时 getAuditStatus 返回 undefined，
            // 仅跳过本轮，不计入「连续轮询失败」，避免错误停止整个轮询。
            if (!status) return;

            setProgress(status.progress || 0);
            if (status.stage) setCurrentStage(status.stage);

            if (status.status === 'completed') {
               const result = await getAuditResult(taskId, { page: 1, size: 200 });
               if (stopped) return;
               setIssues((result.issues || []).map(withAnchorFallback));
               updateFinalElapsed();
               setIsComplete(true);
               setProgress(100);
               setCurrentStage('审核完成');
               setShouldConnectStream(false);
               setHasStartedAudit(false);
               setError(null);
               return;
            }
            setAuditStartedAt((prev) => prev || Date.now());

            const fallbackResult = await getAuditResult(taskId, {
               page: 1,
               size: 200,
            });
            if (stopped) return;
            // P2 后 getResult 在「进行中」也会返回增量 issues，不能因「有 finding」就判完成；
            // 完成必须以 auditResult 离开 pending（revise/pass）为准。
            const done = !!fallbackResult.auditResult
               && fallbackResult.auditResult !== 'pending';
            if (done) {
               setIssues((fallbackResult.issues || []).map(withAnchorFallback));
               updateFinalElapsed();
               setIsComplete(true);
               setProgress(100);
               setCurrentStage('审核完成');
               setShouldConnectStream(false);
               setHasStartedAudit(false);
               setError(null);
               return;
            }
            // 进行中：仅用增量结果刷新展示，绝不切完成态、不关流。
            if ((fallbackResult.issues?.length || 0) > 0) {
               setIssues((fallbackResult.issues || []).map(withAnchorFallback));
            }

            if (status.status === 'failed') {
               setError('审核任务执行失败，请点击重新审核');
               setShouldConnectStream(false);
               setHasStartedAudit(false);
               if (storageKey) {
                  localStorage.removeItem(storageKey);
               }
               setTaskId(null);
            }
         } catch (e) {
            if (stopped) return;
            pollFailRef.current += 1;
            console.error(`[AuditTask] 状态轮询失败 (${pollFailRef.current}/${MAX_POLL_FAILURES}):`, e);
            if (pollFailRef.current >= MAX_POLL_FAILURES) {
               console.error('[AuditTask] 连续轮询失败达上限，停止轮询');
               setError('审核任务连接失败，请刷新页面后重试');
               setShouldConnectStream(false);
               setHasStartedAudit(false);
               if (storageKey) {
                  try { localStorage.removeItem(storageKey); } catch { /* ignore */ }
               }
               setTaskId(null);
            }
         }
      };

      pollFailRef.current = 0;
      syncStatus();
      const timer = window.setInterval(syncStatus, 3000);

      return () => {
         stopped = true;
         window.clearInterval(timer);
      };
   }, [taskId, isComplete, shouldConnectStream, updateFinalElapsed]);

   // 仅在审核流程活跃时计时；不再因为存在未完成 task 而持续计时（避免把对话时间算入）
   const isAuditingNow = hasStartedAudit || isStarting || shouldConnectStream;

   useEffect(() => {
      if (!isAuditingNow) return;
      if (!auditStartedAt) {
         const now = Date.now();
         setAuditStartedAt(now);
         setElapsedSeconds(0);
         return;
      }
      const syncElapsed = () => {
         setElapsedSeconds(Math.max(0, Math.floor((Date.now() - auditStartedAt) / 1000)));
      };
      syncElapsed();
      const timer = window.setInterval(syncElapsed, 1000);
      return () => {
         window.clearInterval(timer);
      };
   }, [isAuditingNow, auditStartedAt]);

   return {
      taskId,
      startAudit,
      isStarting,
      isAuditing: isAuditingNow,
      progress,
      currentStage,
      elapsedSeconds,
      issues,
      isComplete,
      error,
      summary: {
         totalIssues: issues.length,
         high: issues.filter((i) => i?.severity === 'high').length,
         medium: issues.filter((i) => i?.severity === 'medium').length,
         low: issues.filter((i) => i?.severity === 'low').length,
         info: issues.filter((i) => i?.severity === 'info').length,
      },
      agentProgresses,
      liveFeedEvents,
      phaseEvent,
      phaseHistory,
      statsEvent,
      liveFindings,
   };
};
