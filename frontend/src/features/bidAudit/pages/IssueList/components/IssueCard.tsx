import React from 'react';
import { Tag } from 'antd';
import { EllipsisTooltip } from '@/components/EllipsisTooltip/EllipsisTooltip';
import { useStyles } from '../style';
import type { AuditIssue } from '../types';

interface IssueCardProps {
   issue: AuditIssue;
}

interface SeverityMeta {
   text: string;
   color?: string;
}

export const IssueCard: React.FC<IssueCardProps> = ({ issue }) => {
   const { styles, theme } = useStyles();

   const severity: SeverityMeta = (() => {
      if (issue.isCritical) return { text: '重大', color: theme.colorError };
      switch (issue.severity) {
         case 'high':
            return { text: '高风险', color: theme.colorError };
         case 'medium':
            return { text: '中风险', color: theme.colorWarning };
         case 'low':
            return { text: '低风险', color: theme.colorWarning };
         case 'info':
            return { text: '信息', color: theme.colorPrimary };
         default:
            return { text: issue.severity || '未知' };
      }
   })();

   const page = issue.location?.pageNumber ?? issue.anchorPage;

   return (
      <div className={styles.mobileCard}>
         <div className={styles.mobileCardHeader}>
            <span className={styles.mobileCardTitle}>
               {issue.issueNo ? `问题 ${issue.issueNo}` : '问题详情'}
            </span>
            <div className={styles.mobileCardBadges}>
               <Tag color={severity.color} style={{ margin: 0 }}>
                  {severity.text}
               </Tag>
               {issue.category && (
                  <Tag style={{ margin: 0 }}>{issue.category}</Tag>
               )}
            </div>
         </div>

         <div className={styles.mobileCardMeta}>
            <div className={styles.mobileCardDesc}>
               <EllipsisTooltip text={issue.description} />
            </div>
            <div className={styles.mobileCardMetaRow}>
               <span>位置：{page ? `第 ${page} 页` : '-'}</span>
               {issue.agentName && <span>来源：{issue.agentName}</span>}
            </div>
         </div>
      </div>
   );
};