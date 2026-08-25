import React from 'react';
import { Layout, Menu, Typography } from 'antd';
import { useNavigate, useLocation } from 'react-router-dom';
import { useSelector } from 'react-redux';
import {
   LayoutDashboard,
   FileSearch,
   BookOpen,
   Building2,
} from 'lucide-react';

import { useSidebarStyle } from './style';
import type { RootState } from '../../store';
import { canAccessTenantManage } from '../../features/tenant/access';
import bidAuditLogo from '../../assets/bid-audit.svg';

const { Sider } = Layout;

const BASE_NAV_ITEMS = [
   { key: '/dashboard', icon: <LayoutDashboard size={18} />, label: '工作台' },
   { key: '/bidReview', icon: <FileSearch size={18} />, label: '审核列表' },
   { key: '/library', icon: <BookOpen size={18} />, label: '标准库管理' },
];

const TENANT_MANAGE_ITEM = {
   key: '/tenant-manage',
   icon: <Building2 size={18} />,
   label: '租户管理',
};

/**
 * 只有租户拥有者/管理员（或尚未加入任何租户、需要创建首个租户的用户）
 * 才能看到「租户管理」入口；普通成员/审核/只读用户隐藏。
 */
function useNavItems() {
   const tenantList = useSelector((state: RootState) => state.auth.tenantList);
   return canAccessTenantManage(tenantList)
      ? [...BASE_NAV_ITEMS, TENANT_MANAGE_ITEM]
      : BASE_NAV_ITEMS;
}

export const Sidebar: React.FC<{ collapsed: boolean }> = ({ collapsed }) => {
   const { styles, theme } = useSidebarStyle();
   const navigate = useNavigate();
   const location = useLocation();
   const navItems = useNavItems();

   const getSelectedKeys = () => {
      const { pathname } = location;

      if (pathname.startsWith('/bidReview')) {
         return ['/bidReview'];
      }
      if (pathname.startsWith('/tenant-manage')) {
         return ['/tenant-manage'];
      }
      return [pathname];
   };

   return (
      <Sider
         trigger={null}
         collapsible
         collapsed={collapsed}
         collapsedWidth={60}
         width={190}
         className={styles.sider}
      >
         {/* Logo 区域 */}
         <div
            style={{
               height: '6vh',
               minHeight: '50px',
               maxHeight: '60px',
               display: 'flex',
               alignItems: 'center',
               justifyContent: 'center',
               overflow: 'hidden',
               gap: '8px',
               borderBottom: `1px solid ${theme.colorBorderSecondary}`,
            }}
         >
            <img 
               src={bidAuditLogo} 
               width='20' 
               height='20' 
               alt='智能标书审核系统'
            />

            {!collapsed && (
               <Typography.Title
                  level={4}
                  style={{
                     color: theme.colorPrimary,
                     margin: 0,
                     whiteSpace: 'nowrap',
                     fontSize: '1.5rem',
                  }}
               >
                  智能标书审核系统
               </Typography.Title>
            )}
         </div>

         <Menu
            mode='inline'
            selectedKeys={getSelectedKeys()}
            items={navItems}
            onClick={({ key }) => navigate(key)}
            className={styles.menu}
            style={{ fontSize: '1.3rem' }}
         />
      </Sider>
   );
};

export const MobileBottomNav: React.FC = () => {
   const { styles } = useSidebarStyle();
   const navigate = useNavigate();
   const location = useLocation();
   const navItems = useNavItems();

   return (
      <div className={styles.mobileNav}>
         {navItems.map((item) => {
            const isActive = location.pathname === item.key;
            return (
               <div
                  key={item.key}
                  className={`${styles.mobileNavItem} ${
                     isActive ? 'active' : ''
                  }`}
                  onClick={() => navigate(item.key)}
               >
                  {React.cloneElement(item.icon, { size: 20 })}
                  <span>{item.label}</span>
               </div>
            );
         })}
      </div>
   );
};