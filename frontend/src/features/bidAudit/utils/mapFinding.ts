/**
 * 后端 Rust RiskFinding (snake_case) → 前端 AuditIssue (camelCase) 映射工具。
 *
 * 后端 JSON 字段（来自 /api/audit-tasks/{taskId}/result 和 SSE 流）：
 *   risk_id, clause_ids, agent, no_risk, severity, risk_type,
 *   source_quote, legal_basis, case_refs, reason, suggestion,
 *   confidence, _initial_tier, _final_tier, _tier_escalated, _truncated,
 *   suggested_agent, citations, page_number, section_path, context
 *
 * 前端 AuditIssue 类型定义在 @/types/audit.ts。
 */

import type {
  AuditIssue,
  Citation,
  SuggestedAgent,
  RiskTier,
  Severity,
  FindingAddedEvent,
} from '@/types/audit';

// ─── 后端原始类型 ───

interface BackendFinding {
  risk_id: string;
  clause_ids: string[];
  block_ids?: string[];
  highlight_rects?: BackendHighlightRect[];
  agent: string;
  no_risk: boolean;
  severity: string;
  is_critical?: boolean;
  critical_reason?: string;
  risk_type: string;
  source_quote: string;
  legal_basis: string[];
  case_refs: string[];
  reason: string;
  suggestion: string;
  confidence: number;
  _initial_tier?: string;
  _final_tier?: string;
  _tier_escalated?: boolean;
  _truncated?: boolean;
  suggested_agent: BackendSuggestedAgent | null;
  citations: BackendCitation[];
  page_number: number;
  section_path: string[];
  context: string;
}

interface BackendCitation {
  title: string;
  url: string;
  site_name?: string;
}

interface BackendHighlightRect {
  page?: number;
  x0?: number;
  top?: number;
  x1?: number;
  bottom?: number;
  page_width?: number;
}

interface BackendSuggestedAgent {
  agent_name?: string;
  agentName?: string;
  agent_prompt?: string;
  agentPrompt?: string;
  section_keywords?: string[];
  sectionKeywords?: string[];
  reason?: string;
}

// ─── 映射函数 ───

const isValidSeverity = (v: string): v is Severity =>
  v === 'high' || v === 'medium' || v === 'low' || v === 'info';

const normalizeTier = (v: string | undefined): RiskTier | undefined =>
  v ? (v.startsWith('L') ? v.slice(0, 2) as RiskTier : undefined) : undefined;

const mapCitation = (c: BackendCitation): Citation => ({
  title: c.title,
  url: c.url,
  siteName: c.site_name,
});

const mapSuggestedAgent = (
  sa: BackendSuggestedAgent | null
): SuggestedAgent | undefined => {
  if (!sa) return undefined;
  return {
    agentName: sa.agent_name || sa.agentName || '',
    agentPrompt: sa.agent_prompt || sa.agentPrompt || '',
    sectionKeywords: sa.section_keywords || sa.sectionKeywords || [],
    reason: sa.reason || '',
  };
};

/**
 * 将后端 Rust RiskFinding 转换为前端 AuditIssue。
 *
 * 字段映射速览：
 *   risk_id       → riskId, issueNo
 *   clause_ids    → clauseIds
 *   agent         → agentName
 *   no_risk       → noRisk
 *   severity      → severity
 *   risk_type     → category
 *   source_quote  → sourceQuote, anchorQuote
 *   legal_basis   → legalBasis
 *   case_refs     → caseRefs
 *   reason        → description
 *   suggestion    → suggestion
 *   confidence    → confidence
 *   _initial_tier → initialTier
 *   _final_tier   → finalTier
 *   _tier_escalated → tierEscalated
 *   _truncated    → truncated
 *   suggested_agent → suggestedAgent
 *   citations     → citations (site_name → siteName)
 *   page_number   → anchorPage, location.pageNumber
 *   section_path  → anchorSection, location.sectionName
 *   context       → anchorQuote (also location.context)
 */
export const mapBackendFinding = (raw: BackendFinding): AuditIssue => {
  const severity: Severity = isValidSeverity(raw.severity)
    ? raw.severity
    : 'info';

  const sectionPath = Array.isArray(raw.section_path) ? raw.section_path : [];
  const sectionName =
    sectionPath.length > 0 ? sectionPath.join(' > ') : undefined;
  const leafSection =
    sectionPath.length > 0 ? sectionPath[sectionPath.length - 1] : undefined;

  return {
    issueNo: raw.risk_id,
    riskId: raw.risk_id,
    severity,
    isCritical: raw.is_critical ?? false,
    criticalReason: raw.critical_reason || undefined,
    category: raw.risk_type || '未分类',
    agentName: raw.agent,
    agent: raw.agent,
    noRisk: raw.no_risk,
    description: raw.reason || '',
    suggestion: raw.suggestion || '',
    sourceQuote: raw.source_quote,
    legalBasis: Array.isArray(raw.legal_basis) ? raw.legal_basis : [],
    caseRefs: Array.isArray(raw.case_refs) ? raw.case_refs : [],
    confidence: typeof raw.confidence === 'number' ? raw.confidence : undefined,
    initialTier: normalizeTier(raw._initial_tier),
    finalTier: normalizeTier(raw._final_tier),
    tierEscalated: raw._tier_escalated ?? false,
    truncated: raw._truncated ?? false,
    suggestedAgent: mapSuggestedAgent(raw.suggested_agent),
    citations: Array.isArray(raw.citations)
      ? raw.citations.map(mapCitation)
      : [],
    clauseIds: Array.isArray(raw.clause_ids) ? raw.clause_ids : [],
    blockIds: Array.isArray(raw.block_ids) ? raw.block_ids : [],
    // 词级精确高亮矩形（后端 source_quote → 命中词的逐行 union bbox）
    highlightRects: Array.isArray(raw.highlight_rects)
      ? raw.highlight_rects.map((r) => ({
          page: r.page ?? 0,
          x0: r.x0 ?? 0,
          top: r.top ?? 0,
          x1: r.x1 ?? 0,
          bottom: r.bottom ?? 0,
          pageWidth: r.page_width ?? 0,
        }))
      : [],
    // 锚定信息
    anchorPage:
      typeof raw.page_number === 'number' && raw.page_number >= 0
        ? raw.page_number
        : undefined,
    anchorSection: sectionName,
    anchorQuote: raw.source_quote || raw.context,
    location: {
      pageNumber: raw.page_number ?? 0,
      sectionName: sectionName || leafSection || '',
      context: raw.context || '',
    },
  };
};

/**
 * 批量转换。
 */
export const mapBackendFindings = (rawList: BackendFinding[]): AuditIssue[] =>
  rawList.map(mapBackendFinding);

/**
 * 尝试检测一个对象是后端格式还是已转换的前端格式。
 * 如果包含后端特征字段（risk_id / reason），视为后端格式。
 */
export const isBackendFormat = (item: Record<string, unknown>): boolean =>
  'risk_id' in item && !('issueNo' in item);

/**
 * 智能转换：自动检测格式并转换。
 */
export const ensureAuditIssue = (item: Record<string, unknown>): AuditIssue =>
  isBackendFormat(item)
    ? mapBackendFinding(item as unknown as BackendFinding)
    : (item as unknown as AuditIssue);

/**
 * SSE `finding_added` 事件 → 前端 AuditIssue。
 *
 * 与 mapBackendFinding 的区别：SSE 事件是「流式阶段的轻量快照」，
 * 字段比最终 result 少（无 case_refs / citations / suggested_agent / _tier 系列 / context）。
 *
 * block_ids 由 Rust 在流式阶段从 clause.source_block_ids 聚合补发（coarse），
 * 前端据此可直接查 bbox 画高亮框；审核完成后 /result 的 mapBackendFinding
 * 会再用 source_quote 反查覆盖为更精确的 blockIds。
 */
export const mapFindingAddedEvent = (event: FindingAddedEvent): AuditIssue => {
  const severity: Severity = isValidSeverity(event.severity)
    ? event.severity
    : 'info';

  const sectionPath = Array.isArray(event.section_path) ? event.section_path : [];
  const sectionName =
    sectionPath.length > 0 ? sectionPath.join(' > ') : undefined;

  return {
    issueNo: event.risk_id,
    riskId: event.risk_id,
    severity,
    isCritical: event.is_critical ?? false,
    criticalReason: event.critical_reason,
    category: event.risk_type || '未分类',
    agentName: event.agent,
    agent: event.agent,
    noRisk: false,
    description: event.reason || '',
    suggestion: event.suggestion || '',
    sourceQuote: event.source_quote,
    legalBasis: Array.isArray(event.legal_basis) ? event.legal_basis : [],
    caseRefs: [],
    citations: [],
    confidence: typeof event.confidence === 'number' ? event.confidence : undefined,
    anchorPage:
      typeof event.page_number === 'number' && event.page_number >= 0
        ? event.page_number
        : undefined,
    anchorSection: sectionName,
    anchorQuote: event.source_quote,
    location: {
      pageNumber: event.page_number ?? 0,
      sectionName: sectionName || '',
      context: event.source_quote || '',
    },
    clauseIds: Array.isArray(event.clause_ids) ? event.clause_ids : [],
    blockIds: Array.isArray(event.block_ids) ? event.block_ids : [],
  };
};
