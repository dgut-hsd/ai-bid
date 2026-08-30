import type {
  FindingAddedEvent,
  TraceEvent,
  AgentProgress,
  AuditIssue,
  SectionTreeNode,
  ClauseState,
  Severity,
} from '@/types/audit';

/** 合并 findings + issues → 按 clause 分组 */
export function buildClauseMap(
  liveFindings: FindingAddedEvent[],
  issues: AuditIssue[],
  traceEvents: TraceEvent[],
  agentProgresses: Map<string, AgentProgress>,
): Map<string, ClauseState> {
  const map = new Map<string, ClauseState>();

  const ensureClause = (clauseId: string, sectionPath: string[], pageNumber?: number): ClauseState => {
    if (!map.has(clauseId)) {
      map.set(clauseId, {
        clauseId,
        sectionPath,
        pageNumber,
        status: 'pending',
        reviewedBy: [],
        risks: [],
      });
    }
    const c = map.get(clauseId)!;
    // 合并 sectionPath（取更详细的）
    if (sectionPath.length > c.sectionPath.length) {
      c.sectionPath = sectionPath;
    }
    if (pageNumber != null && c.pageNumber == null) {
      c.pageNumber = pageNumber;
    }
    return c;
  };

  // 1. 从 liveFindings (SSE finding_added) 构建
  for (const f of liveFindings) {
    const sp = f.section_path ?? [];
    for (const cid of f.clause_ids) {
      const clause = ensureClause(cid, sp, f.page_number);
      // 去重 risk_id
      if (!clause.risks.some(r => r.risk_id === f.risk_id)) {
        clause.risks.push(f);
      }
    }
  }

  // 2. 从 issues (最终结果 / 轮询) 补全
  for (const issue of issues) {
    const riskId = issue.riskId ?? issue.issueNo ?? '';
    const clauseIds = issue.clauseIds ?? [];
    // 尝试从 location.sectionName / anchorSection 推断 section_path
    const secName = issue.location?.sectionName ?? issue.anchorSection ?? '';
    const sp: string[] = secName ? secName.split(' > ') : [];
    for (const cid of clauseIds) {
      const clause = ensureClause(cid, sp, issue.location?.pageNumber);
      // 去重
      if (!clause.risks.some(r => r.risk_id === riskId)) {
        clause.risks.push({
          risk_id: riskId,
          severity: issue.severity,
          is_critical: issue.isCritical ?? false,
          critical_reason: issue.criticalReason ?? '',
          risk_type: issue.category ?? '',
          agent: issue.agent ?? issue.agentName ?? '',
          confidence: issue.confidence ?? 0,
          clause_ids: clauseIds,
          source_quote: '',
          legal_basis: [],
          reason: issue.description ?? '',
          suggestion: issue.suggestion ?? '',
          lifecycle: 'verified',
          page_number: issue.location?.pageNumber,
          section_path: sp.length > 0 ? sp : undefined,
        });
      }
    }
  }

  // 3. 从 trace 事件获取 clause_id（标记为 reviewing）
  for (const ev of traceEvents) {
    if (ev.clause_id) {
      ensureClause(ev.clause_id, [], undefined);
      const c = map.get(ev.clause_id)!;
      if (c.status === 'pending') {
        c.status = 'reviewing';
      }
    }
  }

  // 4. 标记 reviewed 状态：从 agentProgresses 推断
  for (const [, ap] of agentProgresses) {
    if (ap.status === 'completed') {
      for (const [, clause] of map) {
        if (!clause.reviewedBy.includes(ap.agent_label)) {
          clause.reviewedBy.push(ap.agent_label);
        }
        clause.status = 'reviewed';
      }
    }
  }

  return map;
}

/** 从 ClauseState map 构建章节树 */
export function buildSectionTree(clauseMap: Map<string, ClauseState>): SectionTreeNode[] {
  const root: SectionTreeNode[] = [];

  for (const [, clause] of clauseMap) {
    const path = clause.sectionPath.length > 0
      ? clause.sectionPath
      : ['其他条款'];

    insertClauseIntoTree(root, clause, path, 0);
  }

  // 递归排序：有风险的排前面，同级按 riskCount 降序
  sortTree(root);

  return root;
}

function insertClauseIntoTree(
  nodes: SectionTreeNode[],
  clause: ClauseState,
  path: string[],
  depth: number,
) {
  if (depth >= path.length) return;

  const title = path[depth];
  const key = path.slice(0, depth + 1).join(' > ');

  let node = nodes.find(n => n.key === key);
  if (!node) {
    node = {
      key,
      title,
      path: path.slice(0, depth + 1),
      clauseIds: [],
      riskCount: 0,
      maxSeverity: null,
      children: [],
    };
    nodes.push(node);
  }

  if (depth === path.length - 1) {
    // 叶子节点：挂载 clause
    if (!node.clauseIds.includes(clause.clauseId)) {
      node.clauseIds.push(clause.clauseId);
    }
    // 更新统计
    const clauseRisks = clause.risks.filter(r => r.severity !== 'info');
    node.riskCount += clauseRisks.length;
    for (const r of clauseRisks) {
      if (!node.maxSeverity || severityOrder(r.severity) > severityOrder(node.maxSeverity)) {
        node.maxSeverity = r.severity as Severity;
      }
    }
  } else {
    insertClauseIntoTree(node.children, clause, path, depth + 1);
    // 向上传播统计
    propagateStats(node);
  }
}

function propagateStats(node: SectionTreeNode) {
  let totalRisks = 0;
  let maxSev: Severity | null = null;
  for (const child of node.children) {
    totalRisks += child.riskCount;
    if (child.maxSeverity && (!maxSev || severityOrder(child.maxSeverity) > severityOrder(maxSev))) {
      maxSev = child.maxSeverity;
    }
  }
  // 也计算直接挂载在此节点的 clause 风险
  // (已在前面的 insertClauseIntoTree 中计算)
  node.riskCount = Math.max(node.riskCount, totalRisks);
  if (maxSev && (!node.maxSeverity || severityOrder(maxSev) > severityOrder(node.maxSeverity))) {
    node.maxSeverity = maxSev;
  }
}

function sortTree(nodes: SectionTreeNode[]) {
  nodes.sort((a, b) => b.riskCount - a.riskCount || a.title.localeCompare(b.title, 'zh'));
  for (const node of nodes) {
    sortTree(node.children);
  }
}

function severityOrder(s: string): number {
  switch (s) {
    case 'high': return 4;
    case 'medium': return 3;
    case 'low': return 2;
    case 'info': return 1;
    default: return 0;
  }
}

/** 获取 section 的风险状态摘要 */
export function sectionStatus(node: SectionTreeNode): { color: string; label: string } {
  if (!node.maxSeverity) {
    return { color: '#52c41a', label: '无风险' };
  }
  switch (node.maxSeverity) {
    case 'high':
      return { color: '#f5222d', label: `${node.riskCount}处风险` };
    case 'medium':
      return { color: '#fa8c16', label: `${node.riskCount}处风险` };
    case 'low':
      return { color: '#1890ff', label: `${node.riskCount}处风险` };
    default:
      return { color: '#8c8c8c', label: '信息' };
  }
}

/** 获取单个 clause 的审查状态 */
export type ClauseStatusType = 'pending' | 'reviewing' | 'clean' | 'risk_high' | 'risk_medium' | 'risk_low';

export function clauseStatus(clause: ClauseState): { status: ClauseStatusType; color: string; label: string } {
  if (clause.status === 'reviewing') {
    return { status: 'reviewing', color: '#1890ff', label: '审查中' };
  }
  if (clause.status === 'pending') {
    return { status: 'pending', color: '#d9d9d9', label: '待审' };
  }
  const activeRisks = clause.risks.filter(r => r.severity !== 'info');
  if (activeRisks.length === 0) {
    const reviewCount = clause.reviewedBy.length;
    return { status: 'clean', color: '#52c41a', label: reviewCount > 0 ? `${reviewCount} Agent已审` : '已审' };
  }
  const hasHigh = activeRisks.some(r => r.severity === 'high');
  if (hasHigh) {
    return { status: 'risk_high', color: '#f5222d', label: `${activeRisks.length}处风险` };
  }
  const hasMedium = activeRisks.some(r => r.severity === 'medium');
  if (hasMedium) {
    return { status: 'risk_medium', color: '#fa8c16', label: `${activeRisks.length}处风险` };
  }
  return { status: 'risk_low', color: '#1890ff', label: `${activeRisks.length}处风险` };
}
