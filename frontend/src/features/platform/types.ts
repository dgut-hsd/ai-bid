/** 系统管理模块：平台管理员（系统管理者）管理所有企业。 */

export interface PlatformTenant {
   tenant_id: string;
   tenant_code?: string;
   name: string;
   /** ACTIVE / DISABLED / DELETED */
   status: string;
   plan_code?: string;
   owner_user_id?: string;
   owner_username?: string;
   owner_real_name?: string;
   member_count?: number;
   version?: number;
   created_at?: string;
   updated_at?: string;
}

export interface PlatformTenantPage {
   page: number;
   size: number;
   total: number;
   items: PlatformTenant[];
}

export interface CreatePlatformTenantParams {
   name: string;
   tenant_code?: string;
   plan_code?: string;
   owner_username: string;
   owner_password: string;
   owner_real_name: string;
}