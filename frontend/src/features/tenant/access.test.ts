import { describe, expect, it } from 'vitest';
import { canAccessTenantManage } from './access';
import type { TenantSummary } from './types';

describe('canAccessTenantManage', () => {
   it('允许尚未加入任何租户的用户（用于创建首个租户）', () => {
      expect(canAccessTenantManage([])).toBe(true);
   });

   it('允许在任一租户中担任 OWNER 或 ADMIN 的用户', () => {
      const tenants: TenantSummary[] = [
         { tenant_id: '20001', name: 'A', role: 'MEMBER' },
         { tenant_id: '20002', name: 'B', role: 'ADMIN' },
      ];
      expect(canAccessTenantManage(tenants)).toBe(true);
   });

   it('拒绝仅担任 MEMBER / AUDITOR / VIEWER 的用户', () => {
      const tenants: TenantSummary[] = [
         { tenant_id: '20001', name: 'A', role: 'MEMBER' },
         { tenant_id: '20002', name: 'B', role: 'VIEWER' },
      ];
      expect(canAccessTenantManage(tenants)).toBe(false);
   });

   it('角色匹配对大小写不敏感', () => {
      expect(
         canAccessTenantManage([{ tenant_id: '20001', name: 'A', role: 'owner' }])
      ).toBe(true);
   });
});