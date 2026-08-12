import React, { useState, useMemo } from 'react';

import { IssueTypePieChart } from './components/IssueTypePieChart';
import { WeeklyAuditBarChart } from './components/WeeklyAuditBarChart';
import { DashboardTable } from './components/DashboardTable';

import { DashboardStatCard } from '@/components/StatCard/DashboardStatCard';

import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';

import { useStyles } from './style';

import { dashboardMutations, dashboardOptions } from './api/dashboard';

import { Button, Modal, Form, Input, message, Spin, Result } from 'antd';
import {
   AuditOutlined,
   CheckCircleFilled,
   ProjectOutlined,
} from '@ant-design/icons';
import { useUrlState } from '@/hooks/useUrlState';

const STATUS_CONFIG = {
   pending: { label: '待审核', icon: <AuditOutlined />, color: 'green' },
   passed: { label: '已审核', icon: <CheckCircleFilled />, color: 'green' },
};

interface NewProjectFormValues {
   projectName: string;
}

export const DashboardPage: React.FC = () => {
   const { styles } = useStyles();
   const queryClient = useQueryClient();

   const [queryParams, setQueryParams] = useUrlState<{
      page: number;
      size: number;
      statusFilter: 'all' | 'pending' | 'passed';
   }>({
      page: 1,
      size: 9,
      statusFilter: 'all',
   });

   const [isNewProjectModalOpen, setIsNewProjectModalOpen] = useState(false);
   const [newProjectForm] = Form.useForm();

   // 数据获取
   const {
      data: listData,
      isLoading: isListLoading,
      isError: isListError,
      error: listError,
   } = useQuery(dashboardOptions.list());

   const {
      data: issueDistribution,
      isLoading: isIssueDistributionLoading,
      isError: isIssueDistributionError,
   } = useQuery(dashboardOptions.issueDistribution());
   
   const {
      data: auditCount,
      isLoading: isAuditCountLoading,
      isError: isAuditCountError,
   } = useQuery(dashboardOptions.auditCount());

   const stats: Record<string, number> = useMemo(() => {
      const pending =
         listData?.filter((item) => item.parseStatus !== 2).length || 0;

      const passed =
         listData?.filter((item) => item.parseStatus === 2).length || 0;

      return { pending, passed };
   }, [listData]);

   // 待审核 / 已审核 筛选（与 design.md §「列表筛选存于 URL」一致，statusFilter 进入 query）
   const statusFilter = queryParams.statusFilter ?? 'all';
   const filteredData = useMemo(() => {
      if (statusFilter === 'all') return listData || [];
      return (listData || []).filter((item) =>
         statusFilter === 'pending' ? item.parseStatus !== 2 : item.parseStatus === 2
      );
   }, [listData, statusFilter]);

   const handleFilterClick = (key: 'pending' | 'passed') => {
      // 切筛选时重置到第 1 页，避免当前页超出筛选后的总页数导致空列表
      setQueryParams({
         statusFilter: statusFilter === key ? 'all' : key,
         page: 1,
      });
   };

   // 新建项目的 Hook
   const { mutate: submitCreateProject, isPending: isCreating } = useMutation({
      ...dashboardMutations.create(),
      onSuccess: () => {
         message.success('项目创建成功');
         queryClient.invalidateQueries(dashboardOptions.list());
         handleCloseNewProjectModal();
      },
      onError: (error: Error) => {
         message.error(error.message || '项目创建失败，请重试');
      },
   });

   const handlePageChange = (page: number) => {
      setQueryParams({ page });
   };

   const handleOpenNewProjectModal = () => setIsNewProjectModalOpen(true);

   const handleCloseNewProjectModal = () => {
      setIsNewProjectModalOpen(false);
      newProjectForm.resetFields();
   };

   const handleNewProjectSubmit = (values: NewProjectFormValues) => {
      submitCreateProject({
         id: 0,
         projectName: values.projectName,
      });
   };

   return (
      <div className={styles.pageContainer}>
         <div className={styles.mainContent}>
            <div className={styles.leftColumn}>
               <div className={styles.statCardsContainer}>
                  {Object.entries(STATUS_CONFIG).map(([key, config]) => {
                     const active = statusFilter === key;
                     return (
                        <div
                           key={key}
                           onClick={() => handleFilterClick(key as 'pending' | 'passed')}
                           style={{
                              cursor: 'pointer',
                              borderRadius: 6,
                              border: active
                                 ? '2px solid #52c41a'
                                 : '2px solid transparent',
                              boxShadow: active
                                 ? '0 0 0 2px rgba(82,196,26,0.15)'
                                 : undefined,
                              transition: 'border-color 0.2s, box-shadow 0.2s',
                           }}
                        >
                           <DashboardStatCard
                              label={config.label}
                              value={stats[key] || 0}
                              color={config.color}
                              icon={config.icon}
                           />
                        </div>
                     );
                  })}
               </div>

               <div className={styles.headerActions}>
                  <Button type='primary' onClick={handleOpenNewProjectModal}>
                     新建项目
                  </Button>
               </div>

               {isListError ? (
                  <Result
                     status='error'
                     title='项目列表加载失败'
                     subTitle={
                        listError instanceof Error
                           ? listError.message
                           : '请稍后重试'
                     }
                  />
               ) : (
                  <DashboardTable
                     data={filteredData}
                     loading={isListLoading && !listData}
                     total={filteredData.length}
                     currentPage={queryParams.page}
                     pageSize={queryParams.size}
                     onPageChange={handlePageChange}
                  />
               )}
            </div>

            <div className={styles.rightColumn}>
               {isIssueDistributionLoading ? (
                  <Spin />
               ) : isIssueDistributionError ? (
                  <Result status='warning' title='问题分布加载失败' />
               ) : (
                  <IssueTypePieChart data={issueDistribution} />
               )}

               {isAuditCountLoading ? (
                  <Spin />
               ) : isAuditCountError ? (
                  <Result status='warning' title='审核统计加载失败' />
               ) : (
                  <WeeklyAuditBarChart data={auditCount} />
               )}
            </div>
         </div>

         {/* 新建项目模态框 */}
         <Modal
            title='新建项目'
            open={isNewProjectModalOpen}
            onCancel={handleCloseNewProjectModal}
            footer={null}
            width={500}
         >
            <Form
               form={newProjectForm}
               layout='vertical'
               onFinish={handleNewProjectSubmit}
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
                     <Button onClick={handleCloseNewProjectModal}>取消</Button>

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
