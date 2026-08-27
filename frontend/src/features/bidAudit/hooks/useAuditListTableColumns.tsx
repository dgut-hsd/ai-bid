import type { Dispatch, SetStateAction } from 'react';

import { EllipsisTooltip } from '@/components/EllipsisTooltip/EllipsisTooltip';

import type { ProjectItem } from '../types';

import { Button, Popconfirm, Space, Tag } from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { useIsMobile } from '@/hooks/useMediaQuery';
import dayjs from 'dayjs';

interface ColumnsProps {
   styles: Record<string, string>;
   setIsDrawerOpen: Dispatch<SetStateAction<boolean>>;
   setSelectedProject: Dispatch<SetStateAction<number | null>>;
   handleDeleteProject: (projectId: number) => void;
   deletingProjectId: number | null;
   isDeletingProject: boolean;
}

export const useAuditListTableColumns = ({
   styles,
   setIsDrawerOpen,
   setSelectedProject,
   handleDeleteProject,
   deletingProjectId,
   isDeletingProject,
}: ColumnsProps) => {
   const isMobile = useIsMobile();

   const columns: ColumnsType<ProjectItem> = [
      {
         title: '项目名称',
         dataIndex: 'bidName',
         key: 'bidName',
         align: 'center',
         fixed: 'left',
         width: isMobile ? 140 : 200,
         ellipsis: true,
         render: (text: string) => <EllipsisTooltip text={text ?? '-'} />,
      },
      {
         title: '文件类型',
         dataIndex: 'fileCategory',
         key: 'fileCategory',
         align: 'center',
         width: 100,
         render: (fileCategory: string) => (
            <Tag color={'green'}>{fileCategory ?? '-'}</Tag>
         ),
      },
      {
         title: '供应商名称',
         dataIndex: 'supplierName',
         key: 'supplierName',
         align: 'center',
         width: 150,
         responsive: ['md', 'lg', 'xl', 'xxl'],
         ellipsis: true,
         render: (text: string) => <EllipsisTooltip text={text ?? '-'} />,
      },
      {
         title: '上传时间',
         dataIndex: 'uploadTime',
         key: 'uploadTime',
         align: 'center',
         width: 150,
         render: (text) => (text ? dayjs(text).format('YYYY-MM-DD') : '-'),
      },
      {
         title: '审核人',
         dataIndex: 'auditorName',
         key: 'auditorName',
         align: 'center',
         width: 100,
         responsive: ['md', 'lg', 'xl', 'xxl'],
         ellipsis: true,
         render: (text: string) => <EllipsisTooltip text={text ?? '-'} />,
      },
      {
         title: '版本号',
         dataIndex: 'version',
         key: 'version',
         align: 'center',
         width: 100,
         responsive: ['md', 'lg', 'xl', 'xxl'],
      },
      {
         title: '操作',
         key: 'actions',
         align: 'center',
         width: 140,
         fixed: 'right',
         render: (_, record) => {
            const deletingCurrent =
               isDeletingProject && deletingProjectId === record.projectId;
            return (
               <Space size='small'>
                  <Button
                     type='link'
                     onClick={(e) => {
                        e.stopPropagation();
                        setSelectedProject(record.projectId);
                        setIsDrawerOpen(true);
                     }}
                     className={styles.actionLinkBtn}
                  >
                     查看
                  </Button>

                  <span className={styles.actionSeparator}>|</span>

                  <Popconfirm
                     title='确定要删除该项目吗？'
                     description='删除后将同步移除该项目下全部版本与审核记录，且不可恢复。'
                     okText='确认删除'
                     cancelText='取消'
                     okButtonProps={{ danger: true, loading: deletingCurrent }}
                     onConfirm={(e) => {
                        e?.stopPropagation();
                        handleDeleteProject(record.projectId);
                     }}
                     onCancel={(e) => e?.stopPropagation()}
                  >
                     <Button
                        type='link'
                        danger
                        className={styles.actionLinkBtn}
                        loading={deletingCurrent}
                        onClick={(e) => e.stopPropagation()}
                     >
                        删除
                     </Button>
                  </Popconfirm>
               </Space>
            );
         },
      },
   ];

   return columns;
};
