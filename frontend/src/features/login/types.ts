import type { FormInstance } from 'antd';
import type { AuthSession, ApiResponse } from '@/features/tenant/types';

export interface LoginParams {
   /** The backend accepts this contract field and maps it to its legacy phone field. */
   username: string;
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
