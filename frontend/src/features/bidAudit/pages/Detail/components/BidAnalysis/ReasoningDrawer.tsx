import React from 'react';
import { Modal, Tag, Typography, Alert, Space, Progress, Divider } from 'antd';
import {
  FileTextOutlined,
  NodeIndexOutlined,
  SafetyCertificateOutlined,
  BookOutlined,
  BulbOutlined,
  AuditOutlined,
} from '@ant-design/icons';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import type { AuditIssue } from '@/types/audit';
import { SEVERITY_MAP, SEVERITY_COLORS, agentLabel } from '@/types/audit';
import { useIsMobile } from '@/hooks/useMediaQuery';
import TierBadge from './TierBadge';
import CitationList from './CitationList';

const { Text, Title } = Typography;

interface Props {
  issue: AuditIssue | null;
  open: boolean;
  onClose: () => void;
  onLocatePage?: (page: number) => void;
}

/* ── 色板 ── */
const GREEN_BG = '#f6ffed';
const SUGGESTION_BG = '#fffbe6';

const ReasoningDrawer: React.FC<Props> = ({ issue, open, onClose, onLocatePage }) => {
  const isMobile = useIsMobile();
  if (!issue) return null;

  const severityColor = SEVERITY_COLORS[issue.severity] || '#1890ff';
  const description = (issue.description || '').replace(/📎\s*搜索来源[\s\S]*$/g, '').trimEnd();
  const suggestionLines = (issue.suggestion || '')
    .split(/[。；;]/)
    .map((s) => s.trim())
    .filter(Boolean);

  /* 法规链接解析 */
  const parseLawLink = (raw: string) => {
    const m = raw.match(/^\[(.+?)\]\((.+?)\)$/);
    return m ? { name: m[1], url: m[2] } : { name: raw.replace(/^\[|\]$/g, ''), url: '' };
  };

  return (
    <Modal
      title={
        <span style={{ fontWeight: 600 }}>{issue.category}</span>
      }
      open={open}
      onCancel={onClose}
      footer={null}
      width={isMobile ? '100%' : 920}
      centered
      styles={{
        content: {
          position: 'relative',
          overflow: 'hidden',
          padding: 0,
        },
        header: {
          padding: isMobile ? '18px 20px 18px 64px' : '18px 28px 18px 80px',
        },
        body: {
          position: 'static',
          overflow: 'visible',
          padding: 0,
        },
      }}
      destroyOnClose
    >
      {/* ── 左上角斜标签（绝对定位到 .ant-modal-content 顶角）── */}
      <div
        style={{
          position: 'absolute',
          top: 0,
          left: 0,
          zIndex: 10,
          overflow: 'hidden',
          width: 88,
          height: 88,
          pointerEvents: 'none',
        }}
      >
        <div
          style={{
            position: 'absolute',
            top: 14,
            left: -36,
            width: 140,
            transform: 'rotate(-45deg)',
            background: severityColor,
            color: '#fff',
            fontWeight: 600,
            fontSize: 12,
            textAlign: 'center',
            padding: '4px 0',
            lineHeight: '22px',
            letterSpacing: 1,
            boxShadow: '0 1px 3px rgba(0,0,0,0.12)',
          }}
        >
          {issue.isCritical ? '重大问题' : SEVERITY_MAP[issue.severity]}
        </div>
      </div>

      {/* ── 可滚动内容 ── */}
      <div style={{ maxHeight: isMobile ? 'calc(100dvh - 160px)' : 'calc(100vh - 200px)', overflowY: 'auto', padding: isMobile ? '12px 16px 24px' : '12px 28px 24px' }}>
      {/* ── 截断警告 ── */}
      {issue.truncated && (
        <Alert
          type="warning"
          showIcon
          message="审查未完成 — Agent 轮次耗尽，置信度低，建议人工复核"
          style={{ marginBottom: 20 }}
        />
      )}
      {issue.isCritical && (
        <Alert
          type="error"
          showIcon
          message="重大/红线问题"
          description={issue.criticalReason || '该问题需优先人工复核和处置。'}
          style={{ marginBottom: 20 }}
        />
      )}

      {/* ── 摘要行 ── */}
      <div style={{ marginBottom: 24 }}>
        <div style={{ display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: 24, marginBottom: 4 }}>
          {issue.confidence !== undefined && (
            <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <Text type="secondary" style={{ fontSize: 13 }}>置信度</Text>
              <Progress
                percent={Math.round(issue.confidence * 100)}
                size="small"
                style={{ width: 120, margin: 0 }}
                strokeColor={
                  issue.confidence < 0.5 ? '#f5222d' :
                  issue.confidence < 0.7 ? '#fa8c16' : '#52c41a'
                }
              />
            </span>
          )}
          {issue.anchorPage != null && (
            <Text
              type="secondary"
              style={{ fontSize: 13, cursor: 'pointer', textDecoration: 'underline' }}
              onClick={() => onLocatePage?.(issue.anchorPage!)}
            >
              第 {issue.anchorPage} 页
            </Text>
          )}
          {(issue.agentName || issue.agent) && (
            <Text type="secondary" style={{ fontSize: 13 }}>
              {agentLabel(issue.agentName || issue.agent || '')}
            </Text>
          )}
          {issue.tierEscalated && (
            <TierBadge
              initialTier={issue.initialTier}
              finalTier={issue.finalTier}
              tierEscalated={issue.tierEscalated}
            />
          )}
        </div>
        {issue.anchorSection && (
          <Text type="secondary" style={{ fontSize: 12 }}>
            {issue.anchorSection}
          </Text>
        )}
      </div>

      {/* ── 原文摘录 ── */}
      {issue.sourceQuote && (
        <>
          <Title level={5}>
            <FileTextOutlined style={{ marginRight: 6 }} />
            原文摘录
          </Title>
          <Text
            type="secondary"
            style={{
              display: 'block',
              marginBottom: 24,
              paddingLeft: 4,
              fontSize: 15,
              lineHeight: 1.9,
              whiteSpace: 'pre-wrap',
            }}
          >
            {issue.sourceQuote}
          </Text>
        </>
      )}

      {/* ── 推理链 ── */}
      <Title level={5}>
        <NodeIndexOutlined style={{ marginRight: 6 }} />
        推理链
      </Title>
      <div style={{ fontSize: 15, lineHeight: 2, marginBottom: 12 }}>
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{description}</ReactMarkdown>
      </div>

      {/* ── 搜索来源（紧接推理链） ── */}
      <CitationList citations={issue.citations} />

      {/* ── 法规依据 ── */}
      {issue.legalBasis && issue.legalBasis.length > 0 && (
        <>
          <Divider style={{ margin: '8px 0 16px' }} />
          <Title level={5}>
            <SafetyCertificateOutlined style={{ marginRight: 6 }} />
            法规依据
          </Title>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6, marginBottom: 8 }}>
            {issue.legalBasis.map((raw, idx) => {
              const { name, url } = parseLawLink(raw);
              return (
                <div
                  key={idx}
                  style={{
                    padding: '8px 14px',
                    background: GREEN_BG,
                    border: '1px solid #b7eb8f',
                    borderRadius: 6,
                    fontSize: 13,
                  }}
                >
                  <SafetyCertificateOutlined style={{ color: '#52c41a', marginRight: 8, fontSize: 13 }} />
                  {url ? (
                    <a href={url} target="_blank" rel="noreferrer" style={{ color: '#135200' }}>
                      {name}
                    </a>
                  ) : (
                    <Text style={{ fontSize: 13 }}>{name}</Text>
                  )}
                </div>
              );
            })}
          </div>
        </>
      )}

      {/* ── 案例引用 ── */}
      {issue.caseRefs && issue.caseRefs.length > 0 && (
        <>
          <Divider style={{ margin: '8px 0 16px' }} />
          <Title level={5}>
            <BookOutlined style={{ marginRight: 6 }} />
            案例引用
          </Title>
          <Space wrap size={[4, 4]} style={{ marginBottom: 8 }}>
            {issue.caseRefs.map((c, idx) => (
              <Tag key={idx} color="purple">{c}</Tag>
            ))}
          </Space>
        </>
      )}

      {/* ── 证据核验 ── */}
      {issue.verifierReason && (
        <>
          <Divider style={{ margin: '8px 0 16px' }} />
          <Title level={5}>
            <AuditOutlined style={{ marginRight: 6 }} />
            证据核验
          </Title>
          <Text style={{ display: 'block', marginBottom: 24, paddingLeft: 4, fontSize: 15, lineHeight: 1.9, whiteSpace: 'pre-wrap' }}>
            {issue.verifierReason}
          </Text>
        </>
      )}

      {/* ── 修改建议 ── */}
      {issue.suggestion && (
        <>
          <Divider style={{ margin: '8px 0 16px' }} />
          <Title level={5}>
            <BulbOutlined style={{ marginRight: 6 }} />
            修改建议
          </Title>
          <div
            style={{
              padding: '14px 18px',
              background: SUGGESTION_BG,
              border: '1px solid #ffe58f',
              borderRadius: 6,
              fontSize: 15,
              lineHeight: 2,
            }}
          >
            <ul style={{ margin: 0, paddingLeft: 20 }}>
              {suggestionLines.map((line, i) => (
                <li key={i} style={{ marginBottom: i < suggestionLines.length - 1 ? 8 : 0 }}>
                  {line}。
                </li>
              ))}
            </ul>
          </div>
        </>
      )}

      {/* ── 动态 Agent 建议 ── */}
      {issue.suggestedAgent && (
        <Alert
          type="info"
          showIcon
          message={`建议新增 Agent: ${issue.suggestedAgent.agentName}`}
          description={issue.suggestedAgent.reason}
          style={{ marginTop: 20 }}
        />
      )}
      </div>
    </Modal>
  );
};

export default React.memo(ReasoningDrawer);
