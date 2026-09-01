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
  PhaseEvent, StatsEvent,
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
   /** 部分失败阶段名（如 evidence_verify），非空表示结果经过降级、未经完整核验 */
   const [failedStages, setFailedStages] = useState<string[]>([]);
   const pollFailRef = useRef(0);
   const updateFinalElapsed = useCallback(() => {
      if (auditStartedAt <= 0) return;
      const finalSeconds = Math.max(0, Math.floor((Date.now() - auditStartedAt) / 1000));
      setElapsedSeconds((prev) => Math.max(prev, finalSeconds));
   }, [auditStartedAt]);

   /**
    * 接管一个已确认存在的任务，进入流式/轮询阶段。
    * 既用于「建任务成功」，也用于「建任务失败但服务端对账发现任务已建」的兜底恢复。
    */
   const acceptTask = useCallback(
      (nextTaskId: string) => {
         const startedAt = Date.now();
         if (storageKey) {
            try {
               localStorage.setItem(storageKey, JSON.stringify({ taskId: nextTaskId, startedAt }));
            } catch (storageError) {
               console.error('[AuditTask] taskId 持久化失败:', storageError);
            }
         }
         setAuditStartedAt((prev) => prev || startedAt);
         setShouldConnectStream(true);
         setHasStartedAudit(true);
         setTaskId(nextTaskId);
         setHydrated(true);
         setProgress(0);
         setIssues([]);
         setIsComplete(false);
         setError(null);
         setCurrentStage('任务已创建，等待流式数据...');
      },
      [storageKey]
   );

   /** 创建任务最终失败（且服务端对账无任务）时的收尾 */
   const failTask = useCallback((err: Error) => {
      setError(err.message || '任务创建失败');
      setCurrentStage('任务创建失败');
      setHasStartedAudit(false);
   }, []);

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
         setFailedStages([]);
         setCurrentStage('正在创建审核任务...');
      },

      onSuccess: (data) => {
         if (!data?.taskId) {
            console.error('API 异常：后端未返回 taskId', data);
            setError('任务创建响应异常');
            return;
         }
         acceptTask(data.taskId);
      },
      
      onError: (err: Error) => {
         console.error('[AuditTask] 任务创建失败:', err);
         // 兜底对账：后端「创建任务」是异步的（秒回 taskId，审核后台跑），
         // 网络抖动/超时可能把「已建好」误判成「失败」。失败后先按标书向服务端对账一次，
         // 若确有任务则直接接管（acceptTask），否则才真正报失败。
         if (typeof bidId === 'number' && !Number.isNaN(bidId)) {
            getAuditStatusByBid(bidId)
               .then((status) => {
                  if (status?.taskId) {
                     acceptTask(status.taskId);
                  } else {
                     failTask(err);
                  }
               })
               .catch(() => failTask(err));
            return;
         }
         failTask(err);
      },
   });

   // 首次进入详情页且无 localStorage taskId 时，由服务端按标书(bid)裁决「当前任务」：
   // 能直接加载已有的完成结果 / 进行中任务，而不是只丢一个「开始审核」按钮。
   // 有 taskId（localStorage 命中）时不覆盖，交给下方 hydrate 处理。
   useEffect(() => {
      if (typeof bidId !== 'number' || Number.isNaN(bidId)) return;
      if (taskId || isStarting || lastStartAt > 0) return;
      let cancelled = false;
      (async () => {
         try {
            const status = await getAuditStatusByBid(bidId);
            if (cancelled || !status?.taskId) return;
            if (storageKey) {
               try {
                  localStorage.setItem(
                     storageKey,
                     JSON.stringify({ taskId: status.taskId, startedAt: 0 })
                  );
               } catch (e) {
                  console.error('[AuditTask] 服务端 taskId 持久化失败:', e);
               }
            }
            setTaskId(status.taskId);
         } catch (e) {
            console.warn('[AuditTask] 按标书裁决当前任务失败（该标书尚未发起审核）:', e);
         }
      })();
      return () => {
         cancelled = true;
      };
   }, [bidId, taskId, storageKey, isStarting, lastStartAt]);

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
         if (!taskId) {
            setHydrated(true);
            return;
         }
         if (shouldConnectStream) {
            setHydrated(true);
            return;
         }
         try {
            const status = await getAuditStatus(taskId);
            if (cancelled) return;
            const completed = status.status === 'completed';
            setIsComplete(completed);
            if (completed) {
               setFailedStages(status.failedStages || []);
               const result = await getAuditResult(taskId, { page: 1, size: 200 });
               if (cancelled) return;
               setIssues((result.issues || []).map(withAnchorFallback));
               updateFinalElapsed();
               setProgress(100);
               setCurrentStage('审核完成');
               setShouldConnectStream(false);
               setHasStartedAudit(false);
            } else if (status.status === 'failed') {
               if (storageKey) {
                  localStorage.removeItem(storageKey);
               }
               setTaskId(null);
               setIssues([]);
               setProgress(0);
               setCurrentStage('审核失败');
               setIsComplete(false);
               setShouldConnectStream(false);
               setHasStartedAudit(false);
               setError('审核任务执行失败，请点击重新审核');
            } else {
               setAuditStartedAt((prev) => prev || Date.now());
               setProgress(status.progress || 0);
               setCurrentStage(status.stage || '审核进行中...');
               setIsComplete(false);
               setShouldConnectStream(true);
               setHasStartedAudit(true);
            }
            if (status.status !== 'failed') {
               setError(null);
            }
         } catch {
            // 清除 stale 数据，避免死循环
            if (storageKey) {
               try { localStorage.removeItem(storageKey); } catch { /* ignore */ }
            }
            setTaskId(null);
            setShouldConnectStream(false);
            setHasStartedAudit(false);
            setIsComplete(false);
            setError(null);
         } finally {
            if (!cancelled) setHydrated(true);
         }
      };

      setHydrated(false);
      hydrate();

      return () => {
         cancelled = true;
      };
   }, [taskId, storageKey, shouldConnectStream, isStarting, lastStartAt, updateFinalElapsed]);

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
                     setFailedStages(status.failedStages || []);
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

            setProgress(status.progress || 0);
            if (status.stage) setCurrentStage(status.stage);

            if (status.status === 'completed') {
               setFailedStages(status.failedStages || []);
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
            // 进行中(PROCESSING)不再用 getResult 的 issues 数量去推断“已完成”：
            // 长审核期间一旦有一段增量 finding，就会与 hydrate() 的 status=processing
            // 判定互相翻转 setIsComplete/shouldConnectStream，导致 SSE 反复重连
            // （详情页闪烁/卡死）。完成态统一由上方 status === 'completed' 分支与
            // SSE 的 complete 事件收敛；此处仅保留 failed 的收尾。

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
      failedStages,
   };
};
