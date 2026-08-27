/**
 * Tenant API contract types.
 *
 * Tenant-domain fields use the backend's snake_case names. IDs are normalized
 * to strings at the API boundary because the Java service serializes BIGINTs.
 */

export interface AuthUserInfo {
  user_id: string;
  username: string;
  real_name: string;
  /** 是否平台管理员（系统管理者），用于门控「系统管理」入口。 */
  is_platform_admin?: boolean;
}

export interface TenantSummary {
  tenant_id: string;
  tenant_code?: string;
  name: string;
  status?: string;
  role?: string;
  permissions?: string[];
  is_current?: boolean;
  created_at?: string;
}

/** Login, refresh, and switch-tenant response data after normalization. */
export interface AuthSession {
  token: string;
  token_type: string;
  expires_in: number;
  session_version: number;
  user_info: AuthUserInfo;
  current_tenant: TenantSummary | null;
  tenants: TenantSummary[];
}

export interface ApiResponse<T> {
  code: number;
  msg: string;
  data: T | null;
  timestamp: number;
}

export interface TenantListResponse {
  current_tenant_id: string | null;
  items: TenantSummary[];
}

export interface CreateTenantParams {
  /** 租户名称（必填） */
  name: string;
  /** 可选描述 */
  description?: string;
}

export interface TenantMember {
  member_id?: string;
  tenant_id?: string;
  user_id: string;
  username: string;
  real_name?: string;
  role: string;
  permissions?: string[];
  status?: string;
  joined_at?: string;
  last_seen_at?: string;
}

export interface MemberListResponse {
  page: number;
  size: number;
  total: number;
  items: TenantMember[];
}

export interface SwitchTenantParams {
  /** 要切换到的目标租户 ID */
  tenant_id: string;
}
