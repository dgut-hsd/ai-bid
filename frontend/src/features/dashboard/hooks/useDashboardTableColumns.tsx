import type { Dispatch, SetStateAction } from 'react';
import { EllipsisTooltip } from '@/components/EllipsisTooltip/EllipsisTooltip';

import { useIsMobile } from '@/hooks/useMediaQuery';

import type { ProjectItem } from '../types';

import { Tag, Button, Space } from 'antd';
import type { ColumnsType } from 'antd/es/table';
import dayjs from 'dayjs';
import { useNavigate } from 'react-router-dom';

interface ColumnsProps {
   setIsDrawerOpen: Dispatch<SetStateAction<boolean>>;
   setSelectedProject: Dispatch<SetStateAction<number | null>>;
}

export const useDashboardColumns = ({
   setIsDrawerOpen,
   setSelectedProject,
}: ColumnsProps) => {
   const isMobile = useIsMobile();
   const navigate = useNavigate();

   const columns: ColumnsType<ProjectItem> = [
      {
         title: '项目名称',
         dataIndex: 'projectName',
         key: 'projectName',
         fixed: 'left',
         width: isMobile ? 110 : 150,
         align: 'center',
         ellipsis: true,
         render: (text: string) => <EllipsisTooltip text={text ?? '-'} />,
      },
      {
         title: '文件类型',
         dataIndex: 'fileCategory',
         key: 'fileCategory',
         align: 'center',
         width: 70,
         render: (fileCategory: '标书' | '合同') => {
            return <Tag color={'green'}>{fileCategory ?? '-'}</Tag>;
         },
      },
      {
         title: '供应商名称',
         dataIndex: 'supplierName',
         key: 'supplierName',
         width: isMobile ? 110 : 130,
         responsive: ['md', 'lg', 'xl', 'xxl'],
         align: 'center',
         ellipsis: true,
         render: (text: string) => <EllipsisTooltip text={text ?? '-'} />,
      },
      {
         title: '上传时间',
         dataIndex: 'createTime',
         key: 'createTime',
         align: 'center',
         width: 120,
         render: (text) => (text ? dayjs(text).format('YYYY-MM-DD') : '-'),
      },
      {
         title: '审核状态',
         dataIndex: 'parseStatus',
         key: 'parseStatus',
         align: 'center',
         width: 120,
         render: (status: number, record: ProjectItem) => {
            if (status === 2) {
               if (record.auditResult === 'pass') {
                  return <Tag color='green'>审核完成-通过</Tag>;
               }
               return <Tag color='gold'>审核完成-需修改</Tag>;
            }
            if (status === 1) {
               return <Tag color='orange'>审核中</Tag>;
            }
            if (status === 3) {
               return <Tag color='red'>审核失败</Tag>;
            }
            return <Tag color='blue'>待审核</Tag>;
         },
      },
      {
         title: '版本号',
         dataIndex: 'latestVersion',
         key: 'latestVersion',
         align: 'center',
         responsive: ['md', 'lg', 'xl', 'xxl'],
         width: 70,
         render: (version: number) =>
            typeof version === 'number' && version > 0 ? `V${version}` : '-',
      },
      {
         title: '操作',
         key: 'action',
         align: 'center',
         width: 130,
         fixed: 'right',
         render: (record: ProjectItem) => {
            return (
               <Space size={4} wrap={false}>
                  <Button
                     type='link'
                     onClick={(e) => {
                        e.stopPropagation();
                        setSelectedProject(record.id);
                        setIsDrawerOpen(true);
                     }}
                     style={{ padding: 0 }}
                  >
                     查看详情
                  </Button>

                  {'|'}

                  <Button
                     type='link'
                     onClick={(e) => {
                        e.stopPropagation();
                        navigate(`/upload/${record.id}`);
                     }}
                     style={{ padding: 0 }}
                  >
                     上传标书
                  </Button>
               </Space>
            );
         },
      },
   ];

   return columns;
};
