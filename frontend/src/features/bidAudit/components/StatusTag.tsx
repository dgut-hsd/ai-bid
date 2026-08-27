import { Tag } from 'antd';
import type { ParseStatusType } from '../types';

export interface StatusMeta {
   text: string;
   color: string;
}

export function getStatusMeta(parseStatus: ParseStatusType): StatusMeta {
   switch (parseStatus) {
      case 0:
         return { text: '待审核', color: 'blue' };
      case 1:
         return { text: '审核中', color: 'orange' };
      case 2:
         return { text: '已完成', color: 'green' };
      case 3:
         return { text: '审核失败', color: 'red' };
      default:
         return { text: '未知', color: 'default' };
   }
}

interface StatusTagProps {
   parseStatus: ParseStatusType;
}

export const StatusTag: React.FC<StatusTagProps> = ({ parseStatus }) => {
   const meta = getStatusMeta(parseStatus);
   return <Tag color={meta.color}>{meta.text}</Tag>;
};