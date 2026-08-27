/**
 * 共享类型定义 — 以 Rust 后端数据结构为标准。
 *
 * 本文件是审核、Chat、标书等跨特性类型的唯一来源。
 * 各 features 内的 types.ts 中重复定义应逐步移除此处并由 import 替代。
 */

// ─── Severity（4 级，对齐 Rust RiskSeverity） ───

export type Severity = 'info' | 'low' | 'medium' | 'high';

export const SEVERITY_MAP: Record<Severity, string> = {
  info: '信息',
  low: '低风险',
  medium: '中风险',
  high: '高风险',
};

export const SEVERITY_COLORS: Record<Severity, string> = {
  info: '#1890ff',
  low: '#faad14',
  medium: '#fa8c16',
  high: '#f5222d',
};

// ─── Risk Tier（3 级，对齐 Rust RiskTier） ───

export type RiskTier = 'L1' | 'L2' | 'L3';

export const TIER_MAP: Record<RiskTier, string> = {
  L1: '低风险条款',
  L2: '中等风险',
  L3: '高风险条款',
};

export const TIER_COLORS: Record<RiskTier, string> = {
  L1: '#52c41a',
  L2: '#faad14',
  L3: '#f5222d',
};

// ─── 标书文档（统一当前多处 BidDetail / ProjectItem） ───

export interface BidDocument {
  id: number;
  fileName: string;
  filePath: string;
  fileSize: number;
  fileType: string;
  fileCategory: 'bid' | 'contract';
  bidName: string;
  supplierName: string;
  budgetAmount: number | string;
  pageCount: number;
  /** 0=Pending 1=Processing 2=Completed 3=Failed */
  parseStatus: 0 | 1 | 2 | 3;
  uploadUserId: number;
  uploadTime: string;
  version: number;
  projectId: number;
  auditorName?: string;
  auditResult?: string | null;
}

// ─── 审核 ───

export interface AuditLocation {
  pageNumber: number;
  sectionName: string;
  context: string;
}

/** 对齐 Rust Citation — 搜索来源引用 */
export interface Citation {
  title: string;
  url: string;
  siteName?: string;
}

/** 对齐 Rust SuggestedAgent — BlindSpot 动态 Agent 建议 */
export interface SuggestedAgent {
  agentName: string;
  agentPrompt: string;
  sectionKeywords: string[];
  reason: string;
}

/** 对齐 Rust RiskFinding（全 22 字段） */
export interface AuditIssue {
  id?: number;
  issueNo: string;
  riskId?: string;
  severity: Severity;
  /** 是否属于重大/红线问题；重大问题仍使用 severity='high' */
  isCritical?: boolean;
  /** 重大问题判定依据 */
  criticalReason?: string;
  category: string;
  /** 发现此风险的 Agent 名称（如 FactCheckAgent / SemanticRiskAgent） */
  agentName?: string;
  description: string;
  suggestion: string;
  location?: AuditLocation;
  reference?: string;
  sourceQuote?: string;
  legalBasis?: string[];
  caseRefs?: string[];
  confidence?: number;
  /** PDF 锚定 */
  anchorQuote?: string;
  anchorPage?: number;
  anchorSection?: string;
  anchorTokens?: string[];
  anchorCharsRange?: number[];

  // ── Rust RiskFinding 新增字段 ──

  /** 是否判定为无风险 */
  noRisk?: boolean;
  /** 初始风险分级（关键词扫描） */
  initialTier?: RiskTier;
  /** 最终风险分级（可能经动态升降级） */
  finalTier?: RiskTier;
  /** 是否发生过动态升级（L1/L2 → L3） */
  tierEscalated?: boolean;
  /** 是否因 maxTurns 耗尽而截断（需人工复核） */
  truncated?: boolean;
  /** 关联的条款 chunk_id 列表（支持跨条款组合风险） */
  clauseIds?: string[];
  /** 关联的原始 block_id 列表（用于 bbox-based PDF 精确高亮） */
  blockIds?: string[];
  /** 搜索来源引用（结构化，可点击链接） */
  citations?: Citation[];
  /** BlindSpot 建议的动态 Agent */
  suggestedAgent?: SuggestedAgent;
  /** Agent 标签（与 agentName 独立表示，避免冲突） */
  agent?: string;
}

/** 对齐 Rust RoutingSummary + 4 级统计 */
export interface AuditSummary {
  totalIssues: number;
  /** 触发重大风险红线的问题数；它与 high 严重度正交，且是 high 的子集 */
  critical?: number;
  high: number;
  medium: number;
  low: number;
  info: number;

  // ── Rust RoutingSummary 扩展字段 ──
  /** 审查条款总数 */
  totalClauses?: number;
  /** 各 Agent 分配的条款数 */
  agentClauseCounts?: Record<string, number>;
  /** Legal Verify 执行的验证次数 */
  legalVerifyCount?: number;
  /** BlindSpot 发现的新风险数 */
  blindSpotFindings?: number;
}

export interface AuditStatus {
  taskId: string;
  status: string;
  stage: string;
  progress: number;
  issueCount: number;
  failedStages: string[];
}

export interface AuditResult {
  taskId: string;
  auditResult: string;
  summary: AuditSummary;
  issues: AuditIssue[];
  /** 协调器路由统计 */
  routingSummary?: AuditSummary;
  /** 会话知识图谱快照 */
  graphSnapshot?: GraphSnapshot;
}

// ─── 会话知识图谱（对齐 Rust GraphSnapshot） ───

export interface ChunkNode {
  chunkId: string;
  sectionPath: string[];
  pageStart: number;
  pageEnd: number;
  textPreview: string;
  tier: RiskTier;
}

export interface RiskNode {
  finding: AuditIssue;
  lawRefs: string[];
}

export interface AgentNode {
  agentId: string;
  displayName: string;
  role: string;
}

export interface LawNode {
  lawId: string;
  articleNo: string;
  title: string;
}

export interface CaseNode {
  caseId: string;
  title: string;
  summary: string;
}

export interface LinkedChunk {
  chunkId: string;
  reason: string;
}

export interface GraphSnapshot {
  chunks: Record<string, ChunkNode>;
  risks: Record<string, RiskNode>;
  hasRisk: Record<string, string[]>;
  reviewedBy: Record<string, string[]>;
  linkedTo: Record<string, LinkedChunk[]>;
  cites: Record<string, string[]>;
  citedBy: Record<string, string[]>;
  agents: Record<string, AgentNode>;
  laws: Record<string, LawNode>;
  cases: Record<string, CaseNode>;
  contradicts: Record<string, [string, string][]>;
  sameLaw: Record<string, string[]>;
}

// ─── 创建审核任务 ───

export interface CreateTaskParams {
  bidId: number;
  /** Rust Agent 名称（小写），如 factcheck / procedure / semanticrisk */
  enabledAgents?: string[];
  forceRefresh?: boolean;
}

// ─── 审核列表查询 ───

export interface AuditListQueryParams {
  page: number;
  size: number;
  bidName?: string;
  fileCategory?: string;
  status?: string;
  uploadStartTime?: string;
  uploadEndTime?: string;
}

// ─── Chat（对齐 Rust ChatAgent） ───

export interface TextSelectionData {
  text: string;
  blockIds: string[];
  page: number;
  bbox?: { x0: number; top: number; x1: number; bottom: number };
}

/** Rust BlockRef — 原文引用 */
export interface BlockRefCitation {
  type: 'block';
  blockId: string;
  quote: string;
  snippet: string;
  page: number;
}

/** Rust KnowledgeRef — 法规/案例引用 */
export interface KnowledgeRefCitation {
  type: 'law' | 'case' | 'negative_list';
  title: string;
  excerpt: string;
  sourceUrl?: string;
}

export type ChatCitation = BlockRefCitation | KnowledgeRefCitation;

export interface SendChatRequest {
  projectId: number;
  bidId: number;
  content: string;
  mode?: 'default' | 'supplement';
  selection?: TextSelectionData;
  saveToKnowledgeBase?: boolean;
  normalizeBeforeSave?: boolean;
}

export interface SendChatResponse {
  content: string;
  reasoning?: string[];
  citations: ChatCitation[];
  confidence?: number;
  suggestedActions?: string[];
}

export interface ChatHistoryItem {
  id: number;
  projectId: number;
  bidId: number;
  role: 'user' | 'assistant';
  content: string;
  createTime: string;
}

export interface FetchChatHistoryParams {
  projectId: number;
  bidId: number;
  days?: number;
}

// ─── SSE 实时推送事件类型（§17.1） ───

/** Agent 审查进度事件 */
export interface AgentProgress {
  agent_id: string;
  agent_label: string;
  clauses_done: number;
  clauses_total: number;
  raw_findings: number;
  status: 'pending' | 'running' | 'completed' | 'failed';
}

/** ReAct 实时动态事件 */
export interface TraceEvent {
  event_type: string;  // turn_start / agent_thought / tool_call / tool_result / output_finding / agent_bus_send / agent_bus_recv
  agent_name: string;
  turn: number;
  clause_id?: string;
  summary: string;
  payload?: Record<string, unknown>;
  timestamp?: string;
}

/** 管线阶段切换事件 */
export interface PhaseEvent {
  phase: string;       // route / execute / merge / legal_verify / blind_spot / debate / triage
  phase_index: number; // 1-7
  total_phases: number; // 7
  message: string;
}

/** Agent 中文名映射表（大小写不敏感，匹配时统一 toLowerCase） */
const AGENT_LABEL_MAP: Record<string, string> = {
  factcheck: '事实核验',
  factcheckagent: '事实核验',
  procedure: '流程合规',
  procedureagent: '流程合规',
  ruleengine: '法规匹配',
  ruleengineagent: '法规匹配',
  semanticrisk: '风险识别',
  semanticriskagent: '风险识别',
  fiscal_compliance: '财政合规',
  technical_standard: '技术规范',
  bid_evaluation: '评标合规',
  legal_compliance: '法律合规',
  blind_spot: '隐性风险',
  blindspotagent: '隐性风险',
  legal_verify: '法条核验',
  debate: '争议裁决',
  demandagent: '需求合理性',
  demand: '需求合理性',
};

/** 根据 Agent ID/名称 返回中文名，未匹配则返回原名 */
export const agentLabel = (id: string): string =>
  AGENT_LABEL_MAP[id.toLowerCase()] || id;

/** @deprecated 使用 agentLabel() 替代 */
export const AGENT_LABELS: Record<string, string> = AGENT_LABEL_MAP;

// ─── SSE 实时推送：finding_added / finding_updated / finding_removed / stats ───

/** Finding 生命周期（对齐 Rust FindingLifecycle） */
export type FindingLifecycle = 'verified' | 'blind_spot' | 'debated';

/** 对齐 Rust ReviewEvent::FindingAdded */
export interface FindingAddedEvent {
  risk_id: string;
  severity: string;
  is_critical?: boolean;
  critical_reason?: string;
  risk_type: string;
  agent: string;
  confidence: number;
  clause_ids: string[];
  source_quote: string;
  legal_basis: string[];
  reason: string;
  suggestion: string;
  lifecycle: FindingLifecycle;
  page_number?: number;
  section_path?: string[];
  /** 关联的原始 block_id（Rust 流式补发，用于直接查 bbox 画高亮框） */
  block_ids?: string[];
}

/** 对齐 Rust ReviewEvent::FindingUpdated */
export interface FindingUpdatedEvent {
  risk_id: string;
  changes: Array<{
    field: string;
    old_value?: string;
    new_value?: string;
  }>;
  reason: string;
}

/** 对齐 Rust ReviewEvent::FindingRemoved */
export interface FindingRemovedEvent {
  risk_id: string;
  reason: string;
  merged_into?: string;
}

/** 对齐 Rust ReviewEvent::Stats */
export interface StatsEvent {
  phase: string;
  total_raw: number;
  total_merged: number;
  total_verified: number;
  high: number;
  medium: number;
  low: number;
  info: number;
}

// ─── 条款动态地图（Clause Activity Map）数据结构 ───

/** 条款审查状态 */
export type ClauseReviewStatus = 'pending' | 'reviewing' | 'reviewed';

/** 单条条款的审查状态 */
export interface ClauseState {
  clauseId: string;
  sectionPath: string[];
  pageNumber?: number;
  status: ClauseReviewStatus;
  reviewedBy: string[];
  risks: FindingAddedEvent[];
}

/** 章节树节点 */
export interface SectionTreeNode {
  key: string;
  title: string;
  /** 从根到当前节点的路径段列表 */
  path: string[];
  /** 此节点下的所有条款 clause_id */
  clauseIds: string[];
  /** 此节点下的所有风险（含子节点） */
  riskCount: number;
  /** 最高严重度 */
  maxSeverity: Severity | null;
  children: SectionTreeNode[];
}

/** 管线阶段中文映射 */
export const PHASE_LABELS: Record<string, string> = {
  route: '智能路由',
  execute: '并行审查',
  merge: '去重合并',
  legal_verify: '法条验证',
  blind_spot: '盲点扫描',
  debate: '辩论裁决',
  triage: '最终分流',
};

/** 管线阶段顺序 */
export const PHASE_ORDER = [
  'route', 'execute', 'merge', 'legal_verify',
  'blind_spot', 'debate', 'triage',
];
