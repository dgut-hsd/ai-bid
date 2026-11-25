# SessionGraph 实时 provisional 原子共享 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让单条款成功发现以 provisional 状态原子写入 SessionGraph，并通过图级、条款级版本供其他 Agent 在下一轮 ReAct 增量读取。

**Architecture:** 将必须一致读写的图数据收口到一个 `RwLock<GraphState>`；成功审查通过 `commit_review_result` 一次提交 finding、边、ReviewAttempt、覆盖率和版本。ReActLoop 按 `chunk_version` 拉取变化后的完整条款上下文，Coordinator 只负责汇总结果，不再批量重复写图。

**Tech Stack:** Rust 2024、`std::sync::RwLock`、Tokio、Serde、Utoipa、React/TypeScript、Vitest

**Spec:** `docs/superpowers/specs/2026-08-21-session-graph-realtime-provisional-design.md`

## Global Constraints

- PR2 基于 PR1 提交 `cc2c1fc`，不得混入 PR3 的 confirmed、merged、rejected 或最终审计快照重建。
- Agent 写入的真实 finding 一律是 `provisional`；当前 LLM 调用不中断，只允许下一轮 ReAct 看见更新。
- GraphSnapshot 保留全部旧字段；新字段使用 serde 默认值兼容历史数据。
- 一个多条款 finding 只创建一个 RiskNode；所有 Vec 形态边保持稳定顺序并去重。
- 正常成功、NoRisk 和失败的 ReviewAttempt 状态变化必须与相关图状态在同一个 GraphState 写锁内完成。
- 严格执行 RED → GREEN → REFACTOR；每项生产行为必须先看到对应测试因缺少该行为而失败。
- 代码、注释和错误消息遵守仓库中文规范；文件使用 UTF-8 无 BOM、LF 换行。
- 每次提交只包含当前任务列出的文件，禁止使用 `git add .` 或 `git add -A`。

---

### Task 1: 统一 GraphState 并保持快照协议兼容

**Files:**
- Modify: `backend-rust/src/agents/types.rs`
- Modify: `backend-rust/src/agents/session_graph.rs`
- Modify: `backend-rust/src/agents/coordinator.rs`
- Test: `backend-rust/src/agents/types.rs`
- Test: `backend-rust/src/agents/session_graph.rs`

**Interfaces:**
- Produces: `FindingState::Provisional`
- Produces: `RiskNode.state: FindingState`
- Produces: `GraphSnapshot.graph_version: u64`
- Produces: `GraphSnapshot.chunk_versions: HashMap<String, u64>`
- Produces: private `GraphState` owned by `SessionGraph.state: RwLock<GraphState>`
- Produces: `LinkedChunk: PartialEq + Eq` so linked edges can use the same stable-order dedupe helper
- Preserves: every existing public SessionGraph method signature used outside `session_graph.rs`

- [ ] **Step 1: Write failing compatibility and state tests**

Add these behavior tests before changing production types:

```rust
#[test]
fn test_graph_snapshot_old_json_defaults_versions() {
  let snapshot: GraphSnapshot = serde_json::from_value(serde_json::json!({
    "chunks": {}, "risks": {}, "has_risk": {}, "reviewed_by": {},
    "linked_to": {}, "cites": {}, "cited_by": {}, "agents": {},
    "laws": {}, "cases": {}, "contradicts": {}, "same_law": {},
    "review_attempts": {}
  }))
  .expect("旧快照必须继续可读");

  assert_eq!(snapshot.graph_version, 0);
  assert!(snapshot.chunk_versions.is_empty());
}

#[test]
fn test_snapshot_reads_one_consistent_graph_state() {
  let graph = SessionGraph::new();
  graph.add_chunk(make_test_chunk("ch_001"));
  let snapshot = graph.snapshot();

  assert_eq!(snapshot.graph_version, 1);
  assert_eq!(snapshot.chunk_versions.get("ch_001"), Some(&1));
  assert!(snapshot.chunks.contains_key("ch_001"));
}
```

The production mutation caught by these tests is dropping version defaults or updating nodes without the corresponding version.

- [ ] **Step 2: Run the tests and verify RED**

Run:

```powershell
cargo test test_graph_snapshot_old_json_defaults_versions --lib
cargo test test_snapshot_reads_one_consistent_graph_state --lib
```

Expected: compilation fails because `graph_version`, `chunk_versions` and `RiskNode.state` do not exist.

- [ ] **Step 3: Add the protocol types and unified state**

Add to `types.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum FindingState {
  #[default]
  Provisional,
}

pub struct RiskNode {
  pub finding: RiskFinding,
  pub law_refs: Vec<String>,
  #[serde(default)]
  pub state: FindingState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedChunk {
  pub chunk_id: String,
  pub reason: String,
}
```

Extend `GraphSnapshot` and `GraphSnapshot::new()`:

```rust
#[serde(default)]
pub graph_version: u64,
#[serde(default)]
pub chunk_versions: HashMap<String, u64>,
```

Replace the graph-related locks in `session_graph.rs` with:

```rust
#[derive(Clone, Default)]
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

pub struct SessionGraph {
  state: RwLock<GraphState>,
  risk_id_counter: AtomicU64,
  scout_complete: AtomicBool,
  search_results: RwLock<HashMap<String, Vec<SearchCacheEntry>>>,
}
```

Use these exact mutation helpers so a public mutation increments the graph once and each affected chunk once:

```rust
fn bump_versions(state: &mut GraphState, chunk_ids: impl IntoIterator<Item = String>) -> u64 {
  state.graph_version = state.graph_version.saturating_add(1);
  let mut unique = std::collections::HashSet::new();
  for chunk_id in chunk_ids {
    if unique.insert(chunk_id.clone()) {
      let version = state.chunk_versions.entry(chunk_id).or_default();
      *version = version.saturating_add(1);
    }
  }
  state.graph_version
}

fn push_unique<T: PartialEq>(items: &mut Vec<T>, value: T) -> bool {
  if items.contains(&value) {
    false
  } else {
    items.push(value);
    true
  }
}
```

Migrate `start_review_attempt`, `complete_review_attempt`, `fail_review_attempt`, `fail_started_attempts`, all node/edge writers,
all query methods, counters and `snapshot()` to obtain exactly one `state` guard per public operation. `snapshot()` clones all fields,
`graph_version` and `chunk_versions` from the same read guard. Preserve `search_results` and `scout_complete` as independent runtime state.

Update every `RiskNode` literal in `backend-rust/src/agents/` with `state: FindingState::Provisional`. The four production
literals are currently in `coordinator.rs`; the test fixture literal is in `session_graph.rs`.

- [ ] **Step 4: Run focused tests and verify GREEN**

Run:

```powershell
cargo test agents::session_graph::tests --lib
cargo test agents::types::tests::test_graph_snapshot --lib
```

Expected: all selected tests pass; existing SessionGraph behavior remains unchanged and the new version assertions pass.

- [ ] **Step 5: Commit Task 1**

```powershell
git add -- backend-rust/src/agents/types.rs backend-rust/src/agents/session_graph.rs backend-rust/src/agents/coordinator.rs
git commit -m "refactor(agents): 统一 SessionGraph 原子状态"
```

---

### Task 2: 原子提交 provisional finding、覆盖率和去重边

**Files:**
- Modify: `backend-rust/src/agents/types.rs`
- Modify: `backend-rust/src/agents/session_graph.rs`
- Test: `backend-rust/src/agents/session_graph.rs`

**Interfaces:**
- Consumes: `GraphState`, `FindingState::Provisional`, `push_unique`, `bump_versions`
- Produces: `GraphCommit { graph_version, chunk_versions }`
- Produces: `SessionGraph::commit_review_result(&self, attempt_id, outcome, findings) -> Result<GraphCommit, String>`
- Produces: `SessionGraph::upsert_provisional_findings(&self, findings) -> Result<GraphCommit, String>` for paths without ReviewAttempt

- [ ] **Step 1: Write failing atomicity, rollback, multi-clause and dedupe tests**

Add independent tests with literal expectations:

```rust
#[test]
fn commit_review_result_publishes_one_multi_clause_risk_atomically() {
  let graph = SessionGraph::new();
  graph.add_chunks(vec![make_test_chunk("ch_001"), make_test_chunk("ch_002")]);
  let attempt_id = graph
    .start_review_attempt(AgentId::FactCheck, "ch_001")
    .expect("应创建尝试");
  let mut finding = make_test_risk("R_001", "ch_001").finding;
  finding.clause_ids = vec!["ch_001".into(), "ch_002".into(), "ch_002".into()];
  finding.legal_basis = vec!["《测试法》第1条".into(), "《测试法》第1条".into()];

  let commit = graph
    .commit_review_result(&attempt_id, ReviewAttemptOutcome::Findings, &[finding])
    .expect("应原子提交");
  let snapshot = graph.snapshot();

  assert_eq!(snapshot.risks.len(), 1);
  assert_eq!(snapshot.risks["R_001"].state, FindingState::Provisional);
  assert_eq!(snapshot.has_risk["ch_001"], vec!["R_001"]);
  assert_eq!(snapshot.has_risk["ch_002"], vec!["R_001"]);
  assert_eq!(snapshot.cites["R_001"], vec!["《测试法》第1条"]);
  assert_eq!(snapshot.cited_by["《测试法》第1条"], vec!["R_001"]);
  assert_eq!(snapshot.reviewed_by["ch_001"], vec![AgentId::FactCheck]);
  assert_eq!(snapshot.review_attempts[&attempt_id].finding_ids, vec!["R_001"]);
  assert_eq!(commit.graph_version, snapshot.graph_version);
  assert_eq!(commit.chunk_versions.len(), 2);
}

#[test]
fn invalid_review_commit_rolls_back_every_field_and_version() {
  let graph = SessionGraph::new();
  graph.add_chunk(make_test_chunk("ch_001"));
  let attempt_id = graph
    .start_review_attempt(AgentId::FactCheck, "ch_001")
    .expect("应创建尝试");
  let before = graph.snapshot();

  let error = graph
    .commit_review_result(&attempt_id, ReviewAttemptOutcome::Findings, &[])
    .expect_err("空 Findings 必须失败");
  let after = graph.snapshot();

  assert!(error.contains("Findings"));
  assert_eq!(after.graph_version, before.graph_version);
  assert_eq!(after.risks.len(), before.risks.len());
  assert_eq!(after.has_risk, before.has_risk);
  assert_eq!(
    after.review_attempts[&attempt_id].status,
    ReviewAttemptStatus::Started
  );
}

#[test]
fn no_risk_commit_completes_attempt_without_risk_node() {
  let graph = SessionGraph::new();
  graph.add_chunk(make_test_chunk("ch_001"));
  let attempt_id = graph
    .start_review_attempt(AgentId::Procedure, "ch_001")
    .expect("应创建尝试");

  graph
    .commit_review_result(&attempt_id, ReviewAttemptOutcome::NoRisk, &[])
    .expect("NoRisk 应成功");
  let snapshot = graph.snapshot();

  assert!(snapshot.risks.is_empty());
  assert_eq!(snapshot.reviewed_by["ch_001"], vec![AgentId::Procedure]);
  assert_eq!(
    snapshot.review_attempts[&attempt_id].outcome,
    Some(ReviewAttemptOutcome::NoRisk)
  );
}

#[test]
fn failed_attempt_changes_only_its_chunk_version_and_never_coverage() {
  let graph = SessionGraph::new();
  graph.add_chunk(make_test_chunk("ch_001"));
  let attempt_id = graph
    .start_review_attempt(AgentId::Procedure, "ch_001")
    .expect("应创建尝试");
  let before = graph.snapshot().chunk_versions["ch_001"];

  graph
    .fail_review_attempt(
      &attempt_id,
      ReviewAttemptErrorCode::TaskPanic,
      "模拟任务异常",
    )
    .expect("应收口失败尝试");
  let snapshot = graph.snapshot();

  assert_eq!(snapshot.chunk_versions["ch_001"], before + 1);
  assert!(!snapshot.reviewed_by.contains_key("ch_001"));
  assert!(snapshot.risks.is_empty());
}

#[test]
fn repeated_edge_and_identical_upsert_writes_remain_unique() {
  let graph = SessionGraph::new();
  graph.add_chunks(vec![make_test_chunk("ch_001"), make_test_chunk("ch_002")]);
  graph.add_linked_to("ch_001", "ch_002", "同类风险");
  graph.add_linked_to("ch_001", "ch_002", "同类风险");
  graph.add_contradicts("ch_001", "ch_002", "要求冲突");
  graph.add_contradicts("ch_001", "ch_002", "要求冲突");
  let finding = make_test_risk("R_001", "ch_001").finding;

  graph
    .upsert_provisional_findings(std::slice::from_ref(&finding))
    .expect("首次写入应成功");
  let before_retry = graph.snapshot();
  graph
    .upsert_provisional_findings(&[finding])
    .expect("相同重试应幂等成功");
  let after_retry = graph.snapshot();

  assert_eq!(after_retry.linked_to["ch_001"].len(), 1);
  assert_eq!(after_retry.contradicts["ch_001"].len(), 1);
  assert_eq!(after_retry.contradicts["ch_002"].len(), 1);
  assert_eq!(after_retry.has_risk["ch_001"], vec!["R_001"]);
  assert_eq!(after_retry.graph_version, before_retry.graph_version);
}
```

The production mutations caught are partial writes before validation, per-clause RiskNode insertion, duplicate edge pushes and fake NoRisk nodes.

- [ ] **Step 2: Run the tests and verify RED**

Run each new test by exact name. Expected: compilation fails because `commit_review_result` and `GraphCommit` do not exist.

- [ ] **Step 3: Implement validation and one-lock commit**

Add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCommit {
  pub graph_version: u64,
  pub chunk_versions: HashMap<String, u64>,
}
```

Implement `commit_review_result` in this order while holding one write guard:

```rust
pub fn commit_review_result(
  &self,
  attempt_id: &str,
  outcome: ReviewAttemptOutcome,
  findings: &[RiskFinding],
) -> Result<GraphCommit, String> {
  let real_findings = findings.iter().filter(|finding| !finding.no_risk).collect::<Vec<_>>();
  validate_review_commit(outcome, &real_findings)?;
  let mut state = self.state.write().map_err(|_| "SessionGraph 状态写锁已中毒".to_string())?;
  validate_started_attempt(&state, attempt_id)?;
  validate_risk_conflicts(&state, &real_findings)?;
  let attempt_chunk = state.review_attempts[attempt_id].chunk_id.clone();
  let mut affected = vec![attempt_chunk.clone()];
  for finding in real_findings {
    affected.extend(upsert_provisional_in_state(&mut state, finding)?);
  }
  complete_attempt_in_state(&mut state, attempt_id, outcome, &real_findings)?;
  let graph_version = bump_versions(&mut state, affected.clone());
  Ok(build_graph_commit(&state, graph_version, affected))
}
```

Keep every validation helper pure and mutation-free. `validate_risk_conflicts` serializes the existing and proposed `RiskNode` with
`serde_json::to_value`; byte-equivalent JSON is an idempotent retry, while the same risk ID with different content is rejected.
`upsert_provisional_in_state` deduplicates clause IDs and law refs before updating node and edges.
`complete_attempt_in_state` derives finding IDs and `reviewed_by`; callers no longer pass finding IDs separately.

Implement `upsert_provisional_findings` with the same validation and state helper for Scout and static fallback paths that have no attempt.
An identical retry returns the current graph version without increasing versions or duplicating edges.

Retain `complete_review_attempt` temporarily as a compatibility wrapper for NoRisk-only callers; make Findings callers migrate to
`commit_review_result` in Task 4, then restrict or remove the wrapper after `rg` confirms no real-finding caller remains.

- [ ] **Step 4: Run focused tests and verify GREEN**

```powershell
cargo test commit_review_result --lib
cargo test invalid_review_commit --lib
cargo test no_risk_commit --lib
cargo test failed_attempt_changes_only_its_chunk_version --lib
cargo test repeated_edge_and_identical_upsert_writes_remain_unique --lib
cargo test agents::session_graph::tests --lib
```

Expected: all selected tests pass with no duplicate edges and no version change on failed validation.

- [ ] **Step 5: Commit Task 2**

```powershell
git add -- backend-rust/src/agents/types.rs backend-rust/src/agents/session_graph.rs
git commit -m "feat(agents): 原子提交 provisional 审查发现"
```

---

### Task 3: 按条款版本增量读取共享白板

**Files:**
- Modify: `backend-rust/src/agents/types.rs`
- Modify: `backend-rust/src/agents/session_graph.rs`
- Modify: `backend-rust/src/agents/react_loop.rs`
- Test: `backend-rust/src/agents/session_graph.rs`
- Test: `backend-rust/src/agents/react_loop.rs`

**Interfaces:**
- Consumes: `GraphState.chunk_versions`
- Produces: `VersionedClauseContext { version, context }`
- Produces: `SessionGraph::query_clause_context_since(chunk_id, known_version) -> Option<VersionedClauseContext>`
- Preserves: `SessionGraph::query_clause_context(chunk_id) -> ClauseContext` as a compatibility wrapper

- [ ] **Step 1: Write failing version-read tests**

```rust
#[test]
fn clause_context_since_returns_only_after_version_change() {
  let graph = SessionGraph::new();
  graph.add_chunk(make_test_chunk("ch_001"));
  let first = graph
    .query_clause_context_since("ch_001", None)
    .expect("首次读取必须返回上下文");

  assert!(
    graph
      .query_clause_context_since("ch_001", Some(first.version))
      .is_none()
  );

  let attempt_id = graph
    .start_review_attempt(AgentId::FactCheck, "ch_001")
    .expect("应创建尝试");
  graph
    .commit_review_result(&attempt_id, ReviewAttemptOutcome::NoRisk, &[])
    .expect("应完成尝试");
  let changed = graph
    .query_clause_context_since("ch_001", Some(first.version))
    .expect("条款变化后必须返回新上下文");

  assert!(changed.version > first.version);
  assert_eq!(changed.context.reviewed_by, vec![AgentId::FactCheck]);
}
```

Add a ReActLoop behavior test using a recording LLM: first turn sees the initial context once; after another graph writer commits a finding,
the next turn receives exactly one new `[Session 记忆更新 vN]` system message; an unchanged third turn receives no duplicate message.
Assert the conversation passed to the real ReActLoop boundary, not calls made on the recording double.

- [ ] **Step 2: Run the tests and verify RED**

```powershell
cargo test clause_context_since_returns_only_after_version_change --lib
cargo test react_loop_injects_graph_context_only_when_chunk_version_changes --lib
```

Expected: compilation fails because the versioned query and ReAct version tracking do not exist.

- [ ] **Step 3: Implement one-lock context construction and ReAct tracking**

Add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionedClauseContext {
  pub version: u64,
  pub context: ClauseContext,
}
```

Create private `build_clause_context(state: &GraphState, chunk_id: &str) -> ClauseContext`. Both public query methods take one read guard and
call this helper. `query_clause_context_since` returns `None` only when `known_version == current_version`; absent chunk versions use zero.

Inside `ReActLoop::react_loop`, initialize:

```rust
let mut known_chunk_version: Option<u64> = None;
```

At Step 0a call `query_clause_context_since`. When it returns a value, update `known_chunk_version` before pushing a system message whose
prefix is `[Session 记忆更新 v{version}]`. Reuse the existing reviewed-by, risk, linked, same-law and contradiction formatting. When it returns
`None`, push no SessionGraph message.

- [ ] **Step 4: Run focused tests and verify GREEN**

```powershell
cargo test clause_context_since --lib
cargo test react_loop_injects_graph_context_only_when_chunk_version_changes --lib
cargo test agents::react_loop --lib
```

Expected: version tests and all existing ReActLoop tests pass.

- [ ] **Step 5: Commit Task 3**

```powershell
git add -- backend-rust/src/agents/types.rs backend-rust/src/agents/session_graph.rs backend-rust/src/agents/react_loop.rs
git commit -m "feat(agents): 按条款版本增量读取共享状态"
```

---

### Task 4: 单条款完成后即时写图并删除 Coordinator 重复写入

**Files:**
- Modify: `backend-rust/src/agents/react_loop.rs`
- Modify: `backend-rust/src/agents/coordinator.rs`
- Test: `backend-rust/src/agents/react_loop.rs`
- Test: `backend-rust/src/agents/coordinator.rs`

**Interfaces:**
- Consumes: `SessionGraph::commit_review_result`
- Consumes: `SessionGraph::upsert_provisional_findings`
- Preserves: `ClauseReviewProgress` as outer-timeout result recovery, not as a graph writer
- Preserves: current public FindingAdded SSE payload and Coordinator result ordering

- [ ] **Step 1: Write failing realtime visibility and no-duplicate tests**

Extend the existing `GatedNoRiskLlm` fixture into `GatedClauseLlm`. It receives `started`, `released`,
`release_notify` and `block_marker`. A request containing the marker waits on the Notify loop; other requests immediately return an
`output_finding` call containing one literal risk. Then add this Tokio test:

```rust
#[tokio::test]
async fn completed_clause_is_visible_before_agent_batch_finishes() {
  let graph = Arc::new(SessionGraph::new());
  let clauses = vec![
    make_test_clause("ch_fast", "立即返回真实风险"),
    make_test_clause("ch_slow", "模拟阻塞直到测试结束"),
  ];
  graph.add_chunks(clauses.iter().map(chunk_from_clause).collect());
  let started = Arc::new(Notify::new());
  let released = Arc::new(AtomicBool::new(false));
  let release_notify = Arc::new(Notify::new());
  let llm_factory = gated_clause_factory(
    started.clone(),
    released.clone(),
    release_notify.clone(),
    "模拟阻塞".to_string(),
  );
  let review_graph = graph.clone();
  let review = tokio::spawn(async move {
    review_clauses_parallel_report(
      &clauses,
      test_react_loop,
      llm_factory,
      Arc::new(crate::agents::tools::ToolRegistry::new),
      2,
      Some(review_graph),
      None,
      AgentId::FactCheck,
      None,
    )
    .await
  });
  started.notified().await;

  let snapshot = graph.snapshot();
  let fast_attempt = snapshot
    .review_attempts
    .values()
    .find(|attempt| attempt.chunk_id == "ch_fast")
    .expect("快速条款必须已经提交");
  assert_eq!(fast_attempt.status, ReviewAttemptStatus::Completed);
  assert_eq!(snapshot.risks.len(), 1);
  assert_eq!(snapshot.has_risk["ch_fast"].len(), 1);
  assert!(!review.is_finished());

  released.store(true, Ordering::SeqCst);
  release_notify.notify_waiters();
  review.await.expect("审查任务不得 panic");
}
```

Define `make_test_clause`, `chunk_from_clause`, `test_react_loop` and `gated_clause_factory` in the existing test module.
They return the literal fixture shapes used by neighboring ReAct tests; `gated_clause_factory` returns
`Arc<dyn Fn() -> Box<dyn LlmClient> + Send + Sync>`. These helpers remain test-only. No sleep-based timing assertion is allowed.

Extend the Coordinator completion and Execute-timeout recovery tests to assert each recovered `risk_id` occurs once in `risks`, once per
`has_risk` edge and once per law edge after the whole review returns.

- [ ] **Step 2: Run the tests and verify RED**

```powershell
cargo test completed_clause_is_visible_before_agent_batch_finishes --lib
cargo test execute_timeout_keeps_completed_clause_result --lib
```

Expected: realtime visibility fails because findings are still written after the Agent batch; duplicate assertions expose current repeated writes.

- [ ] **Step 3: Commit the graph before progress and result publication**

In both sequential `ReActLoop::review` and `review_clauses_parallel_report_with_progress`, replace successful
`complete_review_attempt(... finding_ids)` calls with:

```rust
graph
  .commit_review_result(attempt_id, outcome, &findings)
  .map_err(anyhow::Error::msg)?;
```

For NoRisk this commits only attempt/coverage/version. For incomplete, timeout and panic paths retain atomic failure closure and never write a
truncated ENGINE_ERROR finding into risks. Record ClauseReviewProgress only after the graph transaction succeeds.

In Coordinator:

- Delete the SessionGraph write loop currently executed after `report.findings` returns.
- Delete the SessionGraph write loop in Execute outer-timeout recovery.
- Keep the existing FindingAdded SSE emission loop and result recovery logic.
- Route Scout Hypothesis and BlindSpot static fallback through `upsert_provisional_findings`.
- Keep BlindSpot normal ReAct findings on `commit_review_result`; do not add a second fallback write.

Run `rg -n "add_risk_with_edges|complete_review_attempt" backend-rust/src/agents` and verify no normal real-finding batch path remains.

- [ ] **Step 4: Run focused tests and verify GREEN**

```powershell
cargo test completed_clause_is_visible_before_agent_batch_finishes --lib
cargo test execute_timeout_keeps_completed_clause_result --lib
cargo test agents::coordinator::tests --lib
cargo test agents::react_loop --lib
```

Expected: the realtime test passes while the slow clause remains blocked; all SessionGraph-related Coordinator/ReAct tests pass. Record the
known unrelated dynamic-Agent fixture failure separately if the full Coordinator filter still includes it.

- [ ] **Step 5: Commit Task 4**

```powershell
git add -- backend-rust/src/agents/react_loop.rs backend-rust/src/agents/coordinator.rs
git commit -m "fix(agents): 单条款完成后立即共享 provisional 发现"
```

---

### Task 5: 同步 GraphSnapshot 前端映射和验证文档

**Files:**
- Modify: `frontend/src/types/audit.ts`
- Modify: `frontend/src/features/bidAudit/utils/mapFinding.ts`
- Modify: `frontend/src/features/bidAudit/utils/mapFinding.test.ts`
- Modify: `backend-rust/docs/实现.md`
- Modify: `backend-rust/docs/验证.md`

**Interfaces:**
- Consumes: backend `graph_version`, `chunk_versions`, RiskNode `state`
- Produces: frontend `graphVersion`, `chunkVersions`, RiskNode `state`
- Preserves: old payload defaults to graphVersion `0`, empty chunkVersions and state `provisional`

- [ ] **Step 1: Write failing frontend protocol tests**

Extend the existing backend snapshot fixture with:

```typescript
graph_version: 7,
chunk_versions: { ch_001: 3 },
risks: {
  R_001: {
    finding: makeBackendFinding(),
    law_refs: ['《测试法》第1条'],
    state: 'provisional',
  },
},
```

Assert literal mapped values:

```typescript
expect(result.graphVersion).toBe(7)
expect(result.chunkVersions).toEqual({ ch_001: 3 })
expect(result.risks.R_001.state).toBe('provisional')
```

Add an old-payload test omitting all three fields and assert `0`, `{}` and `'provisional'`.

- [ ] **Step 2: Run the frontend test and verify RED**

```powershell
node node_modules/vitest/vitest.mjs run src/features/bidAudit/utils/mapFinding.test.ts
```

Expected: assertions fail because the mapper drops version and state fields.

- [ ] **Step 3: Map the new fields with backward-compatible defaults**

Add frontend types:

```typescript
export type FindingState = 'provisional'

export interface GraphRiskNode {
  finding: RiskFinding
  lawRefs: string[]
  state: FindingState
}

export interface GraphSnapshot {
  graphVersion: number
  chunkVersions: Record<string, number>
  // existing fields remain unchanged
}
```

Extend backend transport types and `mapBackendGraphSnapshot`:

```typescript
graphVersion: snapshot.graph_version ?? 0,
chunkVersions: snapshot.chunk_versions ?? {},
state: risk.state ?? 'provisional',
```

Update `backend-rust/docs/实现.md` with the atomic transaction and next-turn visibility flow. Update `backend-rust/docs/验证.md` with exact
focused regression commands and the distinction between related passes and known repository baseline failures.

- [ ] **Step 4: Run frontend and protocol verification**

```powershell
node node_modules/vitest/vitest.mjs run src/features/bidAudit/utils/mapFinding.test.ts
node node_modules/vitest/vitest.mjs run
node node_modules/typescript/bin/tsc -b
node node_modules/eslint/bin/eslint.js src/types/audit.ts src/features/bidAudit/utils/mapFinding.ts
```

Expected: focused and full Vitest pass, TypeScript compiles and changed production files have no ESLint errors.

- [ ] **Step 5: Commit Task 5**

```powershell
git add -- frontend/src/types/audit.ts frontend/src/features/bidAudit/utils/mapFinding.ts frontend/src/features/bidAudit/utils/mapFinding.test.ts backend-rust/docs/实现.md backend-rust/docs/验证.md
git commit -m "docs(agents): 同步 SessionGraph 版本协议与验证"
```

---

### Task 6: 完整回归、范围审计和 PR2 交付准备

**Files:**
- Verify only; modify only files already listed in Tasks 1-5 when a PR2-specific regression requires repair

**Interfaces:**
- Verifies: PR2 remains a stacked delta on top of `cc2c1fc`
- Verifies: no PR3 lifecycle or final snapshot adjudication entered the diff

- [ ] **Step 1: Run Rust formatting and focused regressions**

```powershell
rustfmt --edition 2024 --check src/agents/types.rs src/agents/session_graph.rs src/agents/react_loop.rs src/agents/coordinator.rs
cargo test agents::session_graph::tests --lib
cargo test agents::react_loop --lib
cargo test completed_clause_is_visible_before_agent_batch_finishes --lib
cargo test execute_timeout_keeps_completed_clause_result --lib
cargo check --all-targets
```

Expected: all PR2-related tests and compilation pass.

- [ ] **Step 2: Run broader baselines without misreporting them**

```powershell
cargo test --lib
cargo clippy --lib --tests -- -D warnings
```

Expected: compare failures against the documented PR1 baseline. Any new failure in a touched path blocks completion; known unrelated failures are
recorded with exact names and counts instead of being called green.

- [ ] **Step 3: Run frontend regression gates**

```powershell
node node_modules/vitest/vitest.mjs run
node node_modules/typescript/bin/tsc -b
node node_modules/eslint/bin/eslint.js src/types/audit.ts src/features/bidAudit/utils/mapFinding.ts
```

Expected: 0 frontend test, type or changed-production lint failures.

- [ ] **Step 4: Audit exact diff and boundaries**

```powershell
git status --short
git diff --check cc2c1fc...HEAD
git diff --name-only cc2c1fc...HEAD
rg -n "confirmed|merged|rejected|FindingTransition" backend-rust/src/agents frontend/src
```

Expected: only Tasks 1-5 files plus this plan/spec are present; search shows no newly introduced PR3 state machine. Review the diff to ensure the
Coordinator has no normal or timeout-recovery duplicate graph write.

- [ ] **Step 5: Prepare the final handoff**

Report exact commits, changed files, focused test counts, broader baseline failures, worktree path and the stacked-PR base/head. Do not push or
create PR2 until the user explicitly requests submission.
