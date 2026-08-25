import { Navigate, Outlet } from 'react-router-dom';
import { useSelector } from 'react-redux';
import type { RootState } from '../store';
import { canAccessTenantManage } from '../features/tenant/access';

/**
 * 租户管理路由守卫：无租户（待创建）或拥有者/管理员可进入，
 * 普通成员、审核、只读用户被重定向到工作台。
 */
export function TenantManageGuard() {
   const tenantList = useSelector((state: RootState) => state.auth.tenantList);

   if (!canAccessTenantManage(tenantList)) {
      return <Navigate to='/dashboard' replace />;
   }

   return <Outlet />;
}