import { createStyles } from 'antd-style';

export const useStyles = createStyles(({ css }) => ({
   timeline: css`
      .ant-timeline-item {
         padding-bottom: 12px; /* 卡片间距交给 item padding，竖线才能穿过间距连到下一张卡 */
      }

      /* 圆点垂直居中到版本卡片 */
      .ant-timeline-item-head {
         inset-block-start: 50% !important;
         top: 50% !important;
         transform: translateY(-50%);
      }

      /* 竖线默认贯穿整张卡片（从上一张卡底部连到下一张卡顶部） */
      .ant-timeline-item-tail {
         inset-block-start: 0 !important;
         top: 0 !important;
         height: 100% !important;
      }

      /* 首张卡：竖线从圆点(中心)起向下，不越过卡片上缘 */
      .ant-timeline-item:first-child .ant-timeline-item-tail {
         top: 50% !important;
         height: 50% !important;
      }

      /* 末张卡：竖线接到圆点(中心)即止，不再向下延伸 */
      .ant-timeline-item-last .ant-timeline-item-tail {
         display: block !important;
         top: 0 !important;
         height: 50% !important;
      }
   `,
}));
