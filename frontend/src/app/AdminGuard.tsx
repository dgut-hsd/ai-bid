import { Navigate, Outlet } from 'react-router-dom';
import { useSelector } from 'react-redux';
import type { RootState } from '../store';
import { isCurrentTenantOwner } from '../features/admin/access';

/**
 * 系统管理路由守卫：仅企业 OWNER 可进入，MEMBER 被重定向到工作台。
 */
export function AdminGuard() {
   const { tenantList, currentTenantId } = useSelector(
      (state: RootState) => state.auth
   );

   if (!isCurrentTenantOwner(tenantList, currentTenantId)) {
      return <Navigate to='/bidReview' replace />;
   }

   return <Outlet />;
}