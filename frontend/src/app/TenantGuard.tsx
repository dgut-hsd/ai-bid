import { Navigate, Outlet, useLocation } from 'react-router-dom';
import { Spin } from 'antd';
import { useSelector } from 'react-redux';
import { useTenant } from '../features/tenant/hooks/useTenant';
import type { RootState } from '../store';

/**
 * 确保用户已有租户上下文后再渲染子路由。
 * 无租户时：平台管理员跳转「系统管理」，其他用户跳转租户管理页。
 */
export function TenantGuard() {
   const location = useLocation();
   const { tenantList, isLoading } = useTenant();
   const isPlatformAdmin = useSelector(
      (state: RootState) => state.auth.isPlatformAdmin
   );

   // 正在加载租户列表
   if (isLoading) {
      return (
         <div style={{
            display: 'flex',
            justifyContent: 'center',
            alignItems: 'center',
            height: '100%',
            minHeight: 200,
         }}>
            <Spin tip='加载租户信息…' />
         </div>
      );
   }

   // 租户列表为空 → 引导创建租户
   if (tenantList.length === 0) {
      return (
         <Navigate
            to={isPlatformAdmin ? '/platform/enterprises' : '/tenant-manage'}
            state={{ from: location, reason: 'no-tenant' }}
            replace
         />
      );
   }

   return <Outlet />;
}
