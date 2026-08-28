import React from 'react';
import { useParams, useNavigate, useSearchParams } from 'react-router-dom';
import { Result, Button, Typography, Segmented } from 'antd';
import { FileTextOutlined, SearchOutlined } from '@ant-design/icons';
import { useStyles } from './style';
import { useIsMobile } from '@/hooks/useMediaQuery';
import PdfPreview from './components/PDFPreview/PdfPreview';
import type { PdfPreviewRef, BBoxData } from './components/PDFPreview/PdfPreview';
import BidAnalysis from './components/BidAnalysis/BidAnalysis';
import { useAuditTask } from './hooks/useAuditTask';
import { useMockAuditTask } from './hooks/useMockAuditTask';
import { auditDetailOptions } from './api/auditDetail';
import { useQuery } from '@tanstack/react-query';
import { Loading } from '@/components/Loading/Loading';
import type { BidDetail } from './types';

const { Text } = Typography;

/**
 * Mock 数据开关 — 两种方式启用：
 * 1. 环境变量: VITE_USE_MOCK_AUDIT=true (全局)
 * 2. URL 参数: ?mock=1 (单页面)
 */
const useIsMock = () => {
   const [searchParams] = useSearchParams();
   const envMock = import.meta.env.VITE_USE_MOCK_AUDIT === 'true';
   const urlMock = searchParams.get('mock') === '1';
   return envMock || urlMock;
};

/** Mock 模式下的伪造标书信息 */
const MOCK_BID_DATA: BidDetail = {
   id: 999,
   fileName: '清华大学智慧校园项目招标文件.pdf',
   filePath: '',
   fileSize: 0,
   fileType: 'pdf',
   fileCategory: 'bid',
   bidName: '清华大学智慧校园项目招标文件',
   supplierName: '模拟供应商',
   budgetAmount: 0,
   pageCount: 120,
   parseStatus: 2,
   uploadUserId: 0,
   uploadTime: '2025-06-15 10:30:00',
   version: 1,
   projectId: 1,
};

export const DetailPage: React.FC = () => {
   const { styles } = useStyles();
   const { id: bidId } = useParams<{ id: string }>();
   const navigate = useNavigate();
   const isMock = useIsMock();
   const isMobile = useIsMobile();
   // 移动端全屏视图切换：'pdf'（标书文档）| 'analysis'（审核分析）
   const [mobileView, setMobileView] = React.useState<'pdf' | 'analysis'>('pdf');
   const pdfPreviewRef = React.useRef<PdfPreviewRef>(null);
   const handleLocateIssuePage = React.useCallback((page: number, highlightText?: string, fallbackTokens?: string[]) => {
      pdfPreviewRef.current?.jumpToPage(page, highlightText, fallbackTokens);
      // 移动端：从分析页定位某条风险时，自动切回文档视图以便查看高亮
      if (isMobile) setMobileView('pdf');
   }, [isMobile]);

   const handleLocateBboxes = React.useCallback((page: number, bboxes: BBoxData[], highlightText?: string, fallbackTokens?: string[]) => {
      pdfPreviewRef.current?.highlightBboxes(page, bboxes, highlightText, fallbackTokens);
      if (isMobile) setMobileView('pdf');
   }, [isMobile]);

   // 真实 API 请求（mock 模式下跳过）
   const {
      data: apiBidData,
      isLoading,
      isError,
   } = useQuery({
      ...auditDetailOptions.bidDetail(Number(bidId)),
      enabled: !isMock && !!bidId && !isNaN(Number(bidId)),
   });

   // mock 模式下使用伪造 bidData
   const bidData: BidDetail | undefined = isMock
      ? { ...MOCK_BID_DATA, id: Number(bidId) || 999 }
      : apiBidData;

   // 真实审核 hook
   const realAudit = useAuditTask(bidId ? Number(bidId) : undefined);

   // Mock 审核 hook
   const mockAudit = useMockAuditTask();

   // 根据开关选择数据源
   const {
      taskId,
      startAudit,
      isStarting,
      isAuditing,
      currentStage,
      elapsedSeconds,
      issues,
      isComplete,
      summary,
      error,
      agentProgresses,
      liveFeedEvents,
      phaseEvent,
      statsEvent,
      liveFindings,
      failedStages,
   } = isMock ? mockAudit : realAudit;

   if (isLoading) {
      return <Loading loading={isLoading} />;
   }

   if (isError || !bidData) {
      return (
         <div className={styles.detailContainer}>
            <Text type='secondary'>暂无项目详细信息或加载失败</Text>
         </div>
      );
   }

   if (error) {
      return (
         <div style={{ padding: '50px' }}>
            <Result
               status='error'
               title='审核任务异常'
               subTitle={error}
               extra={[
                  <Button
                     type='primary'
                     key='console'
                     onClick={() =>
                        startAudit({ bidId: Number(bidId), webSearchEnabled: false, forceRefresh: true })
                     }
                  >
                     重试
                  </Button>,

                  <Button key='back' onClick={() => navigate(-1)}>
                     返回列表
                  </Button>,
               ]}
            />
         </div>
      );
   }

   return (
      <div className={styles.container}>
         {isMobile && (
            <div className={styles.mobileSwitcher}>
               <Segmented
                  block
                  value={mobileView}
                  onChange={(value) => setMobileView(value as 'pdf' | 'analysis')}
                  options={[
                     { label: '标书文档', value: 'pdf', icon: <FileTextOutlined /> },
                     { label: '审核分析', value: 'analysis', icon: <SearchOutlined /> },
                  ]}
               />
            </div>
         )}

         <div className={styles.mainContent}>
            <div
               className={styles.leftPanel}
               style={isMobile ? { display: mobileView === 'pdf' ? 'flex' : 'none' } : undefined}
            >
               <PdfPreview
                  ref={pdfPreviewRef}
                  fileUrl={`${import.meta.env.VITE_API_BASE_URL}/api/bid-documents/${bidData.id}/download`}
                  fileType={bidData.fileType}
                  isComplete={isComplete}
               />
            </div>

            <BidAnalysis
               style={isMobile ? { display: mobileView === 'analysis' ? 'flex' : 'none' } : undefined}
               onAudit={(options) =>
                  startAudit({
                     bidId: bidData.id,
                     webSearchEnabled: !!options?.webSearchEnabled,
                     forceRefresh: !!options?.forceRefresh,
                  })
               }
               projectId={bidData.projectId}
               bidId={bidData.id}
               taskId={taskId}
               isStarting={isStarting}
               isAuditing={isAuditing}
               elapsedSeconds={elapsedSeconds}
               issues={issues}
               isComplete={isComplete}
               currentStage={currentStage}
               summary={summary}
               onLocateIssuePage={handleLocateIssuePage}
               onLocateBboxes={handleLocateBboxes}
               currentFileName={bidData.fileName || bidData.bidName}
               currentFileId={bidData.id}
               agentProgresses={agentProgresses}
               liveFeedEvents={liveFeedEvents}
               phaseEvent={phaseEvent}
               statsEvent={statsEvent}
               liveFindings={liveFindings}
               failedStages={failedStages}
            />
         </div>
      </div>
   );
};
