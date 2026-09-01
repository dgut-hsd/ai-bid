import React, { useEffect, useState } from 'react';
import { Typography, Space, Button } from 'antd';
import { LinkOutlined, DownOutlined, UpOutlined } from '@ant-design/icons';
import type { Citation } from '@/types/audit';

const { Link } = Typography;

interface CitationListProps {
  citations?: Citation[];
}

/** 折叠状态下最多展示的条数 */
const COLLAPSED_COUNT = 5;

/**
 * 搜索来源引用列表 — 对齐 Rust Citation。
 * 将结构化引用渲染为可点击的外部链接；超过 {@link COLLAPSED_COUNT} 条默认折叠，可展开/收起。
 */
const CitationList: React.FC<CitationListProps> = ({ citations }) => {
  const [expanded, setExpanded] = useState(false);

  // 切换 issue 时重置折叠状态，避免上一个 issue 的展开状态残留。
  useEffect(() => {
    setExpanded(false);
  }, [citations]);

  if (!citations || citations.length === 0) return null;

  const hasMore = citations.length > COLLAPSED_COUNT;
  const visible = expanded || !hasMore ? citations : citations.slice(0, COLLAPSED_COUNT);

  return (
    <div style={{ marginTop: 8 }}>
      <div style={{ fontSize: 12, color: '#8c8c8c', marginBottom: 4 }}>
        <LinkOutlined /> 搜索来源引用
      </div>
      <Space direction="vertical" size={2} style={{ width: '100%' }}>
        {visible.map((c, idx) => (
          <div key={idx} style={{ fontSize: 12 }}>
            <Link
              href={c.url}
              target="_blank"
              rel="noopener noreferrer"
              style={{ fontSize: 12 }}
            >
              {c.title}
            </Link>
            {c.siteName && (
              <span style={{ color: '#999', marginLeft: 4 }}>({c.siteName})</span>
            )}
          </div>
        ))}
      </Space>
      {hasMore && (
        <Button
          type="link"
          size="small"
          style={{ padding: 0, height: 'auto', fontSize: 12 }}
          onClick={() => setExpanded((v) => !v)}
        >
          {expanded ? (
            <>
              <UpOutlined /> 收起
            </>
          ) : (
            <>
              <DownOutlined /> 展开全部（{citations.length} 条）
            </>
          )}
        </Button>
      )}
    </div>
  );
};

export default React.memo(CitationList);