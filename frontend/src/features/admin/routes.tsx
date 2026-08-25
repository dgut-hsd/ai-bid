import type { RouteObject } from 'react-router-dom';
import { AdminUsersPage } from './pages/AdminUsersPage';

export const adminRoutes: RouteObject[] = [
  {
    path: 'admin/users',
    element: <AdminUsersPage />,
  },
];