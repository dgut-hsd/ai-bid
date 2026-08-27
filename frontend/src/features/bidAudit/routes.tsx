import { lazy } from 'react';
import type { RouteObject } from 'react-router-dom';

const BidAuditList = lazy(() =>
   import('./BidAuditList').then((m) => ({ default: m.BidAuditList }))
);
const DetailPage = lazy(() =>
   import('./pages/Detail/DetailPage').then((m) => ({ default: m.DetailPage }))
);
const IssueListPage = lazy(() =>
   import('./pages/IssueList/IssueListPage').then((m) => ({ default: m.IssueListPage }))
);
const ReportPage = lazy(() =>
   import('./pages/Report/ReportPage').then((m) => ({ default: m.ReportPage }))
);

export const bidAuditRoutes: RouteObject[] = [
   {
      path: 'bidReview',
      children: [
         { index: true, element: <BidAuditList /> },
         {
            path: 'detail/:id',
            element: <DetailPage />,
         },
         {
            path: 'issues/:id',
            element: <IssueListPage />,
         },
         {
            path: 'report/:id',
            element: <ReportPage />,
         },
      ],
   },
];
