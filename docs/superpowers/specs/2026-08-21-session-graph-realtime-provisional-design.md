# SessionGraph 实时 provisional 原子共享设计

## 1. 背景

PR1 已引入 `ReviewAttempt` 生命周期，并将覆盖率改为只统计成功完成的审查尝试。但正常 Execute 流程仍在单个
Agent 的全部条款结束后，才由 Coordinator 批量把 finding 写入 SessionGraph。其他并行 Agent 即使在下一轮
ReAct 拉取上下文，也可能看不到已经完成的条款发现。

当前 `SessionGraph` 还为节点和每类边分别维护 `RwLock`。`add_risk_with_edges()` 按顺序获取多个独立写锁，无法
保证 RiskNode、`has_risk`、`cites`、`cited_by`、LawNode 和版本信息在同一个可见时刻提交。多条款 finding 又会
针对每个条款重复调用该方法，造成同一个 RiskNode 被覆盖以及边重复追加。

本设计将 SessionGraph 定位为 Agent 之间的实时协作白板：Agent 产出的早期发现只能作为 `provisional` 线索，
其他 Agent 可以在下一轮 ReAct 中读取，但不能把它当作最终裁决结果。

## 2. 目标

- 单条款成功结束后立即把真实 finding 写入 SessionGraph，不等待 Agent 批次完成。
- finding、相关边、ReviewAttempt 成功状态和兼容 `reviewed_by` 在一个原子事务中提交。
- 一个多条款 finding 只保留一个 RiskNode，同时为所有关联条款建立去重边。
- 提供图级和条款级版本，让 Agent 能判断下一轮 ReAct 是否需要重新注入共享上下文。
- 保持现有 GraphSnapshot 字段、最终 findings 输出和外部 SSE 协议兼容。

## 3. 非目标

- 不在本 PR 执行 Merge、LegalVerify、Debate 或 Triage 的最终状态裁决。
- 不新增 `confirmed`、`merged`、`rejected` 的转换历史；这些属于 PR3。
- 不重建最终审计快照，也不保证最终 findings 与工作图完全一致；这些属于 PR3。
- 不实现中途打断正在进行的 LLM 调用。共享状态只在下一轮 ReAct 拉取时生效。
- 不把 SessionGraph 改造成持久化数据库或完整事件溯源系统。

## 4. 方案比较

### 4.1 统一 GraphState 和单个 RwLock（采用）

把图节点、边、审查尝试和版本信息放入一个 `GraphState`，所有一致性写入只获取一次写锁。实现简单，快照天然
一致，适合当前 Session 内存图规模。

### 4.2 保留多锁并规定加锁顺序（不采用）

改动较小，但所有读写路径都必须遵守同一锁顺序，容易产生死锁或遗漏。快照需要同时持有大量读锁，仍难证明没有
跨字段中间态。

### 4.3 以追加事件日志作为唯一事实源（不采用）

审计和回放能力最好，但需要投影、压缩和状态迁移机制，会提前侵入 PR3，超出本 PR 的单一职责。

## 5. 状态模型

`SessionGraph` 保留不属于图事务的数据结构，例如全局 risk ID 计数器。所有必须一致读取或写入的图数据进入：

```rust
struct GraphState {
  chunks: HashMap<String, ChunkNode>,
  risks: HashMap<String, RiskNode>,
  has_risk: HashMap<String, Vec<String>>,
  reviewed_by: HashMap<String, Vec<AgentId>>,
  linked_to: HashMap<String, Vec<LinkedChunk>>,
  cites: HashMap<String, Vec<String>>,
  cited_by: HashMap<String, Vec<String>>,
  contradicts: HashMap<String, Vec<(String, String)>>,
  same_law: HashMap<String, Vec<String>>,
  agents: HashMap<AgentId, AgentNode>,
  laws: HashMap<String, LawNode>,
  cases: HashMap<String, CaseNode>,
  review_attempts: HashMap<String, ReviewAttempt>,
  graph_version: u64,
  chunk_versions: HashMap<String, u64>,
}
```

`SessionGraph` 使用 `RwLock<GraphState>`。`risk_id_counter`、Scout 完成标志和预搜索缓存不参与 finding 提交的一致性，
继续作为独立运行态字段保留。

PR2 为 `RiskNode` 增加只表示工作图语义的 `state` 字段，其当前合法值只有 `provisional`。使用 serde 默认值保证旧
GraphSnapshot 可以读取。PR3 再扩展最终裁决状态和 `FindingTransition` 历史。

## 6. 原子提交接口

新增单一入口：

```rust
pub fn commit_review_result(
  &self,
  attempt_id: &str,
  outcome: ReviewAttemptOutcome,
  findings: &[RiskFinding],
) -> Result<GraphCommit, String>
```

事务在取得写锁后先完成全部校验，再执行任何修改：

1. `attempt_id` 必须存在且仍为 Started。
2. `Findings` 必须至少包含一个非 `no_risk` finding；`NoRisk` 不得携带真实 finding。
3. 每个 finding 的 `risk_id` 不得为空，`clause_ids` 不得为空。
4. ReviewAttempt 的 `finding_ids` 从真实 finding 派生，调用方不能单独传入一份可能不一致的 ID 列表。
5. 校验失败时不修改任何节点、边、attempt 或版本。

校验通过后一次完成：

- 每个 finding 只插入一个 `RiskNode { state: Provisional }`。
- 为 finding 的每个 `clause_id` 写入唯一 `has_risk` 边。
- 唯一写入 `cites`、`cited_by` 和 LawNode，并在同一状态内推导 `same_law`。
- 将 ReviewAttempt 转为 Completed，并派生唯一 `reviewed_by` 边。
- `graph_version` 增长一次。
- attempt 条款和所有 finding 关联条款组成受影响条款集合，每个 `chunk_version` 只增长一次。

`GraphCommit` 返回本次 `graph_version`、受影响条款及其新版本，供日志、测试和后续调用使用。失败尝试也通过统一
GraphState 写锁收口并更新相关版本，但不会创建 RiskNode。

## 7. 边去重和多条款 finding

内部继续使用 `Vec` 保持现有 GraphSnapshot JSON 形状和稳定顺序，但所有追加动作统一通过 `push_unique` 或等价
辅助函数执行。

- `has_risk`：同一 `chunk_id + risk_id` 唯一。
- `reviewed_by`：同一 `chunk_id + agent_id` 唯一。
- `cites`：同一 `risk_id + law_ref` 唯一。
- `cited_by`：同一 `law_ref + risk_id` 唯一。
- `same_law`：同一方向的 `chunk_id + other_chunk_id` 唯一，并保持双向边。
- `linked_to`、`contradicts`：按目标 ID 和原因组合去重。

多条款 finding 先插入一次 RiskNode，再遍历去重后的 `clause_ids` 建边。Coordinator 不再按 clause 重复提交同一
RiskNode。

## 8. 版本化读取

GraphSnapshot 新增兼容字段：

```rust
#[serde(default)]
pub graph_version: u64,
#[serde(default)]
pub chunk_versions: HashMap<String, u64>,
```

条款增量读取接口为：

```rust
pub fn query_clause_context_since(
  &self,
  chunk_id: &str,
  known_version: Option<u64>,
) -> Option<VersionedClauseContext>
```

- 调用方没有版本时返回当前完整上下文。
- `known_version` 等于当前条款版本时返回 `None`。
- 版本不同时，在同一个读锁下返回完整、一致的条款上下文和最新版本。

这不是事件差量：条款变化时返回该条款的最新完整上下文，避免维护 PR3 才需要的事件历史。ReActLoop 为当前审查
维护已注入的条款版本；每轮开始只在版本变化时追加新的“共享白板更新”。

## 9. 执行流程

正常条款路径调整为：

1. 获取并发许可。
2. 创建 Started ReviewAttempt。
3. 执行单条款 ReAct。
4. 分类为 Findings、NoRisk 或失败。
5. Findings/NoRisk 调用 `commit_review_result()`；成功返回后才记录 ClauseReviewProgress 和发送完成进度。
6. 失败路径原子收口 attempt，不写 provisional risk。

Coordinator 删除 Agent 批次结束后的 SessionGraph finding 写入。Execute 外层超时仍使用 PR1 的
ClauseReviewProgress 恢复已完成结果，但不再把恢复结果重复写图。现有公开 FindingAdded SSE 的时机和字段保持不变，
避免在 PR2 同时修改外部事件协议。

Scout 的 Hypothesis 仍保留现有语义，但写入也迁移到统一 GraphState，确保快照一致和边去重。BlindSpot 的正常发现
使用相同原子提交入口；静态 fallback 没有 ReviewAttempt 时使用受限的 provisional upsert 入口，并遵守相同去重和
版本规则。

## 10. 错误处理

- 锁中毒、attempt 不存在、终态重复迁移和输入不变量失败均返回明确中文错误。
- 原子提交校验发生在修改之前，失败时版本不得增长。
- 风险 ID 冲突但内容不同视为错误，不允许静默覆盖。
- 完全相同的 provisional upsert 可以安全返回现有节点，但不得重复边或重复增长条款版本。
- ReAct 条款任务若在提交时失败，该条款按执行失败处理，不得进入成功覆盖率。

## 11. 测试策略

严格采用 TDD，每项生产行为先建立失败测试：

1. SessionGraph 原子性：提交后 RiskNode、所有边、attempt、reviewed_by 和版本同时可见。
2. 回滚语义：非法 finding 提交失败后状态和版本完全不变。
3. 多条款 finding：一个 RiskNode，多条唯一 has_risk 边。
4. 边去重：重复法条、重复条款和幂等 upsert 不产生重复边。
5. 版本读取：未变化返回 None；提交后只提高受影响条款版本并返回新上下文。
6. NoRisk：完成 attempt、更新覆盖率和版本，但不创建 RiskNode。
7. 实时可见性：快速条款完成、慢条款仍阻塞时，另一个读取方已经能读到 provisional finding。
8. Coordinator 回归：Agent 批次完成和 Execute 超时恢复不会再次写入相同 finding。
9. 兼容性：缺少版本和 state 字段的旧 GraphSnapshot 仍能反序列化。

验证命令至少包括相关 SessionGraph、ReActLoop、Coordinator 单元测试，`cargo check --all-targets`、目标 Rust 文件
格式检查及 `git diff --check`。仓库既有全量测试失败必须与本 PR 回归分开报告。

## 12. PR 与交付边界

PR2 分支为 `codex/session-graph-realtime-provisional`，从 PR1 提交 `cc2c1fc` 派生。PR1 未合并时，PR2 以 PR1
分支为临时基线形成堆叠 PR；PR1 合并后同步最新 `dev` 并把 PR2 目标切回 `dev`。

PR2 只交付实时 provisional 原子共享、版本化读取和边去重。最终状态裁决、FindingTransition 历史及最终审计快照
一致性全部留给 PR3。
