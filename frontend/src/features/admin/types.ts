/** 系统管理模块：部门用户管理。 */

export type AdminRole = 'OWNER' | 'MEMBER';

export interface AdminUser {
  user_id: string;
  username: string;
  real_name?: string;
  role: AdminRole;
  /** ACTIVE / DISABLED */
  status: string;
  member_id?: string;
  created_at?: string;
}

export interface CreateUserParams {
  username: string;
  password: string;
  real_name: string;
  role: AdminRole;
}