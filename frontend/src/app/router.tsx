import { createBrowserRouter, Navigate } from 'react-router-dom';
import MainLayout from '../components/layout/MainLayout';
import { RouteGuard } from './RouteGuard';

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
         ...dashboardRoutes,
         ...uploadRoutes,
         ...bidAuditRoutes,
         ...libraryRoutes,
         ...tenantRoutes,
      ],
   },
]);
