import React from 'react';
import { Tag, Button, Popconfirm } from 'antd';
import { EyeOutlined, DeleteOutlined, UploadOutlined } from '@ant-design/icons';
import { useNavigate } from 'react-router-dom';
import dayjs from 'dayjs';
import { StatusTag } from './StatusTag';
import type { ProjectItem } from '../types';

interface AuditCardProps {
   record: ProjectItem;
   deleting: boolean;
   onView: (projectId: number) => void;
   onDelete: (projectId: number) => void;
   styles: Record<string, string>;
}

export const AuditCard: React.FC<AuditCardProps> = ({
   record,
   deleting,
   onView,
   onDelete,
   styles,
}) => {
   const navigate = useNavigate();
   const uploadDate = record.uploadTime
      ? dayjs(record.uploadTime).format('YYYY-MM-DD HH:mm')
      : '-';

   return (
      <div className={styles.auditCard} onClick={() => onView(record.projectId)}>
         <div className={styles.auditCardHeader}>
            <span className={styles.auditCardName}>{record.bidName || '-'}</span>
            <div className={styles.auditCardBadges}>
               <StatusTag
                  parseStatus={record.parseStatus}
                  auditResult={record.auditResult}
               />
               <Tag style={{ margin: 0 }}>V{record.version}</Tag>
            </div>
         </div>

         <div className={styles.auditCardMeta}>
            <span>审核人：{record.auditorName || '-'}</span>
            <span>上传：{uploadDate}</span>
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