import { Tag } from 'antd';
import type { ParseStatusType } from '../types';

export interface StatusMeta {
   text: string;
   color: string;
}

export function getStatusMeta(
   parseStatus: ParseStatusType,
   auditResult?: string | null
): StatusMeta {
   switch (parseStatus) {
      case 0:
         return { text: '待审核', color: 'blue' };
      case 1:
         return { text: '审核中', color: 'orange' };
      case 3:
         return { text: '审核失败', color: 'red' };
      case 2:
         return auditResult === 'pass'
            ? { text: '已通过', color: 'green' }
            : { text: '需修改', color: 'gold' };
      default:
         return { text: '未知', color: 'default' };
   }
}

interface StatusTagProps {
   parseStatus: ParseStatusType;
   auditResult?: string | null;
}

export const StatusTag: React.FC<StatusTagProps> = ({
   parseStatus,
   auditResult,
}) => {
   const meta = getStatusMeta(parseStatus, auditResult);
   return <Tag color={meta.color}>{meta.text}</Tag>;
};