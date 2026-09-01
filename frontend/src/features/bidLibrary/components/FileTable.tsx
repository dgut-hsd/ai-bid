import { Table, Pagination, Tag, Popconfirm } from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { useStyles } from '../style';
import { CategoryMap, ApplicableScopeMap, type KnowledgeFile } from '../types';
import { EllipsisTooltip } from '@/components/EllipsisTooltip/EllipsisTooltip';
import { useIsMobile } from '@/hooks/useMediaQuery';
import { MobileFileCard } from './MobileFileCard';

interface FileTableProps {
   files: KnowledgeFile[]; // 文件列表数据
   total: number; // 总文件数
   currentPage: number; // 当前页码
   pageSize: number; // 每页条数
   loading: boolean;
   onPageChange: (page: number) => void; // 页码变化回调
   onView: (file: KnowledgeFile) => void; // 查看文件详情回调
   onDownload: (file: KnowledgeFile) => void; // 下载文件回调
   onEdit: (file: KnowledgeFile) => void; // 编辑文件回调
   onDelete: (file: KnowledgeFile) => void; // 删除文件回调
}

// --- 文件表格组件 ---
export function FileTable({
   files,
   total,
   currentPage,
   pageSize,
   loading,
   onPageChange,
   onView,
   onDownload,
   onEdit,
   onDelete,
}: FileTableProps) {
   const { styles, theme } = useStyles();
   const isMobile = useIsMobile();

   // --- 表格列配置 ---
   const columns: ColumnsType<KnowledgeFile> = [
      {
         title: '文件名',
         dataIndex: 'fileName',
         key: 'fileName',
         width: isMobile ? 150 : 200,
         fixed: 'left',
         align: 'center',
         ellipsis: true,
         render: (text: string) => <EllipsisTooltip text={text} />,
      },
      {
         title: '文件类型',
         dataIndex: 'category',
         key: 'category',
         width: 130,
         align: 'center',
         render: (category: string) => {
            const isDark = theme.colorBgContainer === '#1d1d1d'; // 判断是否为深色模式
            // 不同分类的颜色配置
            const colors: Record<string, { bg: string; text: string }> = {
               regulation: {
                  bg: theme.colorSuccessBg || '#f6ffed',
                  text: theme.colorSuccess || '#52c41a',
               },
               price: {
                  bg: theme.colorInfoBg || '#e6f7ff',
                  text: theme.colorInfo || '#1890ff',
               },
               supplier: {
                  bg: theme.colorWarningBg || '#fff7e6',
                  text: theme.colorWarning || '#fa8c16',
               },
               contract: {
                  bg: isDark ? 'rgba(114, 46, 209, 0.15)' : '#f9f0ff',
                  text: isDark ? '#b37feb' : '#722ed1',
               },
               case: {
                  bg: isDark ? 'rgba(19, 194, 194, 0.12)' : '#e6fffb',
                  text: isDark ? '#5cdbd3' : '#13c2c2',
               },
               other: {
                  bg: theme.colorBgContainerDisabled || '#f5f5f5',
                  text: theme.colorTextSecondary || '#8c8c8c',
               },
            };
            const color = colors[category] || colors.other;
            return (
               <Tag
                  style={{
                     backgroundColor: color.bg,
                     color: color.text,
                     border: 'none',
                  }}
                  className={styles.fileTypeTag}
               >
                  {CategoryMap[category as keyof typeof CategoryMap]}
               </Tag>
            );
         },
      },
      {
         title: '适用范围',
         dataIndex: 'applicableScope',
         key: 'applicableScope',
         width: 110,
         align: 'center',
         render: (scope: string) => {
             // 如果后端返回了真正的 scope，就进行映射
             if (scope && ApplicableScopeMap[scope as keyof typeof ApplicableScopeMap]) {
                 return ApplicableScopeMap[scope as keyof typeof ApplicableScopeMap];
             }
             // 否则临时处理 tags
             if (scope?.includes('procurement')) return '采购类';
             if (scope?.includes('engineering')) return '工程类';
             return '通用';
         }
      },
      {
         title: '上传人',
         dataIndex: 'uploadUserName', // 后端现在返回了 uploadUserName
         key: 'uploadUserName',
         width: 110,
         align: 'center',
         render: (userName: string) => {
            return userName || '-';
         }
      },
      {
         title: '上传时间',
         dataIndex: 'uploadTime',
         key: 'uploadTime',
         width: 150,
         align: 'center',
      },
      {
         title: '状态',
         dataIndex: 'status',
         key: 'status',
         width: 80,
         align: 'center',
         render: (status: number | string) => {
            const normalized = String(status);
            const enabled =
               normalized === '1' ||
               normalized.toLowerCase() === 'enabled' ||
               normalized === '启用';
            return (
               <Tag
                  className={
                     enabled ? styles.statusTagEnabled : styles.statusTagDisabled
                  }
               >
                  {enabled ? '启用' : '停用'}
               </Tag>
            );
         },
      },
      {
         title: '操作',
         key: 'action',
         width: 240,
         fixed: 'right',
         align: 'center',
         render: (_: unknown, record: KnowledgeFile) => (
            <span style={{ whiteSpace: 'nowrap' }}>
               <span
                  className={styles.actionLink}
                  onClick={() => onView(record)}
               >
                  查看
               </span>
               <span className={styles.actionSeparator}>|</span>
               <span className={styles.actionLink} onClick={() => onDownload(record)}>
                  下载
               </span>
               <span className={styles.actionSeparator}>|</span>
               <span
                  className={styles.actionLink}
                  onClick={() => onEdit(record)}
               >
                  编辑
               </span>
               <span className={styles.actionSeparator}>|</span>
               <Popconfirm
                  title='确定要删除该文件吗？'
                  description='删除后将无法参与审核，且不可恢复，请谨慎操作！'
                  onConfirm={() => onDelete(record)}
                  okText='确认删除'
                  cancelText='取消'
                  okButtonProps={{ danger: true }}
               >
                  <span className={styles.actionLink}>删除</span>
               </Popconfirm>
            </span>
         ),
      },
   ];

   // 移动端：紧凑卡片列表，替代横向滚动的表格
   if (isMobile) {
      return (
         <div className={styles.mobileListContainer}>
            <div className={styles.mobileCardList}>
               {files.map((file) => (
                  <MobileFileCard
                     key={file.id}
                     file={file}
                     styles={styles}
                     onView={onView}
                     onDownload={onDownload}
                     onEdit={onEdit}
                     onDelete={onDelete}
                  />
               ))}
            </div>

            {files.length > 0 && (
               <div
                  style={{
                     display: 'flex',
                     justifyContent: 'center',
                     marginTop: 12,
                  }}
               >
                  <Pagination
                     current={currentPage}
                     pageSize={pageSize}
                     total={total}
                     size='small'
                     showSizeChanger={false}
                     onChange={onPageChange}
                  />
               </div>
            )}
         </div>
      );
   }

   return (
      <div className={styles.tableContainer}>
         {/* 文件表格 */}
         <Table
            columns={columns}
            dataSource={files}
            rowKey='id'
            loading={loading}
            pagination={false}
            bordered={false}
            scroll={{ x: 'max-content' }}
         />

         <div className={styles.paginationRow}>
            <Pagination
               current={currentPage}
               pageSize={pageSize}
               total={total}
               showQuickJumper
               onChange={onPageChange}
            />
         </div>
      </div>
   );
}
