import React, { useState } from 'react';
import { Table, App } from 'antd';
import { useQuery } from '@tanstack/react-query';

import { VersionDrawer } from '@/components/VersionDrawer/VersionDrawer';
import { useAuditListTableColumns } from '../hooks/useAuditListTableColumns';
import { auditListOptions, useDeleteTenderVersion } from '../api/auditList';
import type { ProjectItem } from '../types';

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
   const { message } = App.useApp();
   const [isDrawerOpen, setIsDrawerOpen] = useState<boolean>(false);
   const [selectedProject, setSelectedProject] = useState<number | null>(null);
   const [deletingVersionId, setDeletingVersionId] = useState<number | null>(
      null
   );

   const { data: versions, isFetching: isVersionsFetching } = useQuery({
      ...auditListOptions.detail(selectedProject),
      enabled: !!selectedProject,
   });

   const { mutate: deleteVersionMutation, isPending: isDeletingVersion } =
      useDeleteTenderVersion();

   const handleDeleteVersion = (versionId: number) => {
      setDeletingVersionId(versionId);
      deleteVersionMutation(versionId, {
         onSuccess: () => {
            message.success('版本删除成功');
         },
         onError: (error) => {
            message.error(
               error instanceof Error ? error.message : '版本删除失败，请稍后重试'
            );
         },
         onSettled: () => {
            setDeletingVersionId(null);
         },
      });
   };

   const columns = useAuditListTableColumns({
      styles,
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
            projectId={selectedProject}
            onDeleteVersion={handleDeleteVersion}
            deletingVersionId={deletingVersionId}
            isDeletingVersion={isDeletingVersion}
         />
      </div>
   );
};