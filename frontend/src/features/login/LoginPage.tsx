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
            navigate(from, { replace: true });
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