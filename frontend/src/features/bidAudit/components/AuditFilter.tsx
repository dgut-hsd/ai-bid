import React, { useEffect } from 'react';
import { useIsMobile } from '@/hooks/useMediaQuery';
import { useDebounce } from '@/hooks/useDebounce';

import { ResponsiveRangePicker } from '@/components/ResponsiveRangePicker/ResponsiveRangePicker';

import type {
   AuditListQueryParams,
   FileCategory,
   FileCategoryCode,
} from '../types';
import { Form, Row, Col, Input, Select, Button, Space } from 'antd';
import { ReloadOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';

const FILE_CATEGORY_CONFIG: { value: FileCategoryCode; label: FileCategory }[] = [
   { value: 'bid', label: '标书' },
   { value: 'contract', label: '合同' },
];

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
   const { run: debouncedSubmit } = useDebounce(() => form.submit(), 500);

   useEffect(() => {
      form.setFieldsValue({
         bidName: queryParams.bidName,
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

   interface FormValues {
      bidName?: string;
      fileCategory?: FileCategoryCode;
      uploadDateRange?: [dayjs.Dayjs, dayjs.Dayjs];
   }

   const handleFinish = (values: FormValues) => {
      onSearch({
         bidName: values.bidName,
         fileCategory: values.fileCategory,
         uploadStartTime:
            values.uploadDateRange?.[0]?.format('YYYY-MM-DD') || '',
         uploadEndTime: values.uploadDateRange?.[1]?.format('YYYY-MM-DD') || '',
      });
   };

   return (
      <div className={styles.filterSection}>
         <Form
            form={form}
            name='auditFilter'
            onFinish={handleFinish}
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
                     maxWidth: isMobile ? '100%' : '350px',
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

               <Col sm={12} md={8} flex={isMobile ? 1 : 'none'}>
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
                  <Form.Item label={isMobile ? null : ' '} colon={false}>
                     <Space
                        style={{
                           display: 'flex',
                           alignItems: 'center',
                           justifyContent: isMobile ? 'flex-end' : 'flex-start',
                        }}
                     >
                        <Button
                           icon={<ReloadOutlined />}
                           onClick={onReset}
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
