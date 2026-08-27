import { Tag, type TableColumnsType } from 'antd';
import { EllipsisTooltip } from '@/components/EllipsisTooltip/EllipsisTooltip';
import type { GlobalToken } from 'antd';
import type { AuditIssue } from '../types';

export const useTableColumns = (
   currentPage: number = 1,
   pageSize: number = 10,
   theme: GlobalToken
) => {
   const columns: TableColumnsType<AuditIssue> = [
      {
         title: '序号',
         key: 'index',
         width: 70,
         responsive: ['md', 'lg', 'xl', 'xxl'],
         align: 'center',
         render: (_: unknown, __: AuditIssue, index: number) =>
            (currentPage - 1) * pageSize + index + 1,
      },
      {
         title: '严重程度',
         dataIndex: 'severity',
         align: 'center',
         width: 90,
         render: (severity: string, record: AuditIssue) => {
            const config: Record<string, { color: string; text: string }> = {
               high: { color: theme.colorError, text: '高风险' },
               medium: { color: theme.colorWarning, text: '中风险' },
               low: { color: theme.colorWarning, text: '低风险' },
               info: { color: theme.colorPrimary, text: '信息' },
            };
            const current = record.isCritical
               ? { color: theme.colorError, text: '重大' }
               : config[severity] || {
               color: 'default',
               text: '未知',
               };
            return (
               <Tag
                  color={current.color}
                  style={{ fontSize: '1.2rem', padding: '2px 6px' }}
               >
                  {current.text}
               </Tag>
            );
         },
      },
      {
         title: '问题维度',
         dataIndex: 'category',
         align: 'center',
         responsive: ['md', 'lg', 'xl', 'xxl'],
         width: 120,
         render: (cat: string) => cat,
      },
      {
         title: '问题描述',
         dataIndex: 'description',
         width: 300,
         ellipsis: true,
         render: (text: string) => <EllipsisTooltip text={text} />,
      },
      {
         title: '所在位置',
         dataIndex: ['location', 'pageNumber'],
         align: 'center',
         width: 100,
         render: (pageNumber: number) =>
            pageNumber ? `第 ${pageNumber} 页` : '-',
      },
   ];

   return columns;
};
