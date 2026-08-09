# ADR-002: 多租户隔离边界、内部签名与迁移回滚

- 状态：Accepted
- 日期：2026-08-06
- 适用范围：Java API、MySQL 行级隔离、文件、Redis、队列、SSE、Trace、Rust 内部服务
- 需求来源：`docs/多租户系统实施计划.md` v1.1
- 关联文档：[租户领域模型 ADR](tenant-model.md)、[多租户接口契约](../../backend-java/docs/多租户接口契约.md)

## 1. 背景和目标

系统采用共享 MySQL、共享 Schema、行级 `tenant_id` 隔离。租户边界不能只存在
于 Controller，因为资源会经由 Mapper、下载/预览、异步队列、SSE 回放、Trace、
Redis 和 Rust 引擎继续流动。本 ADR 冻结端到端的权威链路和可回滚迁移顺序。

安全目标是：任意来自租户 A 的请求、任务、缓存键、文件路径或 Rust 调用都不能
读取、写入或订阅租户 B 的资源；跨租户访问在外部 API 上不泄露资源是否存在。

## 2. 隔离决策

### 2.1 权威链路

```text
Bearer JWT
  -> Redis Session
  -> sys_user 状态
  -> tenant_member(user_id, tenant_id, status)
  -> tenant(status)
  -> TenantRequestContext
  -> Mapper/Service/文件/Redis/队列/SSE
  -> Java 签名 Header
  -> Rust 校验 Header、签名、时间窗、重放键和资源 tenant_id
```

客户端可以请求切换租户，但不能通过请求体、查询参数或自定义 Header 直接决定
资源归属。`tenant_id` 路径参数只用于定位候选资源，必须与服务端 CurrentTenant
一致。Rust 不接受浏览器直接发来的租户 Header。

### 2.2 请求生命周期

1. 认证拦截器解析 Bearer token，验证签名和有效期。
2. 从 Redis Session 读取用户、当前租户和 `session_version`；不存在或版本不符
   时返回 `401 AUTH_INVALID` 或 `401 TENANT_SESSION_STALE`。
3. 查询 `sys_user`、`tenant_member` 和 `tenant`，只允许 ACTIVE 用户、ACTIVE
   成员和 ACTIVE 租户建立普通业务上下文。
4. 创建不可变 `TenantRequestContext`，生成或继承 `request_id`。
5. Service 和 Mapper 使用上下文写入或过滤 `tenant_id`；请求体中的同名字段被忽略。
6. 写操作追加 `tenant_audit_log`。审计失败是否阻断业务由动作等级决定，但
   成员/所有权/租户状态变更必须阻断。
7. 异步消息、SSE 回放和 Rust 调用显式携带上下文，不读取裸 ThreadLocal。

### 2.3 数据库访问

优先使用 MyBatis-Plus 租户拦截器或统一 Mapper 基类，确保所有
`SELECT`、`UPDATE`、`DELETE` 自动带当前 `tenant_id`。插入时由服务端填充
`tenant_id`，不能从 DTO 复制。

特殊 SQL 必须同时满足：

- 使用明确的 `@TenantBypass` 标记或等价元数据；
- 只允许系统任务或平台管理员调用；
- 写入 `tenant_audit_log`；
- 有专门的跨租户负向测试；
- 显式使用租户集合或平台级数据源，不能静默绕过过滤器。

跨租户资源、父子资源租户不一致和不存在的资源统一对外返回
`404 RESOURCE_NOT_FOUND`。只有已经确认资源属于当前租户而角色不足时才返回
`403 TENANT_ROLE_FORBIDDEN`。

### 2.4 文件和缓存命名空间

物理路径固定使用服务端生成的租户前缀：

```text
storage/tenants/{tenantId}/tenders/{yyyy-MM-dd}/{uuid}.pdf
storage/tenants/{tenantId}/knowledge/{yyyy-MM-dd}/{uuid}.pdf
preview-cache/{tenantId}/{documentId}/...
output/tenants/{tenantId}/documents/{documentId}/...
```

用户可控文件名只作为元数据，不得直接拼接到路径。路径必须经过 `normalize`
并验证仍位于配置的根目录内。下载、预览、导出和删除都要先查当前租户资源。

租户业务 Redis key 固定带前缀：

```text
tenant:{tenantId}:session:{sessionId}
tenant:{tenantId}:audit-task:{taskId}
tenant:{tenantId}:sse:{taskId}
tenant:{tenantId}:chat:{projectId}
tenant:{tenantId}:quota:{metric}
```

全局 key 只能保存平台级配置、密钥版本或不可归属的基础设施状态。任务、聊天、
SSE、配额、幂等和重试 key 都必须包含 `tenantId`。

### 2.5 队列和 Outbox

每条任务消息至少包含以下 JSON 字段：

```json
{
  "schema_version": 1,
  "tenant_id": "20001",
  "task_id": "task-01J9J2J8F0M7Y8B6J5G4T3R2Q1",
  "actor_user_id": "10001",
  "request_id": "8b9c0e7f-9d8b-4f86-b77d-9a3d4c2f5001"
}
```

Worker 消费前重新校验租户、资源和成员状态；重试、死信、幂等键、指标标签
都保留 `tenant_id`。消息缺少 `tenant_id` 或与资源不一致时进入死信并返回
`TENANT_CONTEXT_INVALID`，不得降级为旧的用户过滤。

### 2.6 SSE 和事件回放

Java 对外审核流使用现有路径：

```text
GET /api/audit-tasks/{taskId}/stream
Accept: text/event-stream
Authorization: Bearer <token>
Last-Event-ID: <last persisted event id, optional>
```

订阅前检查任务的 `tenant_id` 和当前上下文。SSE 连接键为
`(tenant_id, task_id, user_id)`；`audit_task_event.persist` 和 `replay` 都必须
按租户过滤。事件 envelope、事件名和重连规则见
[多租户接口契约](../../backend-java/docs/多租户接口契约.md)。

租户切换、成员移除、租户停用或 session version 变化时：

1. Java 关闭旧租户的连接；
2. 旧连接最多发送一个 `error` 事件，`error_code` 为
   `TENANT_SESSION_STALE`，随后结束；
3. 客户端必须丢弃旧租户的 `Last-Event-ID` 和缓存，使用新 token 建立新连接；
4. 旧 token 重新连接返回 `401 TENANT_SESSION_STALE`，不能用新租户 ID 覆盖旧上下文。

### 2.7 Java 到 Rust 的内部 Header 和签名

Rust 只接受来自受信任 Java 网关的内部请求。每个 HTTP 请求，包括 SSE 建连，
都必须携带下列 Header：

| Header | 格式 | 必填 | 说明 |
|---|---|---:|---|
| `X-Tenant-Id` | 十进制字符串 | 是 | Java 已验证的当前租户 ID |
| `X-User-Id` | 十进制字符串 | 是 | 当前用户 ID；系统任务使用 `0` |
| `X-Request-Id` | UUID 或等价唯一字符串 | 是 | 一次请求/重试链路的幂等和追踪键 |
| `X-Internal-Timestamp` | Unix epoch seconds | 是 | 签名时间 |
| `X-Internal-Signature` | `v1=<64 lowercase hex>` | 是 | HMAC-SHA256 签名 |

`Authorization`、浏览器传来的 `X-Tenant-Id` 和其他客户端身份 Header 不直接
转发为 Rust 的信任依据。内部共享密钥从配置或密钥服务读取，代码、日志和文档
示例不得使用生产密钥。

#### Canonical request

签名使用请求实际发送的 UTF-8 body bytes。先计算：

```text
body_sha256 = lowercase_hex(SHA-256(body_bytes))
```

`path_with_query` 是以 `/` 开头的请求路径和原样 query string；没有 query 时只
使用路径。方法转成大写；字段之间使用一个 LF，不加尾部 LF：

```text
v1
{METHOD}
{path_with_query}
{X-Internal-Timestamp}
{X-Tenant-Id}
{X-User-Id}
{X-Request-Id}
{body_sha256}
```

签名值：

```text
X-Internal-Signature = v1=lowercase_hex(
  HMAC-SHA256(internal_shared_secret, canonical_request UTF-8 bytes)
)
```

Rust 校验：

- 时间窗口为当前时间前后 300 秒；
- `X-Request-Id` 在 `(tenant_id, request_id)` 命名空间内只能成功接受一次，
  重放缓存至少保留 10 分钟；
- 缺 Header、格式错误、HMAC 不匹配、时间过期或 request ID 重放分别记录
  `INTERNAL_SIGNATURE_MISSING`、`INTERNAL_SIGNATURE_INVALID`、
  `INTERNAL_SIGNATURE_EXPIRED` 或 `INTERNAL_REQUEST_REPLAYED`；
- Header 租户与请求资源租户不一致时返回 `INTERNAL_TENANT_MISMATCH`；
- Rust 不以路径中的 document ID 单独授权，必须查 `(tenant_id, document_id)`。

#### Rust 作用域

文档原文、脱敏副本、向量索引、审核结果、内存缓存、输出文件和清理任务都必须
带租户作用域。`document_id` 可以全局唯一，但缓存键必须使用
`(tenant_id, document_id)`；相同 document ID 在不同租户的读取结果必须互不可见。

## 3. 迁移策略

迁移分为可重复执行的 Expand、Backfill、Dual-write、Enforce、Contract 五阶段。
每一阶段都由开关控制，数据库 Expand 变更保留到 Contract 完成后。

### 3.1 Expand

1. 建立 `tenant`、`tenant_member`、`tenant_invitation`、
   `tenant_audit_log`。
2. 资源表增加 nullable `tenant_id` 及以 `tenant_id` 开头的必要索引。
3. 不改变旧查询和旧用户过滤，新增表和列只读或旁路写入。
4. 建立迁移唯一键，保证重跑不会重复创建租户、成员或邀请。

验收：新列存在、索引存在、旧接口可用、没有强制租户过滤误伤。

### 3.2 Backfill

每个现有用户创建一个稳定的个人租户，`tenant_code` 使用不可变的
`user-{userId}`，用户成为该租户的 `OWNER`。资源归属按以下优先级确定：

1. `project.user_id`；
2. `bid_document.upload_user_id`；
3. `audit_task.audit_user_id` 或关联标书/项目的已确定 owner；
4. `knowledge_file.upload_user_id`、`chat_message.user_id`；
5. Trace、Outbox 和任务事件从父任务或父资源继承。

无法唯一确定 owner 的记录进入迁移隔离队列并阻止 Contract；禁止猜测或随机
分配租户。回填操作使用 `(source_table, source_id)` 幂等键。

验收：

- 所有核心资源表 `tenant_id IS NULL` 为 0，除非明确处于 Expand；
- 每个用户至少存在一个 ACTIVE 成员关系；
- 每个用户回填前后的可见资源数量一致；
- 重复执行不重复创建租户、成员或资源归属。

### 3.3 Dual-write

应用写入时同时保存旧用户归属和新 `tenant_id`，并在同一事务内校验：

- 创建资源的 `tenant_id` 来自 TenantContext；
- 更新、删除、下载、预览、导出使用租户作用域；
- 旧 user scope 与 tenant scope 的可见集合一致；
- 队列、Outbox、SSE 事件、文件路径和 Rust Header 都带租户字段。

双写校验失败时拒绝写入并记录 request ID，不允许静默只写旧字段。

### 3.4 Enforce

按内部测试租户、5%、25%、100% 灰度启用：

- TenantRequestContext、统一过滤器和 `tenant_id` 非空写入强制开启；
- 缺少租户上下文的资源写入返回 `503 TENANT_MIGRATION_NOT_READY`；
- 跨租户资源统一返回 404；
- 新登录、切换租户、SSE 和 Java 到 Rust 调用全部使用新契约。

### 3.5 Contract

只有在备份、恢复演练、数据核对和负向测试通过后才能：

- 将资源表 `tenant_id` 改为 `NOT NULL`；
- 删除仅用于隔离的旧 user filter 分支；
- 关闭 legacy fallback；
- 保留 `user_id`、`upload_user_id` 等审计/责任字段。

Contract 不是回滚点。Contract 前必须记录 schema 版本和可恢复备份。

## 4. 灰度与回滚

推荐顺序：

```text
关闭强制过滤
  -> Expand
  -> Backfill + 核对
  -> 内部租户 Dual-write
  -> 新登录会话和 Enforce
  -> 5% -> 25% -> 100%
  -> Contract
```

出现以下任一情况立即停止晋级：

- 跨租户读取、下载、SSE 或 Rust 结果泄漏；
- 回填数量不一致或出现非预期空 `tenant_id`；
- 核心接口错误率、P95 延迟超过发布门槛；
- 租户切换造成大面积会话失效；
- 签名重放或 Header 伪造被接受。

回滚规则：

1. 先关闭 `tenant.enforce` 和灰度开关，阻止新流量进入强制读路径；
2. 读路径回退到已验证的兼容逻辑，保留 Expand 列、表和租户数据；
3. 暂停会产生跨租户风险的异步消费和 Rust 代理，保留审计；
4. 不执行破坏性数据库回滚，不删除已写入的租户数据；
5. 排查并修复后从 Dual-write 或 Enforce 重新开始；
6. Contract 后不允许直接回滚 `NOT NULL`，必须走数据库恢复演练和显式 schema
   变更审批。

## 5. 观测和测试门槛

每个请求、任务、SSE 事件和 Rust 调用至少记录 `request_id`、`tenant_id`、
资源 ID、结果和延迟；日志中不得记录邀请原 token 或内部共享密钥。

最低测试矩阵：

- A/B 两租户的 ID、分页、排序、模糊查询、导出、下载和预览互不可见；
- 父子资源 `tenant_id` 不一致时失败；
- Worker 重试、死信、幂等和 SSE 回放保持租户字段；
- Rust 缺 Header、错误签名、过期签名、重放和跨租户 document ID 全部拒绝；
- 回填重跑不重复创建租户/成员，回填前后可见资源数量一致；
- 5% -> 25% -> 100% 开关切换和应用回滚不破坏 Expand 数据。
