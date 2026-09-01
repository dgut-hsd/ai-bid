/**
 * Unit tests for src/types/audit.ts — pure functions and data maps.
 *
 * These tests cover label lookups, severity/tier maps, and phase-label
 * completeness. No DOM or API mocking is needed since everything tested
 * here is a pure function or a static Record.
 */
import { describe, expect, it } from 'vitest';
import {
  agentLabel,
  SEVERITY_MAP,
  SEVERITY_COLORS,
  TIER_MAP,
  TIER_COLORS,
  PHASE_LABELS,
  PHASE_ORDER,
} from './audit';

// ─── agentLabel() ──────────────────────────────────────────────

describe('agentLabel()', () => {
  it('returns the correct Chinese label for every known agent ID', () => {
    const expectations: [string, string][] = [
      ['factcheck', '事实核验'],
      ['factcheckagent', '事实核验'],
      ['procedure', '流程合规'],
      ['procedureagent', '流程合规'],
      ['ruleengine', '法规匹配'],
      ['ruleengineagent', '法规匹配'],
      ['semanticrisk', '风险识别'],
      ['semanticriskagent', '风险识别'],
      ['fiscal_compliance', '财政合规'],
      ['technical_standard', '技术规范'],
      ['bid_evaluation', '评标合规'],
      ['legal_compliance', '法律合规'],
      ['blind_spot', '隐性风险'],
      ['blindspotagent', '隐性风险'],
      ['legal_verify', '法条核验'],
      ['debate', '争议裁决'],
      ['demandagent', '需求合理性'],
      ['demand', '需求合理性'],
    ];

    for (const [id, expected] of expectations) {
      expect(agentLabel(id)).toBe(expected);
    }
  });

  it('is case-insensitive to agent IDs', () => {
    expect(agentLabel('FACTCHECK')).toBe('事实核验');
    expect(agentLabel('FactCheck')).toBe('事实核验');
    expect(agentLabel('PROCEDUREAGENT')).toBe('流程合规');
    expect(agentLabel('Blind_Spot')).toBe('隐性风险');
    expect(agentLabel('DEMAND')).toBe('需求合理性');
    expect(agentLabel('LEGAL_VERIFY')).toBe('法条核验');
  });

  it('returns the original ID string for unknown agent IDs (fallback)', () => {
    expect(agentLabel('nonexistent_agent')).toBe('nonexistent_agent');
    expect(agentLabel('')).toBe('');
    expect(agentLabel('custom_agent_x42')).toBe('custom_agent_x42');
  });

  it('does not error on whitespace-only or oddly formatted inputs', () => {
    // The function simply lower-cases and looks up — unknown strings
    // fall back to the original input.
    expect(typeof agentLabel('  ')).toBe('string');
    expect(typeof agentLabel('FACT_CHECK')).toBe('string');
    expect(typeof agentLabel('fact-check')).toBe('string');
  });
});

// ─── SEVERITY_MAP ──────────────────────────────────────────────

describe('SEVERITY_MAP', () => {
  it('contains exactly the four Severity keys with correct Chinese labels', () => {
    expect(SEVERITY_MAP).toEqual({
      info: '信息',
      low: '低风险',
      medium: '中风险',
      high: '高风险',
    });
  });

  it('has every key present in the Severity union type', () => {
    // Severity = 'info' | 'low' | 'medium' | 'high'
    expect(Object.keys(SEVERITY_MAP).sort()).toEqual(
      ['high', 'info', 'low', 'medium'], // alphabetic sort order
    );
  });
});

// ─── SEVERITY_COLORS ───────────────────────────────────────────

describe('SEVERITY_COLORS', () => {
  it('covers all four severity levels with valid hex colors', () => {
    const hexColor = /^#[0-9a-f]{6}$/;
    for (const severity of ['info', 'low', 'medium', 'high'] as const) {
      expect(SEVERITY_COLORS[severity]).toMatch(hexColor);
    }
  });
});

// ─── TIER_MAP + TIER_COLORS ────────────────────────────────────

describe('TIER_MAP', () => {
  it('maps all three RiskTier values to the correct Chinese labels', () => {
    expect(TIER_MAP).toEqual({
      L1: '低风险条款',
      L2: '中等风险',
      L3: '高风险条款',
    });
  });
});

describe('TIER_COLORS', () => {
  it('covers all three risk tiers and returns valid hex colors', () => {
    const hexColor = /^#[0-9a-f]{6}$/;
    for (const tier of ['L1', 'L2', 'L3'] as const) {
      expect(TIER_COLORS[tier]).toMatch(hexColor);
    }
  });
});

// ─── PHASE_LABELS ──────────────────────────────────────────────

describe('PHASE_LABELS', () => {
  it('has every key in PHASE_ORDER mapped to a non-empty label', () => {
    for (const phase of PHASE_ORDER) {
      const label = PHASE_LABELS[phase];
      expect(label).toBeDefined();
      expect(label.length).toBeGreaterThan(0);
    }
  });

  it('contains no extra keys beyond those in PHASE_ORDER', () => {
    expect(Object.keys(PHASE_LABELS).sort()).toEqual([...PHASE_ORDER].sort());
  });
});
