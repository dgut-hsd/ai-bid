import React from 'react';
import { Typography, Tooltip, theme } from 'antd';
import {
  RobotOutlined,
  UserOutlined,
  ExclamationCircleOutlined,
} from '@ant-design/icons';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeRaw from 'rehype-raw';
import { useStyles } from './style';
import type { ChatMessage } from '../../hooks/useAiChat';

const { Text, Paragraph, Link } = Typography;

type StructuredIssue = {
  title?: string;
  severity?: string;
  rationale?: string;
  suggestions?: string[];
};

type SavedSummaryBlock = {
  title: string;
  point: string;
};

type SourceRef = {
  key: string;
  fileId?: number;
  fileName: string;
  sourceType?: string;
  pageNumber?: number;
  sectionName?: string;
  previewUrl?: string;
};

const severityLabel: Record<string, string> = {
  critical: '严重',
  warning: '警告',
  info: '提示',
};

const extractTailSuggestions = (content: string): string[] => {
  const tail = content.slice(Math.max(content.lastIndexOf(']') + 1, 0)).trim();
  if (!tail) return [];
  const parts = tail
    .replace(/\s+/g, ' ')
    .split(/(?=细化|绑定|约定|明确|补充|增加|删除)/)
    .map((item) => item.replace(/^[,，;；\s]+/, '').trim())
    .filter(Boolean);
  return parts.filter((item) => item.length >= 6);
};

const parseLooseIssues = (content: string): StructuredIssue[] | null => {
  const titlePattern = /"title"\s*:\s*"([^"]+)"/g;
  const matches = Array.from(content.matchAll(titlePattern));
  if (matches.length === 0) return null;

  const issues: StructuredIssue[] = [];
  for (let i = 0; i < matches.length; i++) {
    const start = matches[i].index ?? 0;
    const end =
      i + 1 < matches.length ? (matches[i + 1].index ?? content.length) : content.length;
    const block = content.slice(start, end);

    const titleMatch = block.match(/"title"\s*:\s*"([^"]+)"/);
    const severityMatch = block.match(/"severity"\s*:\s*"([^"]+)"/);
    const rationaleMatch = block.match(/"rationale"\s*:\s*"([\s\S]*?)"(?:\s*,\s*"|$)/);
    const suggestionsBlockMatch = block.match(/"suggestions"\s*:\s*\[([\s\S]*?)\]/);
    const suggestions = suggestionsBlockMatch
      ? Array.from(suggestionsBlockMatch[1].matchAll(/"([^"]+)"/g)).map((m) => m[1])
      : [];

    issues.push({
      title: titleMatch?.[1],
      severity: severityMatch?.[1],
      rationale: rationaleMatch?.[1],
      suggestions,
    });
  }

  const tailSuggestions = extractTailSuggestions(content);
  if (tailSuggestions.length > 0 && issues.length > 0) {
    const last = issues[issues.length - 1];
    last.suggestions = [...(last.suggestions || []), ...tailSuggestions];
  }

  return issues;
};

const parseStructuredIssues = (content: string): StructuredIssue[] | null => {
  const trimmed = content.trim();
  const tryParse = (raw: string): unknown => {
    try {
      return JSON.parse(raw);
    } catch {
      return null;
    }
  };

  const direct = tryParse(trimmed);
  if (Array.isArray(direct)) return direct as StructuredIssue[];

  const start = trimmed.indexOf('[');
  const end = trimmed.lastIndexOf(']');
  if (start >= 0 && end > start) {
    const sliced = tryParse(trimmed.slice(start, end + 1));
    if (Array.isArray(sliced)) return sliced as StructuredIssue[];
  }
  return parseLooseIssues(trimmed);
};

const stripTrailingJsonNoise = (content: string): string => {
  let cleaned = content;
  cleaned = cleaned.replace(/[""']?citations[""']?\s*:\s*\[[\s\S]*$/i, '');
  cleaned = cleaned.replace(/[""']meta[""']?\s*:\s*\{[\s\S]*$/i, '');
  cleaned = cleaned.replace(/[,\s]+$/, '').trim();
  return cleaned;
};

const prettifyRawContent = (content: string): string => {
  return stripTrailingJsonNoise(content)
    .replace(/},\s*{/g, '}\n\n{')
    .replace(/"title"\s*:\s*/g, '\n标题：')
    .replace(/"severity"\s*:\s*/g, '\n级别：')
    .replace(/"rationale"\s*:\s*/g, '\n依据：')
    .replace(/"suggestions"\s*:\s*\[/g, '\n建议：\n[')
    .replace(/\],\s*{/g, ']\n\n{')
    .replace(/"\s*,\s*"/g, '"\n"')
    .replace(/\s{2,}/g, ' ')
    .trim();
};

const parseSavedSummary = (content: string): SavedSummaryBlock[] | null => {
  const prefix = '已保存记录，归纳内容如下：';
  const text = String(content || '').trim();
  if (!text.startsWith(prefix)) {
    return null;
  }
  const body = text
    .slice(prefix.length)
    .trim()
    .replace(/\\n/g, '\n');
  if (!body) {
    return [];
  }
  const blockRegex = /【([^】]+)】\s*\n(?:- 要点：)?([\s\S]*?)(?=\n\s*【|$)/g;
  const blocks: SavedSummaryBlock[] = [];
  for (const match of body.matchAll(blockRegex)) {
    const title = (match[1] || '').trim();
    const point = (match[2] || '').trim().replace(/\n+/g, '\n');
    if (!title || !point) continue;
    blocks.push({ title, point });
  }
  return blocks;
};

const toPositiveInt = (value: unknown): number | undefined => {
  if (typeof value === 'number' && Number.isFinite(value) && value > 0) {
    return Math.floor(value);
  }
  if (typeof value === 'string') {
    const parsed = Number.parseInt(value, 10);
    if (Number.isFinite(parsed) && parsed > 0) {
      return parsed;
    }
  }
  return undefined;
};

/** 运行时引用格式（Java 后端可能附加 meta 信封） */
type RuntimeCitation = {
  meta?: Record<string, unknown>;
  documentName?: string;
  pageNumber?: number;
};

const getSourceRefs = (message: ChatMessage): SourceRef[] => {
  if (!Array.isArray(message.citations) || message.citations.length === 0) {
    return [];
  }
  const refs: SourceRef[] = [];
  const dedupe = new Set<string>();
  for (const citation of message.citations) {
    const rt = citation as unknown as RuntimeCitation;
    const meta = rt.meta ?? {};
    const fileId = toPositiveInt(meta.fileId ?? meta.file_id ?? meta.document_id);
    const sourceTypeRaw = String(meta.sourceType ?? meta.source_type ?? '').toLowerCase();
    const sourceType = sourceTypeRaw === 'knowledge' ? sourceTypeRaw : undefined;
    if (!sourceType) {
      continue;
    }
    const fileName =
      String(meta.fileName ?? meta.file_name ?? rt.documentName ?? '').trim() ||
      (fileId ? `文件#${fileId}` : '未知文件');
    const pageNumber = toPositiveInt(meta.pageNumber ?? rt.pageNumber);
    const sectionName =
      typeof meta.sectionName === 'string' && meta.sectionName.trim()
        ? meta.sectionName.trim()
        : undefined;
    const previewUrl =
      fileId && sourceType === 'knowledge'
        ? `${import.meta.env.VITE_API_BASE_URL}/api/knowledge-files/${fileId}/preview`
        : undefined;
    const key = `${sourceType || 'unknown'}-${fileId || fileName}-${pageNumber || 'na'}`;
    if (dedupe.has(key)) {
      continue;
    }
    dedupe.add(key);
    refs.push({
      key,
      fileId,
      fileName,
      sourceType,
      pageNumber,
      sectionName,
      previewUrl,
    });
  }
  return refs;
};

export const MessageBubble: React.FC<{ message: ChatMessage }> = React.memo(
  ({ message }) => {
    const { token } = theme.useToken();
    const { styles } = useStyles();
    const isUser = message.role === 'user';
    const sourceRefs = !isUser ? getSourceRefs(message) : [];

    const bubbleBg = isUser ? token.colorFillAlter : token.colorBgContainer;
    const bubbleBorder = isUser ? 'none' : `1px solid ${token.colorBorderSecondary}`;
    const borderRadius = isUser
      ? '16px 16px 4px 16px'
      : '16px 16px 16px 4px';

    return (
      <div
        style={{
          display: 'flex',
          flexDirection: isUser ? 'row-reverse' : 'row',
          gap: 8,
          marginBottom: 16,
          alignItems: 'flex-end',
        }}
      >
        {/* ── Avatar ── */}
        <div
          style={{
            width: 32,
            height: 32,
            borderRadius: 10,
            flexShrink: 0,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            fontSize: 14,
            background: isUser ? token.colorFillAlter : token.colorPrimary,
            color: isUser ? token.colorPrimary : '#fff',
          }}
        >
          {isUser ? <UserOutlined /> : <RobotOutlined />}
        </div>

        {/* ── Bubble + Timestamp ── */}
        <div
          style={{
            display: 'flex',
            flexDirection: 'column',
            alignItems: isUser ? 'flex-end' : 'flex-start',
            maxWidth: '75%',
          }}
        >
          {/* Bubble */}
          <div
            style={{
              padding: '10px 14px',
              borderRadius,
              background: bubbleBg,
              border: bubbleBorder,
              wordBreak: 'break-word',
              position: 'relative',
            }}
          >
            {/* ── Reasoning chain (AI messages only) ── */}
            {!isUser && message.reasoning && message.reasoning.length > 0 && (
              <details open style={{ marginBottom: 12 }}>
                <summary style={{
                  fontSize: 12,
                  color: token.colorTextSecondary,
                  cursor: 'pointer',
                  userSelect: 'none',
                  padding: '4px 0',
                }}>
                  推理过程（共 {message.reasoning.length} 步）
                </summary>
                <div style={{
                  marginTop: 8,
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 8,
                }}>
                  {message.reasoning.map((step, i) => (
                    <div key={i} style={{
                      padding: '8px 12px',
                      borderRadius: 6,
                      background: token.colorFillAlter,
                      border: `1px solid ${token.colorBorderSecondary}`,
                    }}>
                      <div style={{
                        fontSize: 11,
                        color: token.colorTextQuaternary,
                        marginBottom: 4,
                      }}>
                        第 {i + 1} 步
                      </div>
                      <div style={{
                        fontSize: 12,
                        lineHeight: 1.65,
                        color: token.colorTextSecondary,
                        whiteSpace: 'pre-wrap',
                        wordBreak: 'break-word',
                      }}>
                        {step}
                      </div>
                    </div>
                  ))}
                </div>
              </details>
            )}

            {/* ── Content parsing ── */}
            {(() => {
              const savedBlocks =
                !isUser && message.content ? parseSavedSummary(message.content) : null;
              if (savedBlocks && savedBlocks.length > 0) {
                return (
                  <div style={{ display: 'grid', gap: 8 }}>
                    <Text strong style={{ fontSize: 13 }}>
                      已保存记录（归纳）
                    </Text>
                    {savedBlocks.map((item, idx) => (
                      <div
                        key={`${item.title}-${idx}`}
                        style={{
                          border: `1px solid ${token.colorBorderSecondary}`,
                          borderRadius: 8,
                          padding: '8px 10px',
                          background: token.colorBgContainer,
                        }}
                      >
                        <Text strong style={{ fontSize: 13 }}>
                          {idx + 1}. {item.title}
                        </Text>
                        <Paragraph
                          style={{
                            margin: '6px 0 0',
                            fontSize: 13,
                            lineHeight: 1.6,
                            whiteSpace: 'pre-wrap',
                          }}
                        >
                          {item.point}
                        </Paragraph>
                      </div>
                    ))}
                  </div>
                );
              }

              const issues =
                !isUser && message.content
                  ? parseStructuredIssues(message.content)
                  : null;

              if (issues && issues.length > 0) {
                return (
                  <div style={{ display: 'grid', gap: 10 }}>
                    {issues.map((item, idx) => (
                      <div
                        key={`${item.title ?? 'item'}-${idx}`}
                        style={{
                          border: `1px solid ${token.colorBorderSecondary}`,
                          borderRadius: 8,
                          padding: '8px 10px',
                          background: token.colorBgContainer,
                        }}
                      >
                        <Text strong style={{ fontSize: 13 }}>
                          {idx + 1}. {item.title || '未命名问题'}
                        </Text>
                        <Text
                          style={{
                            marginLeft: 8,
                            fontSize: 12,
                            color: token.colorTextSecondary,
                          }}
                        >
                          {severityLabel[item.severity || ''] ||
                            item.severity ||
                            '未知级别'}
                        </Text>
                        {item.rationale && (
                          <Paragraph
                            style={{
                              margin: '6px 0 0',
                              fontSize: 13,
                              lineHeight: 1.6,
                              whiteSpace: 'pre-wrap',
                            }}
                          >
                            {item.rationale}
                          </Paragraph>
                        )}
                        {Array.isArray(item.suggestions) &&
                          item.suggestions.length > 0 && (
                            <ul
                              style={{
                                margin: '6px 0 0',
                                paddingLeft: 18,
                              }}
                            >
                              {item.suggestions
                                .filter(
                                  (s) =>
                                    !!s &&
                                    s.trim().length > 1 &&
                                    !/^[,，。.:：;；\s-]+$/.test(s)
                                )
                                .map((s, i) => (
                                  <li
                                    key={`${idx}-${i}`}
                                    style={{
                                      fontSize: 13,
                                      lineHeight: 1.6,
                                    }}
                                  >
                                    {s}
                                  </li>
                                ))}
                            </ul>
                          )}
                      </div>
                    ))}
                  </div>
                );
              }

              // User messages: plain text with pre-wrap
              if (isUser) {
                return (
                  <Paragraph
                    style={{
                      margin: 0,
                      fontSize: 13,
                      lineHeight: 1.65,
                      whiteSpace: 'pre-wrap',
                      color: token.colorTextBase,
                    }}
                  >
                    {message.content}
                  </Paragraph>
                );
              }

              // AI messages: render as Markdown
              const mdContent = prettifyRawContent(message.content);
              return (
                <div className={styles.markdownContent}>
                  <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeRaw]}>
                    {mdContent}
                  </ReactMarkdown>
                </div>
              );
            })()}

            {/* ── Source references ── */}
            {!isUser && sourceRefs.length > 0 && (
              <div
                style={{
                  marginTop: 8,
                  paddingTop: 6,
                  borderTop: `1px dashed ${token.colorBorderSecondary}`,
                  display: 'grid',
                  gap: 4,
                }}
              >
                <Text style={{ fontSize: 12, color: token.colorTextSecondary }}>
                  引用来源
                </Text>
                <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
                  {sourceRefs.map((ref) => (
                    <Link
                      key={ref.key}
                      disabled={!ref.previewUrl}
                      onClick={(event) => {
                        if (!ref.previewUrl) {
                          return;
                        }
                        event.preventDefault();
                        window.open(ref.previewUrl, '_blank', 'noopener,noreferrer');
                      }}
                    >
                      {ref.fileName}
                      {ref.pageNumber ? `（第${ref.pageNumber}页）` : ''}
                    </Link>
                  ))}
                </div>
                {/* KnowledgeRef source_url */}
                {message.citations && message.citations.some(
                  (c: any) => c?.sourceUrl || c?.url
                ) && (
                  <div style={{ marginTop: 4 }}>
                    {message.citations
                      .filter((c: any) => c?.sourceUrl || c?.url)
                      .map((c: any, idx: number) => (
                        <div key={idx} style={{ fontSize: 12, marginBottom: 2 }}>
                          <a
                            href={c.sourceUrl || c.url}
                            target="_blank"
                            rel="noopener noreferrer"
                            style={{ color: '#1677ff' }}
                          >
                            {c.title || c.excerpt || '外部参考'}
                          </a>
                          {c.title && <span style={{ color: '#999', marginLeft: 4 }}>{c.title}</span>}
                        </div>
                      ))}
                  </div>
                )}
              </div>
            )}

            {/* ── Confidence bar ── */}
            {!isUser && message.confidence !== undefined && (
              <div style={{ marginTop: 8, display: 'flex', alignItems: 'center', gap: 8 }}>
                <Text type="secondary" style={{ fontSize: 11 }}>
                  置信度
                </Text>
                <div
                  style={{
                    width: 80,
                    height: 6,
                    borderRadius: 3,
                    background: '#f0f0f0',
                    overflow: 'hidden',
                  }}
                >
                  <div
                    style={{
                      width: `${Math.round(message.confidence * 100)}%`,
                      height: '100%',
                      background:
                        message.confidence < 0.5 ? '#f5222d' :
                        message.confidence < 0.7 ? '#fa8c16' : '#52c41a',
                      borderRadius: 3,
                    }}
                  />
                </div>
                <Text style={{ fontSize: 11 }}>
                  {Math.round(message.confidence * 100)}%
                </Text>
              </div>
            )}

            {/* ── Suggested actions ── */}
            {!isUser && message.suggestedActions && message.suggestedActions.length > 0 && (
              <div style={{ marginTop: 6 }}>
                {message.suggestedActions.map((action: string, idx: number) => (
                  <span
                    key={idx}
                    style={{
                      display: 'inline-block',
                      padding: '2px 8px',
                      margin: '2px 4px 2px 0',
                      fontSize: 11,
                      borderRadius: 10,
                      background: '#e6f4ff',
                      color: '#1677ff',
                      cursor: 'pointer',
                    }}
                  >
                    {action}
                  </span>
                ))}
              </div>
            )}

            {/* ── Error indicator ── */}
            {message.status === 'error' && (
              <Tooltip title="Send failed – please retry">
                <ExclamationCircleOutlined
                  style={{
                    color: token.colorError,
                    position: 'absolute',
                    right: -20,
                    top: 8,
                    cursor: 'pointer',
                  }}
                />
              </Tooltip>
            )}
          </div>

          {/* ── Timestamp (below bubble) ── */}
          <Text
            style={{
              fontSize: 10,
              color: token.colorTextQuaternary,
              marginTop: 4,
              padding: '0 2px',
            }}
          >
            {new Date(message.createTime).toLocaleTimeString([], {
              hour: '2-digit',
              minute: '2-digit',
            })}
          </Text>
        </div>
      </div>
    );
  }
);
