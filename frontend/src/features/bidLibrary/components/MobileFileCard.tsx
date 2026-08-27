import React from 'react';
import {
   Tag,
   Button,
   Dropdown,
   Modal,
   type MenuProps,
} from 'antd';
import {
   MoreOutlined,
   EyeOutlined,
   DownloadOutlined,
   EditOutlined,
   DeleteOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';

import {
   CategoryMap,
   ApplicableScopeMap,
   type KnowledgeFile,
} from '../types';

interface MobileFileCardProps {
   file: KnowledgeFile;
   styles: Record<string, string>;
   onView: (file: KnowledgeFile) => void;
   onDownload: (file: KnowledgeFile) => void;
   onEdit: (file: KnowledgeFile) => void;
   onDelete: (file: KnowledgeFile) => void;
}

// 分类标签颜色（与桌面表格 Tag 配色保持一致）
const categoryColors: Record<string, string> = {
   regulation: 'green',
   price: 'blue',
   supplier: 'orange',
   contract: 'purple',
   case: 'cyan',
   other: 'default',
};

/**
 * 移动端标准库文件紧凑卡片：
 * 第一行 文件名 + 分类标签；第二行 适用范围·上传人·时间；第三行 状态 + 操作。
 */
export const MobileFileCard: React.FC<MobileFileCardProps> = ({
   file,
   styles,
   onView,
   onDownload,
   onEdit,
   onDelete,
}) => {
   const uploadTime = file.uploadTime
      ? dayjs(file.uploadTime).format('YYYY-MM-DD')
      : '-';
   const enabled = file.status === 1;

   const menuItems: MenuProps['items'] = [
      { key: 'view', icon: <EyeOutlined />, label: '查看', onClick: () => onView(file) },
      { key: 'download', icon: <DownloadOutlined />, label: '下载', onClick: () => onDownload(file) },
      { key: 'edit', icon: <EditOutlined />, label: '编辑', onClick: () => onEdit(file) },
      { type: 'divider' },
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
      <div className={styles.mobileCard} onClick={() => onView(file)}>
         <div className={styles.mobileCardHeader}>
            <span className={styles.mobileCardName}>{file.fileName}</span>
            <Tag
               color={categoryColors[file.category] ?? 'default'}
               className={styles.mobileCardCategory}
            >
               {CategoryMap[file.category]}
            </Tag>
         </div>

         <div className={styles.mobileCardMeta}>
            {ApplicableScopeMap[file.applicableScope] || '通用'} ·{' '}
            {file.uploadUserName || '-'} · {uploadTime}
         </div>

         <div className={styles.mobileCardFooter}>
            <Tag
               color={enabled ? 'success' : 'default'}
               className={styles.mobileCardStatus}
            >
               {enabled ? '启用' : '停用'}
            </Tag>

            <span onClick={(e) => e.stopPropagation()}>
               <Dropdown
                  menu={{ items: menuItems }}
                  trigger={['click']}
                  placement='bottomRight'
               >
                  <Button size='small' type='text' className={styles.mobileCardMore}>
                     操作 <MoreOutlined />
                  </Button>
               </Dropdown>
            </span>
         </div>
      </div>
   );
};