import type { PageParams } from '@/api/types';
// 共享类型统一从此处引入
export type { Severity, AuditIssue, AuditSummary, BidDocument } from '@/types/audit';

export const ParseStatus = {
  Pending: 0,
  Processing: 1,
  Completed: 2,
  Failed: 3,
} as const;

export type ParseStatusType = (typeof ParseStatus)[keyof typeof ParseStatus];

export type FileCategory = '标书' | '合同';

/** 文件类型的存储码：DB 存英文码(bid/contract)，前端筛选传码、展示时用中文标签 */
export type FileCategoryCode = 'bid' | 'contract';

/** 审核列表项（项目+版本聚合视图） */
export interface AuditListItem {
  projectId: number;
  projectName: string;
  createTime: string;
  latestVersion: number;
  fileCategory: FileCategory;
  supplierName: string;
  auditorName: string;
}

/** 审核列表查询参数 */
export interface AuditListQueryParams extends PageParams {
  bidName?: string;
  fileCategory?: FileCategoryCode;
  status?: number;
  uploadStartTime?: string;
  uploadEndTime?: string;
}

/** 标书文档项（审核列表用扁平版本） */
export interface ProjectItem {
  id: number;
  fileName: string;
  filePath: string;
  fileSize: number;
  fileType: string;
  fileCategory: FileCategory;
  bidName: string;
  supplierName: string;
  budgetAmount: number;
  pageCount: number;
  parseStatus: ParseStatusType;
  uploadUserId: number;
  uploadTime: string;
  version: number;
  projectId: number;
  auditorName: string;
}

/** 审核状态统计（首页顶部状态 Tab 角标），对齐后端 TenderStatsVO */
export interface TenderStats {
  allCount: number;
  pendingCount: number;
  processingCount: number;
  completedCount: number;
  failedCount: number;
}
