import React, { useEffect, useRef, useState, useCallback } from 'react';
import { Tag, Typography } from 'antd';
import {
  BulbOutlined,
  SearchOutlined,
  FileSearchOutlined,
  WarningOutlined,
  SendOutlined,
  MessageOutlined,
  PlayCircleOutlined,
  LinkOutlined,
  BarChartOutlined,
  CaretRightOutlined,
  FileTextOutlined,
} from '@ant-design/icons';
import type { TraceEvent } from '@/types/audit';
import { AGENT_LABELS } from '@/types/audit';

const { Text } = Typography;

interface Props {
  events: TraceEvent[];
}

const DARK_GREEN = '#52c41a';
const LIGHT_GREEN = '#e8f5e9';

const EVENT_ICON: Record<string, React.ReactNode> = {
  turn_start: <PlayCircleOutlined style={{ color: DARK_GREEN }} />,
  agent_thought: <BulbOutlined style={{ color: '#fa8c16' }} />,
  tool_call: <SearchOutlined style={{ color: '#1677ff' }} />,
  tool_result: <FileSearchOutlined style={{ color: DARK_GREEN }} />,
  output_finding: <WarningOutlined style={{ color: DARK_GREEN }} />,
  agent_bus_send: <SendOutlined style={{ color: DARK_GREEN }} />,
  agent_bus_recv: <MessageOutlined style={{ color: DARK_GREEN }} />,
  call_log: <BarChartOutlined style={{ color: '#722ed1' }} />,
};

const EVENT_LABEL: Record<string, string> = {
  turn_start: '审查轮次',
  agent_thought: '推理',
  tool_call: '工具调用',
  tool_result: '工具结果',
  output_finding: '风险发现',
  agent_bus_send: '跨Agent通知',
  agent_bus_recv: '收到通知',
  call_log: '调用统计',
};

interface SearchSource {
  title?: string;
  url?: string;
  score?: string;
  snippet?: string;
}

const isImportant = (type: string) => type === 'output_finding';

// ─── 事件详情面板（可展开） ──────────────────────────────────

const EventDetail: React.FC<{ event: TraceEvent }> = ({ event }) => {
  const p = event.payload as Record<string, unknown> | undefined;
  if (!p) return null;

  // agent_thought: 完整推理内容
  if (event.event_type === 'agent_thought' && p.content) {
    return (
      <div style={{
        marginTop: 6,
        padding: '8px 12px',
        background: '#fffbe6',
        border: '1px solid #ffe58f',
        borderRadius: 4,
        fontSize: 12,
        lineHeight: '20px',
        whiteSpace: 'pre-wrap',
        wordBreak: 'break-word',
        maxHeight: 300,
        overflowY: 'auto',
        color: '#595959',
      }}>
        <Text type="secondary" style={{ fontSize: 10, display: 'block', marginBottom: 4 }}>
          💭 完整推理内容：
        </Text>
        {String(p.content)}
      </div>
    );
  }

  // tool_call: 完整参数
  if (event.event_type === 'tool_call' && p.arguments) {
    const args = p.arguments as Record<string, unknown>;
    return (
      <div style={{ marginTop: 6 }}>
        <Text type="secondary" style={{ fontSize: 10, display: 'block', marginBottom: 2 }}>
          🔧 工具参数：
        </Text>
        <div style={{
          padding: '6px 10px',
          background: '#e6f4ff',
          border: '1px solid #91caff',
          borderRadius: 4,
          fontSize: 11,
          lineHeight: '18px',
          fontFamily: 'monospace',
          whiteSpace: 'pre-wrap',
          wordBreak: 'break-word',
          maxHeight: 200,
          overflowY: 'auto',
          color: '#595959',
        }}>
          {Object.entries(args)
            .filter(([k]) => !k.startsWith('_'))
            .map(([k, v]) => (
              <div key={k} style={{ marginBottom: 2 }}>
                <span style={{ color: '#1677ff', fontWeight: 500 }}>{k}:</span>{' '}
                <span>{typeof v === 'string' ? v : JSON.stringify(v)}</span>
              </div>
            ))}
        </div>
      </div>
    );
  }

  // tool_result: 返回内容
  if (event.event_type === 'tool_result') {
    return (
      <div style={{ marginTop: 6 }}>
        {/* read_section 文本预览 */}
        {Boolean(p.text_preview) && (
          <div>
            <Text type="secondary" style={{ fontSize: 10, display: 'block', marginBottom: 2 }}>
              📖 读取的条款文本 {p.truncated ? '(截取前2000字符)' : ''}：
            </Text>
            <div style={{
              padding: '8px 12px',
              background: '#f6ffed',
              border: '1px solid #b7eb8f',
              borderRadius: 4,
              fontSize: 12,
              lineHeight: '20px',
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
              maxHeight: 250,
              overflowY: 'auto',
              color: '#595959',
            }}>
              {String(p.text_preview)}
            </div>
          </div>
        )}

        {/* 搜索结果 items */}
        {Array.isArray(p.items) && p.items.length > 0 && (
          <div>
            <Text type="secondary" style={{ fontSize: 10, display: 'block', marginBottom: 4, marginTop: 4 }}>
              🔍 搜索结果（{(p.items as unknown[]).length} 条）：
            </Text>
            {(p.items as unknown[]).slice(0, 5).map((item: unknown, i: number) => {
              const h = item as Record<string, unknown>;
              const title = String(h.title || '');
              const url = String(h.url || '');
              const snippet = String(h.snippet || h.snippet_preview || '').slice(0, 300);
              const score = h.score != null ? Number(h.score).toFixed(2) : '';
              return (
                <div key={i} style={{
                  padding: '4px 8px',
                  marginBottom: 4,
                  background: '#fafafa',
                  borderRadius: 4,
                  border: '1px solid #f0f0f0',
                  fontSize: 11,
                }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 4, marginBottom: 2 }}>
                    {url ? (
                      <a href={url} target="_blank" rel="noopener noreferrer"
                        style={{ fontSize: 12, fontWeight: 500, color: '#1677ff' }}>
                        <LinkOutlined style={{ marginRight: 2 }} />
                        {title || url}
                      </a>
                    ) : (
                      <Text strong style={{ fontSize: 12 }}>{title}</Text>
                    )}
                    {score && <Tag color="blue" style={{ margin: 0, fontSize: 10, lineHeight: '14px' }}>{score}</Tag>}
                  </div>
                  {snippet && (
                    <div style={{ color: '#8c8c8c', fontSize: 11, lineHeight: '16px' }}>
                      {snippet}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}

        {/* 通用 raw_preview */}
        {Boolean(p.raw_preview) && !p.text_preview && (
          <div>
            <Text type="secondary" style={{ fontSize: 10, display: 'block', marginBottom: 2 }}>
              📋 工具返回内容 {p.truncated ? '(截断)' : ''}：
            </Text>
            <div style={{
              padding: '6px 10px',
              background: '#fafafa',
              border: '1px solid #f0f0f0',
              borderRadius: 4,
              fontSize: 11,
              lineHeight: '18px',
              fontFamily: 'monospace',
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
              maxHeight: 200,
              overflowY: 'auto',
              color: '#8c8c8c',
            }}>
              {String(p.raw_preview)}
            </div>
          </div>
        )}
      </div>
    );
  }

  // output_finding: 完整推理链 + 建议
  if (event.event_type === 'output_finding') {
    const truncated = p.truncated as boolean;
    return (
      <div style={{ marginTop: 6, display: 'flex', flexDirection: 'column', gap: 8 }}>
        {truncated && (
          <div style={{
            padding: '6px 10px',
            background: '#fff2e8',
            border: '1px solid #ffbb96',
            borderRadius: 4,
            fontSize: 11,
            color: '#d46b08',
          }}>
            ⚠️ 审查截断 — max_turns 耗尽，置信度低，建议人工复核
          </div>
        )}
        {Boolean(p.source_quote) && (
          <div>
            <Text type="secondary" style={{ fontSize: 10 }}>
              <FileTextOutlined style={{ marginRight: 4 }} />原文摘录：
            </Text>
            <div style={{
              padding: '6px 10px',
              background: '#f6ffed',
              border: '1px solid #b7eb8f',
              borderRadius: 4,
              fontSize: 12,
              lineHeight: '18px',
              maxHeight: 120,
              overflowY: 'auto',
              color: '#595959',
              marginTop: 2,
            }}>
              {String(p.source_quote)}
            </div>
          </div>
        )}
        {Boolean(p.reason) && (
          <div>
            <Text type="secondary" style={{ fontSize: 10 }}>
              <BulbOutlined style={{ marginRight: 4 }} />推理链：
            </Text>
            <div style={{
              padding: '8px 12px',
              background: '#fffbe6',
              border: '1px solid #ffe58f',
              borderRadius: 4,
              fontSize: 12,
              lineHeight: '20px',
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
              maxHeight: 300,
              overflowY: 'auto',
              color: '#595959',
              marginTop: 2,
            }}>
              {String(p.reason)}
            </div>
          </div>
        )}
        {Array.isArray(p.legal_basis) && (p.legal_basis as unknown[]).length > 0 && (
          <div>
            <Text type="secondary" style={{ fontSize: 10 }}>⚖️ 法规依据：</Text>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, marginTop: 2 }}>
              {(p.legal_basis as string[]).map((law, i) => (
                <Tag key={i} color="geekblue" style={{ fontSize: 10, margin: 0 }}>{law}</Tag>
              ))}
            </div>
          </div>
        )}
        {Array.isArray(p.citations) && (p.citations as unknown[]).length > 0 && (
          <div>
            <Text type="secondary" style={{ fontSize: 10 }}>🔗 搜索来源：</Text>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 2, marginTop: 2 }}>
              {(p.citations as Array<{ title: string; url: string; site_name?: string }>).slice(0, 5).map((c, i) => (
                <a key={i} href={c.url || '#'} target="_blank" rel="noopener noreferrer"
                  style={{ fontSize: 10, color: '#1677ff', textDecoration: 'none' }}
                  onClick={(e) => { if (!c.url) e.preventDefault(); }}
                >
                  <LinkOutlined style={{ marginRight: 4, fontSize: 9 }} />
                  [{i + 1}] {c.title}
                  {c.site_name && <span style={{ color: '#bfbfbf', marginLeft: 4 }}>({c.site_name})</span>}
                </a>
              ))}
            </div>
          </div>
        )}
        {Array.isArray(p.case_refs) && (p.case_refs as unknown[]).length > 0 && (
          <div>
            <Text type="secondary" style={{ fontSize: 10 }}>📚 案例引用：</Text>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, marginTop: 2 }}>
              {(p.case_refs as string[]).map((c, i) => (
                <Tag key={i} color="purple" style={{ fontSize: 10, margin: 0 }}>{c}</Tag>
              ))}
            </div>
          </div>
        )}
        {Boolean(p.suggestion) && (
          <div>
            <Text type="secondary" style={{ fontSize: 10 }}>💡 修改建议：</Text>
            <div style={{
              padding: '6px 10px',
              background: '#fffbe6',
              border: '1px solid #ffe58f',
              borderRadius: 4,
              fontSize: 12,
              lineHeight: '18px',
              color: '#595959',
              marginTop: 2,
            }}>
              {String(p.suggestion)}
            </div>
          </div>
        )}
      </div>
    );
  }

  // call_log: token/耗时统计
  if (event.event_type === 'call_log') {
    return (
      <div style={{
        marginTop: 6,
        padding: '6px 12px',
        background: '#f9f0ff',
        border: '1px solid #d3adf7',
        borderRadius: 4,
        fontSize: 12,
        display: 'flex',
        gap: 16,
        flexWrap: 'wrap',
      }}>
        <span>📥 输入 tokens: <Text strong style={{ color: '#722ed1' }}>{String(p.tokens_input ?? '-')}</Text></span>
        <span>📤 输出 tokens: <Text strong style={{ color: '#722ed1' }}>{String(p.tokens_output ?? '-')}</Text></span>
        <span>⏱ 耗时: <Text strong style={{ color: '#722ed1' }}>{String(p.duration_ms ?? '-')}ms</Text></span>
        {Array.isArray(p.tools_called) && (p.tools_called as string[]).length > 0 && (
          <span>🔧 工具: {(p.tools_called as string[]).join(', ') || '(纯文本)'}</span>
        )}
        {p.produced_finding !== undefined && (
          <span>{p.produced_finding ? '✅ 产出finding' : '➡️ 工具调用'}</span>
        )}
      </div>
    );
  }

  return null;
};

// ─── 主组件 ────────────────────────────────────────────────

const LiveReviewFeed: React.FC<Props> = ({ events }) => {
  const bottomRef = useRef<HTMLDivElement>(null);
  const [expandedKeys, setExpandedKeys] = useState<Set<number>>(new Set());

  const toggleExpand = useCallback((idx: number) => {
    setExpandedKeys(prev => {
      const next = new Set(prev);
      if (next.has(idx)) next.delete(idx);
      else next.add(idx);
      return next;
    });
  }, []);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [events.length]);

  if (events.length === 0) {
    return null;
  }

  // 判断事件是否有可展开的详情
  const hasDetail = (event: TraceEvent): boolean => {
    const p = event.payload as Record<string, unknown> | undefined;
    if (!p) return false;
    switch (event.event_type) {
      case 'agent_thought': return !!p.content;
      case 'tool_call': return !!p.arguments;
      case 'tool_result': return !!(p.text_preview || p.items || p.raw_preview);
      case 'output_finding': return !!(p.reason || p.suggestion || p.source_quote);
      case 'call_log': return true;
      default: return false;
    }
  };

  return (
    <div style={{ marginBottom: 16 }}>
      <div style={{
        fontSize: 14,
        fontWeight: 600,
        marginBottom: 10,
        color: '#1a1a1a',
      }}>
        实时审查动态
      </div>
      <div style={{
        maxHeight: 520,
        overflowY: 'auto',
        background: '#fafafa',
        borderRadius: 8,
        padding: '8px 12px',
        border: '1px solid #f0f0f0',
      }}>
        {events.slice(-120).map((event, idx) => {
          const agentLabel = AGENT_LABELS[event.agent_name] || event.agent_name;
          const important = isImportant(event.event_type);
          const isThought = event.event_type === 'agent_thought';
          const isToolCall = event.event_type === 'tool_call';
          const isToolResult = event.event_type === 'tool_result';
          const isCallLog = event.event_type === 'call_log';
          const expandable = hasDetail(event);
          const expanded = expandedKeys.has(idx);

          // tool_result: extract sources from payload
          const p = event.payload as Record<string, unknown> | undefined;
          const sources: SearchSource[] =
            isToolResult && p?.sources
              ? (p.sources as SearchSource[])
              : [];

          return (
            <div
              key={idx}
              style={{
                borderBottom: '1px solid #f5f5f5',
                background: important ? LIGHT_GREEN : isCallLog ? '#f9f0ff' : undefined,
              }}
            >
              {/* ── 主行（可点击展开） ── */}
              <div
                onClick={() => expandable && toggleExpand(idx)}
                style={{
                  display: 'flex',
                  gap: 8,
                  padding: '5px 0',
                  paddingLeft: important ? 8 : 0,
                  paddingRight: important ? 8 : 0,
                  fontSize: 12,
                  lineHeight: '18px',
                  borderRadius: important ? 4 : undefined,
                  cursor: expandable ? 'pointer' : 'default',
                  transition: 'background 0.15s',
                }}
                title={expandable ? '点击展开详情' : undefined}
              >
                {/* Expand toggle */}
                {expandable && (
                  <span style={{ fontSize: 10, flexShrink: 0, marginTop: 3, color: '#bfbfbf' }}>
                    <CaretRightOutlined
                      style={{
                        transform: expanded ? 'rotate(90deg)' : undefined,
                        transition: 'transform 0.2s',
                      }}
                    />
                  </span>
                )}

                {/* Icon */}
                <span style={{ fontSize: 14, flexShrink: 0, marginTop: 1 }}>
                  {EVENT_ICON[event.event_type] || <BulbOutlined style={{ color: DARK_GREEN }} />}
                </span>

                {/* Content */}
                <div style={{ flex: 1, minWidth: 0 }}>
                  {/* Header row: agent + tag + turn */}
                  <span style={{ display: 'flex', alignItems: 'center', gap: 4, marginBottom: 1 }}>
                    <span style={{ fontWeight: 500, color: '#595959' }}>
                      {agentLabel}
                    </span>
                    <Tag
                      style={{
                        margin: 0,
                        fontSize: 10,
                        lineHeight: '16px',
                        padding: '0 4px',
                        color: isCallLog ? '#722ed1' :
                          important ? '#fff' :
                          isThought ? '#d46b08' :
                          isToolCall ? '#1677ff' :
                          isToolResult ? '#389e0d' : '#8c8c8c',
                        background: isCallLog ? '#f9f0ff' :
                          important ? DARK_GREEN :
                          isThought ? '#fff7e6' :
                          isToolCall ? '#e6f4ff' :
                          isToolResult ? '#f6ffed' : '#f5f5f5',
                        border: isCallLog ? '1px solid #d3adf7' :
                          important ? 'none' :
                          isThought ? '1px solid #ffd591' :
                          isToolCall ? '1px solid #91caff' :
                          isToolResult ? '1px solid #b7eb8f' : '1px solid #d9d9d9',
                      }}
                    >
                      {EVENT_LABEL[event.event_type] || event.event_type}
                    </Tag>
                    <span style={{ color: '#bfbfbf', fontSize: 10 }}>
                      T{event.turn}
                    </span>
                    {expandable && (
                      <span style={{ color: '#bfbfbf', fontSize: 10, marginLeft: 4 }}>
                        {expanded ? '▲' : '▼'}
                      </span>
                    )}
                  </span>

                  {/* Summary */}
                  {event.summary && !isToolResult && (
                    <div style={{
                      color: isThought ? '#8c6d00' : isCallLog ? '#722ed1' : '#8c8c8c',
                      wordBreak: 'break-all',
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      display: '-webkit-box',
                      WebkitLineClamp: expanded ? undefined : 2,
                      WebkitBoxOrient: 'vertical',
                    }}>
                      {event.summary}
                    </div>
                  )}

                  {/* tool_result: summary + links */}
                  {isToolResult && event.summary && (
                    <div style={{ color: '#8c8c8c', marginBottom: sources.length > 0 ? 4 : 0 }}>
                      {event.summary}
                    </div>
                  )}
                  {sources.length > 0 && (
                    <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                      {sources.map((src, si) => (
                        <a
                          key={si}
                          href={src.url || '#'}
                          target="_blank"
                          rel="noopener noreferrer"
                          onClick={(e) => {
                            if (!src.url) e.preventDefault();
                          }}
                          style={{
                            display: 'flex',
                            alignItems: 'center',
                            gap: 4,
                            fontSize: 11,
                            color: '#1677ff',
                            textDecoration: 'none',
                            overflow: 'hidden',
                            textOverflow: 'ellipsis',
                            whiteSpace: 'nowrap',
                          }}
                          title={src.title || src.url}
                        >
                          <LinkOutlined style={{ flexShrink: 0, fontSize: 10 }} />
                          <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>
                            {src.title || src.url}
                          </span>
                          {src.score && (
                            <span style={{ color: '#bfbfbf', flexShrink: 0, fontSize: 10 }}>
                              [{src.score}]
                            </span>
                          )}
                        </a>
                      ))}
                    </div>
                  )}
                </div>
              </div>

              {/* ── 展开详情 ── */}
              {expandable && expanded && (
                <div style={{ padding: '0 8px 8px 28px' }}>
                  <EventDetail event={event} />
                </div>
              )}
            </div>
          );
        })}
        <div ref={bottomRef} />
      </div>
    </div>
  );
};

export default LiveReviewFeed;
