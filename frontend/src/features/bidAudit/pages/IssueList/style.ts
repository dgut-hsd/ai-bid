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

      /* 移动端：横向滑动单行，不再挤成 4 列 */
      @media (max-width: 768px) {
         grid-template-columns: none;
         grid-auto-flow: column;
         grid-auto-columns: 150px;
         overflow-x: auto;
         gap: 10px;
         padding: 0 12px 4px;
         scrollbar-width: none;
         &::-webkit-scrollbar {
            display: none;
         }
      }
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

   // 移动端：问题卡片列表
   issueCardList: css`
      display: flex;
      flex-direction: column;
      gap: 10px;
   `,
   issueCard: css`
      background: ${token.colorBgContainer};
      border: 1px solid ${token.colorBorderSecondary};
      border-radius: ${token.borderRadiusLG}px;
      padding: 12px;
   `,
   issueCardHeader: css`
      display: flex;
      gap: 6px;
      align-items: center;
      flex-wrap: wrap;
      margin-bottom: 8px;
   `,
   issueCardDesc: css`
      font-size: 14px;
      color: ${token.colorText};
      line-height: 1.6;
      word-break: break-word;
      display: -webkit-box;
      -webkit-line-clamp: 3;
      -webkit-box-orient: vertical;
      overflow: hidden;
      margin-bottom: 8px;
   `,
   issueCardMeta: css`
      font-size: 12px;
      color: ${token.colorTextSecondary};
      line-height: 1.5;
   `,
}));
