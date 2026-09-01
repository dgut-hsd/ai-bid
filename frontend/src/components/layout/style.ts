import { createStyles } from 'antd-style';

export const useHeaderStyle = createStyles(({ token, css }) => ({
   header: css`
      padding: 0 1.5rem;
      background: ${token.colorBgContainer};
      border-bottom: 1px solid ${token.colorBorderSecondary};
      display: flex;
      align-items: center;
      justify-content: space-between;
      position: sticky;
      height: 6vh;
      min-height: 50px !important;
      max-height: 60px !important;
      /* 覆盖 antd Header 默认 line-height:64px，避免头像与左侧信息不在同一水平线 */
      line-height: normal;
      top: 0;
      z-index: 999;
   `,
   // 左侧容器（折叠按钮 + 面包屑）：可收缩、不挤压右侧工具栏
   headerLeft: css`
      flex: 1 1 auto;
      min-width: 0;
      overflow: hidden;
   `,
   // 面包屑单个条目：长文件名在此截断为省略号，而不是把顶栏撑爆
   crumbLabel: css`
      display: inline-block;
      max-width: 260px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      vertical-align: middle;

      @media (max-width: 768px) {
         max-width: 120px;
      }
   `,
   trigger: css`
      font-size: 1.5rem;
      line-height: 1;
      cursor: pointer;
      transition: all 0.3s ease;
      color: ${token.colorTextSecondary}; /* 默认次要文本色 */
      padding: 0.8rem 1rem;
      border-radius: 4px;

      &:hover {
         color: ${token.colorPrimary}; /* Hover 时变为主题绿 */
         background-color: ${token.colorFillTertiary}; /* Hover 时加一点极浅的底色反馈 */
      }
   `,
   userTrigger: css`
      cursor: pointer;
      padding: 4px 8px;
      line-height: 1;
      border-radius: 6px;
      transition: background-color 0.2s ease;

      &:hover {
         background-color: ${token.colorFillTertiary};
      }
   `,
}));

export const useSidebarStyle = createStyles(({ token, css, isDarkMode }) => {
   const selectedColor = isDarkMode ? '#81C784' : token.colorPrimary;

   return {
      sider: css`
         background: ${token.colorBgLayout};
         border-right: 1px solid ${token.colorBorderSecondary};
         height: 100vh;
         position: sticky;
         top: 0;
         left: 0;
         z-index: 20;
      `,
      menu: css`
         background: transparent !important;
         border-right: none !important;
         padding-top: 8px;

         .ant-menu-item {
            border-radius: 0 !important;
            margin-inline: 0 !important;
            margin-block: 4px !important;
            width: 100% !important; /* 确保宽度撑满 */

            // --- 核心修复：处理折叠后的图标居中 ---
            &.ant-menu-item-only-child,
            .ant-menu-item-icon {
               transition: all 0.3s;
            }
         }

         // 当 Sider 处于 collapsed 状态时（AntD 会在 Menu 上加这个类）
         &.ant-menu-inline-collapsed {
            width: 80px; // 或者你 Sider 设置的 collapsedWidth

            .ant-menu-item {
               padding-inline: 0 !important; // 清除默认的左右内边距
               display: flex;
               justify-content: center;
               align-items: center;

               .ant-menu-item-icon {
                  margin: 0 !important; // 清除图标默认的 margin
                  line-height: 0; // 防止行高导致微小的垂直偏移
                  font-size: 18px; // 保持图标大小一致
               }

               // 隐藏折叠后的文字残留（如果有的话）
               .ant-menu-title-content {
                  display: none;
               }
            }

            // 修复选中态的左侧竖线在折叠时的位置
            .ant-menu-item-selected {
               &::before {
                  content: '';
                  position: absolute;
                  left: 0;
                  top: 0;
                  bottom: 0;
                  width: 4px;
                  background: ${selectedColor};
               }
            }
         }
      `,
      mobileNav: css`
         position: fixed;
         bottom: 0;
         left: 0;
         right: 0;
         height: 60px;
         background-color: ${token.colorBgContainer};
         border-top: 1px solid ${token.colorBorderSecondary};
         display: flex;
         justify-content: space-around;
         align-items: center;
         z-index: 1000;
         padding-bottom: env(safe-area-inset-bottom); /* 适配异形屏底部安全区 */
      `,
      mobileNavItem: css`
         display: flex;
         flex-direction: column;
         align-items: center;
         justify-content: center;
         flex: 1;
         height: 100%;
         color: ${token.colorTextDescription};
         font-size: 12px;
         gap: 4px;
         cursor: pointer;
         transition: color 0.3s;
         &.active {
            color: ${token.colorPrimary};
         }
      `,
   };
});
