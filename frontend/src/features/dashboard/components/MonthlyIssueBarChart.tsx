import React, { useEffect, useMemo, useRef } from 'react';
import * as echarts from 'echarts/core';
import { BarChart } from 'echarts/charts';
import {
   GridComponent,
   GraphicComponent,
   TooltipComponent,
} from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import type { EChartsOption } from 'echarts';
import { COLORS } from '@/theme/constants';

echarts.use([BarChart, GridComponent, GraphicComponent, TooltipComponent, CanvasRenderer]);
import { Typography } from 'antd';
import { useStyles } from '../style';
import type { DailyIssueCountItem } from '../types';

const { Title } = Typography;

interface MonthlyIssueBarChartProps {
   data?: DailyIssueCountItem[];
}

export const MonthlyIssueBarChart: React.FC<MonthlyIssueBarChartProps> = ({
   data,
}) => {
   const chartRef = useRef<HTMLDivElement>(null);
   const { styles } = useStyles();
   const chartData = Array.isArray(data) ? data : [];

   const option: EChartsOption = useMemo(() => {
      const counts = chartData.map((item) => item.count);
      const isEmpty = counts.length === 0 || counts.every((n) => n === 0);

      if (isEmpty) {
         // 空态只渲染占位文字；不能放空 bar 系列——bar 依赖 cartesian2d 坐标系，
         // 没有 xAxis/yAxis/grid 会导致 echarts 读取 undefined 崩溃
         return {
            graphic: {
               type: 'text',
               left: 'center',
               top: 'middle',
               style: {
                  text: '本月暂无问题',
                  fill: COLORS.textSecondary,
                  fontSize: 14,
               },
            },
         } as EChartsOption;
      }

      return {
         grid: {
            top: 20,
            right: 20,
            left: 40,
            bottom: 4,
            containLabel: true,
         },
         tooltip: {
            trigger: 'axis',
            axisPointer: { type: 'shadow' },
            formatter: (params: any) => {
               const val = Array.isArray(params) ? params[0] : params;
               return `${val.name}日<br/>
                     <span style="display:inline-block;margin-right:4px;border-radius:3px;width:10px;height:10px;background-color:${COLORS.primary};"></span>
                     问题数: <b>${val.value}</b> 个`;
            },
         },
         xAxis: {
            type: 'category',
            data: chartData.map((item) => item.name),
            axisLine: { show: false },
            axisTick: { show: false },
            axisLabel: { color: COLORS.textSecondary, fontSize: 10 },
         },
         yAxis: {
            type: 'value',
            max: Math.max(...counts, 10),
            minInterval: 1,
            axisLine: { show: false },
            axisTick: { show: false },
            splitLine: {
               lineStyle: { type: 'dashed', color: COLORS.border },
            },
            axisLabel: { color: COLORS.textSecondary, fontSize: 12 },
         },
         series: [
            {
               type: 'bar',
               data: chartData.map((item) => item.count),
               barMaxWidth: 14,
               itemStyle: { color: COLORS.primary },
            },
         ],
      };
   }, [chartData]);

   // 图表容器始终常驻，空数据时用 graphic 占位，避免空 → 有数据时重建实例
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

      // notMerge=true：空态 graphic 与柱状图之间切换时彻底替换
      chart.setOption(option, true);
   }, [option]);

   return (
      <div className={styles.chartCard}>
         <div style={{ flexShrink: 0, marginBottom: 12 }}>
            <Title level={4} style={{ margin: '5px 0 4px' }}>
               本月发现问题数
            </Title>
         </div>

         <div
            ref={chartRef}
            style={{ width: '100%', flex: 1, minHeight: 150, position: 'relative' }}
         />
      </div>
   );
};