import React from 'react';
import { Button, Slider, Space, Typography } from 'antd';
import {
   FileWordOutlined,
   ArrowLeftOutlined,
   ZoomInOutlined,
   ZoomOutOutlined,
} from '@ant-design/icons';
import { useNavigate, useParams } from 'react-router-dom';
import { useStyles } from '../style';

const { Text } = Typography;

interface ReportHeaderProps {
   scale: number;
   onScaleChange: (scale: number) => void;
   onExportWord: () => void;
   isExporting?: boolean;
}

export const ReportHeader: React.FC<ReportHeaderProps> = ({
   scale,
   onScaleChange,
   onExportWord,
   isExporting = false,
}) => {
   const { styles } = useStyles();
   const navigate = useNavigate();
   const { id: auditId } = useParams<{ id: string }>();

   const handleBack = () => {
      if (auditId) {
         navigate(`/bidReview/detail/${auditId}`);
      } else {
         navigate(-1);
      }
   };

   return (
      <header className={styles.headerContainer}>
         {/* 左侧：缩放 */}
         <Space size='large'>
            <div className={styles.zoomControl}>
               <ZoomOutOutlined
                  style={{ cursor: 'pointer' }}
                  onClick={() => onScaleChange(Math.max(50, scale - 10))}
               />
               <Slider
                  min={50}
                  max={200}
                  step={10}
                  value={scale}
                  onChange={onScaleChange}
                  tooltip={{ formatter: (val) => `${val}%` }}
                  style={{ flex: 1, margin: '0 8px' }}
               />
               <ZoomInOutlined
                  style={{ cursor: 'pointer' }}
                  onClick={() => onScaleChange(Math.min(200, scale + 10))}
               />
               <Text type='secondary' style={{ width: '40px' }}>
                  {scale}%
               </Text>
            </div>
         </Space>

         {/* 右侧：导出与返回 */}
         <Space size='middle'>
            <Button
               type='primary'
               icon={<FileWordOutlined />}
               onClick={onExportWord}
               loading={isExporting}
            >
               导出 Word
            </Button>

            <Button icon={<ArrowLeftOutlined />} onClick={handleBack}>
               返回审核页
            </Button>
         </Space>
      </header>
   );
};
