import { useRef, useState } from 'react';

import {
   Drawer,
   Timeline,
   Card,
   Descriptions,
   Button,
   List,
   Popconfirm,
   Space,
   type DescriptionsProps,
} from 'antd';

import { useNavigate } from 'react-router-dom';

import { StatusTag } from '@/features/bidAudit/components/StatusTag';
import type { ProjectItem } from '@/features/bidAudit/types';
import { CloseOutlined, PlusOutlined } from '@ant-design/icons';
import { useIsMobile } from '@/hooks/useMediaQuery';
import { EllipsisTooltip } from '@/components/EllipsisTooltip/EllipsisTooltip';
import dayjs from 'dayjs';
import { useStyles } from './style';

interface VersionDrawerProps {
   open: boolean;
   onClose: () => void;
   versions: ProjectItem[];
   isFetching: boolean;
   /** 所属项目 ID（用于「上传新版本」跳转）；老调用方可省略 */
   projectId?: number | null;
   onDeleteVersion?: (versionId: number) => void;
   deletingVersionId?: number | null;
   isDeletingVersion?: boolean;
}

export const VersionDrawer = ({
   open,
   onClose,
   versions,
   isFetching,
   projectId,
   onDeleteVersion,
   deletingVersionId,
   isDeletingVersion,
}: VersionDrawerProps) => {
   const cardRefs = useRef<(HTMLDivElement | null)[]>([]);
   const navigate = useNavigate();
   const isMobile = useIsMobile();
   const { styles } = useStyles();

   const [drawerWidth] = useState<string | number>('72vw');

   const handleJump = (index: number) => {
      cardRefs.current[index]?.scrollIntoView({
         behavior: 'smooth',
         block: 'start',
      });
   };

   const getDescriptionsItems = (
      item: ProjectItem
   ): DescriptionsProps['items'] => [
      {
         key: 'bidName',
         label: '文件名',
         children: <EllipsisTooltip text={item.bidName || '-'} />,
         span: isMobile ? 1 : 2,
      },
      { key: 'fileSize', label: '文件大小', children: `${item.fileSize}B` },
      {
         key: 'uploadTime',
         label: '上传时间',
         children: item.uploadTime
            ? dayjs(item.uploadTime).format('YYYY-MM-DD')
            : '-',
      },
      { key: 'pageCount', label: '页数', children: item.pageCount },
      {
         key: 'parseStatus',
         label: '审核状态',
         children: (
            <StatusTag parseStatus={item.parseStatus} />
         ),
      },
      { key: 'auditorName', label: '审核人', children: item.auditorName },
   ];

   return (
      <Drawer
         title={'项目历史版本'}
         open={open}
         onClose={onClose}
         placement={isMobile ? 'top' : 'right'}
         extra={
            projectId != null ? (
               <Button
                  type='primary'
                  icon={<PlusOutlined />}
                  onClick={() => navigate(`/upload/${projectId}`)}
               >
                  上传新版本
               </Button>
            ) : null
         }
         closeIcon={
            <CloseOutlined
               style={{
                  fontSize: '2rem',
                  color: 'green',
               }}
            />
         }
         width={isMobile ? '100%' : drawerWidth}
         height={isMobile ? '70vh' : '100%'}
         loading={isFetching}
         styles={{
            body: { padding: '1rem 1.5rem', scrollbarWidth: 'none' },
            header: { padding: '1.5rem' },
         }}
      >
         <div style={{ display: 'flex', gap: isMobile ? '0.5rem' : '0.75rem' }}>
            <div style={{ minWidth: isMobile ? 70 : 80, flexShrink: 0 }}>
               <div style={{ position: 'sticky', top: 0 }}>
                  <List
                     header={<strong>版本目录</strong>}
                     dataSource={versions}
                     renderItem={(item, index) => (
                        <List.Item style={{ padding: '2px 0', border: 'none' }}>
                           <Button
                              type='link'
                              onClick={() => handleJump(index)}
                           >
                              V{item.version}
                           </Button>
                        </List.Item>
                     )}
                     style={{
                        display: 'flex',
                        flexDirection: 'column',
                        alignItems: 'center',
                     }}
                  />
               </div>
            </div>

            <div style={{ flex: 1 }}>
               <Timeline
                  className={styles.timeline}
                  items={versions.map((item, index) => ({
                     key: item.id,
                     children: (
                        <div
                           ref={(el) => {
                              cardRefs.current[index] = el;
                           }}
                        >
                           <Card
                              title={`V${item.version}`}
                              extra={
                                 <Space size='small'>
                                    <Button
                                       style={{ fontSize: '1.2rem' }}
                                       onClick={() =>
                                          navigate(`/bidReview/detail/${item.id}`)
                                       }
                                    >
                                       {index === 0 && item.parseStatus === 0
                                          ? '进入审核'
                                          : '查看审核详情'}
                                    </Button>

                                    {onDeleteVersion && (
                                       <Popconfirm
                                          title='确定删除该版本吗？'
                                          description='删除后不可恢复。'
                                          okText='删除'
                                          cancelText='取消'
                                          okButtonProps={{ danger: true }}
                                          onConfirm={() =>
                                             onDeleteVersion(item.id)
                                          }
                                       >
                                          <Button
                                             danger
                                             loading={
                                                isDeletingVersion &&
                                                deletingVersionId === item.id
                                             }
                                          >
                                             删除版本
                                          </Button>
                                       </Popconfirm>
                                    )}
                                 </Space>
                              }
                              styles={{
                                 header: { padding: '1rem 1.25rem' },
                                 body: {
                                    padding: '0.9rem 1.1rem',
                                 },
                              }}
                           >
                              <Descriptions
                                 size='small'
                                 column={isMobile ? 1 : 3}
                                 items={getDescriptionsItems(item)}
                                 styles={{ label: { fontSize: '1.1rem' } }}
                              />
                           </Card>
                        </div>
                     ),
                  }))}
               ></Timeline>
            </div>
         </div>
      </Drawer>
   );
};