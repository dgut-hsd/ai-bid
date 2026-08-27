import React from 'react';
import { Tag, Button, Select, Popconfirm } from 'antd';
import {
   EditOutlined,
   KeyOutlined,
   UserDeleteOutlined,
   PauseCircleOutlined,
   PlayCircleOutlined,
} from '@ant-design/icons';
import { useStyles } from '../style';
import type { EnterpriseUser, EnterpriseRole } from '../types';

const ROLE_LABEL: Record<string, { text: string; color: string }> = {
   OWNER: { text: '拥有者', color: 'gold' },
   ADMIN: { text: '管理员', color: 'blue' },
   MEMBER: { text: '成员', color: 'default' },
};

const SELECTABLE_ROLES: EnterpriseRole[] = ['ADMIN', 'MEMBER'];

const ROLE_OPTIONS = SELECTABLE_ROLES.map((role) => ({
   value: role,
   label: ROLE_LABEL[role]?.text ?? role,
}));

interface EnterpriseUserCardProps {
   user: EnterpriseUser;
   roleUpdating: boolean;
   onEdit: (user: EnterpriseUser) => void;
   onResetPassword: (user: EnterpriseUser) => void;
   onToggleStatus: (user: EnterpriseUser) => void;
   onRemove: (user: EnterpriseUser) => void;
   onChangeRole: (user: EnterpriseUser, role: EnterpriseRole) => void;
}

export const EnterpriseUserCard: React.FC<EnterpriseUserCardProps> = ({
   user,
   roleUpdating,
   onEdit,
   onResetPassword,
   onToggleStatus,
   onRemove,
   onChangeRole,
}) => {
   const { styles } = useStyles();

   const roleUpper = user.role?.toUpperCase();
   const roleMeta = ROLE_LABEL[roleUpper] ?? { text: user.role || '-', color: 'default' };
   const canManage = roleUpper !== 'OWNER';
   const suspended = user.status === 'SUSPENDED';
   const displayName = user.real_name || user.username;
   const createdAt = user.created_at
      ? new Date(user.created_at).toLocaleString('zh-CN')
      : '-';

   return (
      <div className={styles.mobileCard}>
         <div className={styles.mobileCardHeader}>
            <span className={styles.mobileCardTitle}>{displayName}</span>
            <div className={styles.mobileCardBadges}>
               {canManage ? (
                  <Select
                     size='small'
                     style={{ width: 96 }}
                     value={user.role}
                     options={ROLE_OPTIONS}
                     disabled={roleUpdating}
                     onChange={(next) =>
                        onChangeRole(user, next as EnterpriseRole)
                     }
                  />
               ) : (
                  <Tag color={roleMeta.color} style={{ margin: 0 }}>
                     {roleMeta.text}
                  </Tag>
               )}
               <Tag
                  color={suspended ? 'orange' : 'green'}
                  style={{ margin: 0 }}
               >
                  {suspended ? '已暂停' : '正常'}
               </Tag>
            </div>
         </div>

         <div className={styles.mobileCardMeta}>
            <div className={styles.mobileCardMetaRow}>
               <span>账号：{user.username}</span>
            </div>
            <div className={styles.mobileCardMetaRow}>
               <span>创建：{createdAt}</span>
            </div>
         </div>

         <div className={styles.mobileCardActions}>
            <Button
               type='link'
               size='small'
               icon={<EditOutlined />}
               onClick={() => onEdit(user)}
            >
               编辑
            </Button>
            <Button
               type='link'
               size='small'
               icon={<KeyOutlined />}
               onClick={() => onResetPassword(user)}
            >
               重置密码
            </Button>
            {canManage && (
               <>
                  <Button
                     type='link'
                     size='small'
                     danger={suspended}
                     icon={suspended ? <PlayCircleOutlined /> : <PauseCircleOutlined />}
                     onClick={() => onToggleStatus(user)}
                  >
                     {suspended ? '恢复' : '暂停'}
                  </Button>
                  <Popconfirm
                     title='移出企业'
                     description='该用户将离开本企业，不影响其账号在其他企业的身份。'
                     okText='移出'
                     cancelText='取消'
                     okButtonProps={{ danger: true }}
                     onConfirm={() => onRemove(user)}
                  >
                     <Button
                        type='link'
                        size='small'
                        danger
                        icon={<UserDeleteOutlined />}
                     >
                        移出
                     </Button>
                  </Popconfirm>
               </>
            )}
         </div>
      </div>
   );
};