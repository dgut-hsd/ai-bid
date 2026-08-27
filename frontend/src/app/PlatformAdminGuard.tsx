import { Navigate, Outlet } from 'react-router-dom';
import { useSelector } from 'react-redux';
import type { RootState } from '../store';

/**
 * 系统管理路由守卫：仅平台管理员（is_platform_admin）可进入。
 * 平台管理员可无当前租户，因此本守卫不依赖租户上下文。
 */
export function PlatformAdminGuard() {
   const isPlatformAdmin = useSelector(
      (state: RootState) => state.auth.isPlatformAdmin
   );

   if (!isPlatformAdmin) {
      return <Navigate to='/bidReview' replace />;
   }

   return <Outlet />;
}