import { createBrowserRouter, Navigate } from 'react-router-dom';
import MainLayout from '../components/layout/MainLayout';
import { RouteGuard } from './RouteGuard';
import { TenantGuard } from './TenantGuard';

import { loginRoutes } from '../features/login/routes';
import { dashboardRoutes } from '../features/dashboard/routes';
import { uploadRoutes } from '../features/bidUpload/routes';
import { bidAuditRoutes } from '../features/bidAudit/routes';
import { libraryRoutes } from '../features/bidLibrary/routes';
import { tenantRoutes } from '../features/tenant/routes';

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

         // 租户管理 — 无需租户上下文即可访问
         ...tenantRoutes,

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
