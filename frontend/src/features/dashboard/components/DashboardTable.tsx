import React, { useMemo, useState } from 'react';
import { Table, Pagination } from 'antd';
import { useStyles } from '../style';
import { useDashboardColumns } from '../hooks/useDashboardTableColumns';
import type { ProjectItem } from '../types';
import { auditListOptions } from '@/features/bidAudit/api/auditList';
import { useQuery } from '@tanstack/react-query';
import { VersionDrawer } from '@/components/VersionDrawer/VersionDrawer';

interface DashboardTableProps {
   data: ProjectItem[];
   loading: boolean;
   total: number;
   currentPage: number;
   pageSize: number;
   onPageChange: (page: number) => void;
}

export const DashboardTable: React.FC<DashboardTableProps> = ({
   data,
   loading,
   total,
   currentPage,
   pageSize,
   onPageChange,
}) => {
   const { styles } = useStyles();

   const [isDrawerOpen, setIsDrawerOpen] = useState<boolean>(false);
   const [selectedProject, setSelectedProject] = useState<number | null>(null);

   const { data: versions, isFetching: isVersionsFetching } = useQuery({
      ...auditListOptions.detail(selectedProject),
      enabled: !!selectedProject,
   });
   
   const currentData = useMemo(() => {
      const startIndex = (currentPage - 1) * pageSize;
      return data.slice(startIndex, startIndex + pageSize);
   }, [data, currentPage, pageSize]);

   const handleCloseVersion = () => {
      setIsDrawerOpen(false);
      setSelectedProject(null);
   };

   const columns = useDashboardColumns({ setIsDrawerOpen, setSelectedProject });

   return (
      <div className={styles.cardWrapper}>
         <div className={styles.tableContainer}>
            <Table
               columns={columns}
               dataSource={currentData}
               rowKey='id'
               tableLayout='fixed'
               scroll={{ x: 'max-content' }}
               onRow={(record) => {
                  return {
                     onClick: () => {
                        setIsDrawerOpen(true);
                        setSelectedProject(record.id);
                     },
                  };
               }}
               loading={loading}
               pagination={false}
            />
         </div>

         <VersionDrawer
            open={isDrawerOpen}
            onClose={handleCloseVersion}
            versions={versions ?? []}
            isFetching={isVersionsFetching}
         />

         <Pagination
            current={currentPage}
            pageSize={pageSize}
            total={total}
            showQuickJumper={true}
            onChange={onPageChange}
            style={{
               display: 'flex',
               justifyContent: 'flex-end',
               alignItems: 'center',
               marginTop: 16,
            }}
         />
      </div>
   );
};
