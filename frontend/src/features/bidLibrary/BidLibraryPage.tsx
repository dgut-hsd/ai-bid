import React, { useRef, useState } from 'react';
import { Empty, App, Modal, Spin } from 'antd';
import dayjs from 'dayjs';
import { useQueryClient } from '@tanstack/react-query';

import { useStyles } from './style';
import { useUrlState } from '@/hooks/useUrlState';

import { SearchBar } from './components/SearchBar';
import { CategoryTabs } from './components/CategoryTabs';
import { StatCard } from './components/StatCard';
import { FilterBar } from './components/FilterBar';
import { FileTable } from './components/FileTable';
import { UploadModal } from './components/UploadModal';
import { EditModal } from './components/EditModal';
import { knowledgeFileApi } from './api/knowledgeFile';


import {
   useKnowledgeData,
   useKnowledgeStatistics,
} from './api/useKnowledgeData';
import type { KnowledgeQueryConfig, KnowledgeFile } from './types';

export const BidLibraryPage: React.FC = () => {
   const { styles } = useStyles();
   const queryClient = useQueryClient();
   const { message } = App.useApp();

   const [queryParams, setQueryParams] = useUrlState<KnowledgeQueryConfig>({
      page: 1,
      size: 10,
      category: 'all',
      keyword: '',
      applicableScope: '',
      status: '',
      startDate: '',
      endDate: '',
   });

   const { data: recordsData, isFetching } = useKnowledgeData(queryParams);

   const { data: statsData } = useKnowledgeStatistics(queryParams);

   const [uploadVisible, setUploadVisible] = useState(false);
   const [filterDrawerOpen, setFilterDrawerOpen] = useState(false);
   const [isUploading, setIsUploading] = useState(false);
   const [editVisible, setEditVisible] = useState(false);
   const [editSubmitting, setEditSubmitting] = useState(false);
   const editSubmittingRef = useRef(false);
   const [selectedFile, setSelectedFile] = useState<KnowledgeFile | null>(null);

   const categoryCounts = statsData || {
      all: 0,
      regulation: 0,
      price: 0,
      supplier: 0,
      contract: 0,
      case: 0,
      other: 0,
   };

   const handleFilterChange = (changes: Partial<KnowledgeQueryConfig>) => {
      setQueryParams({ ...changes, page: 1 });
   };

   const handleDelete = async (file: KnowledgeFile) => {
      try {
         await knowledgeFileApi.delete(file.id);
         message.success('文件删除成功！');
         queryClient.invalidateQueries({ queryKey: ['knowledgeFiles'] });
         queryClient.invalidateQueries({ queryKey: ['knowledgeStats'] });
      } catch {
         message.error('文件删除失败');
      }
   };

   const handleReset = () => {
      setQueryParams({
         page: 1,
         size: 10,
         category: 'all',
         keyword: '',
         applicableScope: '',
         status: '',
         startDate: '',
         endDate: '',
      });
   };

   // 移动端「筛选」按钮角标：统计已激活的次要筛选条件数
   const activeFilterCount = [
      queryParams.applicableScope ? 1 : 0,
      queryParams.status ? 1 : 0,
      queryParams.startDate || queryParams.endDate ? 1 : 0,
   ].reduce((s, n) => s + n, 0);

   return (
      <div className={styles.pageContainer}>
         <Modal
            title='上传中'
            open={isUploading}
            closable={false}
            footer={null}
            centered
         >
            <div style={{ textAlign: 'center', padding: '20px 0' }}>
               <Spin size='large' />
               <p style={{ marginTop: 16 }}>正在上传，请稍候...</p>
            </div>
         </Modal>

         {/* 上半部分：分为左右 */}
         <div className={styles.mainLayout}>
            {/* 核心左侧：操作区 */}
            <div className={styles.leftSection}>
               {/* 左侧的上部分：搜索 + 上传 */}
               <SearchBar
                  searchKeyword={queryParams.keyword || ''}
                  onSearchChange={(keyword) => handleFilterChange({ keyword })}
                  onUploadClick={() => setUploadVisible(true)}
                  onFilterClick={() => setFilterDrawerOpen(true)}
                  activeFilterCount={activeFilterCount}
               />

               {/* 左侧的下部分：Tab + 筛选器 (新增了一层 div 包裹) */}
               <div className={styles.filterColumn}>
                  <CategoryTabs
                     selectedCategory={queryParams.category || 'all'}
                     onCategoryChange={(category) =>
                        handleFilterChange({ category: category as KnowledgeQueryConfig['category'] })
                     }
                  />

                  <FilterBar
                     applicableScopeFilter={queryParams.applicableScope || ''}
                     onApplicableScopeChange={(scope) =>
                        handleFilterChange({ applicableScope: scope })
                     }
                     statusFilter={queryParams.status || ''}
                     onStatusChange={(status) =>
                        handleFilterChange({ status })
                     }
                     dateRange={
                        queryParams.startDate
                           ? [
                                dayjs(queryParams.startDate),
                                dayjs(queryParams.endDate),
                             ]
                           : null
                     }
                     onDateRangeChange={(dates) => {
                        if (dates && dates[0] && dates[1]) {
                           handleFilterChange({
                              startDate: dates[0].format('YYYY-MM-DD'),
                              endDate: dates[1].format('YYYY-MM-DD'),
                           });
                        } else {
                           handleFilterChange({ startDate: '', endDate: '' });
                        }
                     }}
                     onReset={handleReset}
                     drawerOpen={filterDrawerOpen}
                     onDrawerClose={() => setFilterDrawerOpen(false)}
                  />
               </div>
            </div>

            {/* 核心右侧：统计卡片区 */}
            <div className={styles.rightSection}>
               <StatCard categoryCounts={categoryCounts} />
            </div>
         </div>

         {recordsData?.records?.length ? (
            <FileTable
               files={recordsData.records}
               loading={isFetching}
               total={recordsData.total}
               currentPage={queryParams.page}
               pageSize={queryParams.size}
               onPageChange={(page) => setQueryParams({ page })}
               onView={async (file) => {
                  const hide = message.loading('加载文件中...', 0);
                  try {
                     const blob = await knowledgeFileApi.preview(file.id);
                     
                     // 检查是否为 Blob 对象或类 Blob 对象
                     const isBlob = blob instanceof Blob || 
                        (blob && typeof blob === 'object' && 
                         ((blob as { constructor?: { name?: string } }).constructor?.name === 'Blob' || Object.prototype.toString.call(blob) === '[object Blob]'));
                         
                     if (isBlob && ((blob as Blob).type.includes('pdf') || (blob as Blob).type.includes('application/octet-stream') || (blob as Blob).size > 0)) {
                        if ((blob as Blob).type.includes('text/plain')) {
                           const text = await (blob as Blob).text();
                           message.error(text || '该文件类型暂不支持在线预览，请下载查看');
                           return;
                        }
                        const url = window.URL.createObjectURL(blob as Blob);
                        window.open(url, '_blank');
                     } else {
                        // 可能是被拦截器处理过的错误对象
                        console.error('Download error:', blob);
                        message.error('预览文件失败');
                     }
                  } catch (error) {
                     console.error(error);
                     message.error('预览文件失败');
                  } finally {
                     hide();
                  }
               }}
               onEdit={(file) => {
                  setSelectedFile(file);
                  setEditVisible(true);
               }}
               onDelete={handleDelete}
               onDownload={async (file) => {
                  const hide = message.loading('文件下载中...', 0);
                  try {
                     const blob = await knowledgeFileApi.download(file.id);
                     
                     // 检查是否为 Blob 对象或类 Blob 对象
                     const isBlob = blob instanceof Blob || 
                        (blob && typeof blob === 'object' && 
                         ((blob as { constructor?: { name?: string } }).constructor?.name === 'Blob' || Object.prototype.toString.call(blob) === '[object Blob]'));
                         
                     if (isBlob) {
                        const url = window.URL.createObjectURL(blob as Blob);
                        const a = document.createElement('a');
                        a.href = url;
                        a.download = file.fileName;
                        document.body.appendChild(a);
                        a.click();
                        window.URL.revokeObjectURL(url);
                        document.body.removeChild(a);
                        message.success('下载成功');
                     } else {
                        console.error('Download error:', blob);
                        message.error('下载失败');
                     }
                  } catch (_e) {
                     console.error(_e);
                     message.error('下载失败');
                  } finally {
                     hide();
                  }
               }}
            />
         ) : (
            <div className={styles.tableContainer}>
               <Empty
                  description={
                     isFetching ? '数据加载中...' : '暂无符合条件的标准库文件'
                  }
                  style={{ padding: '48px' }}
               />
            </div>
         )}

         <UploadModal
            visible={uploadVisible}
            onCancel={() => setUploadVisible(false)}
            onConfirm={(formData) => {
               const runUpload = async () => {
                  setIsUploading(true);
                  try {
                     const res = await knowledgeFileApi.upload(formData);
                     void res;
                     message.success('上传成功');
                     setUploadVisible(false);

                     // 上传成功后，刷新列表数据和统计数据
                     queryClient.invalidateQueries({ queryKey: ['knowledgeFiles'] });
                     queryClient.invalidateQueries({ queryKey: ['knowledgeStats'] });
                  } catch (error) {
                     console.error(error);
                     message.error('上传失败');
                  } finally {
                     setIsUploading(false);
                  }
               };
               void runUpload();
            }}
         />
         <EditModal
            visible={editVisible}
            selectedFile={selectedFile}
            submitting={editSubmitting}
            onCancel={() => {
               if (editSubmitting) return;
               setEditVisible(false);
            }}
            onConfirm={async (values) => {
               if (!selectedFile || editSubmittingRef.current) return;
               editSubmittingRef.current = true;
               const hide = message.loading('修改中...', 0);
               try {
                  setEditSubmitting(true);
                  await knowledgeFileApi.update(selectedFile.id, {
                     fileName: values.fileName.trim(),
                     category: values.category,
                     applicableScope: values.applicableScope,
                     description: values.description?.trim() || '',
                     status: values.status,
                  });
                  message.success('修改成功');
                  setEditVisible(false);
                  queryClient.invalidateQueries({ queryKey: ['knowledgeFiles'] });
                  queryClient.invalidateQueries({ queryKey: ['knowledgeStats'] });
               } catch (error) {
                  const errMsg =
                     typeof error === 'object' &&
                     error &&
                     'response' in error &&
                     typeof (error as { response?: { data?: { message?: string } } })
                        .response?.data?.message === 'string'
                        ? (error as { response?: { data?: { message?: string } } })
                             .response?.data?.message
                        : '修改失败';
                  console.error(error);
                  message.error(errMsg || '修改失败');
               } finally {
                  editSubmittingRef.current = false;
                  setEditSubmitting(false);
                  hide();
               }
            }}
         />

      </div>
   );
};
