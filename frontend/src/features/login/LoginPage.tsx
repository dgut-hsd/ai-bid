import { useState } from 'react';
import { useNavigate, useLocation } from 'react-router-dom';
import { useDispatch } from 'react-redux';

import { Form, App } from 'antd';

import { setAuthSession } from '@/store/slices/authSlice';

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
         const response = await loginMutate({ username: phone, password });

         if (response.code === 200 && response.data) {
            dispatch(
               setAuthSession({
                  session: response.data,
                  rememberMe: remember,
               })
            );

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
                  : '/dashboard';
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
      } catch (error: unknown) {
         const errorRecord =
            typeof error === 'object' && error !== null
               ? error as { response?: { data?: { msg?: string } }; message?: string }
               : undefined;
         const errMsg =
            errorRecord?.response?.data?.msg ||
            errorRecord?.message ||
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
