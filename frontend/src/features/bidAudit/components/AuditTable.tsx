import React, { useState } from 'react';
import { VersionDrawer } from '@/components/VersionDrawer/VersionDrawer';

import { useAuditListTableColumns } from '../hooks/useAuditListTableColumns';

import type { ProjectItem } from '../types';

import { Table } from 'antd';
import { useQuery } from '@tanstack/react-query';

import { auditListOptions } from '../api/auditList';

interface AuditTableProps {
   styles: Record<string, string>;
   data: ProjectItem[];
   isFetching: boolean;
   total: number;
   page: number;
   onPageChange: (newPage: number) => void;
   handleDeleteProject: (projectId: number) => void;
   deletingProjectId: number | null;
   isDeletingProject: boolean;
}

export const AuditTable: React.FC<AuditTableProps> = ({
   styles,
   data,
   isFetching,
   total,
   page,
   onPageChange,
   handleDeleteProject,
   deletingProjectId,
   isDeletingProject,
}) => {
   const [isDrawerOpen, setIsDrawerOpen] = useState<boolean>(false);
   const [selectedProject, setSelectedProject] = useState<number | null>(null);

   const { data: versions, isFetching: isVersionsFetching } = useQuery({
      ...auditListOptions.detail(selectedProject),
      enabled: !!selectedProject,
   });

   const columns = useAuditListTableColumns({
      styles,
      setIsDrawerOpen,
      setSelectedProject,
      handleDeleteProject,
      deletingProjectId,
      isDeletingProject,
   });

   return (
      <div className={styles.tableContainer}>
         <Table
            columns={columns}
            dataSource={data ?? []}
            rowKey='projectId'
            onRow={(record) => {
               return {
                  onClick: () => {
                     setIsDrawerOpen(true);
                     setSelectedProject(record.projectId);
                  },
               };
            }}
            loading={isFetching}
            scroll={{ x: 'max-content' }}
            pagination={{
               current: page,
               pageSize: 10,
               total: total,
               showQuickJumper: true,
               onChange: onPageChange,
               size: 'small',
            }}
         />

         <VersionDrawer
            open={isDrawerOpen}
            onClose={() => setIsDrawerOpen(false)}
            versions={versions ?? []}
            isFetching={isVersionsFetching}
         />
      </div>
   );
};
