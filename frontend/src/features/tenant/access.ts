import type { TenantSummary } from './types';

/** 具备租户管理入口权限的角色（后端使用大写角色名）。 */
const MANAGEMENT_ROLES = new Set(['OWNER', 'ADMIN']);

/**
 * 判断用户能否进入「租户管理」页。
 *
 * 规则：
 * - 尚未加入任何租户 → 允许（用于创建首个租户的引导）。
 * - 至少在一个租户中担任 OWNER / ADMIN → 允许。
 * - 其它（仅 MEMBER / AUDITOR / VIEWER）→ 不允许。
 */
export function canAccessTenantManage(tenantList: TenantSummary[]): boolean {
   if (!tenantList || tenantList.length === 0) {
      return true;
   }
   return tenantList.some((tenant) =>
      MANAGEMENT_ROLES.has((tenant.role ?? '').toUpperCase())
   );
}