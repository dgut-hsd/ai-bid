import React from 'react';
import { Tag, Button, Dropdown } from 'antd';
import type { MenuProps } from 'antd';
import {
   MoreOutlined,
   EditOutlined,
   KeyOutlined,
   UserDeleteOutlined,
} from '@ant-design/icons';
import { useStyles } from '../style';
import type { AdminUser, AdminRole } from '../types';

const ROLE_LABEL: Record<AdminRole, { text: string; color: string }> = {
   OWNER: { text: '拥有者', color: 'gold' },
   MEMBER: { text: '成员', color: 'default' },
};

interface UserCardProps {
   user: AdminUser;
   onEdit: (user: AdminUser) => void;
   onReset: (user: AdminUser) => void;
   onRemove: (user: AdminUser) => void;
}

export const UserCard: React.FC<UserCardProps> = ({
   user,
   onEdit,
   onReset,
   onRemove,
}) => {
   const { styles } = useStyles();
   const role = ROLE_LABEL[user.role] ?? { text: user.role, color: 'default' };
   const active = user.status === 'ACTIVE';

   const menuItems: MenuProps['items'] = [
      {
         key: 'reset',
         icon: <KeyOutlined />,
         label: '重置密码',
         onClick: () => onReset(user),
      },
      {
         key: 'remove',
         icon: <UserDeleteOutlined />,
         danger: true,
         label: '移除',
         onClick: () => onRemove(user),
      },
   ];

   return (
      <div className={styles.userCard}>
         <div className={styles.userCardHeader}>
            <span className={styles.userCardName}>
               {user.real_name || user.username}
            </span>
            <div className={styles.userCardBadges}>
               <Tag color={role.color} style={{ margin: 0 }}>
                  {role.text}
               </Tag>
               <Tag color={active ? 'green' : 'red'} style={{ margin: 0 }}>
                  {active ? '启用' : '停用'}
               </Tag>
            </div>
         </div>

         <div className={styles.userCardMeta}>
            <div>账号：{user.username}</div>
            {user.created_at && (
               <div>创建：{new Date(user.created_at).toLocaleString('zh-CN')}</div>
            )}
         </div>

         <div className={styles.userCardActions}>
            <Button
               type='link'
               size='small'
               icon={<EditOutlined />}
               onClick={() => onEdit(user)}
            >
               编辑
            </Button>
            <Dropdown
               menu={{ items: menuItems }}
               trigger={['click']}
               placement='bottomRight'
            >
               <Button type='text' size='small' icon={<MoreOutlined />} />
            </Dropdown>
         </div>
      </div>
   );
};