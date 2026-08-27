import { Select, Button, Drawer } from 'antd';
import { ReloadOutlined } from '@ant-design/icons';
import { useStyles } from '../style';
import { useIsMobile } from '@/hooks/useMediaQuery';
import { ResponsiveRangePicker } from '@/components/ResponsiveRangePicker/ResponsiveRangePicker';
import type { Dayjs } from 'dayjs';
import type { KnowledgeQueryConfig } from '../types';

interface FilterBarProps {
   applicableScopeFilter: NonNullable<KnowledgeQueryConfig['applicableScope']>;
   onApplicableScopeChange: (
      value: NonNullable<KnowledgeQueryConfig['applicableScope']>
   ) => void;
   statusFilter: NonNullable<KnowledgeQueryConfig['status']>;
   onStatusChange: (value: NonNullable<KnowledgeQueryConfig['status']>) => void;
   dateRange: [Dayjs | null, Dayjs | null] | null;
   onDateRangeChange: (value: [Dayjs | null, Dayjs | null] | null) => void;
   onReset: () => void;
   drawerOpen: boolean;
   onDrawerClose: () => void;
}

export function FilterBar({
   applicableScopeFilter,
   onApplicableScopeChange,
   statusFilter,
   onStatusChange,
   dateRange,
   onDateRangeChange,
   onReset,
   drawerOpen,
   onDrawerClose,
}: FilterBarProps) {
   const { styles } = useStyles();
   const isMobile = useIsMobile();

   const scopeControl = (
      <div className={styles.filterItem}>
         <span className={styles.filterLabel}>适用范围</span>
         <Select
            placeholder='采购类/工程类/通用'
            value={applicableScopeFilter || undefined}
            onChange={onApplicableScopeChange}
            allowClear
            style={{ width: isMobile ? '100%' : 180 }}
            options={[
               { value: 'procurement', label: '采购类' },
               { value: 'engineering', label: '工程类' },
               { value: 'general', label: '通用' },
            ]}
         />
      </div>
   );

   const statusControl = (
      <div className={styles.filterItem}>
         <span className={styles.filterLabel}>状态</span>
         <Select
            placeholder='启用/停用'
            value={statusFilter || undefined}
            onChange={onStatusChange}
            allowClear
            style={{ width: isMobile ? '100%' : 150 }}
            options={[
               { value: 'enabled', label: '启用' },
               { value: 'disabled', label: '停用' },
            ]}
         />
      </div>
   );

   const dateControl = (
      <div className={styles.filterItem}>
         <span className={styles.filterLabel}>上传时间</span>
         <ResponsiveRangePicker value={dateRange} onChange={onDateRangeChange} />
      </div>
   );

   // 移动端：筛选放进底部抽屉，顶部只保留搜索行
   if (isMobile) {
      return (
         <Drawer
            title='筛选条件'
            placement='bottom'
            height='auto'
            open={drawerOpen}
            onClose={onDrawerClose}
         >
            <div className={styles.filterBar}>
               {scopeControl}
               {statusControl}
               {dateControl}
            </div>

            <div
               style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  marginTop: 16,
               }}
            >
               <Button icon={<ReloadOutlined />} onClick={onReset}>
                  重置
               </Button>
               <Button type='primary' onClick={onDrawerClose}>
                  完成
               </Button>
            </div>
         </Drawer>
      );
   }

   // 桌面端：内联展示，保持原顺序（适用/状态/重置 + 时间整行）
   return (
      <div className={styles.filterBar}>
         {scopeControl}
         {statusControl}
         <div className={styles.filterItem}>
            <Button icon={<ReloadOutlined />} onClick={onReset}>
               重置
            </Button>
         </div>
         {dateControl}
      </div>
   );
}