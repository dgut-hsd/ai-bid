# SessionGraph 最终裁决与审计快照一致性设计

## 1. 目标

在 PR2 的实时 `provisional` 协作白板之上，由 Coordinator 在 Merge、LegalVerify、Debate 和 Triage 全部结束后执行一次
最终裁决，使 `CoordinatorOutput.findings` 与 `graph_snapshot` 表达同一份最终事实。

- Agent 仍只能追加 `provisional` finding。
- Coordinator 独占 `confirmed`、`merged`、`rejected` 状态流转。
- 最终 `findings` 只包含 `confirmed` finding。
- 完整 `graph_snapshot` 保留原始节点、审查尝试、被合并节点和状态转换历史。
- `merged` 节点必须指向最终保留的目标 finding。
- 明确证伪的 finding 记录为 `rejected` 并保留原因。
- 证据不足但没有被证伪、或仅为 Hypothesis 的 finding 保持 `provisional`，不得伪装成 rejected。

## 2. 状态与历史

`FindingState` 扩展为 `provisional | confirmed | merged | rejected`。`RiskNode` 增加可选的 `merged_into` 和
`decision_reason`，旧快照缺失字段时保持兼容。

每次实际状态变化追加一条 `FindingTransition`：

```rust
pub struct FindingTransition {
  pub risk_id: String,
  pub from: FindingState,
  pub to: FindingState,
  pub reason: String,
  pub merged_into: Option<String>,
  pub decided_at: String,
}
```

相同最终裁决重试必须幂等，不重复追加转换历史或增长图版本。终态不得被另一个终态覆盖。

## 3. Coordinator 裁决输入

Merge 返回保留 finding 以及 `source_risk_id -> target_risk_id` 的合并记录。两次 Merge 产生的链必须解析到最终保留目标；
目标不在最终 findings 中时，源 finding 不标记为 merged，而是继续保持 provisional。

Coordinator 在 Triage 后调用一个原子接口，传入最终 findings、已解析合并映射和显式 rejected 决策。当前管线没有可靠的
显式证伪信号，因此证据准入失败和 Hypothesis 过滤不自动转为 rejected；接口保留 rejected 能力供现有或后续明确裁决调用。

## 4. 原子最终快照

最终裁决在单个 `GraphState` 写锁中完成：

1. 校验每个最终 finding 已存在于工作图且内容合法。
2. 校验所有 merged 目标属于最终 finding。
3. 校验 rejected 与 confirmed/merged 不冲突。
4. 把最终 finding 的最新字段同步回对应 RiskNode 并标记 confirmed。
5. 标记 merged/rejected，写入原因、目标和转换历史。
6. 重建 `has_risk`、`cites`、`cited_by`、`laws`、`same_law`，使派生边与节点当前内容一致。
7. 每个实际变化的条款版本增长一次，图版本整次裁决只增长一次。

校验失败不得修改任何状态。Coordinator 只有在裁决成功后才返回 `CoordinatorOutput`，返回的 `graph_snapshot` 是裁决后的
最终审计快照，不再是 Merge 前工作图的直接快照。

## 5. PR2 审查修复

合并交付同时修复已经确认的 PR2 问题：

- BlindSpot 按条款的 ReviewAttempt 状态处理部分成功/失败，只对失败或未收口条款执行静态 fallback。
- 静态 fallback 的真实 finding 通过受限 upsert 写入 SessionGraph；正常 ReAct finding 不重复 upsert。
- 顺序 ReAct 和 Scout 的图提交失败不得继续作为成功 finding 发布。
- 旧 `complete_review_attempt` 收窄为 NoRisk 兼容入口，禁止绕过原子 finding 提交。

## 6. 协议兼容

Rust 和前端保留现有 GraphSnapshot 字段。新增字段均提供默认值；前端将完整状态联合类型和转换历史映射为 camelCase。
API 继续只返回一个 `graph_snapshot`，其含义升级为最终审计快照，不新增并行的外部快照字段。

## 7. 验收

- 混合 BlindSpot 成功/失败不会漏兜底或误标成功条款。
- 静态 fallback finding 同时存在于最终结果与图中。
- 原子裁决支持 confirmed、merged、rejected 和 provisional 保留。
- 合并链解析到最终 confirmed 目标。
- 最终 findings 的 ID 集合与图中 confirmed 节点集合完全一致。
- 最终 finding 修改后的 severity、reason、legal basis 与图节点一致，派生边无残留。
- 旧 GraphSnapshot 仍能反序列化，前端旧载荷仍有兼容默认值。
