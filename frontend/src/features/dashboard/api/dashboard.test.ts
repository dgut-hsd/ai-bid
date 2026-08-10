import { describe, expect, it } from 'vitest';
import { dashboardOptions } from './dashboard';

describe('dashboard query options', () => {
  it('disables all tenant-scoped dashboard queries without a current tenant', () => {
    expect(dashboardOptions.list(null).enabled).toBe(false);
    expect(dashboardOptions.issueDistribution(null).enabled).toBe(false);
    expect(dashboardOptions.auditCount(null).enabled).toBe(false);
  });

  it('enables all tenant-scoped dashboard queries for the current tenant', () => {
    expect(dashboardOptions.list('20001').enabled).toBe(true);
    expect(dashboardOptions.issueDistribution('20001').enabled).toBe(true);
    expect(dashboardOptions.auditCount('20001').enabled).toBe(true);
  });

  it('scopes dashboard cache keys by tenant', () => {
    expect(dashboardOptions.list('20001').queryKey).toEqual([
      'dashboardList',
      '20001',
    ]);
    expect(dashboardOptions.issueDistribution('20002').queryKey).toEqual([
      'issueDistribution',
      '20002',
    ]);
    expect(dashboardOptions.auditCount('20002').queryKey).toEqual([
      'auditCount',
      '20002',
    ]);
  });
});
