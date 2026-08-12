import React from 'react';
import { Typography, Spin, Button } from 'antd';
import {
  LoadingOutlined,
  CheckCircleOutlined,
  CloseCircleOutlined,
  ArrowRightOutlined,
} from '@ant-design/icons';

const { Text } = Typography;

interface PipelineProgressProps {
  currentStage: string;
  isComplete: boolean;
  /** 审核完成后手动跳转到「审核结果」的回调（不传则不显示入口） */
  onViewResults?: () => void;
}

/** 将 backend stage 映射为用户可读的简短描述 */
const stageLabel = (stage: string, isComplete: boolean): string => {
  if (isComplete) return '审核完成';
  if (!stage) return '等待开始';
  const upper = stage.toUpperCase();
  if (upper.includes('UPLOAD')) return '正在解析文档…';
  if (upper.includes('EXTRACT') || upper.includes('DOC')) return '正在提取文档内容…';
  if (upper.includes('REVIEW') || upper.includes('审')) return '正在智能分析中，请耐心等待…';
  if (upper.includes('SUMM') || upper.includes('汇总')) return '正在汇总结果…';
  if (upper.includes('PEND') || upper.includes('创建')) return '正在准备…';
  return stage;
};

/**
 * 审核进度指示 — 后端审核是同步阻塞调用，无细粒度进度，
 * 因此使用不间断旋转动画表示"进行中"。
 */
const PipelineProgress: React.FC<PipelineProgressProps> = ({ currentStage, isComplete, onViewResults }) => {
  const label = stageLabel(currentStage, isComplete);

  // 审核完成：不自动跳页，改为在「审核过程」页给一条醒目的完成横幅 + 手动入口，
  // 用户可以继续看当前卡片，想看结果时自己点。
  if (isComplete) {
    return (
      <div
        style={{
          padding: '12px 14px',
          marginBottom: 6,
          display: 'flex',
          alignItems: 'center',
          gap: 10,
          background: '#f6ffed',
          border: '1px solid #b7eb8f',
          borderRadius: 8,
        }}
      >
        <CheckCircleOutlined style={{ color: '#52c41a', fontSize: 26 }} />
        <Text strong style={{ fontSize: 20, lineHeight: 1.2, color: '#389e0d', letterSpacing: 1 }}>
          {label}
        </Text>
        {onViewResults && (
          <Button
            type="link"
            size="small"
            style={{ marginLeft: 'auto', padding: 0, color: '#389e0d' }}
            onClick={onViewResults}
          >
            查看审核结果 <ArrowRightOutlined />
          </Button>
        )}
      </div>
    );
  }

  // 审核失败的情况（由使用方判断是否需要特殊样式，这里保守处理）
  if (currentStage && currentStage.toUpperCase().includes('FAIL')) {
    return (
      <div style={{ padding: '6px 0', marginBottom: 4, display: 'flex', alignItems: 'center', gap: 6 }}>
        <CloseCircleOutlined style={{ color: '#f5222d', fontSize: 14 }} />
        <Text type="danger" style={{ fontSize: 13 }}>{label}</Text>
      </div>
    );
  }

  return (
    <div style={{ padding: '6px 0', marginBottom: 4, display: 'flex', alignItems: 'center', gap: 8 }}>
      <Spin indicator={<LoadingOutlined style={{ fontSize: 14 }} spin />} />
      <Text style={{ fontSize: 13 }}>{label}</Text>
    </div>
  );
};

export default React.memo(PipelineProgress);
