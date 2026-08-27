import React from 'react';
import { Tag, Button, Dropdown, type MenuProps, Modal } from 'antd';
import { MoreOutlined, EyeOutlined, DownloadOutlined, EditOutlined, DeleteOutlined, CheckCircleOutlined, CloseCircleOutlined } from '@ant-design/icons';
import type { KnowledgeFile } from '../api/types';

interface MobileFileCardProps {
  file: KnowledgeFile;
  categoryMap: Record<string, string>;
  categoryColorMap: Record<string, string>;
  onView: (file: KnowledgeFile) => void;
  onDownload: (file: KnowledgeFile) => void;
  onEdit: (file: KnowledgeFile) => void;
  onDelete: (file: KnowledgeFile) => void;
  onStatusChange: (file: KnowledgeFile) => void;
}

const scopeMap: Record<string, string> = {
  procurement: '采购类',
  engineering: '工程类',
  general: '通用',
};

export const MobileFileCard: React.FC<MobileFileCardProps> = ({
  file,
  categoryMap,
  categoryColorMap,
  onView,
  onDownload,
  onEdit,
  onDelete,
  onStatusChange,
}) => {
  const moreMenuItems: MenuProps['items'] = [
    {
      key: 'status',
      icon: file.status === 1 ? <CloseCircleOutlined /> : <CheckCircleOutlined />,
      label: file.status === 1 ? '停用' : '启用',
      onClick: () => onStatusChange(file),
    },
    {
      key: 'download',
      icon: <DownloadOutlined />,
      label: '下载',
      onClick: () => onDownload(file),
    },
    {
      key: 'edit',
      icon: <EditOutlined />,
      label: '编辑',
      onClick: () => onEdit(file),
    },
    {
      type: 'divider',
    },
    {
      key: 'delete',
      icon: <DeleteOutlined />,
      label: '删除',
      danger: true,
      onClick: () => {
        Modal.confirm({
          title: '确定要删除该文件吗？',
          content: '删除后将无法参与审核，且不可恢复，请谨慎操作！',
          okText: '确认删除',
          cancelText: '取消',
          okButtonProps: { danger: true },
          onOk: () => onDelete(file),
        });
      },
    },
  ];

  return (
    <div className="mobile-file-card">
      <div className="mobile-file-card-header">
        <div className="mobile-file-card-title-row">
          <span className="mobile-file-card-name">{file.fileName}</span>
          <div className="mobile-file-card-badges">
            <Tag color={categoryColorMap[file.category]} className="mobile-file-card-category">
              {categoryMap[file.category]}
            </Tag>
            <Tag 
              color={file.status === 1 ? 'success' : 'default'}
              className="mobile-file-card-status"
            >
              {file.status === 1 ? '启用' : '停用'}
            </Tag>
          </div>
        </div>
      </div>
      <div className="mobile-file-card-info">
        <span className="mobile-file-card-meta">
          {file.uploadTime} · {scopeMap[file.applicableScope]} · {file.uploadUserName}
        </span>
      </div>
      <div className="mobile-file-card-actions">
        <Button 
          type="link" 
          size="small"
          icon={<EyeOutlined />}
          onClick={() => onView(file)}
          className="mobile-file-card-view-btn"
        >
          查看
        </Button>
        <Dropdown
          menu={{ items: moreMenuItems }}
          trigger={['click']}
          placement="bottomRight"
        >
          <Button 
            type="text" 
            size="small"
            icon={<MoreOutlined />}
            className="mobile-file-card-more-btn"
          />
        </Dropdown>
      </div>
    </div>
  );
};
