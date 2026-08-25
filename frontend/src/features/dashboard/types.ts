/**
 * Dashboard 类型 — 项目级别的类型保留在此，
 * 标书/审核基础类型从 @/types/audit 引入。
 */
export type { BidDocument } from '@/types/audit';

export interface ProjectParams {
  id: number;
  projectName: string;
  supplierName?: string;
}

export interface auditTask {
  id: number;
  taskId: string;
  bidId: number;
  taskStatus: number;
  auditResult: string;
  issueCount: number;
  criticalCount: number;
  warningCount: number;
  infoCount: number;
  startTime: string;
  endTime: string;
  auditUserId: number;
  createTime: string;
}

export interface auditReport {
  id: number;
  auditId: number;
  docContent: string;
  version: number;
  generateTime: string;
}

/** 项目下标书及其审核报告 */
export interface BidDetail {
  tender: import('@/types/audit').BidDocument;
  auditTask: auditTask;
  auditReport: auditReport;
}

/** Dashboard 项目列表项（含子标书） */
export interface ProjectItem {
  id: number;
  userId: number;
  projectName: string;
  supplierName: string;
  parseStatus: number; // 0=未审核, 1=已审核
  latestVersion: number;
  createTime: string;
  updateTime: string;
  auditResult?: string | null;
  tenders: BidDetail[];
}

/** 问题类型分布：后端按 audit_issue.category（实际为 Rust 引擎的 risk_type 中文标签）分组计数 */
export type DistributionResponse = Record<string, number>;

export interface IssueChartItem {
  name: string;
  value: number;
}

/** 月度每日问题数（name=当月第几日，count=问题数） */
export interface DailyIssueCountItem {
  name: string;
  count: number;
}