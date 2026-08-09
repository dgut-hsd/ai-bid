import { useState } from 'react';
import { useNavigate, useLocation } from 'react-router-dom';
import { useDispatch } from 'react-redux';

import { Form, App } from 'antd';

import { setCredentials } from '@/store/slices/authSlice';

import { useLoginMutation, useRegisterMutation } from './hooks/useAuth';

import { LoginView } from './components/LoginView';
import type { LoginFormValues } from './components/LoginForm';
import type { RegisterFormValues } from './types';

export function LoginPage() {
   const navigate = useNavigate();
   const location = useLocation();
   const dispatch = useDispatch();

   const { message } = App.useApp();

   const [activeTab, setActiveTab] = useState('login');
   const [loginForm] = Form.useForm<LoginFormValues>();
   const [registerForm] = Form.useForm<RegisterFormValues>();

   const { mutateAsync: loginMutate, isPending: loginLoading } =
      useLoginMutation();
   const { mutateAsync: registerMutate, isPending: registerLoading } =
      useRegisterMutation();

   const onLoginFinish = async (values: LoginFormValues) => {
      try {
         const { phone, password, remember } = values;
         const response = await loginMutate({ phone, password });

         if (response.code === 200 && response.data) {
            // 后端使用 snake_case，需映射为 camelCase
            const loginData = response.data as any;
            dispatch(
               setCredentials({
                  token: loginData.token,
                  userInfo: {
                     id: loginData.user_info?.user_id ?? loginData.user_info?.id,
                     username: loginData.user_info?.username,
                     realName: loginData.user_info?.realName || '',
                     tenantId: null,
                     tenantName: null,
                     isSuperAdmin: false,
                     role: loginData.current_tenant?.role || '',
                  },
                  rememberMe: remember,
               })
            );

            message.success('登录成功');

            const from =
               (location.state as any)?.from?.pathname || '/dashboard';
            navigate(from, { replace: true });
         } else {
            message.error(response.msg || '登录失败');
            loginForm.setFieldValue('password', '');
         }
      } catch (error) {
         console.error('Login error: ', error);
         loginForm.setFieldValue('password', '');
      }
   };

   const onRegisterFinish = async (values: RegisterFormValues) => {
      try {
         const response = await registerMutate(values);
         if (response.code === 200) {
            message.success('注册成功，请登录');
            setActiveTab('login');
            registerForm.resetFields();
         } else {
            message.error(response.msg || '注册失败');
         }
      } catch (error: any) {
         const errMsg =
            error?.response?.data?.msg ||
            error?.message ||
            '注册失败，请稍后重试';
         message.error(errMsg);
      }
   };

   return (
      <LoginView
         activeTab={activeTab}
         setActiveTab={setActiveTab}
         loginLoading={loginLoading}
         registerLoading={registerLoading}
         loginForm={loginForm}
         registerForm={registerForm}
         onLoginFinish={onLoginFinish}
         onRegisterFinish={onRegisterFinish}
      />
   );
}
