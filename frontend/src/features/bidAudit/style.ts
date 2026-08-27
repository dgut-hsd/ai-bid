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
}));
