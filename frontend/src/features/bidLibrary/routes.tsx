import { lazy } from 'react';
import type { RouteObject } from 'react-router-dom';

const BidLibraryPage = lazy(() =>
   import('./BidLibraryPage').then((m) => ({ default: m.BidLibraryPage }))
);

export const libraryRoutes: RouteObject[] = [
   { path: '/library', element: <BidLibraryPage /> },
];
