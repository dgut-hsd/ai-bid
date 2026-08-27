import { lazy } from 'react';
import type { RouteObject } from 'react-router-dom';

const TenantManagePage = lazy(() =>
   import('./pages/TenantManagePage').then((m) => ({ default: m.TenantManagePage }))
);

export const tenantRoutes: RouteObject[] = [
   {
      path: 'tenant-manage',
      element: <TenantManagePage />,
   },
];
