import React from 'react';
import { Button, Popconfirm } from 'antd';
import { EyeOutlined, DeleteOutlined, UploadOutlined } from '@ant-design/icons';
import { useNavigate } from 'react-router-dom';
import { StatusTag } from './StatusTag';
import type { ProjectItem } from '../types';

interface AuditCardProps {
   record: ProjectItem;
   deleting: boolean;
   onView: (projectId: number) => void;
   onDelete: (projectId: number) => void;
   styles: Record<string, string>;
}

/**
 * 移动端招标文件卡片 —— 只保留核心信息：
 * 项目名称 + 审核状态 + 操作按钮（查看 / 上传新版本 / 删除）。
 */
export const AuditCard: React.FC<AuditCardProps> = ({
   record,
   deleting,
   onView,
   onDelete,
   styles,
}) => {
   const navigate = useNavigate();

   return (
      <div className={styles.auditCard} onClick={() => onView(record.projectId)}>
         <div className={styles.auditCardHeader}>
            <span className={styles.auditCardName}>{record.bidName || '-'}</span>
            <span className={styles.auditCardStatus}>
               <StatusTag parseStatus={record.parseStatus} />
            </span>
         </div>

         <div className={styles.auditCardActions}>
            <Button
               type='link'
               size='small'
               icon={<EyeOutlined />}
               onClick={(e) => {
                  e.stopPropagation();
                  onView(record.projectId);
               }}
            >
               查看
            </Button>

            <Button
               type='link'
               size='small'
               icon={<UploadOutlined />}
               onClick={(e) => {
                  e.stopPropagation();
                  navigate(`/upload/${record.projectId}`);
               }}
            >
               上传新版本
            </Button>

            <Popconfirm
               title='确定要删除该项目吗？'
               description='删除后将同步移除该项目下全部版本与审核记录，且不可恢复。'
               okText='确认删除'
               cancelText='取消'
               okButtonProps={{ danger: true, loading: deleting }}
               onConfirm={(e) => {
                  e?.stopPropagation();
                  onDelete(record.projectId);
               }}
               onCancel={(e) => e?.stopPropagation()}
            >
               <Button
                  type='link'
                  danger
                  size='small'
                  icon={<DeleteOutlined />}
                  loading={deleting}
                  onClick={(e) => e.stopPropagation()}
               >
                  删除
               </Button>
            </Popconfirm>
         </div>
      </div>
   );
};