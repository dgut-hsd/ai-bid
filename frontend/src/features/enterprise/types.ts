/** 企业管理模块：企业 OWNER 管理本企业用户。 */

/** 企业可自主分配的角色（OWNER 只能由平台分配/转移）。 */
export type EnterpriseRole = 'ADMIN' | 'MEMBER';

export interface EnterpriseUser {
   user_id: string;
   username: string;
   real_name?: string;
   /** 租户角色：OWNER / ADMIN / MEMBER */
   role: string;
   /** 成员状态：ACTIVE / SUSPENDED */
   status: string;
   member_id?: string;
   created_at?: string;
}

export interface CreateUserParams {
   username: string;
   password: string;
   real_name: string;
   role: EnterpriseRole;
}

export interface UpdateUserParams {
   username?: string;
   real_name?: string;
}

export interface UpdateMemberParams {
   role?: EnterpriseRole;
   status?: 'ACTIVE' | 'SUSPENDED';
}