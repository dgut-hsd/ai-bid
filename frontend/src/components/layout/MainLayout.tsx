import React, { useState, useEffect, Suspense } from 'react';
import { Layout } from 'antd';
import { Outlet } from 'react-router-dom';
import { Loading } from '@/components/Loading/Loading';
import { createStyles } from 'antd-style';
import { Sidebar, MobileBottomNav } from './Sidebar';
import { Header } from './Header';

const { Content } = Layout;

const useStyles = createStyles(({ token, css }) => ({
   layout: css`
      min-height: 100vh;
      background: ${token.colorBgLayout};
   `,
   content: css`
      background: ${token.colorBgContainer};
      border-radius: ${token.borderRadiusLG}px;
      padding: 1rem;
      height: 100%;
      overflow-x: clip;
      box-shadow: ${token.colorBgContainer === '#FFFFFF'
         ? '0 2px 8px rgba(0,0,0,0.02)'
         : 'none'};
   `,
}));

const MainLayout: React.FC = () => {
   const { styles } = useStyles();
   const [collapsed, setCollapsed] = useState(false);
   const [isMobile, setIsMobile] = useState(window.innerWidth <= 768);

   useEffect(() => {
      const handleResize = () => {
         const mobileState = window.innerWidth <= 768;
         setIsMobile(mobileState);
         if (mobileState) {
            setCollapsed(true);
         }
      };

      window.addEventListener('resize', handleResize);
      return () => window.removeEventListener('resize', handleResize);
   }, []);

   return (
      <Layout className={styles.layout}>
         {!isMobile && <Sidebar collapsed={collapsed} />}

         <Layout style={{ paddingBottom: isMobile ? '60px' : '0' }}>
            <Header
               collapsed={collapsed}
               onToggle={() => setCollapsed(!collapsed)}
               isMobile={isMobile}
            />

            <Content style={{ margin: 12 }}>
               <div className={styles.content}>
                  <Suspense fallback={<Loading loading />}>
                     <Outlet />
                  </Suspense>
               </div>
            </Content>

            {isMobile && <MobileBottomNav />}
         </Layout>
      </Layout>
   );
};

export default MainLayout;
