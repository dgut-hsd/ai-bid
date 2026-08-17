import type { BaseResponse } from '@/api/types';
import type {
  ApiResponse,
  AuthSession,
  AuthUserInfo,
  MemberListResponse,
  TenantListResponse,
  TenantMember,
  TenantSummary,
} from '@/features/tenant/types';

type JsonObject = Record<string, unknown>;

function isJsonObject(value: unknown): value is JsonObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function requireObject(value: unknown, label: string): JsonObject {
  if (!isJsonObject(value)) {
    throw new Error(`Invalid ${label}`);
  }
  return value;
}

function requireString(value: unknown, label: string): string {
  if (typeof value !== 'string' || value.trim() === '') {
    throw new Error(`Invalid ${label}`);
  }
  return value;
}

function optionalString(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() !== '' ? value : undefined;
}

function requireId(value: unknown, label: string): string {
  if (typeof value === 'string' && value.trim() !== '') {
    return value;
  }
  if (typeof value === 'number' && Number.isSafeInteger(value)) {
    return String(value);
  }
  throw new Error(`Invalid ${label}`);
}

function requireNumber(value: unknown, label: string): number {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value;
  }
  if (typeof value === 'string' && value.trim() !== '') {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return parsed;
  }
  throw new Error(`Invalid ${label}`);
}

function normalizePermissions(value: unknown): string[] {
  if (value === undefined || value === null) return [];
  if (!Array.isArray(value) || !value.every((item) => typeof item === 'string')) {
    throw new Error('Invalid tenant permissions');
  }
  return value;
}

export function normalizeTenantSummary(value: unknown): TenantSummary {
  const raw = requireObject(value, 'tenant summary');
  const current =
    typeof raw.is_current === 'boolean'
      ? raw.is_current
      : typeof raw.current === 'boolean'
        ? raw.current
        : undefined;

  return {
    tenant_id: requireId(raw.tenant_id, 'tenant_id'),
    tenant_code: optionalString(raw.tenant_code),
    name: requireString(raw.name, 'tenant name'),
    status: optionalString(raw.status),
    role: optionalString(raw.role),
    permissions: normalizePermissions(raw.permissions),
    is_current: current,
    created_at: optionalString(raw.created_at),
  };
}

function normalizeUserInfo(value: unknown): AuthUserInfo {
  const raw = requireObject(value, 'user_info');
  // UserInfoVO currently annotates only its id field as user_id; realName is
  // retained as an explicit compatibility spelling for that Java response.
  const realName =
    optionalString(raw.real_name) ??
    optionalString(raw.realName) ??
    requireString(raw.username, 'user_info.username');

  return {
    user_id: requireId(raw.user_id, 'user_info.user_id'),
    username: requireString(raw.username, 'user_info.username'),
    real_name: realName,
  };
}

export function normalizeAuthSession(value: unknown): AuthSession {
  const raw = requireObject(value, 'auth session');
  const tenantsValue = raw.tenants;
  if (!Array.isArray(tenantsValue)) {
    throw new Error('Invalid tenants');
  }

  const currentTenant =
    raw.current_tenant === null || raw.current_tenant === undefined
      ? null
      : normalizeTenantSummary(raw.current_tenant);

  return {
    token: requireString(raw.token, 'token'),
    token_type: optionalString(raw.token_type) ?? 'Bearer',
    expires_in: requireNumber(raw.expires_in, 'expires_in'),
    session_version: requireNumber(raw.session_version, 'session_version'),
    user_info: normalizeUserInfo(raw.user_info),
    current_tenant: currentTenant,
    tenants: tenantsValue.map(normalizeTenantSummary),
  };
}

export function normalizeAuthResponse(
  response: BaseResponse<unknown>
): ApiResponse<AuthSession> {
  return normalizeResponse(response, normalizeAuthSession);
}

export function normalizeResponse<T>(
  response: BaseResponse<unknown>,
  normalize: (value: unknown) => T
): ApiResponse<T> {
  if (response.code !== 200 || response.data === null || response.data === undefined) {
    return {
      code: response.code,
      msg: response.msg,
      data: null,
      timestamp: response.timestamp,
    };
  }

  return {
    code: response.code,
    msg: response.msg,
    data: normalize(response.data),
    timestamp: response.timestamp,
  };
}

export function normalizeTenantListResponse(value: unknown): TenantListResponse {
  const raw = requireObject(value, 'tenant list');
  const items = raw.items;
  if (!Array.isArray(items)) throw new Error('Invalid tenant list items');

  return {
    current_tenant_id:
      raw.current_tenant_id === null || raw.current_tenant_id === undefined
        ? null
        : requireId(raw.current_tenant_id, 'current_tenant_id'),
    items: items.map(normalizeTenantSummary),
  };
}

export function normalizeTenantMember(value: unknown): TenantMember {
  const raw = requireObject(value, 'tenant member');
  return {
    member_id:
      raw.member_id === undefined ? undefined : requireId(raw.member_id, 'member_id'),
    tenant_id:
      raw.tenant_id === undefined ? undefined : requireId(raw.tenant_id, 'tenant_id'),
    user_id: requireId(raw.user_id, 'user_id'),
    username: requireString(raw.username, 'username'),
    real_name: optionalString(raw.real_name) ?? optionalString(raw.realName),
    role: requireString(raw.role, 'role'),
    permissions: normalizePermissions(raw.permissions),
    status: optionalString(raw.status),
    joined_at: optionalString(raw.joined_at),
    last_seen_at: optionalString(raw.last_seen_at),
  };
}

export function normalizeMemberListResponse(value: unknown): MemberListResponse {
  const raw = requireObject(value, 'member list');
  if (!Array.isArray(raw.items)) throw new Error('Invalid member list items');
  return {
    page: requireNumber(raw.page, 'member list page'),
    size: requireNumber(raw.size, 'member list size'),
    total: requireNumber(raw.total, 'member list total'),
    items: raw.items.map(normalizeTenantMember),
  };
}

export function normalizeTenantResponse(
  response: BaseResponse<unknown>
): ApiResponse<TenantSummary> {
  return normalizeResponse(response, normalizeTenantSummary);
}
