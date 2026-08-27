import { useLocation, Navigate } from 'react-router-dom';
import { useSelector } from 'react-redux';
import type { RootState } from '../store';

interface RouteGuardProps {
   children: React.ReactNode;
   requireAuth?: boolean;
}

export function RouteGuard({ children, requireAuth = true }: RouteGuardProps) {
   const location = useLocation();
   const { isAuthenticated } = useSelector((state: RootState) => state.auth);

   if (requireAuth && !isAuthenticated) {
      return <Navigate to='/login' state={{ from: location }} replace />;
   }

   if (!requireAuth && isAuthenticated) {
      return <Navigate to='/bidReview' replace />;
   }

   return <>{children}</>;
}
