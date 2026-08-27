import { createStyles } from 'antd-style';

/**
 * 系统管理模块样式 — 移动端表格→卡片
 * 桌面端沿用页面内联样式 + antd Card，此处仅提供移动端卡片样式。
 */
export const useStyles = createStyles(({ css, token }) => ({
   mobileCardList: css`
      display: flex;
      flex-direction: column;
      gap: 12px;
      width: 100%;
   `,
   mobileCard: css`
      background: ${token.colorBgContainer};
      border: 1px solid ${token.colorBorderSecondary};
      border-radius: ${token.borderRadiusLG}px;
      padding: 12px 14px;
      box-shadow: 0 1px 2px rgba(0, 0, 0, 0.03);

      &:active {
         box-shadow: 0 2px 8px rgba(0, 0, 0, 0.06);
      }
   `,
   mobileCardHeader: css`
      display: flex;
      justify-content: space-between;
      align-items: flex-start;
      gap: 8px;
   `,
   mobileCardTitle: css`
      font-size: 1.4rem;
      font-weight: 600;
      color: ${token.colorText};
      line-height: 1.4;
      word-break: break-all;
   `,
   mobileCardBadges: css`
      display: flex;
      align-items: center;
      gap: 6px;
      flex-wrap: wrap;
      flex-shrink: 0;
   `,
   mobileCardMeta: css`
      display: flex;
      flex-direction: column;
      gap: 6px;
      margin-top: 10px;
      font-size: 1.2rem;
      color: ${token.colorTextSecondary};
   `,
   mobileCardMetaRow: css`
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
      flex-wrap: wrap;
   `,
   mobileCardActions: css`
      display: flex;
      align-items: center;
      gap: 4px;
      flex-wrap: wrap;
      margin-top: 10px;
      padding-top: 10px;
      border-top: 1px solid ${token.colorBorderSecondary};
   `,
}));