import { Navigate, Outlet } from 'react-router-dom';
import { useSelector } from 'react-redux';
import type { RootState } from '../store';
import { isCurrentTenantOwner } from '../features/enterprise/access';

/**
 * 企业管理路由守卫：仅当前企业 OWNER 可进入，其余角色被重定向到工作台。
 */
export function EnterpriseOwnerGuard() {
   const { tenantList, currentTenantId } = useSelector(
      (state: RootState) => state.auth
   );

   if (!isCurrentTenantOwner(tenantList, currentTenantId)) {
      return <Navigate to='/bidReview' replace />;
   }

   return <Outlet />;
}