import React, { useState, useMemo } from 'react';
import { useNavigate } from 'react-router-dom';
import { useStyles } from './style';
import { App, Result, Button, Modal, Form, Input, Tabs } from 'antd';
import { PlusOutlined, ProjectOutlined } from '@ant-design/icons';

import { AuditFilter } from './components/AuditFilter';
import { AuditTable } from './components/AuditTable';
import { Loading } from '@/components/Loading/Loading';

import type { AuditListQueryParams } from './types';
import { useUrlState } from '@/hooks/useUrlState';

import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { auditListOptions, useDeleteProject } from './api/auditList';
import { dashboardMutations } from '@/features/dashboard/api/dashboard';

const STATUS_TAB_ITEMS = [
   { key: 'all', label: '全部' },
   { key: '0', label: '待审核' },
   { key: '1', label: '审核中' },
   { key: '2', label: '已通过' },
   { key: '4', label: '需修改' },
];

interface NewProjectFormValues {
   projectName: string;
}

export const BidAuditList: React.FC = () => {
   const { styles } = useStyles();
   const { message } = App.useApp();
   const navigate = useNavigate();
   const queryClient = useQueryClient();

   const [deletingProjectId, setDeletingProjectId] = useState<number | null>(
      null
   );
   const [isCreateModalOpen, setIsCreateModalOpen] = useState(false);
   const [createForm] = Form.useForm<NewProjectFormValues>();

   const [queryParams, setQueryParams] = useUrlState<AuditListQueryParams>({
      page: 1,
      size: 10,
      bidName: '',
      fileCategory: undefined,
      status: undefined,
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
   const sortedRecords = useMemo(() => {
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
         status: undefined,
         uploadStartTime: '',
         uploadEndTime: '',
      });
   };

   const handleStatusChange = (key: string) => {
      setQueryParams({
         status: key === 'all' ? undefined : Number(key),
         page: 1,
      });
   };

   const handlePageChange = (page: number) => {
      setQueryParams({ page });
   };

   const { mutate: deleteProjectMutation, isPending: isDeletingProject } =
      useDeleteProject();

   const handleDeleteProject = (projectId: number) => {
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

   const { mutateAsync: submitCreateProject, isPending: isCreating } =
      useMutation({
         ...dashboardMutations.create(),
      });

   const handleCreateFinish = async (values: NewProjectFormValues) => {
      try {
         const created = await submitCreateProject({
            id: 0,
            projectName: values.projectName,
         });
         message.success('项目创建成功，请上传招标文件');
         queryClient.invalidateQueries({ queryKey: ['dashboardList'] });
         setIsCreateModalOpen(false);
         createForm.resetFields();
         navigate(`/upload/${created.id}`);
      } catch (error) {
         message.error(
            error instanceof Error ? error.message : '项目创建失败，请重试'
         );
      }
   };

   return (
      <div className={styles.pageContainer}>
         <Tabs
            activeKey={
               queryParams.status === undefined
                  ? 'all'
                  : String(queryParams.status)
            }
            onChange={handleStatusChange}
            items={STATUS_TAB_ITEMS}
            style={{ marginBottom: 8 }}
         />

         <AuditFilter
            styles={styles}
            queryParams={queryParams}
            onSearch={handleSearch}
            onReset={handleReset}
            extra={
               <Button
                  type='primary'
                  icon={<PlusOutlined />}
                  onClick={() => setIsCreateModalOpen(true)}
               >
                  新建项目并上传招标文件
               </Button>
            }
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
               handleDeleteProject={handleDeleteProject}
               deletingProjectId={deletingProjectId}
               isDeletingProject={isDeletingProject}
            />
         )}

         {/* 新建项目模态框 */}
         <Modal
            title='新建项目'
            open={isCreateModalOpen}
            onCancel={() => setIsCreateModalOpen(false)}
            footer={null}
            width={500}
         >
            <Form
               form={createForm}
               layout='vertical'
               onFinish={handleCreateFinish}
            >
               <Form.Item
                  label='项目名称'
                  name='projectName'
                  rules={[{ required: true, message: '请输入项目名称' }]}
               >
                  <Input
                     prefix={<ProjectOutlined />}
                     placeholder='请输入项目名称'
                  />
               </Form.Item>

               <Form.Item style={{ marginBottom: 0 }}>
                  <div
                     style={{
                        display: 'flex',
                        gap: '8px',
                        justifyContent: 'flex-end',
                     }}
                  >
                     <Button onClick={() => setIsCreateModalOpen(false)}>
                        取消
                     </Button>

                     <Button
                        type='primary'
                        htmlType='submit'
                        loading={isCreating}
                     >
                        创建项目
                     </Button>
                  </div>
               </Form.Item>
            </Form>
         </Modal>
      </div>
   );
};