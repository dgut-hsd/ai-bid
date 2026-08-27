import { lazy } from 'react';
import type { RouteObject } from 'react-router-dom';

const BidUploadPage = lazy(() =>
   import('./BidUploadPage').then((m) => ({ default: m.BidUploadPage }))
);

export const uploadRoutes: RouteObject[] = [
   { path: '/upload/:projectId', element: <BidUploadPage /> },
];
