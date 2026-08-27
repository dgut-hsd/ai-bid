import { lazy } from 'react';
import type { RouteObject } from 'react-router-dom';

const LoginPage = lazy(() =>
   import('./LoginPage').then((m) => ({ default: m.LoginPage }))
);

export const loginRoutes: RouteObject[] = [
   { path: '/login', element: <LoginPage /> },
];
