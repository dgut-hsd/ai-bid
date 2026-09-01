import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => ({
   pageContainer: css`
      display: flex;
      flex-direction: column;
      height: 100%;
   `,

   mainContent: css`
      display: flex;

      @media (max-width: 768px) {
         flex-direction: column;
      }
   `,

   leftColumn: css`
      display: flex;
      flex: 0 0 75%;
      flex-direction: column;
      gap: 16px;
      padding: 1rem 2rem;
      min-width: 0;
   `,

   rightColumn: css`
      flex: 0 0 25%;
      padding: 1rem 2rem;
      min-width: 0;
      display: flex;
      flex-direction: column;
      gap: 1rem;
   `,

   statCardsContainer: css`
      display: grid;
      grid-template-columns: repeat(4, 1fr);
      gap: 16px;

      @media (max-width: 768px) {
         grid-template-columns: repeat(2, 1fr);
      }
   `,

   headerActions: css`
      display: flex;
      width: 100%;
      gap: 8px;

      @media (max-width: 768px) {
         button {
            flex: 1;
         }
      }
   `,

   buttonContainer: css`
      display: flex;
      gap: 8px;
      justify-content: flex-end;
      margin-top: 16px;
   `,

   cardWrapper: css`
      border-radius: 8px;
   `,

   // 表格容器，用于处理表格横向滚动
   tableContainer: css`
      width: 100%;

      .ant-table {
         font-size: 1.2rem;
      }

      .ant-table-thead > tr > th {
         background-color: ${token.colorFillAlter} !important;
         color: ${token.colorText};
         font-weight: 600;
         font-size: 1.3rem;
         padding: 8px 8px;
      }

      .ant-table-tbody > tr > td {
         padding: 8px 8px;
      }

      .action-space {
         gap: 4px;

         button {
            font-size: 1.2rem;
            padding: 6px 8px;
         }
      }
   `,

   statusTag: css`
      text-align: center;
      font-size: 1.2rem;
      font-weight: 500;
      letter-spacing: 1px;
   `,

   chartCard: css`
      border-radius: 8px;
      padding: 1.5rem 1rem;
      box-shadow: 0 2px 8px rgba(0, 0, 0, 0.08);
      /* 去掉 height:100%：它和 flex:1 冲突，会让每张卡片撑到整列高度导致面板超高 */
      min-height: 0;
      min-width: 0;
      flex: 1;
      display: flex;
      flex-direction: column;
   `,

   actionSpace: css`
      button {
         padding: 0 2px;
      }
   `,
}));
