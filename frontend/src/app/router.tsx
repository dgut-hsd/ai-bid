import { Suspense } from 'react';
import { createBrowserRouter, Navigate } from 'react-router-dom';
import MainLayout from '../components/layout/MainLayout';
import { Loading } from '../components/Loading/Loading';
import { RouteGuard } from './RouteGuard';
import { TenantGuard } from './TenantGuard';
import { AdminGuard } from './AdminGuard';

import { loginRoutes } from '../features/login/routes';
import { uploadRoutes } from '../features/bidUpload/routes';
import { bidAuditRoutes } from '../features/bidAudit/routes';
import { libraryRoutes } from '../features/bidLibrary/routes';
import { adminRoutes } from '../features/admin/routes';

// basename 跟随 Vite 的 base 配置：base='/aibid/' 时 import.meta.env.BASE_URL 自动等于 '/aibid/'，
// 路由据此识别子路径前缀；base='/'（本地开发）时 basename 为 undefined（根路径）。
const basename =
   import.meta.env.BASE_URL && import.meta.env.BASE_URL !== '/'
      ? import.meta.env.BASE_URL.replace(/\/+$/, '')
      : undefined;

export const router = createBrowserRouter([
   {
      path: '/login',
      element: (
         <Suspense fallback={<Loading loading fullScreen />}>
            <RouteGuard requireAuth={false}>{loginRoutes[0].element}</RouteGuard>
         </Suspense>
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
         { index: true, element: <Navigate to='/bidReview' replace /> },

         // 系统管理 — 仅企业 OWNER 可访问
         {
            element: <AdminGuard />,
            children: adminRoutes,
         },

         // 业务路由 — 需要租户上下文
         {
            element: <TenantGuard />,
            children: [
               // 工作台与审核列表已合并为「招标文件」主工作区，旧工作台路径重定向到新列表
               { path: 'dashboard', element: <Navigate to='/bidReview' replace /> },
               ...uploadRoutes,
               ...bidAuditRoutes,
               ...libraryRoutes,
            ],
         },
      ],
   },
], basename ? { basename } : {});
