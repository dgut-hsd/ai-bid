import React, { useMemo } from 'react';
import { Breadcrumb } from 'antd';
import { useLocation, Link, matchPath } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { bidOptions } from '@/api/bid';

const breadcrumbNameMap: Record<string, string> = {
   '/upload': '文件上传',
   '/bidReview': '招标文件',
   '/library': '标准库管理',
};

export const HeaderBreadcrumb: React.FC = () => {
   const location = useLocation();

   const matchDetail = matchPath('/bidReview/detail/:bidId', location.pathname);
   const matchIssues = matchPath('/bidReview/issues/:bidId', location.pathname);
   const matchReport = matchPath('/bidReview/report/:bidId', location.pathname);

   const bidId =
      (matchDetail?.params.bidId ||
         matchIssues?.params.bidId ||
         matchReport?.params.bidId) ??
      '';
   const safeBidId = /^\d+$/.test(bidId) ? bidId : '';

   const { data: bidDetail, isLoading } = useQuery(bidOptions.detail(safeBidId));

   const breadcrumbItems = useMemo(() => {
      const crumbs = [{ label: '招标文件', path: '/bidReview' }];

      if (location.pathname.startsWith('/bidReview')) {
         if (safeBidId) {
            const label = isLoading
               ? '加载中...'
               : bidDetail?.bidName || '审核详情';

            crumbs.push({
               label: label,
               path: `/bidReview/detail/${safeBidId}`,
            });

            if (matchIssues) {
               crumbs.push({
                  label: '问题清单详情',
                  path: `/bidReview/issues/${safeBidId}`,
               });
            } else if (matchReport) {
               crumbs.push({
                  label: '审核报告',
                  path: `/bidReview/report/${safeBidId}`,
               });
            }
         }
      } else {
         const pathSnippets = location.pathname.split('/').filter((i) => i);

         pathSnippets.forEach((_, index) => {
            const url = `/${pathSnippets.slice(0, index + 1).join('/')}`;
            if (breadcrumbNameMap[url] && url !== '/bidReview') {
               crumbs.push({ label: breadcrumbNameMap[url], path: url });
            }
         });
      }

      return crumbs.map((crumb, index) => {
         return {
            key: crumb.path,
            title:
               index === crumbs.length - 1 ? (
                  crumb.label
               ) : (
                  <Link to={crumb.path}>{crumb.label}</Link>
               ),
         };
      });
   }, [
      location.pathname,
      safeBidId,
      bidDetail,
      isLoading,
      matchIssues,
      matchReport,
   ]);

   return (
      <Breadcrumb
         items={breadcrumbItems}
         separator='>'
         style={{ fontSize: '1.2rem' }}
      />
   );
};
