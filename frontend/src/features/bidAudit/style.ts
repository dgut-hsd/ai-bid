import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => ({
   pageContainer: css`
      background-color: ${token.colorBgLayout};
      min-height: 100%;

      .ant-tabs-tab-btn {
         font-size: 1.2rem;
      }
   `,
   filterSection: css`
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      padding: 1rem;
      border-bottom: 1px solid #eee;
      background-color: ${token.colorBgContainer};
   `,
   // 移动端：单行搜索 + 吸顶
   mobileFilterSticky: css`
      position: sticky;
      top: 0;
      z-index: 100;
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 8px 12px;
      background-color: ${token.colorBgContainer};
      border-bottom: 1px solid ${token.colorBorderSecondary};
   `,
   mobileSearchInput: css`
      flex: 1;
   `,
   tableContainer: css`
      background-color: ${token.colorBgContainer};
      padding: 1rem;

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
   actionLinkBtn: css`
      padding: 0 !important;
      height: auto !important;
      line-height: 1.2 !important;
      font-size: 1.2rem !important;
   `,
   actionSeparator: css`
      color: ${token.colorTextTertiary};
      user-select: none;
   `,

   // ─── 移动端：表格 → 卡片（只显示项目名称 + 状态 + 操作） ───
   mobileListContainer: css`
      padding: 12px;
   `,
   mobileCardList: css`
      display: flex;
      flex-direction: column;
      gap: 12px;
      width: 100%;
   `,
   auditCard: css`
      background: ${token.colorBgContainer};
      border: 1px solid ${token.colorBorderSecondary};
      border-radius: ${token.borderRadiusLG}px;
      padding: 12px 14px;
      box-shadow: 0 1px 2px rgba(0, 0, 0, 0.03);

      &:active {
         box-shadow: 0 2px 8px rgba(0, 0, 0, 0.06);
      }
   `,
   auditCardHeader: css`
      display: flex;
      justify-content: space-between;
      align-items: flex-start;
      gap: 8px;
   `,
   auditCardName: css`
      font-size: 1.5rem;
      font-weight: 600;
      color: ${token.colorText};
      line-height: 1.4;
      word-break: break-all;
      min-width: 0;
   `,
   auditCardStatus: css`
      flex-shrink: 0;
   `,
   auditCardActions: css`
      display: flex;
      align-items: center;
      gap: 4px;
      flex-wrap: wrap;
      margin-top: 10px;
      padding-top: 10px;
      border-top: 1px solid ${token.colorBorderSecondary};
   `,
}));
