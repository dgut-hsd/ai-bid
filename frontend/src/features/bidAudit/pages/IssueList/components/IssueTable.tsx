import React from 'react';
import { Table, Button, Pagination, Tooltip, Empty } from 'antd';
import { ExportOutlined } from '@ant-design/icons';
import { useStyles } from '../style';
import type { AuditIssue, IssueQueryParams } from '../types';

import { IssueTableFilter } from './IssueTableFilter';
import { IssueCard } from './IssueCard';
import { useTableColumns } from '../hooks/useTableColumns';
import { useNavigate, useParams } from 'react-router-dom';
import { Loading } from '@/components/Loading/Loading';
import { useIsMobile } from '@/hooks/useMediaQuery';

interface IssueTableProps {
   issues: AuditIssue[];
   loading: boolean;
   total: number;
   queryParams: IssueQueryParams;
   onFilterChange: (filters: Partial<IssueQueryParams>) => void;
}

export const IssueTable: React.FC<IssueTableProps> = ({
   issues,
   loading,
   total,
   queryParams,
   onFilterChange,
}) => {
   const { styles, theme } = useStyles();
   const isMobile = useIsMobile();
   const navigate = useNavigate();
   const { id: bidId } = useParams<{ id: string }>();

   const columns = useTableColumns(queryParams.page, queryParams.size, theme);

   const handleReset = () => {
      onFilterChange({
         severity: 'all',
         category: 'all',
         keyword: '',
         page: 1,
      });
   };

   if (loading) {
      return <Loading loading={loading} />;
   }

   return (
      <div className={styles.tableArea}>
         <IssueTableFilter
            severity={queryParams.severity ?? 'all'}
            category={queryParams.category ?? 'all'}
            keyword={queryParams.keyword ?? ''}
            onChange={(filters) => onFilterChange({ ...filters, page: 1 })}
            onReset={handleReset}
         />

         {isMobile ? (
            issues.length > 0 ? (
               <div className={styles.issueCardList}>
                  {issues.map((issue) => (
                     <IssueCard key={issue.issueNo} issue={issue} />
                  ))}
               </div>
            ) : (
               <Empty
                  description='暂无符合条件的审核问题'
                  style={{ padding: '32px 0' }}
               />
            )
         ) : (
            <Table<AuditIssue>
               columns={columns}
               dataSource={issues}
               rowKey='issueNo'
               pagination={false}
               scroll={{ x: 'max-content' }}
            />
         )}

         <div className={styles.paginationArea}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
               <Tooltip title='回车键(Enter)键确认跳转' placement='topRight'>
                  <span style={{ display: 'inline-block' }}>
                     <Pagination
                        current={queryParams.page}
                        pageSize={queryParams.size}
                        total={total}
                        showQuickJumper={!isMobile}
                        size={isMobile ? 'small' : undefined}
                        onChange={(page) => onFilterChange({ page })}
                        style={{ fontSize: '1.2rem' }}
                     />
                  </span>
               </Tooltip>
            </div>

            <Button
               type='primary'
               icon={<ExportOutlined />}
               onClick={() => {
                  if (bidId) {
                     navigate(`/bidReview/report/${bidId}`);
                  }
               }}
            >
               导出审核报告
            </Button>
         </div>
      </div>
   );
};