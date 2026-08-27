import React from 'react';
import { Input } from 'antd';
import { useStyles } from '../style';

interface ReportSettingsProps {
   fileName: string;
   onFileNameChange: (name: string) => void;
}

export const ReportSettings: React.FC<ReportSettingsProps> = ({
   fileName,
   onFileNameChange,
}) => {
   const { styles } = useStyles();

   return (
      <aside className={styles.settingsArea}>
         {/* 1. 导出格式 (只读) */}
         <div className={styles.settingSection}>
            <div className={styles.settingLabel}>导出格式</div>
            <Input value='Word (.docx)' disabled />
         </div>

         {/* 2. 文件名配置 */}
         <div className={styles.settingSection}>
            <div className={styles.settingLabel}>文件名配置</div>
            <Input
               value={fileName}
               onChange={(e) => onFileNameChange(e.target.value)}
               placeholder='请输入导出的文件名'
               suffix='.docx'
               allowClear
            />
         </div>
      </aside>
   );
};