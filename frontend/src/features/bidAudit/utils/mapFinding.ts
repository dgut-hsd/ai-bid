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
  GraphSnapshot,
  FindingState,
  ReviewAttemptErrorCode,
  ReviewAttemptOutcome,
  ReviewAttemptStatus,
} from '@/types/audit';

// ─── 后端原始类型 ───

interface BackendFinding {
  risk_id: string;
  clause_ids: string[];
  block_ids?: string[];
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

interface BackendSuggestedAgent {
  agent_name?: string;
  agentName?: string;
  agent_prompt?: string;
  agentPrompt?: string;
  section_keywords?: string[];
  sectionKeywords?: string[];
  reason?: string;
}

interface BackendChunkNode {
  chunk_id: string;
  section_path: string[];
  page_start: number;
  page_end: number;
  text_preview: string;
  tier: RiskTier;
}

interface BackendRiskNode {
  finding: BackendFinding;
  law_refs: string[];
  state?: FindingState;
  merged_into?: string;
  decision_reason?: string;
}

type BackendAgentId = string | { Dynamic: string };

interface BackendAgentNode {
  agent_id: BackendAgentId;
  display_name: string;
  role: string;
}

interface BackendLawNode {
  law_id: string;
  article_no: string;
  title: string;
}

interface BackendCaseNode {
  case_id: string;
  title: string;
  summary: string;
}

interface BackendLinkedChunk {
  chunk_id: string;
  reason: string;
}

interface BackendReviewAttempt {
  attempt_id: string;
  agent_id: BackendAgentId;
  chunk_id: string;
  status: ReviewAttemptStatus;
  outcome?: ReviewAttemptOutcome;
  finding_ids: string[];
  error_code?: ReviewAttemptErrorCode;
  error_message?: string;
  started_at: string;
  finished_at?: string;
}

interface BackendFindingTransition {
  risk_id: string;
  from: FindingState;
  to: FindingState;
  reason: string;
  merged_into?: string;
  decided_at: string;
}

export interface BackendGraphSnapshot {
  graph_version?: number;
  chunk_versions?: Record<string, number>;
  chunks: Record<string, BackendChunkNode>;
  risks: Record<string, BackendRiskNode>;
  has_risk: Record<string, string[]>;
  reviewed_by: Record<string, BackendAgentId[]>;
  linked_to: Record<string, BackendLinkedChunk[]>;
  cites: Record<string, string[]>;
  cited_by: Record<string, string[]>;
  agents: Record<string, BackendAgentNode>;
  laws: Record<string, BackendLawNode>;
  cases: Record<string, BackendCaseNode>;
  contradicts: Record<string, [string, string][]>;
  same_law: Record<string, string[]>;
  review_attempts: Record<string, BackendReviewAttempt>;
  finding_transitions?: BackendFindingTransition[];
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

const mapRecord = <T, R>(
  source: Record<string, T>,
  mapper: (value: T) => R,
): Record<string, R> =>
  Object.fromEntries(
    Object.entries(source ?? {}).map(([key, value]) => [key, mapper(value)]),
  );

const mapBackendAgentId = (agentId: BackendAgentId): string =>
  typeof agentId === 'string' ? agentId : agentId.Dynamic;

/** 将 Rust GraphSnapshot 的 snake_case 字段显式转换为前端领域模型。 */
export const mapBackendGraphSnapshot = (
  raw: BackendGraphSnapshot,
): GraphSnapshot => ({
  graphVersion: raw.graph_version ?? 0,
  chunkVersions: raw.chunk_versions ?? {},
  chunks: mapRecord(raw.chunks, (chunk) => ({
    chunkId: chunk.chunk_id,
    sectionPath: chunk.section_path,
    pageStart: chunk.page_start,
    pageEnd: chunk.page_end,
    textPreview: chunk.text_preview,
    tier: chunk.tier,
  })),
  risks: mapRecord(raw.risks, (risk) => ({
    finding: mapBackendFinding(risk.finding),
    lawRefs: risk.law_refs,
    state: risk.state ?? 'provisional',
    mergedInto: risk.merged_into,
    decisionReason: risk.decision_reason,
  })),
  hasRisk: raw.has_risk ?? {},
  reviewedBy: mapRecord(raw.reviewed_by, (agentIds) =>
    agentIds.map(mapBackendAgentId),
  ),
  linkedTo: mapRecord(raw.linked_to, (links) =>
    links.map((link) => ({ chunkId: link.chunk_id, reason: link.reason })),
  ),
  cites: raw.cites ?? {},
  citedBy: raw.cited_by ?? {},
  agents: mapRecord(raw.agents, (agent) => ({
    agentId: mapBackendAgentId(agent.agent_id),
    displayName: agent.display_name,
    role: agent.role,
  })),
  laws: mapRecord(raw.laws, (law) => ({
    lawId: law.law_id,
    articleNo: law.article_no,
    title: law.title,
  })),
  cases: mapRecord(raw.cases, (caseNode) => ({
    caseId: caseNode.case_id,
    title: caseNode.title,
    summary: caseNode.summary,
  })),
  contradicts: raw.contradicts ?? {},
  sameLaw: raw.same_law ?? {},
  reviewAttempts: mapRecord(raw.review_attempts, (attempt) => ({
    attemptId: attempt.attempt_id,
    agentId: mapBackendAgentId(attempt.agent_id),
    chunkId: attempt.chunk_id,
    status: attempt.status,
    outcome: attempt.outcome,
    findingIds: attempt.finding_ids,
    errorCode: attempt.error_code,
    errorMessage: attempt.error_message,
    startedAt: attempt.started_at,
    finishedAt: attempt.finished_at,
  })),
  findingTransitions: (raw.finding_transitions ?? []).map((transition) => ({
    riskId: transition.risk_id,
    from: transition.from,
    to: transition.to,
    reason: transition.reason,
    mergedInto: transition.merged_into,
    decidedAt: transition.decided_at,
  })),
});

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
 * 注意 blockIds 固定为空数组：Rust 的 ReviewEvent::FindingAdded 目前不下发 block_ids
 * （仅最终 /result 才带坐标），所以流式阶段的卡片只能走文本匹配定位，画不出 BBox 框；
 * 待审核完成、拉取 /result 后由 mapBackendFinding 覆盖为带 blockIds 的完整版。
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
    blockIds: [],
  };
};
