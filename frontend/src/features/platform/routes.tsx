import { lazy } from 'react';
import type { RouteObject } from 'react-router-dom';

const PlatformEnterprisesPage = lazy(() =>
   import('./pages/PlatformEnterprisesPage').then((m) => ({ default: m.PlatformEnterprisesPage }))
);

export const platformRoutes: RouteObject[] = [
   {
      path: 'platform/enterprises',
      element: <PlatformEnterprisesPage />,
   },
];