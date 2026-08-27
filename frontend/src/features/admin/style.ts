import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => ({
   mobileCardList: css`
      display: flex;
      flex-direction: column;
      gap: 10px;
   `,
   userCard: css`
      background: ${token.colorBgContainer};
      border: 1px solid ${token.colorBorderSecondary};
      border-radius: ${token.borderRadiusLG}px;
      padding: 12px;
   `,
   userCardHeader: css`
      display: flex;
      justify-content: space-between;
      align-items: flex-start;
      gap: 8px;
      margin-bottom: 8px;
   `,
   userCardName: css`
      flex: 1;
      font-size: 14px;
      font-weight: 600;
      color: ${token.colorText};
      line-height: 1.4;
      word-break: break-all;
      display: -webkit-box;
      -webkit-line-clamp: 2;
      -webkit-box-orient: vertical;
      overflow: hidden;
   `,
   userCardBadges: css`
      display: flex;
      gap: 4px;
      flex-shrink: 0;
   `,
   userCardMeta: css`
      font-size: 12px;
      color: ${token.colorTextSecondary};
      line-height: 1.6;
      margin-bottom: 8px;
   `,
   userCardActions: css`
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding-top: 8px;
      border-top: 1px solid ${token.colorBorderSecondary};

      .ant-btn-link {
         padding: 0 8px;
         font-size: 13px;
      }
   `,
}));