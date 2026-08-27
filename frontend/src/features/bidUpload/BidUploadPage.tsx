import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';

import { useStyles } from './style';

import { useUploadBidMutation } from './api/BidUpload';
import type { BidUploadQueryParams } from './types';

import { useIsMobile } from '@/hooks/useMediaQuery';

import { UploadInstructions } from './components/UploadInstructions';
import { BidForm } from './components/BidForm';
import { UploadDragger } from './components/UploadDragger';

import { App, Modal, Spin } from 'antd';

export const BidUploadPage: React.FC = () => {
   const { styles } = useStyles();
   const { message } = App.useApp();
   const isMobile = useIsMobile();
   const navigate = useNavigate();

   const [collapsed, setCollapsed] = useState(false);

   const { mutateAsync: uploadDoc, isPending } = useUploadBidMutation();

   const handleFinish = async (values: BidUploadQueryParams, file: File) => {
      const uploaded = await uploadDoc({ params: values, file });
      message.success('招标文件上传成功！');
      navigate(`/bidReview/detail/${uploaded.id}`);
   };

   return (
      <>
         <Modal
            title='上传中'
            open={isPending}
            closable={false}
            footer={null}
            centered
         >
            <div style={{ textAlign: 'center', padding: '20px 0' }}>
               <Spin size='large' />
               <p style={{ marginTop: 16 }}>正在上传，请稍候...</p>
            </div>
         </Modal>

         <div className={styles.pageContainer}>
            <div className={styles.mainLayout}>
               <div className={styles.contentArea}>
                  <BidForm
                     onSubmit={handleFinish}
                     isPending={isPending}
                     renderUpload={(file, setFile) => (
                        <UploadDragger file={file} onFileChange={setFile} />
                     )}
                  />
               </div>

               <div style={{ flexShrink: 0 }}>
                  <UploadInstructions
                     collapsed={collapsed}
                     setCollapsed={setCollapsed}
                     isMobile={isMobile}
                  />
               </div>
            </div>
         </div>
      </>
   );
};
