import React from 'react';
import { Layout, Space } from 'antd';
import { MenuUnfoldOutlined, MenuFoldOutlined } from '@ant-design/icons';

import { useHeaderStyle } from './style';
import { HeaderBreadcrumb } from './HeaderComponents/HeaderBreadcrumb';
import { HeaderToolbar } from './HeaderComponents/HeaderToolbar';

const { Header: AntHeader } = Layout;

interface HeaderProps {
   collapsed: boolean;
   onToggle: () => void;
   isMobile: boolean;
}

export const Header: React.FC<HeaderProps> = ({
   collapsed,
   onToggle,
   isMobile,
}) => {
   const { styles } = useHeaderStyle();

   return (
      <AntHeader className={styles.header}>
         <Space size='middle' align='center' className={styles.headerLeft}>
            {!isMobile && (
               <div className={styles.trigger} onClick={onToggle}>
                  {collapsed ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />}
               </div>
            )}

            <HeaderBreadcrumb />
         </Space>

         <HeaderToolbar isMobile={isMobile} />
      </AntHeader>
   );
};
