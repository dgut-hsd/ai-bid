import { Tag, Button } from 'antd';
import { useNavigate } from 'react-router-dom';
import type { HistoryRecord, ReviewStatus } from '../types';
import type { ColumnsType } from 'antd/es/table';
import { useStyles } from '../style';
import { EllipsisTooltip } from '@/components/EllipsisTooltip/EllipsisTooltip';

export const useHistoryTableColumns = () => {
   const { styles } = useStyles();
   const navigate = useNavigate();

   const auditResultsMap: Record<
      ReviewStatus,
      { text: string; color: string }
   > = {
      pass: { text: '已通过', color: 'green' },
      revise: { text: '需修改', color: 'orange' },
      reject: { text: '不通过', color: 'red' },
   };

   const columns: ColumnsType<HistoryRecord> = [
      {
         title: '项目名称',
         dataIndex: 'projectName',
         key: 'projectName',
         fixed: 'left',
         ellipsis: true,
         align: 'center',
         width: 150,
         render: (text: string) => <EllipsisTooltip text={text} />,
      },
      {
         title: '文件类型',
         dataIndex: 'fileCategory',
         align: 'center',
         key: 'fileCategory',
         width: 80,
         render: (fileCategory: string) => (
            <Tag color={'green'}>{fileCategory}</Tag>
         ),
      },
      {
         title: '供应商名称',
         dataIndex: 'supplierName',
         key: 'supplierName',
         align: 'center',
         responsive: ['md', 'lg', 'xl', 'xxl'],
         ellipsis: true,
         width: 150,
         render: (text: string) => <EllipsisTooltip text={text} />,
      },
      {
         title: '审核人',
         dataIndex: 'auditUserName',
         align: 'center',
         key: 'auditUserName',
         responsive: ['md', 'lg', 'xl', 'xxl'],
         width: 80,
         ellipsis: true,
         render: (text: string) => <EllipsisTooltip text={text} />,
      },
      {
         title: '审核时间',
         dataIndex: 'endTime',
         key: 'endTime',
         align: 'center',
         responsive: ['md', 'lg', 'xl', 'xxl'],
         width: 150,
      },
      {
         title: '版本号',
         dataIndex: 'issueCount',
         key: 'issueCount',
         align: 'center',
         responsive: ['md', 'lg', 'xl', 'xxl'],
         width: 80,
      },
      {
         title: '审核结果',
         dataIndex: 'auditResult',
         key: 'auditResult',
         align: 'center',
         width: 80,
         render: (auditResult: ReviewStatus) => (
            <Tag
               color={auditResultsMap[auditResult].color}
               className={styles.auditResultsTag}
            >
               {auditResultsMap[auditResult].text}
            </Tag>
         ),
      },
      {
         title: '操作',
         key: 'action',
         align: 'center',
         width: 160,
         fixed: 'right',
         render: (_, record) => (
            <Button
               type='link'
               onClick={() => navigate(`/bidReview/detail/${record.id}`)}
            >
               查看详情
            </Button>
         ),
      },
   ];

   return columns;
};
