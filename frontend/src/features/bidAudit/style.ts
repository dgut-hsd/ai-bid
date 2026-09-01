import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => ({
   pageContainer: css`
      background-color: ${token.colorBgLayout};
      min-height: 100%;

      /* 顶部状态 Tab 栏：左右留白不再贴边，并拉开各 Tab 之间的块间距 */
      .ant-tabs-nav {
         margin: 0;
         padding: 8px 1.2rem 0;
      }

      .ant-tabs-tab {
         padding-block: 10px 8px;
      }

      .ant-tabs-tab + .ant-tabs-tab {
         margin-inline-start: 24px;
      }

      .ant-tabs-tab-btn {
         font-size: 1.2rem;
      }
   `,
   // 顶部状态 Tab 的计数角标
   tabCount: css`
      display: inline-flex;
      align-items: center;
      justify-content: center;
      min-width: 1.8rem;
      height: 1.8rem;
      padding: 0 6px;
      margin-left: 8px;
      border-radius: 999px;
      background-color: ${token.colorFillSecondary};
      color: ${token.colorTextSecondary};
      font-size: 1.1rem;
      font-weight: 500;
      line-height: 1;
      vertical-align: middle;
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
      align-items: flex-start;
      justify-content: space-between;
      gap: 8px;
   `,
   auditCardName: css`
      flex: 1;
      min-width: 0;
      font-size: 1.5rem;
      font-weight: 600;
      color: ${token.colorText};
      line-height: 1.4;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
   `,
   auditCardVersion: css`
      flex-shrink: 0;
      margin-inline-end: 0;
   `,
   auditCardFooter: css`
      display: flex;
      align-items: center;
      gap: 8px;
      margin-top: 12px;
   `,
   auditCardTime: css`
      flex: 1;
      min-width: 0;
      text-align: center;
      color: ${token.colorTextTertiary};
      font-size: 1.2rem;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
   `,
}));
