# ADR-001: 多租户领域模型与身份契约

- 状态：Accepted
- 日期：2026-08-06
- 适用范围：Java 业务网关、共享 MySQL Schema、异步任务、SSE 和前端交接契约
- 需求来源：`docs/多租户系统实施计划.md` v1.1
- 关联文档：[租户隔离与迁移 ADR](tenant-isolation.md)、[多租户接口契约](../../backend-java/docs/多租户接口契约.md)

## 1. 背景

当前系统以 `sys_user` 作为全局身份，部分 Service 通过 `user_id` 或
`upload_user_id` 做资源过滤。一个用户需要加入多个企业、团队或个人工作空间，
因此用户身份和业务资源归属必须拆开。共享 MySQL、共享 Schema 的部署形态保持不变，
隔离键统一为业务资源上的 `tenant_id`。

本 ADR 冻结 T0 后续实现必须共同遵守的术语、字段、枚举、所有权规则和会话字段。
字段名以 API JSON 的 `snake_case` 形式书写；Java DTO 可以使用 `@JsonProperty`
或等价配置映射到该形式。旧业务接口的既有字段在兼容窗口内不强制重命名。

## 2. 决策

### 2.1 术语

| 术语 | 稳定定义 |
|---|---|
| User | `sys_user` 中的全局用户身份；同一用户可以加入多个租户 |
| Tenant | 企业、团队或个人工作空间；所有业务资源的唯一隔离域 |
| TenantMember | `user_id` 与 `tenant_id` 的关系，保存角色和成员状态 |
| CurrentTenant | 当前请求的租户，由服务端会话和成员关系确定 |
| TenantRole | `OWNER`、`ADMIN`、`AUDITOR`、`MEMBER`、`VIEWER` |
| TenantResource | 所有带 `tenant_id` 的业务资源 |
| TenantContext | 请求或异步消息中显式传播的 `user_id`、`tenant_id`、`role`、`request_id`、`session_version` |

客户端提交的 `tenant_id` 只能作为候选值。服务端必须使用登录会话、成员表和租户
状态重新解析 CurrentTenant，不能按客户端值直接选择数据。

### 2.2 ID、时间和序列化

- Java 领域 ID 使用 MySQL `BIGINT`；API、JWT、消息和 Header 统一传输为十进制
  字符串，避免 JavaScript 精度丢失，Java 实现映射回 `Long`。
- Rust 文档 ID 继续使用全局唯一字符串（当前为 UUID）；Rust 资源同时保存
  `tenant_id`，不能仅凭文档 ID 作为授权依据。
- 时间统一使用 UTC，数据库精度为毫秒 `DATETIME(3)`；API 使用 RFC 3339
  字符串，例如 `2026-08-06T12:30:00.123Z`。
- `settings_json`、`before_json`、`after_json` 使用 MySQL JSON；API 中保持对象
  或 `null`，不把 JSON 再编码成字符串。
- 租户域 API v1 的 JSON 字段使用 `snake_case`。分页字段固定为
  `page`、`size`、`total`、`items`。
- `version` 是乐观锁版本，初始值为 `0`，每次成功更新递增 1。

### 2.3 表和字段契约

下表是逻辑契约。Expand 阶段允许已有资源表的 `tenant_id` 为空；新表按下列
规则建立。实现可以选择 `VARCHAR` 状态列而不是 MySQL `ENUM`，但枚举值不可改变。

#### `tenant`

| 字段 | 类型/空值 | 规则 |
|---|---|---|
| `id` | `BIGINT NOT NULL` | 主键，服务端生成 |
| `tenant_code` | `VARCHAR(64) NOT NULL` | 全局唯一；小写、数字、`_`、`-`，长度 3-64 |
| `name` | `VARCHAR(128) NOT NULL` | 展示名称 |
| `status` | `VARCHAR(16) NOT NULL` | `ACTIVE`、`DISABLED`、`DELETED` |
| `owner_user_id` | `BIGINT NOT NULL` | 必须对应一个 ACTIVE 的 `OWNER` 成员 |
| `plan_code` | `VARCHAR(32) NOT NULL` | 默认 `STANDARD`；仅表示配置档，不代表计费实现 |
| `settings_json` | `JSON NULL` | 租户设置；敏感配置不得放入此字段 |
| `version` | `BIGINT NOT NULL DEFAULT 0` | 乐观锁版本 |
| `created_at` | `DATETIME(3) NOT NULL` | UTC |
| `updated_at` | `DATETIME(3) NOT NULL` | UTC |
| `deleted_at` | `DATETIME(3) NULL` | 软删除时间；`DELETED` 时必须非空 |

索引：主键 `id`、唯一索引 `tenant_code`、索引 `(status, updated_at)`。

#### `tenant_member`

| 字段 | 类型/空值 | 规则 |
|---|---|---|
| `id` | `BIGINT NOT NULL` | 主键 |
| `tenant_id` | `BIGINT NOT NULL` | 外键语义指向 `tenant.id` |
| `user_id` | `BIGINT NOT NULL` | 外键语义指向 `sys_user.id` |
| `role` | `VARCHAR(16) NOT NULL` | 五个 `TenantRole` 值之一 |
| `status` | `VARCHAR(16) NOT NULL` | `ACTIVE`、`SUSPENDED`、`REMOVED` |
| `joined_at` | `DATETIME(3) NOT NULL` | 首次成为成员的时间，回填时使用资源归属可确定的迁移时间 |
| `invited_by` | `BIGINT NULL` | 发起邀请的用户；自动创建 OWNER 时为空 |
| `last_seen_at` | `DATETIME(3) NULL` | 最近一次在该租户成功请求的时间 |

索引：唯一索引 `(tenant_id, user_id)`、索引 `(user_id, status)`、索引
`(tenant_id, status, role)`。被移除的关系保留，禁止通过删除重建绕过审计。

#### `tenant_invitation`

| 字段 | 类型/空值 | 规则 |
|---|---|---|
| `id` | `BIGINT NOT NULL` | 主键 |
| `tenant_id` | `BIGINT NOT NULL` | 邀请所属租户 |
| `email` | `VARCHAR(320) NOT NULL` | 存储前 trim 并转小写 |
| `role` | `VARCHAR(16) NOT NULL` | 不允许邀请 `OWNER` |
| `token_hash` | `CHAR(64) NOT NULL` | 原始 token 的 SHA-256 小写十六进制；原始 token 不落库 |
| `invited_by` | `BIGINT NOT NULL` | 发起邀请的 ACTIVE 成员 |
| `expires_at` | `DATETIME(3) NOT NULL` | 默认创建后 7 天 |
| `accepted_at` | `DATETIME(3) NULL` | 接受时间 |
| `revoked_at` | `DATETIME(3) NULL` | 撤销时间 |
| `status` | `VARCHAR(16) NOT NULL` | `PENDING`、`ACCEPTED`、`REVOKED`、`EXPIRED` |

索引：唯一索引 `token_hash`、索引 `(tenant_id, status)`、索引
`(email, status)`。API 只返回原始 token 一次；日志、审计和异常信息不得打印它。

#### `tenant_audit_log`

| 字段 | 类型/空值 | 规则 |
|---|---|---|
| `id` | `BIGINT NOT NULL` | 主键，作为 SSE 事件回放的单调 ID 时不复用 |
| `tenant_id` | `BIGINT NOT NULL` | 审计所属租户 |
| `actor_user_id` | `BIGINT NULL` | 系统任务可为空；用户操作必须非空 |
| `action` | `VARCHAR(64) NOT NULL` | 稳定动作名，例如 `tenant.member.role_update` |
| `resource_type` | `VARCHAR(64) NOT NULL` | 资源类型 |
| `resource_id` | `VARCHAR(128) NULL` | 兼容 BIGINT 和 UUID |
| `request_id` | `VARCHAR(64) NOT NULL` | 关联一次请求或异步任务 |
| `before_json` | `JSON NULL` | 脱敏后的变更前快照 |
| `after_json` | `JSON NULL` | 脱敏后的变更后快照 |
| `ip_address` | `VARCHAR(45) NULL` | IPv4 或 IPv6 |
| `user_agent` | `VARCHAR(512) NULL` | 可截断，不得影响写入 |
| `created_at` | `DATETIME(3) NOT NULL` | UTC，只允许追加 |

索引：主键 `id`、索引 `(tenant_id, created_at)`、索引
`(actor_user_id, created_at)`。普通租户成员不能 UPDATE 或 DELETE 审计日志。

### 2.4 资源表的 `tenant_id`

第一批必须覆盖下列表或当前版本对应的实体表：

`project`、`bid_document`、`audit_task`、`audit_issue`、`audit_report`、
`audit_task_event`、`knowledge_file`、`knowledge_chunk`、`chat_message`、
`document_parse_job`、`rag_trigger_outbox`，以及 Trace 会话、事件、区块表。

最终状态为 `tenant_id BIGINT NOT NULL`，并按实际访问模式建立至少一个以
`tenant_id` 开头的组合索引，例如 `(tenant_id, status)`、`(tenant_id, create_time)`
或 `(tenant_id, project_id)`。

`user_id`、`upload_user_id`、`audit_user_id` 只表示操作者或历史责任人，不再承担
业务资源的隔离职责。创建资源时 `tenant_id` 只能来自服务端 TenantContext。

### 2.5 状态和生命周期

租户状态：

```text
ACTIVE -> DISABLED -> ACTIVE
ACTIVE -> DELETED
DISABLED -> DELETED
```

- `DISABLED` 拒绝新写入、登录后的租户切换和新 SSE 订阅，但保留只读管理审计
  所需的查询能力。
- `DELETED` 是软删除终态；业务资源不因 API 删除立即物理删除。
- 删除租户前必须完成 OWNER 转移，且不存在未完成的强制清理任务。

成员状态：

```text
ACTIVE <-> SUSPENDED
ACTIVE -> REMOVED
SUSPENDED -> REMOVED
```

邀请状态：

```text
PENDING -> ACCEPTED
PENDING -> REVOKED
PENDING -> EXPIRED
```

邀请只允许单次成功接受。接受操作必须在事务中完成 token 状态更新和成员
关系创建；重复请求返回稳定错误码而不是再次创建成员。

### 2.6 所有权和角色不变量

1. 每个非 `DELETED` 租户恰好有一个 `owner_user_id`，并且存在一个
   `(tenant_id, owner_user_id, role=OWNER, status=ACTIVE)` 的成员关系。
2. `OWNER` 不允许被普通 `ADMIN` 删除、降级或暂停。
3. `OWNER` 不能被自己降级；转移所有权必须先确认目标用户是当前租户的
   ACTIVE 成员，再以一个事务完成旧 OWNER 降级、新 OWNER 提升和
   `tenant.owner_user_id` 更新。
4. 不允许删除或降级最后一个 OWNER。删除租户前必须完成所有权转移。
5. 邀请接口不接受 `OWNER`；邀请人只能授予 `ADMIN`、`AUDITOR`、`MEMBER`
   或 `VIEWER`。
6. `tenant_audit_log` 必须记录成员邀请、接受、撤销、移除、角色变更、所有权
   转移、租户设置变更、停用、恢复和删除。

### 2.7 权限矩阵

权限名是 API `permissions` 数组的稳定值：

| 权限 | OWNER | ADMIN | AUDITOR | MEMBER | VIEWER |
|---|---:|---:|---:|---:|---:|
| `tenant.read` | ✓ | ✓ | ✓ | ✓ | ✓ |
| `tenant.settings.write` | ✓ | ✓ | - | - | - |
| `tenant.members.invite` | ✓ | ✓ | - | - | - |
| `tenant.members.remove` | ✓ | ✓ | - | - | - |
| `tenant.members.role.write` | ✓ | ✓ | - | - | - |
| `tenant.owner.transfer` | ✓ | - | - | - | - |
| `tender.write` | ✓ | ✓ | ✓ | ✓ | - |
| `audit.start` | ✓ | ✓ | ✓ | ✓ | - |
| `audit.report.read` | ✓ | ✓ | ✓ | ✓ | ✓ |
| `knowledge.write` | ✓ | ✓ | ✓ | ✓ | - |
| `tenant.delete` | ✓ | - | - | - | - |

服务端按角色重新计算权限。客户端提交的 `role` 或 `permissions` 永远不是
授权输入；JWT 中的值只作为已签发会话的快照，成员状态发生变化时必须重新校验。

### 2.8 会话和请求上下文

请求上下文逻辑字段固定为：

```json
{
  "user_id": "10001",
  "tenant_id": "20001",
  "role": "ADMIN",
  "permissions": [
    "tenant.read",
    "tenant.settings.write",
    "tender.write",
    "audit.start",
    "audit.report.read",
    "knowledge.write"
  ],
  "request_id": "8b9c0e7f-9d8b-4f86-b77d-9a3d4c2f5001",
  "session_version": 3
}
```

JWT 至少包含以下 claims：

```json
{
  "sub": "10001",
  "tenant_id": "20001",
  "role": "ADMIN",
  "permissions": ["tenant.read", "tender.write"],
  "session_version": 3,
  "iat": 1786019400,
  "exp": 1786105800
}
```

JWT Secret 从配置或密钥服务读取。每次请求都必须验证 JWT、Redis Session、
用户状态、成员状态和租户状态。切换租户、移除成员、禁用租户、改密或强制退出
都必须使旧会话失效。异步任务必须显式复制上述上下文，不能依赖裸 `ThreadLocal`。

### 2.9 兼容和非目标

- 本 ADR 不定义计费、跨租户报表、平台管理员 RBAC 或前端页面。
- 旧接口可以在迁移窗口内保留 `user_id` 过滤，但新增租户接口不得依赖它。
- 新租户接口的完整 HTTP、错误、SSE 和内部 Header 契约见
  [多租户接口契约](../../backend-java/docs/多租户接口契约.md) 和
  [OpenAPI JSON](../../backend-java/docs/tenant-openapi.json)。

## 3. 后果

正面影响：

- 用户身份、租户成员关系和业务资源归属分离，允许一人多租户。
- 角色和权限可以在 JWT、API 响应、异步消息和审计日志之间统一传递。
- 资源表最终只需要检查一个 `tenant_id` 隔离键。

代价和约束：

- T1 必须先完成幂等回填，T2 才能安全建立 CurrentTenant。
- 所有 Service、Mapper、队列、SSE、文件和 Rust 调用都必须消费
  TenantContext；只修改 Controller 不算完成。
- 角色或字段变更必须先更新本 ADR 与接口契约，再由实现 Agent 修改代码。
