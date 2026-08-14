import type { FormInstance } from 'antd';

export interface LoginParams {
   phone: string;
   password: string;
}

export interface LoginResponse {
   token: string;
   /** 后端返回 user_info（snake_case），见 docs/前端多租户交接文档.md:74 */
   user_info: {
      user_id: number | string;
      username: string;
      realName: string;
   };
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
