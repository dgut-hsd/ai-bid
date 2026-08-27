import { useNavigate } from 'react-router-dom';
import { App, Button, Space } from 'antd';
import type { ColumnsType } from 'antd/es/table';
import {
   DeleteOutlined,
   EyeOutlined,
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
   onView: (projectId: number) => void;
}

export const useAuditListTableColumns = ({
   styles,
   handleDeleteProject,
   deletingProjectId,
   isDeletingProject,
   onView,
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
            <StatusTag parseStatus={record.parseStatus} />
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
         width: 360,
         fixed: 'right',
         render: (_, record) => {
            const deletingCurrent =
               isDeletingProject && deletingProjectId === record.projectId;

            const confirmDelete = () => {
               modal.confirm({
                  title: '确定要删除该项目吗？',
                  content:
                     '删除后将同步移除该项目下全部版本与审核记录，且不可恢复。',
                  okText: '确认删除',
                  cancelText: '取消',
                  okButtonProps: { danger: true },
                  onOk: () => handleDeleteProject(record.projectId),
               });
            };

            return (
               <Space size='small' style={{ whiteSpace: 'nowrap' }}>
                  <Button
                     type='link'
                     size='small'
                     icon={<EyeOutlined />}
                     className={styles.actionLinkBtn}
                     onClick={(e) => {
                        e.stopPropagation();
                        onView(record.projectId);
                     }}
                  >
                     查看详情
                  </Button>

                  <Button
                     type='link'
                     size='small'
                     icon={<UploadOutlined />}
                     className={styles.actionLinkBtn}
                     onClick={(e) => {
                        e.stopPropagation();
                        navigate(`/upload/${record.projectId}`);
                     }}
                  >
                     上传新版本
                  </Button>

                  <Button
                     type='link'
                     size='small'
                     danger
                     icon={<DeleteOutlined />}
                     loading={deletingCurrent}
                     className={styles.actionLinkBtn}
                     onClick={(e) => {
                        e.stopPropagation();
                        confirmDelete();
                     }}
                  >
                     {deletingCurrent ? '删除中…' : '删除项目'}
                  </Button>
               </Space>
            );
         },
      },
   ];

   return columns;
};