import type { FormInstance } from 'antd';

export interface LoginParams {
   phone: string;
   password: string;
}

export interface UserInfo {
   id: number;
   username: string;
   realName: string;
   tenantId: number | null;
   tenantName: string | null;
   isSuperAdmin: boolean;
   role: string;
}

export interface LoginResponse {
   token: string;
   userInfo: UserInfo;
}

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
