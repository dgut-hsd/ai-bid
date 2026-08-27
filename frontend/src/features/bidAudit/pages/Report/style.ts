import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => ({
   // 页面整体容器
   pageContainer: css`
      display: flex;
      flex-direction: column;
      height: 100vh;
      background-color: ${token.colorBgLayout};
      overflow: hidden;

      @media print {
         height: auto !important;
         overflow: visible !important;
         display: block !important;
      }
   `,
   // 顶部操作栏
   headerContainer: css`
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding: 0.5rem 1rem 2rem 1rem;
      background-color: ${token.colorBgContainer};
      border-bottom: 1px solid ${token.colorBorder};
      box-shadow: 0 2px 8px rgba(0, 0, 0, 0.04);
      z-index: 10;
      flex-shrink: 0;

      /* 核心：触发浏览器打印时，强制隐藏顶部栏 */
      @media print {
         display: none !important;
      }

      /* 移动端：适当减小内边距，允许换行 */
      @media (max-width: 768px) {
         padding: 1rem;
         flex-wrap: wrap;
         gap: 1rem;
      }
   `,
   // 缩放控制器容器
   zoomControl: css`
      display: flex;
      align-items: center;
      gap: 12px;
      width: 240px;
      color: ${token.colorTextDescription};

      /* 移动端：隐藏缩放条（移动端可通过双指缩放查看，UI 占用太大） */
      @media (max-width: 768px) {
         display: none;
      }
   `,
   // 主体内容区（预留给 B 和 C）
   mainContent: css`
      flex: 1;
      display: flex;
      overflow: hidden;
      position: relative;

      /* 移动端：双栏变为上下堆叠，允许主体自身滚动 */
      @media (max-width: 768px) {
         flex-direction: column;
         overflow-y: auto;
      }

      @media print {
         height: auto !important;
         overflow: visible !important;
         display: block !important;
      }
   `,

   // ================= B 区：A4 纸张预览区 =================
   previewArea: css`
      flex: 1;
      height: 100%;
      overflow-y: auto; /* 独立滚动条 */
      background-color: ${token.colorBgLayout};
      display: flex;
      justify-content: center; /* 居中纸张 */
      align-items: flex-start;
      padding: 40px 0; /* 上下留出呼吸空间 */

      /* 打印时：取消滚动，占满全屏，隐藏滚动条 */
      @media print {
         flex: 1 1 100%;
         padding: 0;
         overflow: visible;
         display: block;
         height: auto;
         background-color: white;
      }

      /* 移动端：纸张自适应宽度，纵向堆叠阅读，不再横向滚动 */
      @media (max-width: 768px) {
         flex: none;
         height: auto;
         width: 100%;
         padding: 16px 12px;
         justify-content: center;
         overflow-x: hidden;
      }
   `,

   a4Paper: css`
      width: 210mm;
      min-height: 297mm;
      height: max-content;
      background-color: #ffffff;
      /* 模拟真实的 A4 纸张打印边距 (上下 25.4mm，左右 31.8mm 类似 Word 默认) */
      padding: 25.4mm 31.8mm;
      box-shadow: 0 4px 24px rgba(0, 0, 0, 0.08);
      transform-origin: top center; /* 缩放基准点设为顶部居中 */
      transition: transform 0.2s ease-out; /* 缩放时加个平滑动画 */

      /* 内部 Markdown 排版美化 */
      color: ${token.colorTextBase};
      font-size: 14px;
      line-height: 1.8;
      /* 长 URL / 无空格长串在固定 A4 宽度内自动换行，避免内容横向溢出 */
      overflow-wrap: break-word;

      a {
         overflow-wrap: anywhere; /* URL 链接：需要时可在任意字符处断行 */
         word-break: break-all;
      }

      h1 {
         font-size: 24px;
         text-align: center; /* 一级标题居中 (如封面、模块名) */
         margin-top: 32px;
         margin-bottom: 24px;
         color: ${token.colorTextHeading};
      }

      h1:first-child {
         margin-top: 0; /* 修正首个标题的间距 */
      }

      h2 {
         font-size: 18px;
         margin-top: 24px;
         margin-bottom: 16px;
         border-bottom: 1px solid ${token.colorSplit};
         padding-bottom: 8px;
      }
      h3 {
         font-size: 16px;
         margin-top: 16px;
         font-weight: bold;
      }

      ul,
      ol {
         padding-left: 24px;
      }
      li {
         margin-bottom: 8px;
      }

      /* 表格样式美化 */
      table {
         width: 100%;
         border-collapse: collapse;
         margin: 20px 0;
         font-size: 13px;
      }
      th,
      td {
         border: 1px solid ${token.colorBorder};
         padding: 10px 14px;
         text-align: left;
      }
      th {
         background-color: ${token.colorFillAlter};
         font-weight: 600;
      }

      /* 打印时：抹除阴影，强制缩放回 100% 防止变形 */
      @media print {
         width: 100%;
         min-height: auto;
         margin: 0;
         padding: 0;
         box-shadow: none;
         transition: none !important;
         transform: none !important;

         table,
         h2,
         h3 {
            page-break-inside: avoid; /* 旧版浏览器支持 */
            break-inside: avoid; /* 现代浏览器支持 */
         }
      }

      /* 移动端：纸张宽度铺满屏宽、禁用缩放，缩小内边距便于阅读 */
      @media (max-width: 768px) {
         width: 100%;
         min-height: auto;
         margin: 0 auto;
         padding: 12mm 8mm;
         box-shadow: none;
         transform: none !important;
      }
   `,

   // ================= C 区：右侧导出设置区 =================
   settingsArea: css`
      width: 260px;
      background-color: ${token.colorBgContainer};
      border-left: 1px solid ${token.colorBorder};
      padding: 24px;
      display: flex;
      flex-direction: column;
      gap: 32px; /* 模块间大间距 */
      overflow-y: auto;

      /* 核心：打印时不可见！ */
      @media print {
         display: none !important;
      }

      /* 移动端：堆叠在底部，取消左边框，增加顶边框 */
      @media (max-width: 768px) {
         flex: none;
         width: 100%;
         border-left: none;
         border-top: 1px solid ${token.colorBorder};
         padding: 20px 16px;
         overflow: visible; /* 让父容器 mainContent 接管整体滚动 */
      }
   `,

   settingSection: css`
      display: flex;
      flex-direction: column;
      gap: 1rem;
   `,

   settingLabel: css`
      font-weight: 600;
      color: ${token.colorTextBase};
      font-size: 14px;
      position: relative;
      padding-left: 10px;

      /* 加个左侧小竖线装饰，提升企业级 UI 质感 */
      &::before {
         content: '';
         position: absolute;
         left: 0;
         top: 50%;
         transform: translateY(-50%);
         width: 3px;
         height: 14px;
         background-color: ${token.colorPrimary};
         border-radius: 2px;
      }
   `,

   checkboxGroup: css`
      display: grid;
      grid-template-columns: repeat(2, 1fr);
      gap: 0.8rem;

      .ant-checkbox-wrapper {
         margin-inline-start: 0 !important; /* 强制对齐，修复 antd 换行错位 */
      }

      /* 移动端极小屏幕时，自动退化为单列 */
      @media (max-width: 380px) {
         grid-template-columns: 1fr;
      }
   `,
}));
