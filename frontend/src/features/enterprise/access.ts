import type { TenantSummary } from '@/features/tenant/types';

/**
 * 判断「当前租户」是否为企业 OWNER。
 * 企业管理模块仅对企业 OWNER 开放（MEMBER / ADMIN 无权限）。
 */
export function isCurrentTenantOwner(
   tenantList: TenantSummary[],
   currentTenantId: string | null
): boolean {
   if (!currentTenantId) return false;
   const current = tenantList.find((t) => t.tenant_id === currentTenantId);
   return (current?.role ?? '').toUpperCase() === 'OWNER';
}