import React, { useMemo, useState, useEffect } from 'react';
import { Progress, Tag, Typography, Tooltip, Button } from 'antd';
import {
  CaretRightOutlined,
  CheckCircleOutlined,
  LoadingOutlined,
  ClockCircleOutlined,
  SearchOutlined,
  BulbOutlined,
  FileSearchOutlined,
  WarningOutlined,
  PlayCircleOutlined,
  FolderOutlined,
  FolderOpenOutlined,
  InfoCircleOutlined,
  GlobalOutlined,
  LinkOutlined,
  FileTextOutlined,
  MessageOutlined,
} from '@ant-design/icons';
import type {
  TraceEvent,
  AgentProgress,
  PhaseEvent,
  StatsEvent,
  FindingAddedEvent,
  AuditIssue,
  SectionTreeNode,
  ClauseState,
} from '@/types/audit';
import { AGENT_LABELS, PHASE_LABELS, PHASE_ORDER } from '@/types/audit';
import { buildClauseMap, buildSectionTree, clauseStatus } from './buildSectionTree';
import type { ClauseStatusType } from './buildSectionTree';

const { Text } = Typography;

interface Props {
  traceEvents: TraceEvent[];
  liveFindings: FindingAddedEvent[];
  issues: AuditIssue[];
  phaseEvent: PhaseEvent | null;
  statsEvent: StatsEvent | null;
  agentProgresses: Map<string, AgentProgress>;
  isAuditing: boolean;
  isComplete: boolean;
  elapsedSeconds: number;
  onLocateIssuePage: (page: number, highlightText?: string, fallbackTokens?: string[]) => void;
}

// ─── 常量 ──────────────────────────────────────────────────

const DARK_GREEN = '#52c41a';

const EVENT_ICON: Record<string, React.ReactNode> = {
  turn_start: <PlayCircleOutlined style={{ color: DARK_GREEN }} />,
  agent_thought: <BulbOutlined style={{ color: DARK_GREEN }} />,
  tool_call: <SearchOutlined style={{ color: DARK_GREEN }} />,
  tool_result: <FileSearchOutlined style={{ color: DARK_GREEN }} />,
  output_finding: <WarningOutlined style={{ color: DARK_GREEN }} />,
  agent_bus_send: <SearchOutlined style={{ color: DARK_GREEN }} />,
  agent_bus_recv: <FileSearchOutlined style={{ color: DARK_GREEN }} />,
  call_log: <InfoCircleOutlined style={{ color: '#722ed1' }} />,
};

const EVENT_LABEL: Record<string, string> = {
  turn_start: '轮次',
  agent_thought: '推理',
  tool_call: '工具',
  tool_result: '结果',
  output_finding: '发现',
  agent_bus_send: '通知',
  agent_bus_recv: '收到',
  call_log: '统计',
};

const SEVERITY_TAG_COLOR: Record<string, string> = {
  high: 'red',
  medium: 'orange',
  low: 'blue',
  info: 'default',
};

const SEVERITY_LABEL: Record<string, string> = {
  high: '高风险',
  medium: '中风险',
  low: '低风险',
  info: '信息',
};

/** 条款状态 → 图标 */
const CLAUSE_STATUS_ICON: Record<ClauseStatusType, React.ReactNode> = {
  pending: <ClockCircleOutlined style={{ color: '#d9d9d9', fontSize: 12 }} />,
  reviewing: <LoadingOutlined style={{ color: '#1890ff', fontSize: 12 }} />,
  clean: <CheckCircleOutlined style={{ color: '#52c41a', fontSize: 12 }} />,
  risk_high: <WarningOutlined style={{ color: '#f5222d', fontSize: 12 }} />,
  risk_medium: <WarningOutlined style={{ color: '#fa8c16', fontSize: 12 }} />,
  risk_low: <InfoCircleOutlined style={{ color: '#1890ff', fontSize: 12 }} />,
};

// ─── Agent 迷你进度 ───────────────────────────────────────

const AgentMiniCards: React.FC<{ progresses: Map<string, AgentProgress> }> = ({ progresses }) => {
  const agents = Array.from(progresses.values());
  if (agents.length === 0) return null;

  return (
    <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, marginBottom: 6 }}>
      {agents.map((ap, idx) => {
        const agentKey = ap.agent_id || ap.agent_label || `agent-${idx}`;
        const label = AGENT_LABELS[ap.agent_id] || ap.agent_label || ap.agent_id;
        const pct = ap.clauses_total > 0 ? Math.round((ap.clauses_done / ap.clauses_total) * 100) : 0;
        const done = ap.status === 'completed';
        const running = ap.status === 'running';
        return (
          <Tooltip key={agentKey} title={`${label}: ${ap.clauses_done}/${ap.clauses_total} 条款${ap.raw_findings > 0 ? ` | ${ap.raw_findings} 疑似` : ''}`}>
            <Tag
              color={done ? 'green' : running ? 'processing' : 'default'}
              style={{ margin: 0, fontSize: 11, cursor: 'default' }}
            >
              {done ? <CheckCircleOutlined /> : running ? <LoadingOutlined /> : <ClockCircleOutlined />}
              {' '}{label} {pct}%
            </Tag>
          </Tooltip>
        );
      })}
    </div>
  );
};

// ─── 阶段进度条 ────────────────────────────────────────────

const PhaseBar: React.FC<{
  phaseEvent: PhaseEvent | null;
  statsEvent: StatsEvent | null;
  elapsedSeconds: number;
  isComplete: boolean;
}> = ({ phaseEvent, statsEvent, elapsedSeconds, isComplete }) => {
  const currentPhase = phaseEvent?.phase ?? '';
  const currentIdx = PHASE_ORDER.indexOf(currentPhase);
  const total = phaseEvent?.total_phases ?? 7;
  const pct = total > 0 ? Math.round(((currentIdx >= 0 ? currentIdx + 1 : 0) / total) * 100) : 0;

  const mins = Math.floor(elapsedSeconds / 60);
  const secs = elapsedSeconds % 60;
  const timeStr = `${mins}:${secs.toString().padStart(2, '0')}`;

  return (
    <div style={{ marginBottom: isComplete ? 2 : 8 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 4 }}>
        <span style={{ fontSize: 13, fontWeight: 500 }}>
          {isComplete && <CheckCircleOutlined style={{ color: DARK_GREEN, marginRight: 4 }} />}
          {currentPhase ? (
            <>{PHASE_LABELS[currentPhase] || currentPhase}</>
          ) : (
            '准备审查...'
          )}
          {phaseEvent && !isComplete && (
            <Text type="secondary" style={{ fontSize: 11, marginLeft: 8 }}>
              {currentIdx + 1}/{total} 阶段
            </Text>
          )}
        </span>
        {isComplete ? (
          <Text type="secondary" style={{ fontSize: 11 }}>耗时 {timeStr}</Text>
        ) : (
          <Text type="secondary" style={{ fontSize: 11 }}>{timeStr}</Text>
        )}
      </div>
      {!isComplete && (
        <Progress percent={pct} size="small" showInfo={false} strokeColor={DARK_GREEN} />
      )}
      {statsEvent && (statsEvent.high > 0 || statsEvent.medium > 0 || statsEvent.low > 0) && (
        <div style={{ marginTop: 4, display: 'flex', gap: 6, flexWrap: 'wrap' }}>
          {statsEvent.high > 0 && <Tag color="red" style={{ fontSize: 10, margin: 0 }}>高 {statsEvent.high}</Tag>}
          {statsEvent.medium > 0 && <Tag color="orange" style={{ fontSize: 10, margin: 0 }}>中 {statsEvent.medium}</Tag>}
          {statsEvent.low > 0 && <Tag color="blue" style={{ fontSize: 10, margin: 0 }}>低 {statsEvent.low}</Tag>}
          {statsEvent.info > 0 && <Text type="secondary" style={{ fontSize: 10 }}>信息 {statsEvent.info}</Text>}
        </div>
      )}
    </div>
  );
};

// ─── 风险卡 ────────────────────────────────────────────────

const RiskCard: React.FC<{ finding: FindingAddedEvent }> = ({ finding }) => {
  const severityColor = SEVERITY_TAG_COLOR[finding.severity] || 'default';
  const pct = Math.round((finding.confidence || 0) * 100);

  return (
    <div style={{
      background: '#fff',
      borderRadius: 6,
      border: '1px solid #f0f0f0',
      padding: '8px 10px',
      marginTop: 4,
      marginBottom: 4,
      fontSize: 12,
    }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 4 }}>
        <span style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
          <Tag color={severityColor} style={{ margin: 0, fontSize: 10, lineHeight: '16px' }}>
            {SEVERITY_LABEL[finding.severity] || finding.severity}
          </Tag>
          <Text strong style={{ fontSize: 12 }}>{finding.risk_type}</Text>
          <Text type="secondary" style={{ fontSize: 10 }}>{finding.risk_id}</Text>
        </span>
        <span style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
          <Text type="secondary" style={{ fontSize: 10 }}>
            {AGENT_LABELS[finding.agent] || finding.agent}
          </Text>
          {pct > 0 && (
            <Tooltip title={`置信度 ${pct}%`}>
              <Tag style={{ margin: 0, fontSize: 10, lineHeight: '16px' }}>{pct}%</Tag>
            </Tooltip>
          )}
        </span>
      </div>

      {finding.source_quote && (
        <div style={{
          background: '#fafafa',
          borderRadius: 4,
          padding: '4px 8px',
          marginBottom: 4,
          color: '#595959',
          fontSize: 11,
          maxHeight: 40,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}>
          <MessageOutlined style={{ marginRight: 4, color: '#bfbfbf' }} />
          {finding.source_quote}
        </div>
      )}

      {finding.legal_basis.length > 0 && (
        <div style={{ marginBottom: 2 }}>
          {finding.legal_basis.slice(0, 2).map((law, i) => (
            <Tag key={i} color="geekblue" style={{ fontSize: 10, margin: '0 4px 2px 0' }}>
              <FileTextOutlined style={{ marginRight: 2 }} />
              {law.length > 40 ? law.slice(0, 40) + '...' : law}
            </Tag>
          ))}
        </div>
      )}

      {finding.reason && (
        <div style={{ color: '#8c8c8c', fontSize: 11, marginTop: 2, lineHeight: '16px' }}>
          {finding.reason.length > 120 ? finding.reason.slice(0, 120) + '...' : finding.reason}
        </div>
      )}
    </div>
  );
};

// ─── 章节树节点 ────────────────────────────────────────────

const SectionTreeNodeView: React.FC<{
  node: SectionTreeNode;
  clauseMap: Map<string, ClauseState>;
  depth: number;
  expandedKeys: Set<string>;
  onToggle: (key: string) => void;
  onLocateIssuePage: (page: number) => void;
}> = ({ node, clauseMap, depth, expandedKeys, onToggle, onLocateIssuePage }) => {
  const isExpanded = expandedKeys.has(node.key);
  const hasRisks = node.riskCount > 0;
  const indent = depth * 16;

  return (
    <div>
      {/* Section header */}
      <div
        onClick={() => onToggle(node.key)}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          padding: '5px 6px',
          paddingLeft: 6 + indent,
          cursor: 'pointer',
          borderRadius: 4,
          background: hasRisks ? '#fffbe6' : undefined,
          transition: 'background 0.2s',
          fontSize: 13,
        }}
      >
        <CaretRightOutlined
          style={{
            fontSize: 10,
            color: '#bfbfbf',
            transform: isExpanded ? 'rotate(90deg)' : undefined,
            transition: 'transform 0.2s',
          }}
        />
        {isExpanded
          ? <FolderOpenOutlined style={{ color: '#faad14', fontSize: 14 }} />
          : <FolderOutlined style={{ color: hasRisks ? '#faad14' : '#bfbfbf', fontSize: 14 }} />
        }
        <span style={{
          fontWeight: hasRisks ? 600 : 400,
          flex: 1,
          minWidth: 0,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}>
          {node.title}
        </span>
        {hasRisks ? (
          <Tag
            color={node.maxSeverity === 'high' ? 'red' : node.maxSeverity === 'medium' ? 'orange' : 'blue'}
            style={{ margin: 0, fontSize: 10, lineHeight: '16px', flexShrink: 0 }}
          >
            {node.riskCount}
          </Tag>
        ) : node.clauseIds.length > 0 ? (
          <CheckCircleOutlined style={{ color: '#52c41a', fontSize: 12, flexShrink: 0 }} />
        ) : null}
      </div>

      {/* Expanded content */}
      {isExpanded && (
        <div>
          {node.children.map(child => (
            <SectionTreeNodeView
              key={child.key}
              node={child}
              clauseMap={clauseMap}
              depth={depth + 1}
              expandedKeys={expandedKeys}
              onToggle={onToggle}
              onLocateIssuePage={onLocateIssuePage}
            />
          ))}

          {node.clauseIds.map(cid => {
            const clause = clauseMap.get(cid);
            if (!clause) return null;
            const st = clauseStatus(clause);
            return (
              <div key={cid} style={{ paddingLeft: 26 + indent, marginBottom: 2 }}>
                {/* Clause row */}
                <div style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 6,
                  padding: '3px 0',
                  fontSize: 12,
                }}>
                  {CLAUSE_STATUS_ICON[st.status]}
                  <Text
                    type="secondary"
                    style={{
                      fontSize: 10,
                      fontFamily: 'monospace',
                      flexShrink: 0,
                      cursor: clause.pageNumber ? 'pointer' : 'default',
                      color: clause.pageNumber ? '#1890ff' : undefined,
                    }}
                    title={clause.pageNumber ? `跳转到第 ${clause.pageNumber} 页` : undefined}
                    onClick={(e) => {
                      e.stopPropagation();
                      if (clause.pageNumber) onLocateIssuePage(clause.pageNumber);
                    }}
                  >
                    {cid}
                  </Text>
                  {clause.pageNumber && (
                    <Text
                      type="secondary"
                      style={{ fontSize: 10, cursor: 'pointer', color: '#1890ff' }}
                      title={`跳转到第 ${clause.pageNumber} 页`}
                      onClick={(e) => {
                        e.stopPropagation();
                        if (clause.pageNumber != null) onLocateIssuePage(clause.pageNumber);
                      }}
                    >
                      p.{clause.pageNumber}
                    </Text>
                  )}
                  <span style={{ flex: 1 }} />
                  <Tag
                    style={{
                      margin: 0,
                      fontSize: 10,
                      lineHeight: '16px',
                      color: st.color,
                      borderColor: st.color,
                      background: '#fff',
                    }}
                  >
                    {st.label}
                  </Tag>
                  {clause.reviewedBy.length > 0 && (
                    <Text type="secondary" style={{ fontSize: 10 }}>
                      {clause.reviewedBy.slice(0, 2).map(a => AGENT_LABELS[a] || a).join(', ')}
                      {clause.reviewedBy.length > 2 && ` +${clause.reviewedBy.length - 2}`}
                    </Text>
                  )}
                </div>

                {/* Risk cards */}
                {clause.risks.filter(r => r.severity !== 'info').map(risk => (
                  <RiskCard key={risk.risk_id} finding={risk} />
                ))}
                {clause.risks.filter(r => r.severity === 'info').length > 0 && (
                  <Text type="secondary" style={{ fontSize: 10, marginLeft: 20 }}>
                    <InfoCircleOutlined style={{ marginRight: 2 }} />
                    {clause.risks.filter(r => r.severity === 'info').length} 条信息提示
                  </Text>
                )}
              </div>
            );
          })}

          {node.children.length === 0 && node.clauseIds.length === 0 && (
            <div style={{ paddingLeft: 26 + indent, fontSize: 11, color: '#bfbfbf' }}>
              （待审查）
            </div>
          )}
        </div>
      )}
    </div>
  );
};

// ─── 详细日志 ──────────────────────────────────────────────

/** 去掉 Rust 端 summary 里的 emoji */
function stripEmoji(s: string): string {
  return s.replace(/[\u{1F300}-\u{1F9FF}\u{2600}-\u{26FF}\u{2700}-\u{27BF}\u{1F000}-\u{1F02F}\u{1F0A0}-\u{1F0FF}\u{1F100}-\u{1F64F}\u{1F680}-\u{1F6FF}\u{1F900}-\u{1F9FF}\u{200D}\u{FE0F}]/gu, '').replace(/\s+/g, ' ').trim();
}

/** 单个 trace 事件行 — 可点击展开详情 */
const TraceEventRow: React.FC<{ ev: TraceEvent }> = ({ ev }) => {
  const [expanded, setExpanded] = useState(false);
  const p = ev.payload as Record<string, unknown> | undefined;
  const hasDetail = !!p && (
    ev.event_type === 'agent_thought' || ev.event_type === 'tool_call' ||
    ev.event_type === 'tool_result' || ev.event_type === 'output_finding' ||
    ev.event_type === 'call_log'
  );

  return (
    <div>
      <div
        onClick={() => hasDetail && setExpanded(v => !v)}
        style={{
          padding: '1px 0', color: '#8c8c8c', display: 'flex', gap: 6, fontSize: 10,
          cursor: hasDetail ? 'pointer' : 'default',
        }}
        title={hasDetail ? '点击展开详情' : undefined}
      >
        <span style={{ flexShrink: 0 }}>{EVENT_ICON[ev.event_type] || <BulbOutlined style={{ color: DARK_GREEN }} />}</span>
        <span style={{ color: '#bfbfbf' }}>T{ev.turn}</span>
        <span style={{ color: '#bfbfbf' }}>{EVENT_LABEL[ev.event_type] || ev.event_type}</span>
        <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1 }}>
          {stripEmoji(ev.summary)}
        </span>
        {hasDetail && (
          <span style={{ color: '#bfbfbf', flexShrink: 0 }}>
            <CaretRightOutlined style={{
              fontSize: 8,
              transform: expanded ? 'rotate(90deg)' : undefined,
              transition: 'transform 0.2s',
            }} />
          </span>
        )}
      </div>
      {expanded && hasDetail && p && (
        <div style={{ padding: '4px 8px 6px 28px', fontSize: 10 }}>
          {/* agent_thought: 完整推理 */}
          {ev.event_type === 'agent_thought' && typeof p.content === 'string' ? (
            <div style={{
              padding: '4px 8px', background: '#fffbe6', border: '1px solid #ffe58f',
              borderRadius: 3, maxHeight: 180, overflowY: 'auto', whiteSpace: 'pre-wrap',
              wordBreak: 'break-word', color: '#595959', lineHeight: '16px',
            }}>
              {p.content}
            </div>
          ) : null}
          {/* tool_call: 完整参数 */}
          {null /* DIAG-486 */}
          {/* tool_result: 内容预览 */}
          {ev.event_type === 'tool_result' && (
            <>
              {Boolean(p.text_preview) && (
                <div style={{
                  padding: '4px 8px', background: '#f6ffed', border: '1px solid #b7eb8f',
                  borderRadius: 3, maxHeight: 150, overflowY: 'auto', whiteSpace: 'pre-wrap',
                  wordBreak: 'break-word', color: '#595959', lineHeight: '16px',
                }}>
                  {String(p.text_preview).slice(0, 800)}
                </div>
              )}
              {Array.isArray(p.items) && (p.items as unknown[]).length > 0 && (
                <div style={{ display: 'flex', flexDirection: 'column', gap: 2, marginTop: 4 }}>
                  {(p.items as unknown[]).slice(0, 3).map((item: unknown, i: number) => {
                    const h = item as Record<string, unknown>;
                    return (
                      <div key={i} style={{ padding: '2px 6px', background: '#fafafa', borderRadius: 3, color: '#8c8c8c' }}>
                        {h.url ? (
                          <a href={String(h.url)} target="_blank" rel="noopener noreferrer" style={{ fontSize: 10 }}>
                            <LinkOutlined style={{ marginRight: 2 }} />{String(h.title || h.url)}
                          </a>
                        ) : (
                          <span>{String(h.title || '')}</span>
                        )}
                      </div>
                    );
                  })}
                </div>
              )}
            </>
          )}
          {/* output_finding: reason */}
          {ev.event_type === 'output_finding' && p.reason && (
            <div style={{
              padding: '4px 8px', background: '#fffbe6', border: '1px solid #ffe58f',
              borderRadius: 3, maxHeight: 200, overflowY: 'auto', whiteSpace: 'pre-wrap',
              wordBreak: 'break-word', color: '#595959', lineHeight: '16px',
            }}>
              {String(p.reason).slice(0, 1000)}
            </div>
          )}
          {/* call_log: 统计 */}
          {ev.event_type === 'call_log' && (
            <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', padding: '2px 8px', color: '#722ed1' }}>
              <span>📥 {String(p.tokens_input ?? '-')} in</span>
              <span>📤 {String(p.tokens_output ?? '-')} out</span>
              <span>⏱ {String(p.duration_ms ?? '-')}ms</span>
              {Boolean(p.tools_called) && <span>🔧 {String((p.tools_called as string[]).join(', ') || '(无)')}</span>}
            </div>
          )}
        </div>
      )}
    </div>
  );
};

const TraceDetailLog: React.FC<{
  traceEvents: TraceEvent[];
  clauseMap: Map<string, ClauseState>;
  defaultOpen?: boolean;
}> = ({ traceEvents, clauseMap, defaultOpen = false }) => {
  const [open, setOpen] = useState(defaultOpen);
  const grouped = useMemo(() => {
    const groups: Map<string, TraceEvent[]> = new Map();
    const global: TraceEvent[] = [];
    for (const ev of traceEvents) {
      if (ev.clause_id) {
        if (!groups.has(ev.clause_id)) groups.set(ev.clause_id, []);
        groups.get(ev.clause_id)!.push(ev);
      } else {
        global.push(ev);
      }
    }
    const sorted = Array.from(groups.entries()).sort((a, b) => b[1].length - a[1].length);
    return { global, clauses: sorted };
  }, [traceEvents]);

  if (traceEvents.length === 0) return null;

  return (
    <div style={{ marginTop: 8 }}>
      <div
        onClick={() => setOpen(v => !v)}
        style={{
          fontSize: 12,
          fontWeight: 500,
          marginBottom: open ? 6 : 0,
          color: '#595959',
          cursor: 'pointer',
          userSelect: 'none',
        }}
      >
        <CaretRightOutlined
          style={{
            marginRight: 4,
            fontSize: 10,
            transform: open ? 'rotate(90deg)' : undefined,
            transition: 'transform 0.2s',
          }}
        />
        引擎详细日志（{traceEvents.length} 条事件）
      </div>
      {open && (
        <div style={{
          maxHeight: 350,
          overflowY: 'auto',
          background: '#fafafa',
          borderRadius: 6,
          border: '1px solid #f0f0f0',
          padding: '6px 10px',
          fontSize: 11,
        }}>
          {grouped.global.length > 0 && (
            <div style={{ marginBottom: 4 }}>
              <Text type="secondary" style={{ fontSize: 10 }}>
                <GlobalOutlined style={{ marginRight: 2 }} />
                全局 ({grouped.global.length})
              </Text>
              {grouped.global.slice(-30).map((ev, i) => (
                <TraceEventRow key={i} ev={ev} />
              ))}
            </div>
          )}

          {grouped.clauses.slice(0, 15).map(([cid, events]) => {
            const clause = clauseMap.get(cid);
            const sectionLabel = clause?.sectionPath?.join(' > ') || cid;
            return (
              <details key={cid} style={{ marginBottom: 2 }}>
                <summary style={{ cursor: 'pointer', fontSize: 11, color: '#595959' }}>
                  <LinkOutlined style={{ marginRight: 4, fontSize: 10 }} />
                  {sectionLabel} ({events.length})
                </summary>
                <div style={{ paddingLeft: 12 }}>
                  {events.slice(-15).map((ev, i) => (
                    <TraceEventRow key={i} ev={ev} />
                  ))}
                </div>
              </details>
            );
          })}
        </div>
      )}
    </div>
  );
};

// ─── 主组件 ────────────────────────────────────────────────

const ClauseActivityMap: React.FC<Props> = ({
  traceEvents,
  liveFindings,
  issues,
  phaseEvent,
  statsEvent,
  agentProgresses,
  isAuditing,
  isComplete,
  elapsedSeconds,
  onLocateIssuePage,
}) => {
  const clauseMap = useMemo(
    () => buildClauseMap(liveFindings, issues, traceEvents, agentProgresses),
    [liveFindings, issues, traceEvents, agentProgresses],
  );

  const sectionTree = useMemo(
    () => buildSectionTree(clauseMap),
    [clauseMap],
  );

  // 审核进行中自动展开有风险的节点；完成后默认全部收起，让用户手动展开
  const [expandedKeys, setExpandedKeys] = useState<Set<string>>(new Set());

  useEffect(() => {
    if (isComplete) {
      // 审核完成后全部收起，把结果露出来
      setExpandedKeys(new Set());
    } else if (sectionTree.length > 0) {
      // 审核进行中：自动展开有风险的节点
      setExpandedKeys(prev => {
        const next = new Set(prev);
        const walk = (nodes: SectionTreeNode[]) => {
          for (const n of nodes) {
            if (n.riskCount > 0 && n.children.length > 0) next.add(n.key);
            walk(n.children);
          }
        };
        walk(sectionTree);
        return next;
      });
    }
  }, [sectionTree, isComplete]);

  const toggleSection = (key: string) => {
    setExpandedKeys(prev => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  /** 收集树中所有节点的 key */
  const allKeys = useMemo(() => {
    const keys: string[] = [];
    const walk = (nodes: SectionTreeNode[]) => {
      for (const n of nodes) {
        keys.push(n.key);
        walk(n.children);
      }
    };
    walk(sectionTree);
    return keys;
  }, [sectionTree]);

  const isAllExpanded = allKeys.length > 0 && allKeys.every(k => expandedKeys.has(k));

  const toggleAll = () => {
    if (isAllExpanded) {
      setExpandedKeys(new Set());
    } else {
      setExpandedKeys(new Set(allKeys));
    }
  };

  const totalClauses = clauseMap.size;
  const reviewedClauses = Array.from(clauseMap.values()).filter(c => c.status === 'reviewed').length;
  const totalRisks = sectionTree.reduce((sum, n) => sum + n.riskCount, 0);
  const clausePct = totalClauses > 0 ? Math.round((reviewedClauses / totalClauses) * 100) : 0;

  if (!isAuditing && !isComplete && sectionTree.length === 0 && clauseMap.size === 0 && traceEvents.length === 0) {
    return null;
  }

  return (
    <div style={{ marginBottom: 16 }}>
      <div style={{
        display: 'flex',
        justifyContent: 'space-between',
        alignItems: 'center',
        marginBottom: 8,
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <span style={{ fontSize: 14, fontWeight: 600, color: '#1a1a1a' }}>
            条款动态地图
          </span>
          {allKeys.length > 0 && (
            <Button
              size="small"
              type="link"
              style={{ fontSize: 11, padding: 0, height: 'auto' }}
              onClick={toggleAll}
            >
              {isAllExpanded ? '全部收起' : '全部展开'}
            </Button>
          )}
        </div>
        {totalClauses > 0 && (
          <Text type="secondary" style={{ fontSize: 11 }}>
            {reviewedClauses}/{totalClauses} 条款已审
            {totalRisks > 0 && `  |  ${totalRisks} 处风险`}
          </Text>
        )}
      </div>

      <PhaseBar phaseEvent={phaseEvent} statsEvent={statsEvent} elapsedSeconds={elapsedSeconds} isComplete={isComplete} />
      {!isComplete && <AgentMiniCards progresses={agentProgresses} />}

      {!isComplete && totalClauses > 0 && (
        <Progress percent={clausePct} size="small" strokeColor="#52c41a" style={{ marginBottom: 8 }} />
      )}

      <div style={{
        maxHeight: 500,
        overflowY: 'auto',
        background: '#fafafa',
        borderRadius: 8,
        border: '1px solid #f0f0f0',
        padding: '6px 4px',
      }}>
        {sectionTree.length === 0 && (
          <div style={{ padding: 12, textAlign: 'center', color: '#bfbfbf', fontSize: 12 }}>
            {isAuditing ? '等待审查结果...' : isComplete ? '无条款数据' : '准备中...'}
          </div>
        )}
        {sectionTree.map(node => (
          <SectionTreeNodeView
            key={node.key}
            node={node}
            clauseMap={clauseMap}
            depth={0}
            expandedKeys={expandedKeys}
            onToggle={toggleSection}
            onLocateIssuePage={onLocateIssuePage}
          />
        ))}
      </div>

      <TraceDetailLog traceEvents={traceEvents} clauseMap={clauseMap} />
    </div>
  );
};

export default React.memo(ClauseActivityMap);
