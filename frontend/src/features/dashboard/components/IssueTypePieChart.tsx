import React, { useEffect, useMemo, useRef } from 'react';
import * as echarts from 'echarts/core';
import { PieChart } from 'echarts/charts';
import {
   GraphicComponent,
   LegendComponent,
   TooltipComponent,
} from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import type { EChartsOption } from 'echarts';
import { COLORS } from '@/theme/constants';

echarts.use([PieChart, GraphicComponent, LegendComponent, TooltipComponent, CanvasRenderer]);
import { Typography } from 'antd';
import { useStyles } from '../style';
import type { IssueChartItem } from '../types';

// 风险类型饼图配色：分类是动态的（可能超过 6 个），超过颜色数量时 ECharts 自动循环取色。
const PIE_COLORS = [
   COLORS.primary, // 学校绿
   COLORS.primaryHover, // 中绿
   COLORS.success, // 成功绿
   '#1890ff', // 蓝
   '#faad14', // 琥珀
   '#f5222d', // 红
   '#722ed1', // 紫
   '#13c2c2', // 青
   '#eb2f96', // 品红
   '#2f54eb', // 靛蓝
   '#a0d911', // 柠檬
   '#fa8c16', // 橙
   '#08979c', // 深青
   '#c41d7f', // 玫红
   '#d48806', // 金
];

interface IssueTypePieChartProps {
   data?: IssueChartItem[];
}

const { Title } = Typography;

export const IssueTypePieChart: React.FC<IssueTypePieChartProps> = ({
   data,
}) => {
   const chartRef = useRef<HTMLDivElement>(null);
   const { styles } = useStyles();
   const chartData = Array.isArray(data) ? data : [];
   const isEmpty = chartData.length === 0;

   const option: EChartsOption = useMemo(() => {
      if (isEmpty) {
         // 空态只渲染占位文字，不放置空 series，与柱状图空态保持一致
         return {
            graphic: {
               type: 'text',
               left: 'center',
               top: 'middle',
               style: {
                  text: '暂无审核问题',
                  fill: COLORS.textSecondary,
                  fontSize: 14,
               },
            },
         } as EChartsOption;
      }

      return {
         color: PIE_COLORS,
         tooltip: {
            trigger: 'item',
         },
         legend: {
            type: 'scroll',
            bottom: 0,
            left: 'center',
            icon: 'square',
            textStyle: {
               fontSize: 12,
               color: COLORS.textSecondary,
            },
         },
         series: [
            {
               name: '问题类型',
               type: 'pie',
               radius: ['42%', '62%'],
               center: ['50%', '44%'],
               avoidLabelOverlap: false,
               padAngle: 2,
               label: {
                  show: false,
                  position: 'center',
               },
               labelLine: {
                  show: false,
               },
               data: chartData,
            },
         ],
      };
   }, [chartData, isEmpty]);

   // 图表容器始终常驻（空数据时用 graphic 显示占位），保证空 → 有数据时无需重建实例
   useEffect(() => {
      if (!chartRef.current) return;
      const chart = echarts.init(chartRef.current);

      const resizeObserver = new ResizeObserver(() => {
         chart.resize();
      });

      resizeObserver.observe(chartRef.current);

      return () => {
         resizeObserver.disconnect();
         chart.dispose();
      };
   }, []);

   useEffect(() => {
      if (!chartRef.current) return;
      const chart = echarts.getInstanceByDom(chartRef.current);
      if (!chart) return;

      // notMerge=true：空态 graphic 与饼图之间切换时彻底替换，避免残留
      chart.setOption(option, true);
   }, [option]);

   return (
      <div className={styles.chartCard}>
         <div style={{ flexShrink: 0, marginBottom: 8 }}>
            <Title level={4} style={{ margin: '5px 0 4px' }}>
               问题类型分布
            </Title>
            <span style={{ fontSize: 12, display: 'block' }}>
               按风险类型统计审核发现的问题数量。
            </span>
         </div>

         <div
            ref={chartRef}
            style={{ width: '100%', flex: 1, minHeight: 150, position: 'relative' }}
         />
      </div>
   );
};