import React, { useEffect, useState } from 'react';
import { useIsMobile } from '@/hooks/useMediaQuery';
import { useDebounce } from '@/hooks/useDebounce';

import { ResponsiveRangePicker } from '@/components/ResponsiveRangePicker/ResponsiveRangePicker';

import type { AuditListQueryParams } from '../types';
import { Form, Row, Col, Input, Button, Space, Drawer, Badge } from 'antd';
import {
   ReloadOutlined,
   FilterOutlined,
   SearchOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';

interface FormValues {
   bidName?: string;
   uploadDateRange?: [dayjs.Dayjs, dayjs.Dayjs];
}

interface AuditFilterProps {
   styles: Record<string, string>;
   queryParams: AuditListQueryParams;
   onSearch: (values: Partial<AuditListQueryParams>) => void;
   onReset: () => void;
   /** 工具栏右侧插槽（如「新建项目并上传招标文件」按钮） */
   extra?: React.ReactNode;
}

export const AuditFilter: React.FC<AuditFilterProps> = ({
   styles,
   queryParams,
   onSearch,
   onReset,
   extra,
}) => {
   const [form] = Form.useForm();
   const isMobile = useIsMobile();
   const [drawerOpen, setDrawerOpen] = useState(false);
   const { run: debouncedSubmit } = useDebounce(() => form.submit(), 500);
   const { run: debouncedSearch } = useDebounce((value: string) => {
      onSearch({ bidName: value });
   }, 400);

   // 搜索框受控值：跟随 queryParams，重置后也能被清空
   const [searchText, setSearchText] = useState(String(queryParams.bidName ?? ''));
   useEffect(() => {
      setSearchText(String(queryParams.bidName ?? ''));
   }, [queryParams.bidName]);

   useEffect(() => {
      form.setFieldsValue({
         uploadDateRange:
            queryParams.uploadStartTime && queryParams.uploadEndTime
               ? [
                    dayjs(queryParams.uploadStartTime),
                    dayjs(queryParams.uploadEndTime),
                 ]
               : undefined,
      });
   }, [queryParams, form]);

   // 激活的次要筛选数量（仅上传时间），用于「筛选」按钮角标
   const activeFilterCount = queryParams.uploadStartTime ||
      queryParams.uploadEndTime
      ? 1
      : 0;

   // 桌面端：整表单提交（含项目名称 + 上传时间）
   const handleDesktopFinish = (values: FormValues) => {
      onSearch({
         bidName: values.bidName,
         uploadStartTime:
            values.uploadDateRange?.[0]?.format('YYYY-MM-DD') || '',
         uploadEndTime: values.uploadDateRange?.[1]?.format('YYYY-MM-DD') || '',
      });
   };

   // 移动端抽屉：只提交上传时间，不动搜索框
   const handleDrawerFinish = (values: FormValues) => {
      onSearch({
         uploadStartTime:
            values.uploadDateRange?.[0]?.format('YYYY-MM-DD') || '',
         uploadEndTime: values.uploadDateRange?.[1]?.format('YYYY-MM-DD') || '',
      });
      setDrawerOpen(false);
   };

   const handleResetAll = () => {
      form.resetFields();
      onReset();
      setDrawerOpen(false);
   };

   // ── 移动端：单行搜索 + 底部抽屉筛选 ──
   if (isMobile) {
      return (
         <div className={styles.mobileFilterSticky}>
            <Input
               className={styles.mobileSearchInput}
               placeholder='搜索项目名称'
               allowClear
               prefix={
                  <SearchOutlined
                     style={{ color: 'rgba(0,0,0,0.25)', fontSize: 14 }}
                  />
               }
               value={searchText}
               onChange={(e) => {
                  setSearchText(e.target.value);
                  debouncedSearch(e.target.value);
               }}
               style={{ height: 36 }}
            />

            <Badge count={activeFilterCount} size='small' offset={[-6, 6]}>
               <Button
                  icon={<FilterOutlined />}
                  onClick={() => setDrawerOpen(true)}
                  style={{ height: 36 }}
               >
                  筛选
               </Button>
            </Badge>

            <Button
               icon={<ReloadOutlined />}
               onClick={handleResetAll}
               style={{ height: 36 }}
               aria-label='重置'
            />

            {extra}

            <Drawer
               title='筛选条件'
               placement='bottom'
               height='auto'
               open={drawerOpen}
               onClose={() => setDrawerOpen(false)}
            >
               <Form form={form} onFinish={handleDrawerFinish} layout='vertical'>
                  <Form.Item name='uploadDateRange' label='上传时间'>
                     <ResponsiveRangePicker />
                  </Form.Item>

                  <Space
                     style={{ width: '100%', justifyContent: 'flex-end' }}
                  >
                     <Button onClick={handleResetAll}>重置</Button>
                     <Button type='primary' htmlType='submit'>
                        确定
                     </Button>
                  </Space>
               </Form>
            </Drawer>
         </div>
      );
   }

   // ── 桌面端：项目名称 + 上传时间 + 重置（左）｜ 额外操作（右）──
   return (
      <div className={styles.filterSection}>
         <Form
            form={form}
            name='auditFilter'
            onFinish={handleDesktopFinish}
            style={{ width: '100%' }}
         >
            <Row
               gutter={16}
               align='middle'
               wrap
               style={{ rowGap: '8px' }}
            >
               <Col flex='1 1 220px' style={{ maxWidth: '320px' }}>
                  <Form.Item
                     name='bidName'
                     label='项目名称'
                     style={{ marginBottom: 0 }}
                  >
                     <Input
                        placeholder='请输入项目名称'
                        allowClear
                        onChange={debouncedSubmit}
                        autoComplete='off'
                        style={{ height: 35 }}
                     />
                  </Form.Item>
               </Col>

               <Col flex='none'>
                  <Form.Item
                     name='uploadDateRange'
                     label='上传时间'
                     style={{ marginBottom: 0 }}
                  >
                     <ResponsiveRangePicker onChange={debouncedSubmit} />
                  </Form.Item>
               </Col>

               <Col flex='none'>
                  <Form.Item style={{ marginBottom: 0 }}>
                     <Button
                        icon={<ReloadOutlined />}
                        onClick={handleResetAll}
                        style={{ height: 35 }}
                     >
                        重置
                     </Button>
                  </Form.Item>
               </Col>

               {extra && (
                  <Col
                     flex='auto'
                     style={{
                        display: 'flex',
                        justifyContent: 'flex-end',
                     }}
                  >
                     {extra}
                  </Col>
               )}
            </Row>
         </Form>
      </div>
   );
};