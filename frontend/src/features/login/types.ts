import type { FormInstance } from 'antd';
import type { AuthSession, ApiResponse } from '@/features/tenant/types';

export interface LoginParams {
   /** 登录账号，对应 sys_user.username。 */
   username: string;
   password: string;
}

export type LoginResponse = AuthSession;
export type AuthResponse = ApiResponse<AuthSession>;

export interface LoginFormValues {
   username: string;
   password: string;
   remember?: boolean;
}

export interface LoginFormProps {
   form: FormInstance<LoginFormValues>;
   loading: boolean;
   onFinish: (values: LoginFormValues) => void;
   buttonClass: string;
}