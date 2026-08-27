import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => ({
   pageContainer: css`
      display: flex;
      flex-direction: column;
      gap: 1rem;
      height: 100%;
      background-color: ${token.colorBgLayout};
      overflow-y: hidden;
   `,
   headerArea: css`
      display: flex;
      flex-direction: column;
      gap: 12px;
      padding: 0 1rem;
      padding-bottom: 1rem;
      background: ${token.colorBgContainer};
   `,
   infoText: css`
      color: ${token.colorTextSecondary};
      font-size: 1.2rem;
   `,
   statsGrid: css`
      display: grid;
      padding: 0 1rem;
      grid-template-columns: repeat(4, 1fr);
      gap: 16px;
   `,
   tableArea: css`
      background: ${token.colorBgContainer};
      padding: 1rem;
      display: flex;
      flex-direction: column;
      width: 100%;
      gap: 16px;
      flex: 1;

      .ant-table {
         font-size: 1.2rem;
      }

      .ant-table-thead > tr > th {
         background-color: ${token.colorFillAlter} !important;
         color: ${token.colorText};
         font-weight: 600;
         font-size: 1.3rem;
         padding: 8px 12px;
      }

      .ant-table-tbody > tr > td {
         padding: 8px 12px;
      }

      button {
         font-size: 1.2rem;
      }
   `,
   filterBar: css`
      display: flex;
      justify-content: space-between;
      align-items: center;
      flex-wrap: wrap;
      gap: 16px;
   `,
   filterControls: css`
      display: flex;
      align-items: center;
      gap: 12px;

      // 桌面端：水平排列，自然宽度
      > * {
         flex: none;
      }

      // 移动端：垂直三列，每项撑满
      @media (max-width: 768px) {
         flex-direction: column;
         align-items: stretch;
         width: 100%;
      }
   `,
   paginationArea: css`
      display: flex;
      justify-content: space-between;
      align-items: center;
      margin: 0.5rem 0;
      flex-wrap: wrap;
      gap: 16;
   `,

   // ─── 移动端：表格 → 卡片 ───
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
   mobileCardDesc: css`
      font-size: 1.2rem;
      color: ${token.colorTextSecondary};
      line-height: 1.5;
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
