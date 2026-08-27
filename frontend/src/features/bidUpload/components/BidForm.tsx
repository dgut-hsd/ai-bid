import React, { useState, useEffect } from 'react';
import { Form, Button, Card, Input } from 'antd';
import { useStyles } from '../style';
import { useIsMobile } from '@/hooks/useMediaQuery';
import { FolderOutlined, ProjectOutlined } from '@ant-design/icons';
import type { BidUploadQueryParams } from '../types';
import { useNavigate, useParams } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { dashboardOptions } from '@/features/dashboard/api/dashboard';

interface Props {
   onSubmit: (values: BidUploadQueryParams, file: File) => Promise<void>;
   isPending: boolean;
   renderUpload: (
      file: File | null,
      setFile: (f: File | null) => void
   ) => React.ReactNode;
}

export const BidForm: React.FC<Props> = ({
   onSubmit,
   isPending,
   renderUpload,
}) => {
   const { styles } = useStyles();
   const isMobile = useIsMobile();
   const navigate = useNavigate();
   const { projectId } = useParams<{ projectId: string }>();

   const [form] = Form.useForm();
   const [file, setFile] = useState<File | null>(null);

   const { data: projectList = [] } = useQuery(dashboardOptions.list());

   const projectName =
      projectList.find((p) => p.id === Number(projectId))?.projectName || '';

   // 文件上传后自动填充文件名
   useEffect(() => {
      if (file) {
         const fileNameWithoutExt = file.name.replace(/\.[^/.]+$/, '');
         form.setFieldsValue({ bidName: fileNameWithoutExt });
      }
   }, [file, form]);

   const handleFinish = async (values: BidUploadQueryParams) => {
      if (!file) {
         return;
      }

      const payload = {
         ...values,
         fileCategory: '招标文件',
         projectId: Number(projectId),
      };

      await onSubmit(payload, file);
      form.resetFields();
      setFile(null);
   };

   const handleCancel = () => {
      form.resetFields();
      navigate('/bidReview');
   };

   return (
      <Card className={styles.cardWrapper}>
         <Form form={form} layout='vertical' onFinish={handleFinish}>
            {renderUpload(file, setFile)}

            <div
               className={styles.buttonContainer}
               style={{
                  flexDirection: isMobile ? 'column' : 'row',
                  gap: isMobile ? '0' : '2rem',
               }}
            >
               <Form.Item label='所属项目' style={{ flex: 1 }}>
                  <Input
                     value={projectName}
                     prefix={<FolderOutlined />}
                     disabled
                  />
               </Form.Item>

               <Form.Item label='文件类型' style={{ flex: 1 }}>
                  <Input
                     value='招标文件'
                     prefix={<ProjectOutlined />}
                     disabled
                  />
               </Form.Item>
            </div>

            <Form.Item
               label='文件名称'
               name='bidName'
               style={{ flex: 1, maxWidth: 800 }}
               rules={[{ required: true, message: '请输入文件名称' }]}
            >
               <Input
                  prefix={<ProjectOutlined />}
                  placeholder='请输入文件名称'
               />
            </Form.Item>

            {/* 提交按钮区 */}
            <Form.Item style={{ marginBottom: 0 }}>
               <div className={styles.buttonContainer}>
                  <Button
                     type='primary'
                     htmlType='submit'
                     loading={isPending}
                     style={{ flex: 1 }}
                  >
                     上传
                  </Button>

                  <Button onClick={handleCancel} disabled={isPending}>
                     取消
                  </Button>
               </div>
            </Form.Item>
         </Form>
      </Card>
   );
};