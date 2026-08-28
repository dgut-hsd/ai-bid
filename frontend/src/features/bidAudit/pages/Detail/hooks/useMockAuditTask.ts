import { useState, useCallback, useRef } from 'react';
import type {
  AuditIssue, AgentProgress, TraceEvent,
  PhaseEvent, StatsEvent, FindingAddedEvent,
} from '@/types/audit';
import { MOCK_FINDINGS } from './mockFindingsData';

// ─── Mock Agent 定义 ───

const MOCK_AGENTS: { id: string; label: string; total: number; findingCount: number }[] = [
  { id: 'factcheck', label: '事实核查Agent', total: 85, findingCount: 5 },
  { id: 'procedure', label: '程序合规Agent', total: 62, findingCount: 2 },
  { id: 'ruleengine', label: '规则引擎Agent', total: 104, findingCount: 12 },
  { id: 'semanticrisk', label: '语义风险Agent', total: 73, findingCount: 0 },
  { id: 'demand', label: '采购需求Agent', total: 91, findingCount: 27 },
  { id: 'blind_spot', label: '盲点扫描Agent', total: 58, findingCount: 6 },
];

const MOCK_STAGES = [
  '正在解析文档结构…',
  '正在提取条款与段落…',
  'Multi-Agent 并行审查中…',
  '事实核查Agent 审查中…',
  '规则引擎Agent 扫描中…',
  '程序合规Agent 检查中…',
  '采购需求Agent 分析中…',
  '盲点扫描Agent 探测中…',
  '正在执行法条验证…',
  '正在辩论裁决…',
  '正在汇总分析结果…',
];

/**
 * Mock 审核任务 — 使用真实后端 findings 数据模拟完整审核流程。
 *
 * 与 useAuditTask 返回相同接口，可直接替换用于 UI 开发。
 */
export const useMockAuditTask = () => {
  const [taskId, setTaskId] = useState<string | null>(null);
  const [progress, setProgress] = useState(0);
  const [issues, setIssues] = useState<AuditIssue[]>([]);
  const [isComplete, setIsComplete] = useState(false);
  const [currentStage, setCurrentStage] = useState('准备开始...');
  const [isStarting, setIsStarting] = useState(false);
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const [agentProgresses, setAgentProgresses] = useState<Map<string, AgentProgress>>(new Map());
  const [liveFeedEvents, setLiveFeedEvents] = useState<TraceEvent[]>([]);
  const [phaseEvent, setPhaseEvent] = useState<PhaseEvent | null>(null);
  const [phaseHistory, setPhaseHistory] = useState<PhaseEvent[]>([]);
  const [statsEvent, setStatsEvent] = useState<StatsEvent | null>(null);
  const [liveFindings, setLiveFindings] = useState<FindingAddedEvent[]>([]);

  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const elapsedTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const delayRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const cleanup = useCallback(() => {
    if (timerRef.current) {
      clearInterval(timerRef.current);
      timerRef.current = null;
    }
    if (elapsedTimerRef.current) {
      clearInterval(elapsedTimerRef.current);
      elapsedTimerRef.current = null;
    }
    if (delayRef.current) {
      clearTimeout(delayRef.current);
      delayRef.current = null;
    }
  }, []);

  const startAudit = useCallback((payload: { bidId: number; webSearchEnabled?: boolean; forceRefresh?: boolean }) => {
    const bidId = payload.bidId;
    cleanup();

    setIsStarting(true);
    setProgress(0);
    setIssues([]);
    setIsComplete(false);
    setCurrentStage('正在创建审核任务...');
    setElapsedSeconds(0);
    setAgentProgresses(new Map());
    setLiveFeedEvents([]);
    setPhaseEvent(null);
    setPhaseHistory([]);
    setStatsEvent(null);
    setLiveFindings([]);

    delayRef.current = setTimeout(() => {
      const mockTaskId = `MOCK-${bidId}-${Date.now()}`;
      setTaskId(mockTaskId);
      setIsStarting(false);
      let currentProgress = 0;
      let stageIdx = 0;
      let eventSeq = 0;

      // 耗时计时器
      let elapsed = 0;
      elapsedTimerRef.current = setInterval(() => {
        elapsed += 1;
        setElapsedSeconds(elapsed);
      }, 1000);

      timerRef.current = setInterval(() => {
        currentProgress += Math.random() * 4 + 1;
        if (currentProgress > 100) currentProgress = 100;
        const pct = Math.round(currentProgress);
        setProgress(pct);

        // 阶段切换
        const newStageIdx = Math.min(
          Math.floor((pct / 100) * MOCK_STAGES.length),
          MOCK_STAGES.length - 1
        );
        if (newStageIdx !== stageIdx) {
          stageIdx = newStageIdx;
          setCurrentStage(MOCK_STAGES[stageIdx]);
          // 模拟管线阶段事件（映射到 7 个 Rust 管线阶段）
          const phaseNames = ['route', 'execute', 'execute', 'execute', 'merge', 'legal_verify', 'blind_spot', 'blind_spot', 'debate', 'triage', 'triage'];
          const phaseIdx = Math.min(newStageIdx, phaseNames.length - 1);
          const p = phaseNames[phaseIdx];
          setPhaseEvent({ phase: p, phase_index: phaseNames.indexOf(p) + 1, total_phases: 7, message: MOCK_STAGES[stageIdx] });
          setPhaseHistory(prev => [...prev, { phase: p, phase_index: phaseNames.indexOf(p) + 1, total_phases: 7, message: MOCK_STAGES[stageIdx] }]);
        }

        // Agent 进度模拟 (pct 15-85)
        if (pct >= 15 && pct < 90) {
          setAgentProgresses((prev) => {
            const next = new Map(prev);
            for (const ag of MOCK_AGENTS) {
              const clauseDone = Math.min(
                ag.total,
                Math.floor(((pct - 15) / 75) * ag.total * (0.85 + Math.random() * 0.3))
              );
              const status: AgentProgress['status'] =
                clauseDone >= ag.total ? 'completed' :
                clauseDone > 0 ? 'running' : 'pending';
              const rawFindings =
                status === 'completed'
                  ? ag.findingCount
                  : Math.floor((clauseDone / ag.total) * ag.findingCount);
              next.set(ag.id, {
                agent_id: ag.id,
                agent_label: ag.label,
                clauses_done: clauseDone,
                clauses_total: ag.total,
                raw_findings: rawFindings,
                status,
              });
            }
            return next;
          });

          // 实时动态流事件 (偶尔产生)
          if (Math.random() < 0.45) {
            eventSeq += 1;
            const eventTypes: TraceEvent['event_type'][] = [
              'agent_thought', 'tool_call', 'tool_result', 'output_finding',
              'agent_bus_send', 'agent_bus_recv',
            ];
            const agents = MOCK_AGENTS.filter(a => a.findingCount > 0);
            const pickAgent = agents[Math.floor(Math.random() * agents.length)];
            const eventType = eventTypes[Math.floor(Math.random() * eventTypes.length)];
            setLiveFeedEvents((prev) => [
              ...prev.slice(-100),
              {
                event_type: eventType,
                agent_name: pickAgent.id,
                turn: Math.floor((pct - 15) / 20) + 1,
                summary:
                  eventType === 'output_finding'
                    ? `发现 ${pickAgent.label} 风险项，置信度 ${Math.round(60 + Math.random() * 35)}%`
                    : eventType === 'tool_call'
                    ? `调用搜索工具: 核查条款合规性`
                    : eventType === 'agent_thought'
                    ? `分析条款潜在风险…`
                    : eventType === 'agent_bus_send'
                    ? `通知 legal_verify Agent 验证法条`
                    : `收到 ${pickAgent.label} 的审查结果`,
                timestamp: new Date().toISOString(),
              },
            ]);
          }
        }

        // 逐渐揭示 findings (pct 30-100)
        if (pct >= 30) {
          const revealRatio = Math.min(1, (pct - 30) / 70);
          const revealCount = Math.floor(revealRatio * MOCK_FINDINGS.length);
          // 按严重程度排序：高风险先揭示
          const sorted = [...MOCK_FINDINGS].sort((a, b) => {
            const order = { high: 0, medium: 1, low: 2, info: 3 };
            return (order[a.severity] ?? 3) - (order[b.severity] ?? 3);
          });
          setIssues(sorted.slice(0, revealCount));
        }

        // 完成
        if (pct >= 100) {
          setCurrentStage('审核完成');
          setIsComplete(true);
          cleanup();

          // 最终 Agent 状态全部 completed
          setAgentProgresses((prev) => {
            const next = new Map(prev);
            for (const ag of MOCK_AGENTS) {
              next.set(ag.id, {
                agent_id: ag.id,
                agent_label: ag.label,
                clauses_done: ag.total,
                clauses_total: ag.total,
                raw_findings: ag.findingCount,
                status: 'completed',
              });
            }
            return next;
          });

          // 所有 findings 展示
          setIssues(
            [...MOCK_FINDINGS].sort((a, b) => {
              const order = { high: 0, medium: 1, low: 2, info: 3 };
              return (order[a.severity] ?? 3) - (order[b.severity] ?? 3);
            })
          );
        }
      }, 200);
    }, 800);
  }, [cleanup]);

  return {
    taskId,
    startAudit,
    isStarting,
    isAuditing: isStarting || (!!taskId && !isComplete),
    progress,
    currentStage,
    elapsedSeconds,
    issues,
    isComplete,
    error: null,
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
    failedStages: [],
  };
};
