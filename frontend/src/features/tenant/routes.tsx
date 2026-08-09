import type { RouteObject } from 'react-router-dom';
import { TenantManagePage } from './pages/TenantManagePage';

export const tenantRoutes: RouteObject[] = [
   {
      path: 'tenant-manage',
      element: <TenantManagePage />,
   },
];
