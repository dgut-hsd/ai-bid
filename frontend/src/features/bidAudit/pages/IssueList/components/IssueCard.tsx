import React from 'react';
import { Tag } from 'antd';
import { useStyles } from '../style';
import type { AuditIssue } from '../types';

interface IssueCardProps {
   issue: AuditIssue;
}

export const IssueCard: React.FC<IssueCardProps> = ({ issue }) => {
   const { styles, theme } = useStyles();

   // 与表格「严重程度」列保持一致的配色/文案
   const severityConfig: Record<string, { color: string; text: string }> = {
      high: { color: theme.colorError, text: '高风险' },
      medium: { color: theme.colorWarning, text: '中风险' },
      low: { color: theme.colorWarning, text: '低风险' },
      info: { color: theme.colorPrimary, text: '信息' },
   };
   const severity = issue.isCritical
      ? { color: theme.colorError, text: '重大' }
      : severityConfig[issue.severity] ?? { color: 'default', text: '未知' };

   const page = issue.location?.pageNumber;
   const section = issue.location?.sectionName;

   return (
      <div className={styles.issueCard}>
         <div className={styles.issueCardHeader}>
            <Tag color={severity.color} style={{ margin: 0 }}>
               {severity.text}
            </Tag>
            <Tag style={{ margin: 0 }}>{issue.category || '-'}</Tag>
         </div>

         <div className={styles.issueCardDesc}>
            {issue.description || '-'}
         </div>

         <div className={styles.issueCardMeta}>
            {page ? `第 ${page} 页` : '-'}
            {section ? ` · ${section}` : ''}
         </div>
      </div>
   );
};