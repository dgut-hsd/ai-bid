import React from 'react';
import { useStyles } from '../style';
import type { HistoryRecord, ReviewStatus } from '../types';

import { Table, Tabs, Pagination } from 'antd';
import { useHistoryTableColumns } from '../hooks/useHistoryTableColumns';
import { COLORS } from '@/theme/constants';

interface HistoryTableProps {
   data: HistoryRecord[];
   loading: boolean;
   total: number;
   currentPage: number;
   pageSize: number;
   activeTab: ReviewStatus | 'all';
   stats: {
      all: number;
      pass: number;
      revise: number;
      reject: number;
      passRatePercent: string | number;
   };
   onPageChange: (page: number) => void;
   onTabChange: (key: string) => void;
}

export const HistoryTable: React.FC<HistoryTableProps> = ({
   data,
   loading,
   total,
   currentPage,
   pageSize,
   activeTab,
   stats,
   onPageChange,
   onTabChange,
}) => {
   const { styles } = useStyles();
   const columns = useHistoryTableColumns();

   const tabItems = [
      { key: 'all', label: `全部 (${stats.all})` },
      { key: 'pass', label: `已通过 (${stats.pass})` },
      { key: 'revise', label: `需修改 (${stats.revise})` },
      { key: 'reject', label: `不通过 (${stats.reject})` },
   ];

   return (
      <div className={styles.cardWrapper} style={{ paddingTop: '5px' }}>
         <Tabs
            activeKey={activeTab}
            items={tabItems}
            onChange={onTabChange}
            size='small'
         />

         <div className={styles.tableWrapper}>
            <Table
               columns={columns}
               dataSource={data}
               rowKey='id'
               loading={loading}
               scroll={{ x: 'max-content' }}
               pagination={false}
            />

            {/* Footer */}
            <div
               style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center',
                  marginTop: 16,
               }}
            >
               <div style={{ fontSize: '1.2rem' }}>
                  共 <strong>{stats.all}</strong> 份标书
                  <span style={{ margin: '0 12px', color: COLORS.border }}>
                     |
                  </span>
                  已通过{' '}
                  <span style={{ color: COLORS.success, fontWeight: 600 }}>
                     {stats.pass}
                  </span>
                  <span style={{ margin: '0 12px', color: COLORS.border }}>
                     |
                  </span>
                  通过率{' '}
                  <strong style={{ color: COLORS.success }}>
                     {stats.passRatePercent}%
                  </strong>
               </div>

               <Pagination
                  current={currentPage}
                  pageSize={pageSize}
                  total={total}
                  showQuickJumper={true}
                  onChange={onPageChange}
                  style={{ fontSize: '1.2rem' }}
               />
            </div>
         </div>
      </div>
   );
};
