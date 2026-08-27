import React, { useState } from 'react';
import { Table, Pagination, Spin, Empty, Skeleton, App } from 'antd';
import { useQuery } from '@tanstack/react-query';

import { VersionDrawer } from '@/components/VersionDrawer/VersionDrawer';
import { useIsMobile } from '@/hooks/useMediaQuery';

import { useAuditListTableColumns } from '../hooks/useAuditListTableColumns';
import { AuditCard } from './AuditCard';
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
   const isMobile = useIsMobile();
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

   const openVersionsDrawer = (projectId: number) => {
      setSelectedProject(projectId);
      setIsDrawerOpen(true);
   };

   const versionsDrawer = (
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
   );

   // 移动端：卡片式列表
   if (isMobile) {
      return (
         <div className={styles.mobileListContainer}>
            {data.length === 0 ? (
               isFetching ? (
                  <Skeleton
                     active
                     paragraph={{ rows: 5 }}
                     style={{ padding: 12 }}
                  />
               ) : (
                  <Empty
                     description='暂无审核项目'
                     style={{ padding: '32px 0' }}
                  />
               )
            ) : (
               <Spin spinning={isFetching}>
                  <div className={styles.mobileCardList}>
                     {data.map((record) => (
                        <AuditCard
                           key={record.projectId}
                           record={record}
                           deleting={
                              isDeletingProject &&
                              deletingProjectId === record.projectId
                           }
                           onView={openVersionsDrawer}
                           onDelete={handleDeleteProject}
                           styles={styles}
                        />
                     ))}
                  </div>
               </Spin>
            )}

            {data.length > 0 && (
               <div
                  style={{
                     display: 'flex',
                     justifyContent: 'center',
                     marginTop: 12,
                  }}
               >
                  <Pagination
                     current={page}
                     pageSize={10}
                     total={total}
                     size='small'
                     showSizeChanger={false}
                     onChange={onPageChange}
                  />
               </div>
            )}

            {versionsDrawer}
         </div>
      );
   }

   return (
      <div className={styles.tableContainer}>
         <Table
            columns={columns}
            dataSource={data ?? []}
            rowKey='projectId'
            onRow={(record) => ({
               onClick: () => openVersionsDrawer(record.projectId),
            })}
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

         {versionsDrawer}
      </div>
   );
};