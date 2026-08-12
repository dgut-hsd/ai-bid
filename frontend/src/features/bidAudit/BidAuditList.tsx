import React from 'react';
import { useStyles } from './style';
import { App, Result } from 'antd';

import { AuditFilter } from './components/AuditFilter';
import { AuditTable } from './components/AuditTable';
import { Loading } from '@/components/Loading/Loading';

import type { AuditListQueryParams } from './types';
import { useUrlState } from '@/hooks/useUrlState';

import { useQuery } from '@tanstack/react-query';
import { auditListOptions, useDeleteProject } from './api/auditList';

export const BidAuditList: React.FC = () => {
   const { styles } = useStyles();
   const { message } = App.useApp();
   const [deletingProjectId, setDeletingProjectId] = React.useState<number | null>(
      null
   );

   const [queryParams, setQueryParams] = useUrlState<AuditListQueryParams>({
      page: 1,
      size: 10,
      bidName: '',
      fileCategory: undefined,
      uploadStartTime: '',
      uploadEndTime: '',
   });

   const {
      data: auditListData,
      isFetching: isAuditListFetching,
      isLoading: isAuditListLoading,
      isError: isAuditListError,
      error: auditListError,
   } = useQuery(auditListOptions.queryList(queryParams));

   // 按上传时间倒序：刚上传的标书直接置顶，不用在列表里翻找
   const sortedRecords = React.useMemo(() => {
      const records = auditListData?.records;
      if (!records || records.length === 0) return records ?? [];
      return [...records].sort((a, b) => {
         const ta = new Date(a.uploadTime || 0).getTime() || 0;
         const tb = new Date(b.uploadTime || 0).getTime() || 0;
         return tb - ta;
      });
   }, [auditListData]);

   const handleSearch = (filterValues: Partial<AuditListQueryParams>) => {
      setQueryParams({ ...filterValues, page: 1 });
   };

   const handleReset = () => {
      setQueryParams({
         page: 1,
         size: 10,
         bidName: '',
         fileCategory: undefined,
         uploadStartTime: '',
         uploadEndTime: '',
      });
   };

   const { mutate: deleteProjectMutation, isPending: isDeletingProject } =
      useDeleteProject();

   const handlePageChange = (page: number) => {
      setQueryParams({ page });
   };

   const handeleDeleteProject = (projectId: number) => {
      setDeletingProjectId(projectId);
      deleteProjectMutation(projectId, {
         onSuccess: () => {
            message.success('项目删除成功');
         },
         onError: (error) => {
            const errMsg =
               error instanceof Error ? error.message : '项目删除失败，请稍后重试';
            message.error(errMsg);
         },
         onSettled: () => {
            setDeletingProjectId(null);
         },
      });
   };

   return (
      <div className={styles.pageContainer}>
         <AuditFilter
            styles={styles}
            queryParams={queryParams}
            onSearch={handleSearch}
            onReset={handleReset}
         />
         {isAuditListLoading && !auditListData && (
            <Loading loading={true} fullScreen={true} />
         )}
         {isAuditListError && (
            <Result
               status='error'
               title='审核列表加载失败'
               subTitle={
                  auditListError instanceof Error
                     ? auditListError.message
                     : '请稍后重试'
               }
            />
         )}
         {auditListData && (
            <AuditTable
               styles={styles}
               data={sortedRecords}
               total={auditListData.total}
               isFetching={isAuditListFetching}
               page={queryParams.page}
               onPageChange={handlePageChange}
               handleDeleteProject={handeleDeleteProject}
               deletingProjectId={deletingProjectId}
               isDeletingProject={isDeletingProject}
            />
         )}
      </div>
   );
};
