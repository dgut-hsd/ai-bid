import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token, isDarkMode }) => ({
   container: css`
      display: flex;
      flex-direction: column;
      height: calc(100vh - 60px);
      min-height: calc(100vh - 60px);
      margin: -12px;
      position: relative;
      background-color: ${token.colorBgLayout};
      overflow: hidden;

      .ant-tabs-tab-btn {
         font-size: 1.5rem;
      }

      @media (max-width: 768px) {
         // 移动端扣除底部 60px 导航栏，dvh 规避手机地址栏高度抖动
         height: calc(100dvh - 120px);
         min-height: 0;
      }
   `,

   mobileSwitcher: css`
      display: none;

      @media (max-width: 768px) {
         display: block;
         flex-shrink: 0;
         padding: 0.5rem 0.75rem;
         background-color: ${token.colorBgContainer};
         border-bottom: 1px solid ${token.colorBorderSecondary};
      }
   `,

   mainContent: css`
      display: flex;
      gap: 0.5rem;
      padding: 0;
      flex: 1;
      height: 100%;
      min-height: 0;
      overflow: hidden;

      @media (max-width: 768px) {
         flex-direction: column; /* 移动端切换为上下堆叠 */
      }
   `,

   leftPanel: css`
      flex: 0 0 55%;
      min-width: 0;
      min-height: 0;
      display: flex;
      flex-direction: column;
      padding: 0.5rem;
      overflow: hidden;

      @media (max-width: 768px) {
         flex: 1 1 auto;
         width: 100%;
         height: auto;
         min-height: 0;
         border-right: none;
      }
   `,

   pdfPanel: css`
      flex: 1;
      min-height: 0;
      display: flex;
      flex-direction: column;
   `,

   rightPanel: css`
      flex: 0 0 44%;
      min-height: 0;
      min-width: 0;
      position: relative;
      overflow: hidden;
      padding: 0.5rem;
      display: flex;
      flex-direction: column;
      gap: 0.5rem;

      @media (max-width: 768px) {
         flex: 1 1 auto;
         width: 100%;
         height: auto;
         min-height: 0;
      }

      .ant-tabs-nav-list {
         display: grid !important;
         grid-template-columns: 1fr 1fr 1fr !important;
         width: 100% !important;

         .ant-tabs-tab {
            justify-content: center !important;
            margin: 0 !important;
         }
      }

      .ant-tabs {
         height: 100%;
         display: flex;
         flex-direction: column;
      }

      .ant-tabs-content-holder,
      .ant-tabs-content,
      .ant-tabs-tabpane {
         height: 100%;
         overflow: hidden;
      }

      .ant-tabs-tabpane.ant-tabs-tabpane-active {
         display: flex;
         flex-direction: column;
      }

      .ant-tabs-content-holder,
      .ant-tabs-content,
      .ant-tabs-tabpane,
      .ant-tabs-tabpane > div {
         scrollbar-width: none;
         -ms-overflow-style: none;
      }

      .ant-tabs-content-holder::-webkit-scrollbar,
      .ant-tabs-content::-webkit-scrollbar,
      .ant-tabs-tabpane::-webkit-scrollbar,
      .ant-tabs-tabpane > div::-webkit-scrollbar {
         width: 0;
         height: 0;
         display: none;
      }

      *::-webkit-scrollbar {
         width: 0;
         height: 0;
         display: none;
      }
   `,

pdfScrollArea: css`
      flex: 1;
      overflow-y: auto;
      display: flex;
      background-color: ${isDarkMode ? '#1a1a1a' : '#f0f2f5'};
      flex-direction: column;
      align-items: center;
      gap: 24px;
      overscroll-behavior: contain;
   `,

   pageItem: css`
      box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
      background: white;
      transition: width 0.2s ease;

      .react-pdf__Page__canvas {
         width: 100% !important;
         height: auto !important;
      }

      .react-pdf__Page__textContent {
         user-select: text;
         color: transparent !important;
      }

      .react-pdf__Page__textContent span {
         color: transparent !important;
      }

      .react-pdf__Page__textContent .pdf-hit {
         background: rgba(255, 241, 118, 0.5);
         color: transparent !important;
         padding: 0 1px;
         border-radius: 2px;
      }
   `,

   statsGrid: css`
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      gap: 1rem;
   `,

   footer: css`
      // Footer
      position: sticky;
      bottom: 0;
      z-index: 10;

      display: flex;
      justify-content: space-between;
      align-items: center;
      padding: 1rem 2rem;
      background-color: ${token.colorBgContainer};
      border-top: 1px solid ${token.colorBorderSecondary};
      box-shadow: 0 -2px 8px rgba(0, 0, 0, 0.05);

      // 审核结论
      .review-result {
         color: ${token.colorText};
      }

      button {
         font-size: 1.2rem;
         padding: 6px 8px;
      }
   `,

   // pdfToolbar
   toolbar: css`
      background-color: ${token.colorBgContainer};
      padding: 0.4rem 1rem;
      display: flex;
      align-items: center;
      justify-content: center;
      gap: 0.5rem;
      border-top: 1px solid ${token.colorBorderSecondary};
      flex-shrink: 0;
   `,

   actionBtn: css`
      color: ${token.colorPrimary};
      border-color: transparent;
      background: transparent;
      font-size: 12px;
      &:hover {
         background-color: rgba(46, 125, 50, 0.1);
      }
   `,

   severityFilter: css`
      .ant-segmented-item-selected {
         background: #e8f5e9 !important;
      }
   `,

   greenRadio: css`
      .ant-radio-button-wrapper-checked {
         background-color: ${token.colorPrimary};
         border-color: ${token.colorPrimary};
         color: white;
      }
   `,

   // PDF Detail Footer
   detailContainer: css`
      background-color: ${isDarkMode
         ? 'rgba(232, 245, 233, 0.7)'
         : 'rgba(46, 125, 50, 0.15)'};
      backdrop-filter: blur(4px);
      padding: 1rem 1.5rem;
      margin-top: 1rem;
      border-radius: 8px;
      border-bottom: 1px solid ${token.colorBorderSecondary};
      flex-shrink: 0;
   `,

   label: css`
      color: ${token.colorTextDescription};
      margin-right: 8px;
      font-size: 1rem;
   `,

   value: css`
      color: ${token.colorTextBase};
      font-weight: 500;
      font-size: 1rem;
   `,
}));
