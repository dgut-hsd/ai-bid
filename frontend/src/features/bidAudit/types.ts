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
  fileCategory?: FileCategory;
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
  auditResult?: string | null;
}
