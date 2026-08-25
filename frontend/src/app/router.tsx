import { createBrowserRouter, Navigate } from 'react-router-dom';
import MainLayout from '../components/layout/MainLayout';
import { RouteGuard } from './RouteGuard';
import { TenantGuard } from './TenantGuard';
import { AdminGuard } from './AdminGuard';

import { loginRoutes } from '../features/login/routes';
import { dashboardRoutes } from '../features/dashboard/routes';
import { uploadRoutes } from '../features/bidUpload/routes';
import { bidAuditRoutes } from '../features/bidAudit/routes';
import { libraryRoutes } from '../features/bidLibrary/routes';
import { adminRoutes } from '../features/admin/routes';

export const router = createBrowserRouter([
   {
      path: '/login',
      element: (
         <RouteGuard requireAuth={false}>{loginRoutes[0].element}</RouteGuard>
      ),
   },

   {
      path: '/',
      element: (
         <RouteGuard requireAuth={true}>
            <MainLayout />
         </RouteGuard>
      ),
      children: [
         { index: true, element: <Navigate to='/dashboard' replace /> },

         // 系统管理 — 仅企业 OWNER 可访问
         {
            element: <AdminGuard />,
            children: adminRoutes,
         },

         // 业务路由 — 需要租户上下文
         {
            element: <TenantGuard />,
            children: [
               ...dashboardRoutes,
               ...uploadRoutes,
               ...bidAuditRoutes,
               ...libraryRoutes,
            ],
         },
      ],
   },
]);
