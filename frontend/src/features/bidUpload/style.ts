import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css, token }) => ({
   pageContainer: css`
      display: flex;
      flex-direction: column;
      height: 100%;
      min-height: 80vh;
      padding: 0.8rem 1.2rem;

      @media (max-width: 768px) {
         padding: 12px;
      }

      .ant-input-prefix {
         margin-right: 4px;
      }
   `,

   mainLayout: css`
      background: transparent;
      display: flex;
      gap: 24px;
      flex: 1;
      width: 100%;
      max-width: 1280px;
      margin: 0 auto;
      align-items: flex-start;

      @media (max-width: 768px) {
         flex-direction: column;
         gap: 16px;
      }
   `,

   contentArea: css`
      flex: 1;
      min-width: 0;
   `,

   cardWrapper: css`
      border-radius: ${token.borderRadiusLG}px;
      border: none;
      width: 100%;

      .ant-card-body {
         padding: 1rem;
         @media (max-width: 768px) {
            padding: 16px;
         }
      }
   `,

   uploadDragger: css`
      border: 2px dashed ${token.colorPrimary};
      border-radius: ${token.borderRadius}px;
      background: ${token.colorFillAlter};
      padding: 32px 20px;
      text-align: center;
      cursor: pointer;
      transition: all 0.3s;

      &:hover {
         border-color: ${token.colorPrimaryHover};
         background: ${token.controlItemBgHover};
      }

      @media (max-width: 768px) {
         padding: 24px 16px;
      }
   `,

   fileListItem: css`
      display: flex;
      justify-content: space-between;
      align-items: center;
      max-width: 800px;
      padding: 8px 0;
      border-top: 1px solid ${token.colorBorderSecondary};
      margin-top: 12px;
   `,

   
   buttonContainer: css`
      display: flex;
      width: 100%;
      gap: 2rem;
      max-width: 800px;

      button {
         padding: 1.5rem 2rem;
      }
   `,
}));
