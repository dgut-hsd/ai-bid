import { lazy } from 'react';
import type { RouteObject } from 'react-router-dom';

const HistoryPage = lazy(() =>
   import('./HistoryPage').then((m) => ({ default: m.HistoryPage }))
);

export const historyRoutes: RouteObject[] = [
   { path: 'history', element: <HistoryPage /> },
];
