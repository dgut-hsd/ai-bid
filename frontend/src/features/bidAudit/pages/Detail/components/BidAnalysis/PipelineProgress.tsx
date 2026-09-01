import React from 'react';
import { Typography, Spin, Button } from 'antd';
import {
  LoadingOutlined,
  CheckCircleOutlined,
  CloseCircleOutlined,
  ArrowRightOutlined,
  ExclamationCircleOutlined,
} from '@ant-design/icons';

const { Text } = Typography;

interface PipelineProgressProps {
  currentStage: string;
  isComplete: boolean;
  /** 部分失败阶段名（如 evidence_verify），非空表示结果经过降级、未经完整核验 */
  failedStages?: string[];
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

/** 将失败阶段名映射为中文（与 Rust/Java failed_stages 阶段名对齐） */
const failedStageLabel = (stage: string): string => {
  switch (stage) {
    case 'evidence_verify': return '证据核验';
    case 'pipeline': return '审核管线超时';
    case 'execute': return '多智能体审查';
    case 'legal_verify': return '法条验证';
    case 'debate': return '对抗辩论';
    case 'batch_search': return '批量检索';
    case 'blind_spot': return '盲点扫描';
    default: return stage;
  }
};

/**
 * 审核进度指示 — 后端审核是同步阻塞调用，无细粒度进度，
 * 因此使用不间断旋转动画表示"进行中"。
 */
const PipelineProgress: React.FC<PipelineProgressProps> = ({ currentStage, isComplete, failedStages, onViewResults }) => {
  const label = stageLabel(currentStage, isComplete);

  // 审核完成：不自动跳页，改为在「审核过程」页给一条醒目的完成横幅 + 手动入口，
  // 用户可以继续看当前卡片，想看结果时自己点。
  if (isComplete) {
    const partialStages = (failedStages || []).filter(Boolean);
    return (
      <div>
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

        {partialStages.length > 0 && (
          <div
            style={{
              padding: '10px 14px',
              marginBottom: 6,
              display: 'flex',
              alignItems: 'flex-start',
              gap: 10,
              background: '#fffbe6',
              border: '1px solid #ffe58f',
              borderRadius: 8,
            }}
          >
            <ExclamationCircleOutlined style={{ color: '#faad14', fontSize: 18, marginTop: 2 }} />
            <div style={{ flex: 1 }}>
              <Text strong style={{ fontSize: 14, color: '#ad6800' }}>
                审核完成（部分核验未完成）
              </Text>
              <div style={{ fontSize: 12, color: '#ad6800', marginTop: 2, lineHeight: 1.6 }}>
                以下阶段未完成：{partialStages.map(failedStageLabel).join('、')}。部分风险发现未经独立核验即输出，结果仅供参考，建议人工复核后使用。
              </div>
            </div>
          </div>
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
