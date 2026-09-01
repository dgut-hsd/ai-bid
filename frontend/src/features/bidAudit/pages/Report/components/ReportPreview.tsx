import { forwardRef, useMemo } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Typography } from 'antd';
import { useStyles } from '../style';

const { Text } = Typography;

interface ReportPreviewProps {
   scale: number;
   markdownContent: string;
}

export const ReportPreview = forwardRef<HTMLDivElement, ReportPreviewProps>(
   ({ scale, markdownContent }, ref) => {
      const { styles } = useStyles();

      // 性能优化：Markdown 大文本（数十条 findings + 大量 URL）渲染昂贵。
      // 缩放(scale)、文件名输入等高频状态变化会触发本组件重渲染，
      // 用 useMemo 让 ReactMarkdown 仅在内容变化时重建，缩放只改 CSS transform。
      const renderedMarkdown = useMemo(
         () =>
            markdownContent ? (
               <ReactMarkdown remarkPlugins={[remarkGfm]}>
                  {markdownContent}
               </ReactMarkdown>
            ) : null,
         [markdownContent]
      );

      return (
         <div className={styles.previewArea} ref={ref}>
            <div
               className={styles.a4Paper}
               style={{ transform: `scale(${scale / 100})` }}
            >
               {renderedMarkdown ?? (
                  <div
                     style={{
                        display: 'flex',
                        justifyContent: 'center',
                        alignItems: 'center',
                        height: '100%',
                        color: '#999',
                     }}
                  >
                     <Text type='secondary'>
                        暂无报告内容，请勾选左侧报告模块
                     </Text>
                  </div>
               )}
            </div>
         </div>
      );
   }
);

ReportPreview.displayName = 'ReportPreview';
