import type { FormInstance } from 'antd';
import type { AuthSession, ApiResponse } from '@/features/tenant/types';

export interface LoginParams {
   /** 登录使用手机号，与登录表单(LoginFormValues)保持一致。 */
   phone: string;
   password: string;
}

export type LoginResponse = AuthSession;
export type AuthResponse = ApiResponse<AuthSession>;

export interface RegisterParams {
   username: string;
   password: string;
   realName?: string;
   email?: string;
   phone: string;
}

export interface LoginFormValues {
   phone: string;
   password: string;
   remember?: boolean;
}

export interface LoginFormProps {
   form: FormInstance<LoginFormValues>;
   loading: boolean;
   onFinish: (values: LoginFormValues) => void;
   buttonClass: string;
}

export interface RegisterFormValues {
   username: string;
   password: string;
   realName?: string;
   email?: string;
   phone: string;
}

export interface RegisterFormProps {
   form: FormInstance<RegisterFormValues>;
   loading: boolean;
   onFinish: (values: RegisterFormValues) => void;
   buttonClass: string;
}
