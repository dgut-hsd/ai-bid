import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => ({
   timeline: css`
      /* 让时间轴的圆点垂直居中到每张版本卡片中间，而不是默认贴在卡片顶部 */
      .ant-timeline-item {
         padding-bottom: 0;
      }

      .ant-timeline-item-head {
         inset-block-start: 50% !important;
         top: 50% !important;
         transform: translateY(-50%);
      }
   `,
}));