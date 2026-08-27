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

   // 移动端：卡片式列表
   mobileListContainer: css`
      padding: 12px;
      padding-bottom: 24px;
   `,
   mobileCardList: css`
      display: flex;
      flex-direction: column;
      gap: 10px;
   `,
   auditCard: css`
      background: ${token.colorBgContainer};
      border: 1px solid ${token.colorBorderSecondary};
      border-radius: ${token.borderRadiusLG}px;
      padding: 12px;
      transition: box-shadow 0.2s ease;

      &:active {
         transform: scale(0.98);
      }
   `,
   auditCardHeader: css`
      display: flex;
      justify-content: space-between;
      align-items: flex-start;
      gap: 8px;
      margin-bottom: 8px;
   `,
   auditCardName: css`
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
   auditCardBadges: css`
      display: flex;
      gap: 4px;
      flex-shrink: 0;
   `,
   auditCardMeta: css`
      display: flex;
      flex-wrap: wrap;
      gap: 4px 12px;
      font-size: 12px;
      color: ${token.colorTextSecondary};
      line-height: 1.5;
      margin-bottom: 8px;
   `,
   auditCardActions: css`
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
