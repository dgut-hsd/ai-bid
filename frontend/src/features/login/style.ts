import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => ({
   loginContainer: css`
      display: flex;
      flex-direction: column;
      width: 100%;
      height: 100%;
      min-height: 100dvh;
      font-family: ${token.fontFamily};

      /* 响应式：大于768px时呈现左右分栏布局 */
      @media (min-width: 768px) {
         flex-direction: row;
      }
   `,

   loginLeftPanel: css`
      /* 移动端隐藏左侧品牌插图，聚焦登录表单，避免表单被挤到首屏之外 */
      display: none;
      width: 100%;
      background: linear-gradient(180deg, #e8f5e9 0%, #ffffff 100%);
      flex-direction: column;
      justify-content: center;
      align-items: center;
      padding: 24px;
      transition: background 0.3s ease;

      .dark & {
         background: linear-gradient(
            180deg,
            rgba(46, 125, 50, 0.1) 0%,
            ${token.colorBgLayout} 100%
         );
      }

      @media (min-width: 768px) {
         display: flex;
         width: 55%;
         min-height: 100vh;
         padding: 40px;
         justify-content: space-between;
         position: relative;
      }
   `,

   loginBrandContent: css`
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      flex: 1;
      padding: 20px 0;
   `,

   loginBrandTitle: css`
      font-size: 24px;
      font-weight: 600;
      color: ${token.colorPrimary};
      margin: 0 0 16px 0;
      text-align: center;

      @media (min-width: 768px) {
         font-size: 32px;
         margin: 0 0 32px 0;
      }
   `,

   loginIllustration: css`
      margin-bottom: 16px;
      svg {
         width: 120px;
         height: 120px;
      }

      @media (min-width: 768px) {
         margin-bottom: 32px;
         svg {
            width: auto;
            height: auto;
         }
      }
   `,

   loginBrandSubtitle: css`
      font-size: 14px;
      color: ${token.colorTextDescription};
      margin: 0;
      text-align: center;

      @media (min-width: 768px) {
         font-size: 16px;
      }
   `,

   loginRightPanel: css`
      width: 100%;
      background-color: ${token.colorBgContainer};
      display: flex;
      justify-content: center;
      align-items: center;
      min-height: 100dvh;
      padding: 24px 16px;
      transition: background-color 0.3s ease;

      /* 手机端保留浅绿渐变背景：左侧品牌面板在移动端隐藏，这里承接渐变观感 */
      @media (max-width: 767px) {
         background: linear-gradient(180deg, #e8f5e9 0%, #ffffff 100%);
         .dark & {
            background: linear-gradient(
               180deg,
               rgba(46, 125, 50, 0.1) 0%,
               ${token.colorBgLayout} 100%
            );
         }
      }

      @media (min-width: 768px) {
         width: 45%;
         min-height: 100vh;
         padding: 24px;
      }
   `,

   loginCard: css`
      width: 100%;
      max-width: 400px;
      background-color: ${token.colorBgContainer};
      border: 1px solid ${token.colorBorderSecondary};
      border-radius: ${token.borderRadiusLG}px;
      box-shadow: ${token.boxShadowTertiary};
      padding: 20px 16px;
      transition: background-color 0.3s ease, box-shadow 0.3s ease;
      box-sizing: border-box;

      @media (min-width: 768px) {
         box-shadow: ${token.boxShadow};
         padding: 24px;
      }
   `,

   loginCardHeader: css`
      text-align: center;
   `,

   loginLogoWrapper: css`
      display: flex;
      justify-content: center;
      align-items: center;
      gap: 1rem;
      margin-bottom: 1rem;
   `,

   loginCardTitle: css`
      font-size: 18px;
      font-weight: 600;
      color: ${token.colorTextBase};
      margin: 0 0 8px 0;

      @media (min-width: 768px) {
         font-size: 20px;
      }
   `,

   loginCardSubtitle: css`
      font-size: 12px;
      color: ${token.colorTextDescription};
      margin: 0;
   `,

   loginActionRow: css`
      display: flex;
      justify-content: space-between;
      align-items: center;
      width: 100%;
   `,

   loginButton: css`
      width: 100%;
      height: 44px;
      font-size: 14px;
      font-weight: 500;
      border-radius: ${token.borderRadiusLG}px;

      @media (min-width: 768px) {
         height: 48px;
      }
   `,
}));
