# SessionGraph 最终裁决与审计快照 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 PR2 已确认的生命周期错误，并让 Coordinator 最终 findings 与完整 SessionGraph 审计快照保持一致。

**Architecture:** Agent 继续原子追加 provisional finding；Coordinator 收集 Merge 合并关系，在 Triage 后通过单锁事务裁决
confirmed/merged/rejected，重建风险派生边并追加 FindingTransition。外部仍返回一个兼容 GraphSnapshot，但它是最终裁决后的审计快照。

**Tech Stack:** Rust 2024、Tokio、Serde、Utoipa、React/TypeScript、Vitest

**Spec:** `docs/superpowers/specs/2026-08-22-session-graph-final-audit-design.md`

---

### Task 1: 修复 PR2 BlindSpot 与提交失败语义

**Files:**
- Modify/Test: `backend-rust/src/agents/react_loop.rs`
- Modify/Test: `backend-rust/src/agents/coordinator.rs`
- Modify/Test: `backend-rust/src/agents/session_graph.rs`

- [ ] 先增加失败测试：BlindSpot 一条 NoRisk 成功、一条失败时只兜底失败条款；一条有 finding、一条失败时仍兜底失败条款；静态 fallback 的真实 finding 写入图。
- [ ] 运行聚焦测试并确认因当前整批判断和错位 upsert 而失败。
- [ ] 改为根据最新 BlindSpot ReviewAttempt 逐条计算失败/未收口条款，把它们交给仅扫描指定条款的 fallback；移除正常 ReAct 后的重复 upsert，在 fallback 返回前写入真实 finding。
- [ ] 增加失败测试：顺序 ReAct/Scout 的 `commit_review_result` 失败时不返回成功 finding、不发布成功进度；`complete_review_attempt` 不再接受 Findings。
- [ ] 实现最小修复：提交错误转为条款失败；Scout 返回空结果；旧完成接口只允许 NoRisk。
- [ ] 运行 `cargo test blind_spot --lib`、`cargo test agents::react_loop --lib`、`cargo test agents::session_graph::tests --lib` 并提交 `fix(agents): 修复共享白板失败收口与盲点兜底`。

### Task 2: 实现最终状态机和原子裁决

**Files:**
- Modify/Test: `backend-rust/src/agents/types.rs`
- Modify/Test: `backend-rust/src/agents/session_graph.rs`

- [ ] 先增加失败测试，覆盖：最终 finding 变为 confirmed 并同步修改字段；源 finding 合并到 confirmed 目标；显式 rejected 保留原因；未裁决 finding 保持 provisional；非法目标整批回滚；相同裁决重试幂等。
- [ ] 运行测试并确认因为状态、历史和裁决接口不存在而失败。
- [ ] 扩展 `FindingState`，新增 `FindingTransition`、`RiskDecision` 和 GraphSnapshot 兼容字段；RiskNode 增加 `merged_into`、`decision_reason`。
- [ ] 实现 `SessionGraph::finalize_audit(final_findings, merged, rejected)`：锁内先校验，后同步节点状态、重建风险派生边、追加转换历史和版本。
- [ ] 运行 SessionGraph 聚焦测试并提交 `feat(agents): 增加 finding 最终裁决与转换历史`。

### Task 3: Coordinator 收集合并决策并生成最终审计快照

**Files:**
- Modify/Test: `backend-rust/src/agents/coordinator.rs`

- [ ] 先增加失败测试：Merge 返回被移除 ID 到保留 ID 的映射；两段合并链解析到最终目标；Coordinator 输出 findings ID 与 confirmed 节点集合相等，且最终 severity/法条与 RiskNode 相同。
- [ ] 运行测试并确认当前 MergeResult 不保留裁决信息且图仍是原始快照。
- [ ] 扩展 MergeResult，累计两轮 Merge 的合并关系并解析最终目标；Triage 后调用 `finalize_audit`，失败则让 review 返回错误，不落不一致快照。
- [ ] 只把最终 confirmed findings 放进 CoordinatorOutput；保留 provisional、merged、rejected 节点和 ReviewAttempt 历史。
- [ ] 运行 Coordinator 聚焦测试并提交 `fix(agents): 对齐最终结果与审计快照`。

### Task 4: 前端协议、文档与完整验证

**Files:**
- Modify/Test: `frontend/src/types/audit.ts`
- Modify/Test: `frontend/src/features/bidAudit/utils/mapFinding.ts`
- Modify: `backend-rust/docs/实现.md`
- Modify: `backend-rust/docs/验证.md`

- [ ] 先增加前端失败测试，覆盖四种 state、mergedInto、decisionReason、findingTransitions 及旧载荷默认值。
- [ ] 扩展前端类型和 snake_case 到 camelCase 映射，保持旧载荷兼容。
- [ ] 更新实现与验证文档，明确工作图和最终审计快照语义。
- [ ] 运行 Rust 聚焦测试、`cargo check --all-targets`、Rust 全量基线；运行前端全量 Vitest、`tsc -b` 和目标 ESLint；执行 `git diff --check`。
- [ ] 完成最终规格审查、代码质量审查和分支交付检查；PR1 未合并时保持以 PR1 分支为堆叠基线，不直接错误地提交到 `dev`。
