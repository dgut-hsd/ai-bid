import { describe, it, expect } from 'vitest';
import {
  mapBackendFinding,
  mapBackendFindings,
  isBackendFormat,
  ensureAuditIssue,
} from './mapFinding';

// ─── 样本数据 ───

const completeBackendFinding = {
  risk_id: 'RISK-001',
  clause_ids: ['CL-001', 'CL-002'],
  block_ids: ['BLK-001'],
  agent: 'FactCheckAgent',
  no_risk: false,
  severity: 'high',
  is_critical: true,
  critical_reason: '该条款会造成供应商被不合理排除',
  risk_type: '条款冲突',
  source_quote: '供应商应在合同签订后30日内交付全部货物...',
  legal_basis: ['《招标投标法》第46条', '《民法典》第577条'],
  case_refs: ['(2023)最高法民终XX号'],
  reason: '交付期限与招标文件要求不符',
  suggestion: '建议修改为合同签订后15日内交付',
  confidence: 0.89,
  _initial_tier: 'L1',
  _final_tier: 'L2',
  _tier_escalated: true,
  _truncated: false,
  suggested_agent: {
    agent_name: 'TechnicalStandardAgent',
    agent_prompt: '请检查技术规范合规性',
    section_keywords: ['技术参数', '规格'],
    reason: '涉及技术参数需进一步核查',
  },
  citations: [
    { title: '招标投标法', url: 'http://example.com/law', site_name: '国家法律法规数据库' },
  ],
  page_number: 5,
  section_path: ['第三章', '技术需求', '3.1 交付要求'],
  context: '本节规定了交付期限和验收标准',
};

// ─── mapBackendFinding ───

describe('mapBackendFinding', () => {
  it('maps a complete backend finding to a correct AuditIssue', () => {
    const result = mapBackendFinding(completeBackendFinding as any);

    expect(result.issueNo).toBe('RISK-001');
    expect(result.riskId).toBe('RISK-001');
    expect(result.severity).toBe('high');
    expect(result.isCritical).toBe(true);
    expect(result.criticalReason).toBe('该条款会造成供应商被不合理排除');
    expect(result.category).toBe('条款冲突');
    expect(result.agentName).toBe('FactCheckAgent');
    expect(result.agent).toBe('FactCheckAgent');
    expect(result.noRisk).toBe(false);
    expect(result.description).toBe('交付期限与招标文件要求不符');
    expect(result.suggestion).toBe('建议修改为合同签订后15日内交付');
    expect(result.sourceQuote).toBe('供应商应在合同签订后30日内交付全部货物...');
    expect(result.legalBasis).toEqual(['《招标投标法》第46条', '《民法典》第577条']);
    expect(result.caseRefs).toEqual(['(2023)最高法民终XX号']);
    expect(result.confidence).toBe(0.89);
    expect(result.initialTier).toBe('L1');
    expect(result.finalTier).toBe('L2');
    expect(result.tierEscalated).toBe(true);
    expect(result.truncated).toBe(false);
    expect(result.clauseIds).toEqual(['CL-001', 'CL-002']);
    expect(result.blockIds).toEqual(['BLK-001']);
    expect(result.anchorPage).toBe(5);
    expect(result.anchorSection).toBe('第三章 > 技术需求 > 3.1 交付要求');
    expect(result.location).toEqual({
      pageNumber: 5,
      sectionName: '第三章 > 技术需求 > 3.1 交付要求',
      context: '本节规定了交付期限和验收标准',
    });
  });

  it('maps suggested_agent using snake_case fields with priority', () => {
    const result = mapBackendFinding(completeBackendFinding as any);

    expect(result.suggestedAgent).toBeDefined();
    expect(result.suggestedAgent!.agentName).toBe('TechnicalStandardAgent');
    expect(result.suggestedAgent!.agentPrompt).toBe('请检查技术规范合规性');
    expect(result.suggestedAgent!.sectionKeywords).toEqual(['技术参数', '规格']);
    expect(result.suggestedAgent!.reason).toBe('涉及技术参数需进一步核查');
  });

  it('falls back to camelCase fields when snake_case is absent in suggested_agent', () => {
    const input = {
      ...completeBackendFinding,
      suggested_agent: {
        agentName: 'CustomAgent',
        agentPrompt: 'Custom analysis prompt',
        sectionKeywords: ['keyword1'],
        reason: 'custom reason',
      },
    };

    const result = mapBackendFinding(input as any);
    expect(result.suggestedAgent!.agentName).toBe('CustomAgent');
    expect(result.suggestedAgent!.agentPrompt).toBe('Custom analysis prompt');
    expect(result.suggestedAgent!.sectionKeywords).toEqual(['keyword1']);
    expect(result.suggestedAgent!.reason).toBe('custom reason');
  });

  it('returns undefined suggestedAgent when suggested_agent is null', () => {
    const input = { ...completeBackendFinding, suggested_agent: null };
    const result = mapBackendFinding(input as any);
    expect(result.suggestedAgent).toBeUndefined();
  });

  it('maps citations with site_name → siteName', () => {
    const result = mapBackendFinding(completeBackendFinding as any);
    expect(result.citations).toHaveLength(1);
    expect(result.citations![0].title).toBe('招标投标法');
    expect(result.citations![0].url).toBe('http://example.com/law');
    expect(result.citations![0].siteName).toBe('国家法律法规数据库');
  });

  it('defaults to empty array when citations is null', () => {
    const input = { ...completeBackendFinding, citations: null };
    expect(mapBackendFinding(input as any).citations).toEqual([]);
  });

  it('falls back severity to "info" for unrecognized severity values', () => {
    const input = { ...completeBackendFinding, severity: 'critical' };
    expect(mapBackendFinding(input as any).severity).toBe('info');
  });

  it('falls back severity to "info" for empty string', () => {
    const input = { ...completeBackendFinding, severity: '' };
    expect(mapBackendFinding(input as any).severity).toBe('info');
  });

  it('normalizes tier values: preserves L1/L2/L3, strips suffixes', () => {
    const l1 = mapBackendFinding({ ...completeBackendFinding, _initial_tier: 'L1', _final_tier: 'L1' } as any);
    expect(l1.initialTier).toBe('L1');
    expect(l1.finalTier).toBe('L1');

    const escalated = mapBackendFinding({
      ...completeBackendFinding,
      _initial_tier: 'L3_escalated',
      _final_tier: 'L3',
    } as any);
    expect(escalated.initialTier).toBe('L3');
    expect(escalated.finalTier).toBe('L3');
  });

  it('returns undefined tier for values not starting with L', () => {
    const input = { ...completeBackendFinding, _initial_tier: 'XX', _final_tier: '' };
    const result = mapBackendFinding(input as any);
    expect(result.initialTier).toBeUndefined();
    expect(result.finalTier).toBeUndefined();
  });

  it('returns undefined confidence when confidence is not a number', () => {
    const inputNull = { ...completeBackendFinding, confidence: null };
    expect(mapBackendFinding(inputNull as any).confidence).toBeUndefined();

    const inputStr = { ...completeBackendFinding, confidence: '0.89' };
    expect(mapBackendFinding(inputStr as any).confidence).toBeUndefined();
  });

  it('builds anchorQuote from source_quote, falling back to context when empty', () => {
    const hasQuote = mapBackendFinding(completeBackendFinding as any);
    expect(hasQuote.anchorQuote).toBe(completeBackendFinding.source_quote);

    const noQuote = mapBackendFinding({ ...completeBackendFinding, source_quote: '' } as any);
    expect(noQuote.anchorQuote).toBe(completeBackendFinding.context);
  });

  it('handles section_path: single element, multiple elements, empty, null', () => {
    const single = mapBackendFinding({ ...completeBackendFinding, section_path: ['第一章'] } as any);
    expect(single.anchorSection).toBe('第一章');
    expect(single.location!.sectionName).toBe('第一章');

    const empty = mapBackendFinding({ ...completeBackendFinding, section_path: [] } as any);
    expect(empty.anchorSection).toBeUndefined();
    expect(empty.location!.sectionName).toBe('');

    const nil = mapBackendFinding({ ...completeBackendFinding, section_path: null } as any);
    expect(nil.anchorSection).toBeUndefined();
    expect(nil.location!.sectionName).toBe('');
  });

  it('handles non-array legal_basis and case_refs gracefully', () => {
    const input = { ...completeBackendFinding, legal_basis: null, case_refs: undefined };
    const result = mapBackendFinding(input as any);
    expect(result.legalBasis).toEqual([]);
    expect(result.caseRefs).toEqual([]);
  });

  it('handles negative page_number: anchorPage undefined, location preserves raw value', () => {
    const result = mapBackendFinding({ ...completeBackendFinding, page_number: -1 } as any);
    expect(result.anchorPage).toBeUndefined();
    expect(result.location!.pageNumber).toBe(-1);
  });

  it('sets default values for noRisk/truncated/tierEscalated when fields are missing', () => {
    const result = mapBackendFinding({
      risk_id: 'R2',
      clause_ids: [],
      agent: '',
      no_risk: undefined,
      severity: 'info',
      risk_type: '',
      source_quote: '',
      legal_basis: [],
      case_refs: [],
      reason: '',
      suggestion: '',
      confidence: 0,
      suggested_agent: null,
      citations: [],
      page_number: 0,
      section_path: [],
      context: '',
    } as any);

    expect(result.noRisk).toBeUndefined();
    expect(result.tierEscalated).toBe(false);
    expect(result.truncated).toBe(false);
  });
});

// ─── mapBackendFindings ───

describe('mapBackendFindings', () => {
  it('maps each element in the array through mapBackendFinding', () => {
    const results = mapBackendFindings([completeBackendFinding, completeBackendFinding] as any);
    expect(results).toHaveLength(2);
    results.forEach((r, _i) => {
      expect(r.issueNo).toBe('RISK-001');
      expect(r.severity).toBe('high');
    });
  });

  it('returns an empty array when given an empty array', () => {
    expect(mapBackendFindings([])).toEqual([]);
  });
});

// ─── isBackendFormat ───

describe('isBackendFormat', () => {
  it('returns true when object has risk_id and lacks issueNo', () => {
    expect(isBackendFormat({ risk_id: 'R1', reason: 'test' })).toBe(true);
  });

  it('returns false when object has issueNo even if risk_id also present', () => {
    expect(isBackendFormat({ risk_id: 'R1', issueNo: 'R1' })).toBe(false);
  });

  it('returns false when neither risk_id nor issueNo exists', () => {
    expect(isBackendFormat({ reason: 'test' })).toBe(false);
  });

  it('returns false for an empty object', () => {
    expect(isBackendFormat({})).toBe(false);
  });
});

// ─── ensureAuditIssue ───

describe('ensureAuditIssue', () => {
  it('maps a backend-format object through mapBackendFinding', () => {
    const result = ensureAuditIssue(completeBackendFinding as any);
    expect(result.issueNo).toBe('RISK-001');
    expect(result.severity).toBe('high');
    expect(result.anchorPage).toBe(5);
    expect(result.location!.pageNumber).toBe(5);
  });

  it('returns a frontend-format object as-is without re-mapping', () => {
    const frontend = {
      issueNo: 'F1',
      severity: 'medium' as const,
      category: '格式问题',
      description: '描述',
      suggestion: '建议',
      location: { pageNumber: 1, sectionName: '第一章', context: '...' },
    };
    const result = ensureAuditIssue(frontend as any);
    expect(result).toBe(frontend); // same reference === not re-mapped
    expect(result.issueNo).toBe('F1');
  });

  it('treats a hybrid object (risk_id + issueNo) as frontend format', () => {
    const hybrid = { risk_id: 'R1', issueNo: 'F1', reason: 'test' };
    const result = ensureAuditIssue(hybrid as any);
    // isBackendFormat returns false because issueNo exists, so returned as-is
    expect(result).toBe(hybrid);
  });
});
