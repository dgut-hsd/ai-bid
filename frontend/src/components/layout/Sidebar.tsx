import React from 'react';
import { Layout, Menu, Typography } from 'antd';
import { useNavigate, useLocation } from 'react-router-dom';
import { useSelector } from 'react-redux';
import {
   FileSearch,
   BookOpen,
   Users,
   Building2,
} from 'lucide-react';

import { useSidebarStyle } from './style';
import type { RootState } from '../../store';
import { isCurrentTenantOwner } from '../../features/enterprise/access';
import bidAuditLogo from '../../assets/bid-audit.svg';

const { Sider } = Layout;

const BASE_NAV_ITEMS = [
   { key: '/bidReview', icon: <FileSearch size={18} />, label: '招标文件' },
   { key: '/library', icon: <BookOpen size={18} />, label: '标准库管理' },
];

const PLATFORM_ITEM = {
   key: '/platform/enterprises',
   icon: <Building2 size={18} />,
   label: '系统管理',
};

const ENTERPRISE_ITEM = {
   key: '/enterprise/users',
   icon: <Users size={18} />,
   label: '企业管理',
};

/**
 * 平台管理员看到「系统管理」，企业 OWNER 看到「企业管理」；两者可同时出现，普通成员都不显示。
 */
function useNavItems() {
   const { tenantList, currentTenantId, isPlatformAdmin } = useSelector(
      (state: RootState) => state.auth
   );
   const items = [];
   if (isPlatformAdmin) {
      items.push(PLATFORM_ITEM);
   }
   // 业务菜单依赖租户上下文：无任何租户的系统管理者只看「系统管理」。
   if (tenantList.length > 0) {
      items.push(...BASE_NAV_ITEMS);
      if (isCurrentTenantOwner(tenantList, currentTenantId)) {
         items.push(ENTERPRISE_ITEM);
      }
   }
   return items;
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
      if (pathname.startsWith('/platform')) {
         return ['/platform/enterprises'];
      }
      if (pathname.startsWith('/enterprise')) {
         return ['/enterprise/users'];
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