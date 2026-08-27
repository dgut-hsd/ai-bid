import React from 'react';
import { useStyles } from './style';

import { IssueTable } from './components/IssueTable';
import { IssueDashboard } from './components/IssueDashboard';

import { useUrlState } from '@/hooks/useUrlState';
import { useAuditIssuesList } from './hooks/useAuditIssuesList';

import type { IssueQueryParams } from './types';
import { auditIssuesListOptions } from './api/auditIssuesList';

import { useParams } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { Typography } from 'antd';

const { Title, Text } = Typography;

export const IssueListPage: React.FC = () => {
   const { styles } = useStyles();
   const { id: bidId } = useParams<{ id: string }>();
   const taskId = React.useMemo(() => {
      if (!bidId) return '';
      // 兼容旧链接：如果路由里本身就是 taskId，直接使用。
      if (bidId.startsWith('task_')) return bidId;
      try {
         const raw = localStorage.getItem(`auditTask:${bidId}`);
         if (!raw) return '';
         const parsed = JSON.parse(raw) as { taskId?: string };
         return parsed.taskId ?? '';
      } catch {
         return '';
      }
   }, [bidId]);

   const [queryParams, setQueryParams] = useUrlState<IssueQueryParams>({
      page: 1,
      size: 10,
      severity: 'all',
      category: 'all',
      keyword: '',
   });

   const { data: bidData } = useQuery({
      ...auditIssuesListOptions.bidDetail(Number(bidId)),
      enabled: !!bidId && !isNaN(Number(bidId)),
   });

   const { issues, total, summary, isLoading } = useAuditIssuesList(
      taskId ?? '',
      queryParams
   );

   const handleFilterChange = (filters: Partial<IssueQueryParams>) => {
      setQueryParams(filters);
   };

   return (
      <div className={styles.pageContainer}>
         <div className={styles.headerArea}>
            <Title level={4} style={{ margin: '8px 0 0 0' }}>
               问题清单详情
            </Title>

            <Text className={styles.infoText}>
               关联项目：{bidData?.bidName} &nbsp;&nbsp;|&nbsp;&nbsp;
               审核生成时间：{bidData?.uploadTime}
            </Text>
         </div>

         <IssueDashboard summary={summary} />

         <IssueTable
            issues={issues}
            loading={isLoading}
            total={total}
            queryParams={queryParams}
            onFilterChange={handleFilterChange}
         />
      </div>
   );
};

export default IssueListPage;
