import React, { useEffect, useState } from 'react';
import { useIsMobile } from '@/hooks/useMediaQuery';
import { useDebounce } from '@/hooks/useDebounce';

import { ResponsiveRangePicker } from '@/components/ResponsiveRangePicker/ResponsiveRangePicker';

import type {
   AuditListQueryParams,
   FileCategory,
   FileCategoryCode,
} from '../types';
import {
   Form,
   Row,
   Col,
   Input,
   Select,
   Button,
   Space,
   Drawer,
   Badge,
} from 'antd';
import {
   ReloadOutlined,
   FilterOutlined,
   SearchOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';

const FILE_CATEGORY_CONFIG: { value: FileCategoryCode; label: FileCategory }[] = [
   { value: 'bid', label: '标书' },
   { value: 'contract', label: '合同' },
];

interface FormValues {
   bidName?: string;
   fileCategory?: FileCategoryCode;
   uploadDateRange?: [dayjs.Dayjs, dayjs.Dayjs];
}

interface AuditFilterProps {
   styles: Record<string, string>;
   queryParams: AuditListQueryParams;
   onSearch: (values: Partial<AuditListQueryParams>) => void;
   onReset: () => void;
}

export const AuditFilter: React.FC<AuditFilterProps> = ({
   styles,
   queryParams,
   onSearch,
   onReset,
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
         fileCategory: queryParams.fileCategory || undefined,
         uploadDateRange:
            queryParams.uploadStartTime && queryParams.uploadEndTime
               ? [
                    dayjs(queryParams.uploadStartTime),
                    dayjs(queryParams.uploadEndTime),
                 ]
               : undefined,
      });
   }, [queryParams, form]);

   // 激活的次要筛选数量（文件类型 + 时间），用于「筛选」按钮角标
   const activeFilterCount = [
      queryParams.fileCategory ? 1 : 0,
      queryParams.uploadStartTime || queryParams.uploadEndTime ? 1 : 0,
   ].reduce((s, n) => s + n, 0);

   // 桌面端：整表单提交（含项目名称）
   const handleDesktopFinish = (values: FormValues) => {
      onSearch({
         bidName: values.bidName,
         fileCategory: values.fileCategory,
         uploadStartTime:
            values.uploadDateRange?.[0]?.format('YYYY-MM-DD') || '',
         uploadEndTime: values.uploadDateRange?.[1]?.format('YYYY-MM-DD') || '',
      });
   };

   // 移动端抽屉：只提交次要筛选，不动搜索框
   const handleDrawerFinish = (values: FormValues) => {
      onSearch({
         fileCategory: values.fileCategory,
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

                  <Form.Item name='fileCategory' label='文件类型'>
                     <Select
                        options={FILE_CATEGORY_CONFIG}
                        placeholder='请选择文件类型'
                        allowClear
                        style={{ width: '100%' }}
                     />
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

   // ── 桌面端：保持原有左右分栏筛选 ──
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
               style={{
                  display: 'flex',
                  alignItems: 'center',
               }}
            >
               <Col
                  sm={12}
                  md={8}
                  flex={1}
                  style={{
                     width: '100%',
                     maxWidth: '350px',
                  }}
               >
                  <Form.Item name='bidName' label='项目名称'>
                     <Input
                        placeholder='请输入项目名称'
                        allowClear
                        onChange={debouncedSubmit}
                        autoComplete='off'
                        style={{ height: 35 }}
                     />
                  </Form.Item>
               </Col>

               <Col sm={12} md={8} flex='none'>
                  <Form.Item name='uploadDateRange' label='上传时间'>
                     <ResponsiveRangePicker onChange={debouncedSubmit} />
                  </Form.Item>
               </Col>
            </Row>

            <Row gutter={16}>
               <Col xs={24} sm={14} md={6}>
                  <Form.Item name='fileCategory' label='文件类型'>
                     <Select
                        options={FILE_CATEGORY_CONFIG}
                        placeholder='请选择文件类型'
                        allowClear
                        onChange={debouncedSubmit}
                        style={{ height: 35 }}
                     />
                  </Form.Item>
               </Col>

               <Col xs={24} sm={16} md={6}>
                  <Form.Item label=' ' colon={false}>
                     <Space
                        style={{
                           display: 'flex',
                           alignItems: 'center',
                           justifyContent: 'flex-start',
                        }}
                     >
                        <Button
                           icon={<ReloadOutlined />}
                           onClick={handleResetAll}
                           style={{ fontSize: '1.4rem', height: 35 }}
                        >
                           重置
                        </Button>
                     </Space>
                  </Form.Item>
               </Col>
            </Row>
         </Form>
      </div>
   );
};