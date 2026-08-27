import React from 'react';
import { Tag, Button, Dropdown, Modal } from 'antd';
import type { MenuProps } from 'antd';
import {
   MoreOutlined,
   EyeOutlined,
   DownloadOutlined,
   EditOutlined,
   DeleteOutlined,
} from '@ant-design/icons';
import { useStyles } from '../style';
import {
   CategoryMap,
   ApplicableScopeMap,
   type KnowledgeFile,
} from '../types';

interface MobileFileCardProps {
   file: KnowledgeFile;
   onView: (file: KnowledgeFile) => void;
   onDownload: (file: KnowledgeFile) => void;
   onEdit: (file: KnowledgeFile) => void;
   onDelete: (file: KnowledgeFile) => void;
}

const CATEGORY_TAG_COLORS: Record<string, string> = {
   regulation: 'green',
   price: 'blue',
   supplier: 'orange',
   contract: 'purple',
   case: 'cyan',
   other: 'default',
};

export const MobileFileCard: React.FC<MobileFileCardProps> = ({
   file,
   onView,
   onDownload,
   onEdit,
   onDelete,
}) => {
   const { styles } = useStyles();
   const enabled = file.status === 1;

   const moreItems: MenuProps['items'] = [
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
      <div className={styles.mobileCard}>
         <div className={styles.mobileCardHeader}>
            <span className={styles.mobileCardName}>{file.fileName}</span>
            <div className={styles.mobileCardBadges}>
               <Tag
                  color={CATEGORY_TAG_COLORS[file.category] || 'default'}
                  style={{ margin: 0 }}
               >
                  {CategoryMap[file.category]}
               </Tag>
               <Tag
                  color={enabled ? 'success' : 'default'}
                  style={{ margin: 0 }}
               >
                  {enabled ? '启用' : '停用'}
               </Tag>
            </div>
         </div>

         <div className={styles.mobileCardMeta}>
            {file.uploadTime} ·{' '}
            {ApplicableScopeMap[file.applicableScope] || '通用'} ·{' '}
            {file.uploadUserName || '-'}
         </div>

         <div className={styles.mobileCardActions}>
            <Button
               type='link'
               size='small'
               icon={<EyeOutlined />}
               onClick={() => onView(file)}
            >
               查看
            </Button>
            <Dropdown
               menu={{ items: moreItems }}
               trigger={['click']}
               placement='bottomRight'
            >
               <Button type='text' size='small' icon={<MoreOutlined />} />
            </Dropdown>
         </div>
      </div>
   );
};