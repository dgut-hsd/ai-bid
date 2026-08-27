import React from 'react';
import { App, Button, Dropdown, Tag } from 'antd';
import type { MenuProps } from 'antd';
import {
   DownOutlined,
   EyeOutlined,
   DeleteOutlined,
   UploadOutlined,
} from '@ant-design/icons';
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

/**
 * 移动端招标文件卡片 —— 只保留核心信息：
 * 第一行：项目名称（过长省略）；第二行：审核状态 + 上传时间 + 操作按钮。
 */
export const AuditCard: React.FC<AuditCardProps> = ({
   record,
   deleting,
   onView,
   onDelete,
   styles,
}) => {
   const navigate = useNavigate();
   const { modal } = App.useApp();
   const uploadTime = record.uploadTime
      ? dayjs(record.uploadTime).format('YYYY-MM-DD')
      : '-';

   const menuItems: MenuProps['items'] = [
      { key: 'view', icon: <EyeOutlined />, label: '查看' },
      { key: 'upload', icon: <UploadOutlined />, label: '上传新版本' },
      { type: 'divider' },
      { key: 'delete', icon: <DeleteOutlined />, danger: true, label: '删除' },
   ];

   const handleMenuClick: MenuProps['onClick'] = ({ key }) => {
      if (key === 'view') {
         onView(record.projectId);
      } else if (key === 'upload') {
         navigate(`/upload/${record.projectId}`);
      } else if (key === 'delete') {
         modal.confirm({
            title: '确定要删除该项目吗？',
            content: '删除后将同步移除该项目下全部版本与审核记录，且不可恢复。',
            okText: '确认删除',
            okButtonProps: { danger: true, loading: deleting },
            cancelText: '取消',
            onOk: () => {
               onDelete(record.projectId);
            },
         });
      }
   };

   return (
      <div className={styles.auditCard} onClick={() => onView(record.projectId)}>
         <div className={styles.auditCardHeader}>
            <span className={styles.auditCardName} title={record.bidName || '-'}>
               {record.bidName || '-'}
            </span>
            <Tag className={styles.auditCardVersion}>V{record.version}</Tag>
         </div>

         <div className={styles.auditCardFooter}>
            <StatusTag parseStatus={record.parseStatus} />

            <span className={styles.auditCardTime}>{uploadTime}</span>

            <span onClick={(e) => e.stopPropagation()}>
               <Dropdown
                  menu={{ items: menuItems, onClick: handleMenuClick }}
                  trigger={['click']}
               >
                  <Button size='small'>
                     操作 <DownOutlined />
                  </Button>
               </Dropdown>
            </span>
         </div>
      </div>
   );
};