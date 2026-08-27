import { useNavigate } from 'react-router-dom';
import { App, Button, Dropdown } from 'antd';
import type { ColumnsType } from 'antd/es/table';
import type { MenuProps } from 'antd';
import {
   DeleteOutlined,
   EyeOutlined,
   MoreOutlined,
   UploadOutlined,
} from '@ant-design/icons';

import { EllipsisTooltip } from '@/components/EllipsisTooltip/EllipsisTooltip';
import { useIsMobile } from '@/hooks/useMediaQuery';
import { StatusTag } from '../components/StatusTag';
import type { ProjectItem } from '../types';
import dayjs from 'dayjs';

interface ColumnsProps {
   styles: Record<string, string>;
   handleDeleteProject: (projectId: number) => void;
   deletingProjectId: number | null;
   isDeletingProject: boolean;
}

export const useAuditListTableColumns = ({
   styles,
   handleDeleteProject,
   deletingProjectId,
   isDeletingProject,
}: ColumnsProps) => {
   const isMobile = useIsMobile();
   const navigate = useNavigate();
   const { modal } = App.useApp();

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
         title: '上传时间',
         dataIndex: 'uploadTime',
         key: 'uploadTime',
         align: 'center',
         width: 120,
         render: (text) => (text ? dayjs(text).format('YYYY-MM-DD') : '-'),
      },
      {
         title: '版本',
         dataIndex: 'version',
         key: 'version',
         align: 'center',
         width: 80,
         render: (version: number | null) => `V${version ?? '-'}`,
      },
      {
         title: '审核状态',
         key: 'auditStatus',
         align: 'center',
         width: 110,
         render: (_, record) => (
            <StatusTag
               parseStatus={record.parseStatus}
               auditResult={record.auditResult}
            />
         ),
      },
      {
         title: '审核人',
         dataIndex: 'auditorName',
         key: 'auditorName',
         align: 'center',
         width: 110,
         ellipsis: true,
         render: (text: string) => <EllipsisTooltip text={text ?? '-'} />,
      },
      {
         title: '操作',
         key: 'actions',
         align: 'center',
         width: 60,
         fixed: 'right',
         render: (_, record) => {
            const deletingCurrent =
               isDeletingProject && deletingProjectId === record.projectId;

            const items: MenuProps['items'] = [
               {
                  key: 'detail',
                  icon: <EyeOutlined />,
                  label: '查看详情',
               },
               {
                  key: 'upload',
                  icon: <UploadOutlined />,
                  label: '上传新版本',
               },
               { type: 'divider' },
               {
                  key: 'delete',
                  icon: <DeleteOutlined />,
                  label: deletingCurrent ? '删除中…' : '删除项目',
                  danger: true,
                  disabled: deletingCurrent,
               },
            ];

            const onClickMenu: MenuProps['onClick'] = ({ key, domEvent }) => {
               domEvent.stopPropagation();
               if (key === 'detail') {
                  navigate(`/bidReview/detail/${record.id}`);
               } else if (key === 'upload') {
                  navigate(`/upload/${record.projectId}`);
               } else if (key === 'delete') {
                  modal.confirm({
                     title: '确定要删除该项目吗？',
                     content:
                        '删除后将同步移除该项目下全部版本与审核记录，且不可恢复。',
                     okText: '确认删除',
                     cancelText: '取消',
                     okButtonProps: { danger: true },
                     onOk: () => handleDeleteProject(record.projectId),
                  });
               }
            };

            return (
               <Dropdown
                  menu={{ items, onClick: onClickMenu }}
                  trigger={['click']}
                  placement='bottomRight'
               >
                  <Button
                     type='text'
                     aria-label='更多操作'
                     icon={<MoreOutlined />}
                     onClick={(e) => e.stopPropagation()}
                     className={styles.actionLinkBtn}
                  />
               </Dropdown>
            );
         },
      },
   ];

   return columns;
};