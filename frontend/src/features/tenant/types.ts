/**
 * 多租户相关类型定义
 *
 * 字段命名严格对齐飞书《前端多租户交接文档》的接口契约：
 * - 租户接口使用 snake_case（飞书原文）
 * - tenant_id / user_id 等在前端按字符串保存
 */

// ─── AuthSession（登录/刷新/切租户成功后返回） ───────────────────────

export interface AuthSession {
  /** 访问令牌（后续请求 Authorization: Bearer <token>） */
  token: string;
  /** 刷新令牌 */
  refresh_token: string;
  /** 当前租户 ID（字符串） */
  tenant_id: string;
  /** 用户 ID（字符串） */
  user_id: string;
  /** 用户名 */
  username: string;
  /** 真实姓名 */
  real_name?: string;
}

// ─── Tenant（租户） ──────────────────────────────────────────────────

export interface TenantSummary {
  /** 租户 ID（字符串） */
  tenant_id: string;
  /** 租户名称 */
  name: string;
  /** 当前用户在该租户的角色 */
  role?: string;
  /** 创建时间（ISO 字符串或时间戳） */
  created_at?: string;
}

export interface TenantListResponse {
  /** 当前租户 ID */
  current_tenant_id: string;
  /** 租户列表 */
  items: TenantSummary[];
}

export interface CreateTenantParams {
  /** 租户名称（必填） */
  name: string;
  /** 可选描述 */
  description?: string;
}

// ─── Member（租户成员） ──────────────────────────────────────────────

export interface TenantMember {
  /** 用户 ID（字符串） */
  user_id: string;
  /** 用户名 */
  username: string;
  /** 真实姓名 */
  real_name?: string;
  /** 该成员在租户内的角色 */
  role: string;
  /** 加入时间 */
  joined_at?: string;
}

export interface MemberListResponse {
  /** 当前页码 */
  page: number;
  /** 每页大小 */
  size: number;
  /** 总数 */
  total: number;
  /** 成员列表 */
  items: TenantMember[];
}

// ─── Switch Tenant ───────────────────────────────────────────────────

export interface SwitchTenantParams {
  /** 要切换到的目标租户 ID */
  tenant_id: string;
}
