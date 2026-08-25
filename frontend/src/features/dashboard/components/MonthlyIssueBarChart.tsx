import React, { useEffect, useMemo, useRef } from 'react';
import * as echarts from 'echarts';
import { COLORS } from '@/theme/constants';
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

   const option: echarts.EChartsOption = useMemo(() => {
      const counts = chartData.map((item) => item.count);
      const isEmpty = counts.length === 0 || counts.every((n) => n === 0);

      if (isEmpty) {
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
            series: [{ type: 'bar' as const, data: [] }],
         } as echarts.EChartsOption;
      }

      return {
         grid: {
            top: 5,
            right: 15,
            left: 30,
            bottom: 0,
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
         <Title level={4} style={{ marginTop: 5 }}>
            本月发现问题数
         </Title>

         <span style={{ fontSize: 12 }}>按天统计本月审核发现的问题数量。</span>

         <div ref={chartRef} style={{ width: '100%', flex: 1, minHeight: 150 }} />
      </div>
   );
};