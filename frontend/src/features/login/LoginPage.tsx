import { useNavigate, useLocation } from 'react-router-dom';
import { useDispatch } from 'react-redux';

import { Form, App } from 'antd';

import { setAuthSession } from '@/store/slices/authSlice';

import { useLoginMutation } from './hooks/useAuth';

import { LoginView } from './components/LoginView';
import type { LoginFormValues } from './components/LoginForm';

export function LoginPage() {
   const navigate = useNavigate();
   const location = useLocation();
   const dispatch = useDispatch();

   const { message } = App.useApp();

   const [loginForm] = Form.useForm<LoginFormValues>();

   const { mutateAsync: loginMutate, isPending: loginLoading } =
      useLoginMutation();

   const onLoginFinish = async (values: LoginFormValues) => {
      try {
         const { username, password, remember } = values;
         const response = await loginMutate({ username, password });

         if (response.code === 200 && response.data) {
            dispatch(setAuthSession({ session: response.data, rememberMe: remember }));

            message.success('登录成功');

            const locationState = location.state;
            const from =
               typeof locationState === 'object' &&
               locationState !== null &&
               'from' in locationState &&
               typeof locationState.from === 'object' &&
               locationState.from !== null &&
               'pathname' in locationState.from &&
               typeof locationState.from.pathname === 'string'
                  ? locationState.from.pathname
                  : '/bidReview';
            // 平台管理员（系统管理者）无租户上下文，登录后直接进入「系统管理」，
            // 而非业务工作台（业务路由会被 TenantGuard 拦下）。
            const isPlatformAdmin = response.data.user_info.is_platform_admin === true;
            navigate(isPlatformAdmin ? '/platform/enterprises' : from, { replace: true });
         } else {
            message.error(response.msg || '登录失败');
            loginForm.setFieldValue('password', '');
         }
      } catch (error: unknown) {
         console.error('Login error: ', error);
         message.error('登录失败，请检查账号和密码');
         loginForm.setFieldValue('password', '');
      }
   };

   return (
      <LoginView
         loginLoading={loginLoading}
         loginForm={loginForm}
         onLoginFinish={onLoginFinish}
      />
   );
}