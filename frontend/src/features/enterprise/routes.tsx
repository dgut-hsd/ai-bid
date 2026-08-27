import { lazy } from 'react';
import type { RouteObject } from 'react-router-dom';

const EnterpriseUsersPage = lazy(() =>
   import('./pages/EnterpriseUsersPage').then((m) => ({ default: m.EnterpriseUsersPage }))
);

export const enterpriseRoutes: RouteObject[] = [
   {
      path: 'enterprise/users',
      element: <EnterpriseUsersPage />,
   },
];