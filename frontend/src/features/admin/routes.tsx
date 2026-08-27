import { lazy } from 'react';
import type { RouteObject } from 'react-router-dom';

const AdminUsersPage = lazy(() =>
   import('./pages/AdminUsersPage').then((m) => ({ default: m.AdminUsersPage }))
);

export const adminRoutes: RouteObject[] = [
  {
    path: 'admin/users',
    element: <AdminUsersPage />,
  },
];