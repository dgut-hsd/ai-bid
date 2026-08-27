import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => ({
   // 1. 最外层页面容器
   pageContainer: css`
      padding: 2rem;
      min-height: 100%;
      display: flex;
      flex-direction: column;
      gap: 1rem;

      @media (max-width: 768px) {
         padding: 0;
         gap: 0;
      }
   `,

   // 2. 上部分区：主布局 (分为左右)
   mainLayout: css`
      display: flex;
      gap: 24px;
      align-items: stretch; // 核心：强制左右两部分高度统一！
      max-width: 1920px; // 防止在大尺寸带鱼屏下无限拉长
      width: 100%;

      @media (max-width: 768px) {
         flex-direction: column;
         gap: 0;
      }
   `,

   // --- 左侧：操作区 ---
   leftSection: css`
      flex: 0 0 75%;
      min-width: 0; // 防止内部 flex 子元素撑破容器
      display: flex;
      flex-direction: column;
      gap: 2rem;

      @media (max-width: 768px) {
         gap: 0;
      }
   `,

   // --- 右侧：统计卡片区 ---
   rightSection: css`
      flex: 0 0 25%;
      padding-right: 1rem;
      display: flex;

      @media (max-width: 768px) {
         /* 移动端隐藏统计卡，聚焦列表 */
         display: none;
      }
   `,

   // 3. 左侧内部细节排版

   // [上部分] 搜索与上传
   headerRow: css`
      display: flex;
      align-items: center;
      gap: 16px;
      width: 100%;

      @media (max-width: 768px) {
         /* 移动端：单行吸顶，搜索 + 筛选 + 上传 */
         position: sticky;
         top: 0;
         z-index: 100;
         gap: 8px;
         padding: 8px 12px;
         background: ${token.colorBgContainer};
         border-bottom: 1px solid ${token.colorBorderSecondary};
      }
   `,
   searchInput: css`
      flex: 1; // 占据大部分宽度
      height: 35px;
   `,
   uploadBtn: css`
      height: 35px;
      background-color: ${token.colorPrimary};
      border-color: ${token.colorPrimary};
      flex-shrink: 0;

      &:hover {
         background-color: ${token.colorPrimaryHover} !important;
         border-color: ${token.colorPrimaryHover} !important;
      }
   `,

   //[下部分] Tab与筛选器包裹层
   filterColumn: css`
      display: flex;
      flex-direction: column;
      gap: 16px;

      @media (max-width: 768px) {
         gap: 0;
      }
   `,

   // --- 第一行：分类标签 ---
   categoryTabs: css`
      display: flex;
      gap: 1rem 1.5rem;
      flex-wrap: wrap;
      border-bottom: 1px solid ${token.colorBorderSecondary};

      @media (max-width: 768px) {
         /* 移动端：横向滚动单行，不再换行堆积 */
         flex-wrap: nowrap;
         overflow-x: auto;
         padding: 8px 12px 0;
         gap: 0.5rem 1.25rem;
         scrollbar-width: none;
         &::-webkit-scrollbar {
            display: none;
         }
      }
   `,
   categoryTab: css`
      display: flex;
      align-items: center;
      gap: 6px;
      padding: 8px 4px;
      cursor: pointer;
      font-size: 14px;
      color: ${token.colorTextSecondary};
      transition: all 0.2s ease-in;
      margin-bottom: -1px; // 盖住父元素的底边框
      border-bottom: 2px solid transparent;
      white-space: nowrap;

      &:hover {
         color: ${token.colorPrimary};
      }
      &.active {
         color: ${token.colorPrimary};
         border-bottom-color: ${token.colorPrimary};
         font-weight: 500;
      }
   `,

   filterBar: css`
      display: flex;
      flex-wrap: wrap;
      gap: 16px 24px;

      & > div:nth-child(4) {
         flex-basis: 100%;
      }

      @media (max-width: 768px) {
         flex-direction: column;
         align-items: stretch;
         & > div:nth-child(4) {
            flex-basis: auto;
         }
      }
   `,
   filterItem: css`
      display: flex;
      align-items: center;
      gap: 8px;

      @media (max-width: 768px) {
         width: 100%;
         & > * {
            flex: 1;
         }
      }
   `,
   filterLabel: css`
      color: ${token.colorText};
      white-space: nowrap;

      @media (max-width: 768px) {
         text-align: center;
      }
   `,

   // 4. 右侧统计卡片内部排版
   statsCard: css`
      width: 100%;
      height: 100%;
      background: ${token.colorBgContainer};
      border-radius: ${token.borderRadiusLG}px;
      padding: 1.4rem 2rem;
      box-shadow: 0 4px 12px rgba(0, 0, 0, 0.05);
      display: flex;
      flex-direction: column;
      justify-content: center;

      .dark & {
         box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
      }

      @media (max-width: 768px) {
         min-width: 100%;
      }
   `,
   statsRow: css`
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 16px 24px;
      align-items: center;
   `,
   leftColumn: css`
      display: flex;
      flex-direction: column;
      gap: 8px;
   `,
   totalStat: css`
      display: flex;
      flex-direction: column;
      gap: 4px;
      padding-bottom: 8px;
      border-bottom: 1px solid ${token.colorBorderSecondary};
   `,
   totalLabel: css`
      font-size: 14px;
      color: ${token.colorTextSecondary};
   `,
   totalNumber: css`
      font-size: 2.5rem;
      font-weight: 700;
      color: ${token.colorSuccess};
      line-height: 1.1;
   `,
   otherStats: css`
      display: flex;
      flex-direction: column;
      gap: 12px;
   `,
   rightColumn: css`
      display: flex;
      flex-direction: column;
      gap: 12px;
   `,
   statItem: css`
      font-size: 14px;
      display: flex;
      justify-content: space-between;
      align-items: center;
   `,
   statLabel: css`
      white-space: nowrap;
      color: ${token.colorTextSecondary};
   `,
   statValue: css`
      font-weight: 600;
      color: ${token.colorSuccess};
   `,

   // 5. 下部分区：表格
   tableContainer: css`
      flex: 1;
      background: ${token.colorBgContainer};
      border-radius: ${token.borderRadiusLG}px;
      box-shadow: 0 2px 8px rgba(0, 0, 0, 0.05);
      overflow: hidden;

      @media (max-width: 768px) {
         margin: 8px 12px;
      }

      .ant-table {
         font-size: 1.2rem;
      }

      .ant-table-thead > tr > th {
         background-color: ${token.colorFillAlter} !important;
         color: ${token.colorText};
         font-weight: 600;
         font-size: 1.3rem;
         padding: 10px 14px;
      }

      .ant-table-tbody > tr > td {
         padding: 10px 14px;
      }

      .action-space {
         gap: 4px;

         button {
            font-size: 1.2rem;
            padding: 6px 8px;
         }
      }
   `,
   fileTypeTag: css`
      border: none;
      font-weight: 500;
   `,
   statusTagEnabled: css`
      background-color: ${token.colorSuccessBg} !important;
      color: ${token.colorSuccess} !important;
      border: none;
      font-weight: 500;
   `,
   statusTagDisabled: css`
      background-color: ${token.colorBgContainerDisabled} !important;
      color: ${token.colorTextSecondary} !important;
      border: none;
      font-weight: 500;
   `,
   actionLink: css`
      color: ${token.colorPrimary};
      cursor: pointer;
      transition: all 0.2s;
      white-space: nowrap;
      &:hover {
         text-decoration: underline;
      }
   `,
   actionSeparator: css`
      color: ${token.colorBorder};
      margin: 0 8px;
   `,
   paginationRow: css`
      display: flex;
      justify-content: flex-end;
      align-items: center;
      gap: 16px;
      padding: 12px 24px;
      background: ${token.colorBgContainer};
      border-top: 1px solid ${token.colorBorderSecondary};
      flex-wrap: wrap;

      @media (max-width: 768px) {
         justify-content: center;
         padding: 12px;
      }
   `,
   totalText: css`
      font-size: 14px;
      color: ${token.colorTextSecondary};
   `,

   // 6. 移动端：卡片式文件列表
   mobileListContainer: css`
      padding: 12px;
      padding-bottom: 24px;
   `,
   mobileCardList: css`
      display: flex;
      flex-direction: column;
      gap: 10px;
   `,
   mobileCard: css`
      background: ${token.colorBgContainer};
      border: 1px solid ${token.colorBorderSecondary};
      border-radius: ${token.borderRadiusLG}px;
      padding: 12px;
      transition: box-shadow 0.2s ease;

      &:active {
         transform: scale(0.98);
      }
   `,
   mobileCardHeader: css`
      display: flex;
      justify-content: space-between;
      align-items: flex-start;
      gap: 8px;
      margin-bottom: 8px;
   `,
   mobileCardName: css`
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
   mobileCardBadges: css`
      display: flex;
      gap: 4px;
      flex-shrink: 0;
   `,
   mobileCardMeta: css`
      font-size: 12px;
      color: ${token.colorTextSecondary};
      line-height: 1.5;
      margin-bottom: 8px;
   `,
   mobileCardActions: css`
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding-top: 8px;
      border-top: 1px solid ${token.colorBorderSecondary};

      .ant-btn-link {
         color: ${token.colorPrimary};
         padding: 0 8px;
         font-size: 13px;
      }
   `,
}));
