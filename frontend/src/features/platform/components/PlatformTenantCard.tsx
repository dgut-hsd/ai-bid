import React from 'react';
import { Tag, Button, Popconfirm, Typography } from 'antd';
import {
   TeamOutlined,
   SwapOutlined,
   ReloadOutlined,
   PauseCircleOutlined,
   DeleteOutlined,
} from '@ant-design/icons';
import { useStyles } from '../style';
import type { PlatformTenant } from '../types';

const STATUS_META: Record<string, { text: string; color: string }> = {
   ACTIVE: { text: '正常', color: 'green' },
   DISABLED: { text: '已停用', color: 'orange' },
   DELETED: { text: '已删除', color: 'red' },
};

export type TenantLifecycleAction = 'disable' | 'restore' | 'delete';

interface PlatformTenantCardProps {
   tenant: PlatformTenant;
   busy: boolean;
   onMembers: (tenant: PlatformTenant) => void;
   onTransfer: (tenant: PlatformTenant) => void;
   onLifecycle: (tenant: PlatformTenant, action: TenantLifecycleAction) => void;
}

export const PlatformTenantCard: React.FC<PlatformTenantCardProps> = ({
   tenant,
   busy,
   onMembers,
   onTransfer,
   onLifecycle,
}) => {
   const { styles } = useStyles();

   const statusMeta = STATUS_META[tenant.status] ?? {
      text: tenant.status,
      color: 'default',
   };
   const owner =
      tenant.owner_real_name && tenant.owner_username
         ? `${tenant.owner_real_name}（${tenant.owner_username}）`
         : tenant.owner_real_name || tenant.owner_username || '-';
   const createdAt = tenant.created_at
      ? new Date(tenant.created_at).toLocaleString('zh-CN')
      : '-';
   const deleted = tenant.status === 'DELETED';

   return (
      <div className={styles.mobileCard}>
         <div className={styles.mobileCardHeader}>
            <span className={styles.mobileCardTitle}>{tenant.name}</span>
            <div className={styles.mobileCardBadges}>
               <Tag color={statusMeta.color} style={{ margin: 0 }}>
                  {statusMeta.text}
               </Tag>
            </div>
         </div>

         <div className={styles.mobileCardMeta}>
            <div className={styles.mobileCardMetaRow}>
               <span>OWNER：{owner}</span>
            </div>
            <div className={styles.mobileCardMetaRow}>
               <span>编码：{tenant.tenant_code || '-'}</span>
               <span>成员：{tenant.member_count ?? 0}</span>
            </div>
            <div className={styles.mobileCardMetaRow}>
               <span>创建：{createdAt}</span>
            </div>
         </div>

         {deleted ? (
            <div className={styles.mobileCardActions}>
               <Typography.Text type='secondary'>-</Typography.Text>
            </div>
         ) : (
            <div className={styles.mobileCardActions}>
               <Button
                  type='link'
                  size='small'
                  icon={<TeamOutlined />}
                  onClick={() => onMembers(tenant)}
               >
                  成员
               </Button>
               <Button
                  type='link'
                  size='small'
                  icon={<SwapOutlined />}
                  onClick={() => onTransfer(tenant)}
               >
                  转移OWNER
               </Button>
               {tenant.status === 'ACTIVE' ? (
                  <Popconfirm
                     title='停用企业'
                     description='停用后该企业成员将无法登录访问。'
                     okText='停用'
                     cancelText='取消'
                     onConfirm={() => onLifecycle(tenant, 'disable')}
                  >
                     <Button
                        type='link'
                        size='small'
                        danger
                        loading={busy}
                        icon={<PauseCircleOutlined />}
                     >
                        停用
                     </Button>
                  </Popconfirm>
               ) : (
                  <Button
                     type='link'
                     size='small'
                     icon={<ReloadOutlined />}
                     loading={busy}
                     onClick={() => onLifecycle(tenant, 'restore')}
                  >
                     恢复
                  </Button>
               )}
               <Popconfirm
                  title='删除企业'
                  description='删除为软删除，请谨慎操作。'
                  okText='删除'
                  cancelText='取消'
                  okButtonProps={{ danger: true }}
                  onConfirm={() => onLifecycle(tenant, 'delete')}
               >
                  <Button
                     type='link'
                     size='small'
                     danger
                     icon={<DeleteOutlined />}
                  >
                     删除
                  </Button>
               </Popconfirm>
            </div>
         )}
      </div>
   );
};