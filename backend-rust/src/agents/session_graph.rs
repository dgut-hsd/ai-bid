//! SessionGraph — 中期记忆：Session Knowledge Graph (Blackboard 核心)。
//!
//! 设计文档 §7.8 / temp.md Phase 2 定义。
//!
//! ## 架构角色
//!
//! **Blackboard 拉取侧** — Agent 共享工作区：
//! - 所有 Agent 读写同一张图
//! - 每轮 ReAct 拉取已知结论（`query_clause_context`）
//! - 审查发现写入新 Risk 节点 + 边（`add_risk_with_edges`）
//!
//! ## 并发语义
//!
//! SessionGraph 的图状态由单个 `RwLock<GraphState>` 保护：
//! - 节点、边、审查尝试和版本在同一临界区提交
//! - 查询和快照只读取一份一致状态
//! - 预搜索缓存和 Scout 完成标志属于运行态，不参与图事务
//!
//! ## 生命周期
//!
//! Session 结束销毁，不持久化。长期记忆（Neo4j + Qdrant）在 Phase 3+ 实现。

use crate::agents::types::*;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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
    finding_transitions: Vec<FindingTransition>,
    graph_version: u64,
    chunk_versions: HashMap<String, u64>,
}

fn bump_versions(state: &mut GraphState, chunk_ids: impl IntoIterator<Item = String>) -> u64 {
    state.graph_version = state.graph_version.saturating_add(1);
    let mut unique = HashSet::new();
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

/// 一次原子图提交产生的版本信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCommit {
    pub graph_version: u64,
    pub chunk_versions: HashMap<String, u64>,
}

fn deduplicate_strings(values: &[String]) -> Vec<String> {
    let mut unique = Vec::new();
    for value in values {
        push_unique(&mut unique, value.clone());
    }
    unique
}

/// 返回 Agent 工作态同法条邻接发生变化的条款。
fn changed_agent_same_law_chunks(
    before: &HashMap<String, Vec<String>>,
    after: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let mut changed = before
        .keys()
        .chain(after.keys())
        .filter(|chunk_id| before.get(*chunk_id) != after.get(*chunk_id))
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    changed.sort();
    changed
}

fn normalize_provisional_findings(findings: &[RiskFinding]) -> Result<Vec<RiskNode>, String> {
    let mut nodes = Vec::new();
    let mut risk_ids = HashSet::new();
    for finding in findings.iter().filter(|finding| !finding.no_risk) {
        if finding.risk_id.trim().is_empty() {
            return Err("provisional finding 的 risk_id 不得为空".to_string());
        }
        if finding.clause_ids.is_empty() {
            return Err(format!(
                "provisional finding {} 的 clause_ids 不得为空",
                finding.risk_id
            ));
        }
        if !risk_ids.insert(finding.risk_id.clone()) {
            return Err(format!("同次提交包含重复 risk_id: {}", finding.risk_id));
        }
        let mut normalized = finding.clone();
        normalized.clause_ids = deduplicate_strings(&finding.clause_ids);
        normalized.legal_basis = deduplicate_strings(&finding.legal_basis);
        nodes.push(RiskNode {
            law_refs: normalized.legal_basis.clone(),
            finding: normalized,
            state: FindingState::Provisional,
            merged_into: None,
            decision_reason: None,
        });
    }
    Ok(nodes)
}

/// 中期记忆：Session Knowledge Graph。
///
/// 线程安全的内存图，Agent 在审查过程中读写。
pub struct SessionGraph {
    /// 需要一致读写的图数据。
    state: RwLock<GraphState>,
    /// 全局 risk_id 计数器，保证多 Agent 并发写入 SessionGraph 时 ID 唯一。
    ///
    /// 每次 `next_risk_id()` 调用原子递增，返回 `R_001`, `R_002`, ...。
    /// 同一 Session 内不重复，跨 Session 不保证（符合 Session 生命周期语义）。
    risk_id_counter: AtomicU64,
    /// Scout 初筛阶段是否已完成（Phase 2 Agent 在开始审查前检查此标志）。
    scout_complete: AtomicBool,
    /// 预搜索结果缓存 (chunk_id → 条款相关的批量搜索结果)。
    ///
    /// Coordinator 批量搜索阶段写入，Execute Phase 读取并注入 Agent prompt。
    search_results: RwLock<HashMap<String, Vec<SearchCacheEntry>>>,
}

impl SessionGraph {
    /// 创建空的 SessionGraph。
    pub fn new() -> Self {
        Self {
            state: RwLock::new(GraphState::default()),
            risk_id_counter: AtomicU64::new(0),
            scout_complete: AtomicBool::new(false),
            search_results: RwLock::new(HashMap::new()),
        }
    }

    /// 生成全局唯一的 risk_id。多 Agent 并发安全。
    ///
    /// 每次调用返回全局递进的 `R_001`, `R_002`, ...。
    /// 同一 Session 内保证不重复，跨 Session 不保证（符合 Session 生命周期语义）。
    ///
    /// 用于 `ReActLoop::review()` 在审查开始前分配 ID，以及
    /// `Coordinator` 的 BlindSpot fallback 等路径。
    pub fn next_risk_id(&self) -> String {
        let id = self.risk_id_counter.fetch_add(1, Ordering::Relaxed);
        format!("R_{:03}", id + 1)
    }

    /// 创建一条已经取得执行名额的审查尝试。
    pub fn start_review_attempt(
        &self,
        agent_id: AgentId,
        chunk_id: &str,
    ) -> Result<String, String> {
        let attempt_id = uuid::Uuid::new_v4().to_string();
        let attempt = ReviewAttempt {
            attempt_id: attempt_id.clone(),
            agent_id,
            chunk_id: chunk_id.to_string(),
            status: ReviewAttemptStatus::Started,
            outcome: None,
            finding_ids: Vec::new(),
            error_code: None,
            error_message: None,
            started_at: chrono::Utc::now().to_rfc3339(),
            finished_at: None,
        };
        let mut state = self
            .state
            .write()
            .map_err(|_| "SessionGraph 状态写锁已中毒".to_string())?;
        state.review_attempts.insert(attempt_id.clone(), attempt);
        bump_versions(&mut state, [chunk_id.to_string()]);
        Ok(attempt_id)
    }

    /// 将审查尝试标记为成功，并派生兼容的 reviewed_by 边。
    pub fn complete_review_attempt(
        &self,
        attempt_id: &str,
        outcome: ReviewAttemptOutcome,
        finding_ids: Vec<String>,
    ) -> Result<(), String> {
        match outcome {
            ReviewAttemptOutcome::Findings => {
                return Err(
                    "Findings 结果必须使用 commit_review_result 原子提交 finding 与审查状态"
                        .to_string(),
                );
            }
            ReviewAttemptOutcome::NoRisk if !finding_ids.is_empty() => {
                return Err("NoRisk 结果的 finding_ids 必须为空".to_string());
            }
            _ => {}
        }
        let mut state = self
            .state
            .write()
            .map_err(|_| "SessionGraph 状态写锁已中毒".to_string())?;
        let (agent_id, chunk_id) = {
            let attempt = state
                .review_attempts
                .get_mut(attempt_id)
                .ok_or_else(|| format!("审查尝试不存在: {}", attempt_id))?;
            if attempt.status != ReviewAttemptStatus::Started {
                return Err(format!("审查尝试 {} 已结束，禁止重复流转", attempt_id));
            }
            attempt.status = ReviewAttemptStatus::Completed;
            attempt.outcome = Some(outcome);
            attempt.finding_ids = finding_ids;
            attempt.finished_at = Some(chrono::Utc::now().to_rfc3339());
            (attempt.agent_id.clone(), attempt.chunk_id.clone())
        };
        push_unique(
            state.reviewed_by.entry(chunk_id.clone()).or_default(),
            agent_id,
        );
        bump_versions(&mut state, [chunk_id]);
        Ok(())
    }

    fn validate_risk_conflicts(state: &GraphState, nodes: &[RiskNode]) -> Result<(), String> {
        for node in nodes {
            let risk_id = &node.finding.risk_id;
            if let Some(existing) = state.risks.get(risk_id) {
                let existing_json = serde_json::to_value(existing)
                    .map_err(|error| format!("序列化已有风险 {} 失败: {}", risk_id, error))?;
                let proposed_json = serde_json::to_value(node)
                    .map_err(|error| format!("序列化待写风险 {} 失败: {}", risk_id, error))?;
                if existing_json != proposed_json {
                    return Err(format!("risk_id {} 已存在且内容不同", risk_id));
                }
            }
        }
        Ok(())
    }

    fn is_terminal_risk(state: &GraphState, risk_id: &str) -> bool {
        state
            .risks
            .get(risk_id)
            .is_some_and(|node| node.state != FindingState::Provisional)
    }

    fn is_valid_legacy_risk_input(risk: &RiskNode) -> bool {
        risk.state == FindingState::Provisional
            && risk.merged_into.is_none()
            && risk.decision_reason.is_none()
    }

    fn validate_finding_clause_refs(
        state: &GraphState,
        label: &str,
        finding: &RiskFinding,
    ) -> Result<(), String> {
        for clause_id in &finding.clause_ids {
            if clause_id.trim().is_empty() {
                return Err(format!(
                    "{} {} 的 clause_id 不得为空白",
                    label, finding.risk_id
                ));
            }
            if !state.chunks.contains_key(clause_id) {
                return Err(format!(
                    "{} {} 引用的条款不存在: {}",
                    label, finding.risk_id, clause_id
                ));
            }
        }
        Ok(())
    }

    fn agent_visible_same_law_in_state(state: &GraphState) -> HashMap<String, Vec<String>> {
        build_agent_visible_same_law(&state.risks, &state.has_risk, &state.cites, &state.cited_by)
    }

    /// 合并直接变化的风险和受影响法条，并在整批事务中只扫描一次 has_risk。
    fn indexed_chunks_for_changes_in_state(
        state: &GraphState,
        direct_risk_ids: &[String],
        law_refs: &[String],
    ) -> Vec<String> {
        if direct_risk_ids.is_empty() && law_refs.is_empty() {
            return Vec::new();
        }
        let mut related_risk_ids = direct_risk_ids.iter().cloned().collect::<HashSet<_>>();
        for law_ref in law_refs {
            for risk_id in state.cited_by.get(law_ref).into_iter().flatten() {
                let cites_law = state
                    .cites
                    .get(risk_id)
                    .is_some_and(|cites| cites.contains(law_ref));
                let is_visible = state.risks.get(risk_id).is_some_and(|risk| {
                    is_agent_visible_risk(risk)
                        && risk.finding.finding_role != FindingRole::Hypothesis
                });
                if cites_law && is_visible {
                    related_risk_ids.insert(risk_id.clone());
                }
            }
        }
        if related_risk_ids.is_empty() {
            return Vec::new();
        }

        let mut chunks = state
            .has_risk
            .iter()
            .filter(|(_, risk_ids)| {
                risk_ids
                    .iter()
                    .any(|risk_id| related_risk_ids.contains(risk_id))
            })
            .map(|(chunk_id, _)| chunk_id.clone())
            .collect::<Vec<_>>();
        chunks.sort();
        chunks
    }

    fn upsert_provisional_node_in_state(
        state: &mut GraphState,
        node: &RiskNode,
    ) -> (Vec<String>, Vec<String>, bool) {
        let risk_id = node.finding.risk_id.clone();
        let mut affected = Vec::new();
        let mut changed = false;
        if !state.risks.contains_key(&risk_id) {
            state.risks.insert(risk_id.clone(), node.clone());
            changed = true;
        }
        for chunk_id in &node.finding.clause_ids {
            if push_unique(
                state.has_risk.entry(chunk_id.clone()).or_default(),
                risk_id.clone(),
            ) {
                changed = true;
                affected.push(chunk_id.clone());
            }
        }
        let is_hypothesis = node.finding.finding_role == FindingRole::Hypothesis;
        for law_ref in &node.law_refs {
            if push_unique(
                state.cites.entry(risk_id.clone()).or_default(),
                law_ref.clone(),
            ) {
                changed = true;
            }
            // Scout 假设仅供后续 Agent 参考，不提前创建正式法条节点和反向边。
            if !is_hypothesis {
                if push_unique(
                    state.cited_by.entry(law_ref.clone()).or_default(),
                    risk_id.clone(),
                ) {
                    changed = true;
                }
                if !state.laws.contains_key(law_ref) {
                    state.laws.insert(
                        law_ref.clone(),
                        LawNode {
                            law_id: law_ref.clone(),
                            article_no: law_ref.clone(),
                            title: String::new(),
                        },
                    );
                    changed = true;
                }
            }
        }
        if !is_hypothesis {
            for chunk_id in &node.finding.clause_ids {
                let same_law_affected =
                    Self::derive_same_law_edges_in_state(state, &node.law_refs, chunk_id);
                if !same_law_affected.is_empty() {
                    changed = true;
                    affected.extend(same_law_affected);
                }
            }
        }
        let affected_law_refs = if changed && !is_hypothesis {
            node.law_refs.clone()
        } else {
            Vec::new()
        };
        if changed {
            affected.extend(node.finding.clause_ids.clone());
        }
        (deduplicate_strings(&affected), affected_law_refs, changed)
    }

    /// 按当前 RiskNode 全量重建风险派生索引，避免最终字段覆盖后残留旧边。
    fn rebuild_risk_indexes(state: &mut GraphState) {
        Self::rebuild_risk_indexes_parts(
            &state.risks,
            &mut state.has_risk,
            &mut state.cites,
            &mut state.cited_by,
            &mut state.same_law,
            &mut state.laws,
        );
    }

    fn rebuild_risk_indexes_parts(
        risks: &HashMap<String, RiskNode>,
        has_risk: &mut HashMap<String, Vec<String>>,
        cites: &mut HashMap<String, Vec<String>>,
        cited_by: &mut HashMap<String, Vec<String>>,
        same_law: &mut HashMap<String, Vec<String>>,
        laws: &mut HashMap<String, LawNode>,
    ) {
        let previous_risk_laws = cited_by.keys().cloned().collect::<Vec<_>>();
        for law_ref in previous_risk_laws {
            let is_risk_placeholder = laws.get(&law_ref).is_some_and(|law| {
                law.law_id == law_ref && law.article_no == law_ref && law.title.is_empty()
            });
            if is_risk_placeholder {
                laws.remove(&law_ref);
            }
        }
        has_risk.clear();
        cites.clear();
        cited_by.clear();
        same_law.clear();

        let mut risk_ids = risks.keys().cloned().collect::<Vec<_>>();
        risk_ids.sort();
        for risk_id in risk_ids {
            let Some(node) = risks.get(&risk_id).cloned() else {
                continue;
            };
            for chunk_id in &node.finding.clause_ids {
                push_unique(
                    has_risk.entry(chunk_id.clone()).or_default(),
                    risk_id.clone(),
                );
            }
            for law_ref in &node.law_refs {
                push_unique(cites.entry(risk_id.clone()).or_default(), law_ref.clone());
                if node.finding.finding_role != FindingRole::Hypothesis {
                    push_unique(
                        cited_by.entry(law_ref.clone()).or_default(),
                        risk_id.clone(),
                    );
                    laws.entry(law_ref.clone()).or_insert_with(|| LawNode {
                        law_id: law_ref.clone(),
                        article_no: law_ref.clone(),
                        title: String::new(),
                    });
                }
            }
        }

        let cited_risks = cited_by.values().cloned().collect::<Vec<_>>();
        for risk_ids in cited_risks {
            let mut chunks = risk_ids
                .iter()
                .filter_map(|risk_id| risks.get(risk_id))
                .flat_map(|node| node.finding.clause_ids.iter().cloned())
                .collect::<Vec<_>>();
            chunks.sort();
            chunks.dedup();
            for chunk_id in &chunks {
                for other_chunk_id in &chunks {
                    if chunk_id != other_chunk_id {
                        push_unique(
                            same_law.entry(chunk_id.clone()).or_default(),
                            other_chunk_id.clone(),
                        );
                    }
                }
            }
        }
        for values in has_risk.values_mut() {
            values.sort();
        }
        for values in cites.values_mut() {
            values.sort();
        }
        for values in cited_by.values_mut() {
            values.sort();
        }
        for values in same_law.values_mut() {
            values.sort();
        }
    }

    fn finding_content_matches(left: &RiskFinding, right: &RiskFinding) -> Result<bool, String> {
        let left = serde_json::to_value(left)
            .map_err(|error| format!("序列化已有 finding 失败: {}", error))?;
        let right = serde_json::to_value(right)
            .map_err(|error| format!("序列化最终 finding 失败: {}", error))?;
        Ok(left == right)
    }

    fn build_graph_commit(
        state: &GraphState,
        graph_version: u64,
        affected: &[String],
    ) -> GraphCommit {
        let chunk_versions = deduplicate_strings(affected)
            .into_iter()
            .filter_map(|chunk_id| {
                state
                    .chunk_versions
                    .get(&chunk_id)
                    .copied()
                    .map(|version| (chunk_id, version))
            })
            .collect();
        GraphCommit {
            graph_version,
            chunk_versions,
        }
    }

    /// 原子提交单条款审查成功结果。
    pub fn commit_review_result(
        &self,
        attempt_id: &str,
        outcome: ReviewAttemptOutcome,
        findings: &[RiskFinding],
    ) -> Result<GraphCommit, String> {
        let nodes = normalize_provisional_findings(findings)?;
        match outcome {
            ReviewAttemptOutcome::Findings if nodes.is_empty() => {
                return Err("Findings 结果必须包含至少一个真实 finding".to_string());
            }
            ReviewAttemptOutcome::NoRisk if !nodes.is_empty() => {
                return Err("NoRisk 结果不得包含真实 finding".to_string());
            }
            _ => {}
        }

        let mut state = self
            .state
            .write()
            .map_err(|_| "SessionGraph 状态写锁已中毒".to_string())?;
        let (agent_id, attempt_chunk) = {
            let attempt = state
                .review_attempts
                .get(attempt_id)
                .ok_or_else(|| format!("审查尝试不存在: {}", attempt_id))?;
            if attempt.status != ReviewAttemptStatus::Started {
                return Err(format!("审查尝试 {} 已结束，禁止重复流转", attempt_id));
            }
            (attempt.agent_id.clone(), attempt.chunk_id.clone())
        };
        Self::validate_risk_conflicts(&state, &nodes)?;

        let mut affected = vec![attempt_chunk.clone()];
        let mut affected_law_refs = Vec::new();
        let mut changed_risk_ids = Vec::new();
        for node in &nodes {
            let (node_affected, node_law_refs, node_changed) =
                Self::upsert_provisional_node_in_state(&mut state, node);
            affected.extend(node_affected);
            affected_law_refs.extend(node_law_refs);
            if node_changed {
                changed_risk_ids.push(node.finding.risk_id.clone());
            }
        }
        affected.extend(Self::indexed_chunks_for_changes_in_state(
            &state,
            &deduplicate_strings(&changed_risk_ids),
            &deduplicate_strings(&affected_law_refs),
        ));
        let finding_ids = nodes
            .iter()
            .map(|node| node.finding.risk_id.clone())
            .collect::<Vec<_>>();
        {
            let attempt = state
                .review_attempts
                .get_mut(attempt_id)
                .expect("审查尝试已在修改前验证存在");
            attempt.status = ReviewAttemptStatus::Completed;
            attempt.outcome = Some(outcome);
            attempt.finding_ids = finding_ids;
            attempt.finished_at = Some(chrono::Utc::now().to_rfc3339());
        }
        push_unique(
            state.reviewed_by.entry(attempt_chunk.clone()).or_default(),
            agent_id,
        );
        let affected = deduplicate_strings(&affected);
        let graph_version = bump_versions(&mut state, affected.clone());
        Ok(Self::build_graph_commit(&state, graph_version, &affected))
    }

    /// 为没有 ReviewAttempt 的兼容路径幂等写入 provisional finding。
    pub fn upsert_provisional_findings(
        &self,
        findings: &[RiskFinding],
    ) -> Result<GraphCommit, String> {
        let nodes = normalize_provisional_findings(findings)?;
        let mut state = self
            .state
            .write()
            .map_err(|_| "SessionGraph 状态写锁已中毒".to_string())?;
        Self::validate_risk_conflicts(&state, &nodes)?;

        let mut affected = Vec::new();
        let mut affected_law_refs = Vec::new();
        let mut changed_risk_ids = Vec::new();
        let mut changed = false;
        for node in &nodes {
            let (node_affected, node_law_refs, node_changed) =
                Self::upsert_provisional_node_in_state(&mut state, node);
            affected.extend(node_affected);
            affected_law_refs.extend(node_law_refs);
            if node_changed {
                changed_risk_ids.push(node.finding.risk_id.clone());
            }
            changed |= node_changed;
        }
        if !changed {
            return Ok(GraphCommit {
                graph_version: state.graph_version,
                chunk_versions: HashMap::new(),
            });
        }
        affected.extend(Self::indexed_chunks_for_changes_in_state(
            &state,
            &deduplicate_strings(&changed_risk_ids),
            &deduplicate_strings(&affected_law_refs),
        ));
        let affected = deduplicate_strings(&affected);
        let graph_version = bump_versions(&mut state, affected.clone());
        Ok(Self::build_graph_commit(&state, graph_version, &affected))
    }

    /// 原子同步快照中的 Confirmed findings，并复用在线图规则重建全部风险派生索引。
    pub fn sync_snapshot_confirmed_findings(
        snapshot: &mut GraphSnapshot,
        findings: &[RiskFinding],
    ) -> Result<(), String> {
        let mut replacements = HashMap::with_capacity(findings.len());
        for finding in findings {
            if replacements
                .insert(finding.risk_id.clone(), finding.clone())
                .is_some()
            {
                return Err(format!(
                    "最终 findings 包含重复 risk_id: {}",
                    finding.risk_id
                ));
            }
        }
        for risk_id in replacements.keys() {
            let node = snapshot
                .risks
                .get(risk_id)
                .ok_or_else(|| format!("最终快照缺少 risk_id: {}", risk_id))?;
            if node.state != FindingState::Confirmed {
                return Err(format!("仅允许同步 Confirmed 风险节点: {}", risk_id));
            }
        }
        let confirmed_ids = snapshot
            .risks
            .iter()
            .filter(|(_, node)| node.state == FindingState::Confirmed)
            .map(|(risk_id, _)| risk_id.clone())
            .collect::<HashSet<_>>();
        let output_ids = replacements.keys().cloned().collect::<HashSet<_>>();
        if confirmed_ids != output_ids {
            let mut missing_output = confirmed_ids
                .difference(&output_ids)
                .cloned()
                .collect::<Vec<_>>();
            let mut missing_node = output_ids
                .difference(&confirmed_ids)
                .cloned()
                .collect::<Vec<_>>();
            missing_output.sort();
            missing_node.sort();
            return Err(format!(
                "最终 findings 与 Confirmed 节点不一致: 缺少输出={:?}, 缺少 Confirmed 节点={:?}",
                missing_output, missing_node
            ));
        }

        let mut updated = snapshot.clone();
        for (risk_id, finding) in replacements {
            let node = updated
                .risks
                .get_mut(&risk_id)
                .expect("完整校验后 Confirmed 节点必须存在");
            node.finding = finding;
            node.law_refs = node.finding.legal_basis.clone();
        }
        Self::rebuild_risk_indexes_parts(
            &updated.risks,
            &mut updated.has_risk,
            &mut updated.cites,
            &mut updated.cited_by,
            &mut updated.same_law,
            &mut updated.laws,
        );
        *snapshot = updated;
        Ok(())
    }

    /// 原子完成最终审计裁决，并记录 provisional 到终态的转换历史。
    ///
    /// `merged` 使用 source risk_id → target final risk_id；由于该映射不单独携带原因，
    /// 合并原因稳定记录为“合并至 {target}”。`rejected` 使用 source risk_id → 拒绝原因。
    pub fn finalize_audit(
        &self,
        final_findings: &[RiskFinding],
        merged: &HashMap<String, String>,
        rejected: &HashMap<String, String>,
    ) -> Result<GraphCommit, String> {
        let mut normalized_finals = HashMap::new();
        for finding in final_findings {
            if finding.no_risk {
                return Err("最终 finding 不得为 no_risk".to_string());
            }
            if finding.risk_id.trim().is_empty() {
                return Err("最终 finding 的 risk_id 不得为空".to_string());
            }
            if finding.finding_role == FindingRole::Hypothesis {
                return Err(format!(
                    "最终 finding {} 不得为 Hypothesis",
                    finding.risk_id
                ));
            }
            let mut normalized = finding.clone();
            normalized.clause_ids = deduplicate_strings(&finding.clause_ids);
            normalized.legal_basis = deduplicate_strings(&finding.legal_basis);
            if normalized.clause_ids.is_empty() {
                return Err(format!(
                    "最终 finding {} 的 clause_ids 不得为空",
                    finding.risk_id
                ));
            }
            if normalized_finals
                .insert(finding.risk_id.clone(), normalized)
                .is_some()
            {
                return Err(format!(
                    "最终 findings 包含重复 risk_id: {}",
                    finding.risk_id
                ));
            }
        }

        let confirmed_ids = normalized_finals.keys().cloned().collect::<HashSet<_>>();
        let merged_ids = merged.keys().cloned().collect::<HashSet<_>>();
        let rejected_ids = rejected.keys().cloned().collect::<HashSet<_>>();
        if !confirmed_ids.is_disjoint(&merged_ids)
            || !confirmed_ids.is_disjoint(&rejected_ids)
            || !merged_ids.is_disjoint(&rejected_ids)
        {
            return Err("confirmed、merged、rejected 三个集合必须互斥".to_string());
        }
        for (source, reason) in rejected {
            if reason.trim().is_empty() {
                return Err(format!("rejected finding {} 的裁决原因不得为空", source));
            }
        }

        let mut state = self
            .state
            .write()
            .map_err(|_| "SessionGraph 状态写锁已中毒".to_string())?;

        for risk_id in &confirmed_ids {
            if !state.risks.contains_key(risk_id) {
                return Err(format!("最终 finding 在工作图中不存在: {}", risk_id));
            }
            Self::validate_finding_clause_refs(
                &state,
                "最终 finding",
                &normalized_finals[risk_id],
            )?;
        }
        for (source, target) in merged {
            if !state.risks.contains_key(source) {
                return Err(format!("merged 源 finding 不存在: {}", source));
            }
            if !state.risks.contains_key(target) {
                return Err(format!("merged 目标 finding 不存在: {}", target));
            }
            if !confirmed_ids.contains(target) {
                return Err(format!("merged 目标必须属于最终 findings: {}", target));
            }
            Self::validate_finding_clause_refs(
                &state,
                "merged 源 finding",
                &state.risks[source].finding,
            )?;
        }
        for source in rejected.keys() {
            if !state.risks.contains_key(source) {
                return Err(format!("rejected 源 finding 不存在: {}", source));
            }
            Self::validate_finding_clause_refs(
                &state,
                "rejected 源 finding",
                &state.risks[source].finding,
            )?;
        }

        // 锁内先验证所有终态和幂等条件，再写任何字段，保证整批失败不留部分状态。
        for (risk_id, finding) in &normalized_finals {
            let node = &state.risks[risk_id];
            match node.state {
                FindingState::Provisional => {}
                FindingState::Confirmed
                    if Self::finding_content_matches(&node.finding, finding)?
                        && node.law_refs == finding.legal_basis
                        && node.merged_into.is_none()
                        && node.decision_reason.is_none() => {}
                FindingState::Confirmed => {
                    return Err(format!("已 confirmed finding {} 内容不同", risk_id));
                }
                _ => return Err(format!("finding {} 已处于冲突终态", risk_id)),
            }
        }
        for (source, target) in merged {
            let node = &state.risks[source];
            let reason = format!("合并至 {}", target);
            match node.state {
                FindingState::Provisional => {}
                FindingState::Merged
                    if node.merged_into.as_deref() == Some(target.as_str())
                        && node.decision_reason.as_deref() == Some(reason.as_str()) => {}
                _ => return Err(format!("finding {} 已处于冲突终态", source)),
            }
        }
        for (source, reason) in rejected {
            let node = &state.risks[source];
            match node.state {
                FindingState::Provisional => {}
                FindingState::Rejected
                    if node.decision_reason.as_deref() == Some(reason.as_str())
                        && node.merged_into.is_none() => {}
                _ => return Err(format!("finding {} 已处于冲突终态", source)),
            }
        }

        let existing_confirmed = state
            .risks
            .iter()
            .filter(|(_, node)| node.state == FindingState::Confirmed)
            .map(|(risk_id, _)| risk_id.clone())
            .collect::<HashSet<_>>();
        let existing_merged = state
            .risks
            .iter()
            .filter(|(_, node)| node.state == FindingState::Merged)
            .filter_map(|(risk_id, node)| {
                node.merged_into
                    .as_ref()
                    .map(|target| (risk_id.clone(), target.clone()))
            })
            .collect::<HashMap<_, _>>();
        let existing_rejected = state
            .risks
            .iter()
            .filter(|(_, node)| node.state == FindingState::Rejected)
            .filter_map(|(risk_id, node)| {
                node.decision_reason
                    .as_ref()
                    .map(|reason| (risk_id.clone(), reason.clone()))
            })
            .collect::<HashMap<_, _>>();
        let has_existing_terminal = !existing_confirmed.is_empty()
            || !existing_merged.is_empty()
            || !existing_rejected.is_empty();
        if has_existing_terminal
            && (existing_confirmed != confirmed_ids
                || existing_merged != *merged
                || existing_rejected != *rejected)
        {
            return Err("最终裁决重试必须完整包含所有已有终态 finding".to_string());
        }

        let mut changed_ids = HashSet::new();
        let mut affected = HashSet::new();
        for risk_id in confirmed_ids
            .iter()
            .chain(merged_ids.iter())
            .chain(rejected_ids.iter())
        {
            let node = &state.risks[risk_id];
            if node.state == FindingState::Provisional {
                changed_ids.insert(risk_id.clone());
                affected.extend(node.finding.clause_ids.iter().cloned());
            }
        }
        if changed_ids.is_empty() {
            return Ok(GraphCommit {
                graph_version: state.graph_version,
                chunk_versions: HashMap::new(),
            });
        }

        let old_has_risk = state.has_risk.clone();
        let old_same_law = state.same_law.clone();
        let old_agent_same_law = Self::agent_visible_same_law_in_state(&state);
        let decided_at = chrono::Utc::now().to_rfc3339();
        let mut new_transitions = Vec::new();
        for (risk_id, finding) in normalized_finals {
            if !changed_ids.contains(&risk_id) {
                continue;
            }
            let node = state
                .risks
                .get_mut(&risk_id)
                .expect("最终 finding 已在写入前验证存在");
            node.finding = finding;
            node.law_refs = node.finding.legal_basis.clone();
            node.state = FindingState::Confirmed;
            node.merged_into = None;
            node.decision_reason = None;
            affected.extend(node.finding.clause_ids.iter().cloned());
            new_transitions.push(FindingTransition {
                risk_id,
                from: FindingState::Provisional,
                to: FindingState::Confirmed,
                reason: "最终审计确认".to_string(),
                merged_into: None,
                decided_at: decided_at.clone(),
            });
        }
        for (source, target) in merged {
            if !changed_ids.contains(source) {
                continue;
            }
            let reason = format!("合并至 {}", target);
            let node = state
                .risks
                .get_mut(source)
                .expect("merged 源已在写入前验证存在");
            node.state = FindingState::Merged;
            node.merged_into = Some(target.clone());
            node.decision_reason = Some(reason.clone());
            new_transitions.push(FindingTransition {
                risk_id: source.clone(),
                from: FindingState::Provisional,
                to: FindingState::Merged,
                reason,
                merged_into: Some(target.clone()),
                decided_at: decided_at.clone(),
            });
        }
        for (source, reason) in rejected {
            if !changed_ids.contains(source) {
                continue;
            }
            let node = state
                .risks
                .get_mut(source)
                .expect("rejected 源已在写入前验证存在");
            node.state = FindingState::Rejected;
            node.merged_into = None;
            node.decision_reason = Some(reason.clone());
            new_transitions.push(FindingTransition {
                risk_id: source.clone(),
                from: FindingState::Provisional,
                to: FindingState::Rejected,
                reason: reason.clone(),
                merged_into: None,
                decided_at: decided_at.clone(),
            });
        }
        state.finding_transitions.extend(new_transitions);
        state
            .finding_transitions
            .sort_by(|left, right| left.risk_id.cmp(&right.risk_id));

        Self::rebuild_risk_indexes(&mut state);
        let new_agent_same_law = Self::agent_visible_same_law_in_state(&state);
        for chunk_id in old_has_risk.keys().chain(state.has_risk.keys()) {
            if old_has_risk.get(chunk_id) != state.has_risk.get(chunk_id) {
                affected.insert(chunk_id.clone());
            }
        }
        for chunk_id in old_same_law.keys().chain(state.same_law.keys()) {
            if old_same_law.get(chunk_id) != state.same_law.get(chunk_id) {
                affected.insert(chunk_id.clone());
            }
        }
        affected.extend(changed_agent_same_law_chunks(
            &old_agent_same_law,
            &new_agent_same_law,
        ));
        let mut affected = affected.into_iter().collect::<Vec<_>>();
        affected.sort();
        let graph_version = bump_versions(&mut state, affected.clone());
        Ok(Self::build_graph_commit(&state, graph_version, &affected))
    }

    /// 将审查尝试标记为失败；失败尝试不计入 reviewed_by。
    pub fn fail_review_attempt(
        &self,
        attempt_id: &str,
        error_code: ReviewAttemptErrorCode,
        error_message: &str,
    ) -> Result<(), String> {
        let mut state = self
            .state
            .write()
            .map_err(|_| "SessionGraph 状态写锁已中毒".to_string())?;
        let chunk_id = {
            let attempt = state
                .review_attempts
                .get_mut(attempt_id)
                .ok_or_else(|| format!("审查尝试不存在: {}", attempt_id))?;
            if attempt.status != ReviewAttemptStatus::Started {
                return Err(format!("审查尝试 {} 已结束，禁止重复流转", attempt_id));
            }
            attempt.status = ReviewAttemptStatus::Failed;
            attempt.error_code = Some(error_code);
            attempt.error_message = Some(error_message.to_string());
            attempt.finished_at = Some(chrono::Utc::now().to_rfc3339());
            attempt.chunk_id.clone()
        };
        bump_versions(&mut state, [chunk_id]);
        Ok(())
    }

    /// 批量收口指定 Agent、指定条款中仍处于 started 的审查尝试。
    pub fn fail_started_attempts(
        &self,
        agent_id: &AgentId,
        chunk_ids: &[String],
        error_code: ReviewAttemptErrorCode,
        error_message: &str,
    ) -> Result<usize, String> {
        let mut state = self
            .state
            .write()
            .map_err(|_| "SessionGraph 状态写锁已中毒".to_string())?;
        let finished_at = chrono::Utc::now().to_rfc3339();
        let mut closed = 0;
        let mut affected_chunks = Vec::new();
        for attempt in state.review_attempts.values_mut() {
            if attempt.status == ReviewAttemptStatus::Started
                && &attempt.agent_id == agent_id
                && chunk_ids.contains(&attempt.chunk_id)
            {
                attempt.status = ReviewAttemptStatus::Failed;
                attempt.error_code = Some(error_code);
                attempt.error_message = Some(error_message.to_string());
                attempt.finished_at = Some(finished_at.clone());
                affected_chunks.push(attempt.chunk_id.clone());
                closed += 1;
            }
        }
        if closed > 0 {
            bump_versions(&mut state, affected_chunks);
        }
        Ok(closed)
    }

    // ── 写入 (Agent 调用) ──────────────────────────────────────

    /// 添加条款节点。
    pub fn add_chunk(&self, chunk: ChunkNode) {
        if let Ok(mut state) = self.state.write() {
            let chunk_id = chunk.chunk_id.clone();
            state.chunks.insert(chunk_id.clone(), chunk);
            bump_versions(&mut state, [chunk_id]);
        }
    }

    /// 批量添加条款节点（Coordinator PRELOAD 阶段）。
    pub fn add_chunks(&self, chunks: Vec<ChunkNode>) {
        if let Ok(mut state) = self.state.write() {
            let mut chunk_ids = Vec::with_capacity(chunks.len());
            for c in chunks {
                chunk_ids.push(c.chunk_id.clone());
                state.chunks.insert(c.chunk_id.clone(), c);
            }
            if !chunk_ids.is_empty() {
                bump_versions(&mut state, chunk_ids);
            }
        }
    }

    /// 添加风险节点。
    pub fn add_risk(&self, mut risk: RiskNode) {
        // 从 RiskFinding.legal_basis 提取法条引用
        risk.law_refs = risk.finding.legal_basis.clone();
        let risk_id = risk.finding.risk_id.clone();
        if let Ok(mut state) = self.state.write() {
            if !Self::is_valid_legacy_risk_input(&risk) || Self::is_terminal_risk(&state, &risk_id)
            {
                return;
            }
            let before = Self::agent_visible_same_law_in_state(&state);
            let chunk_ids = risk.finding.clause_ids.clone();
            state.risks.insert(risk_id.clone(), risk);
            let after = Self::agent_visible_same_law_in_state(&state);
            let mut affected = chunk_ids;
            affected.extend(Self::indexed_chunks_for_changes_in_state(
                &state,
                std::slice::from_ref(&risk_id),
                &[],
            ));
            affected.extend(changed_agent_same_law_chunks(&before, &after));
            bump_versions(&mut state, affected);
        }
    }

    /// 添加 has_risk 边（chunk → risk）。
    pub fn add_has_risk(&self, chunk_id: &str, risk_id: &str) {
        if let Ok(mut state) = self.state.write() {
            if Self::is_terminal_risk(&state, risk_id) {
                return;
            }
            let before = Self::agent_visible_same_law_in_state(&state);
            if !push_unique(
                state.has_risk.entry(chunk_id.to_string()).or_default(),
                risk_id.to_string(),
            ) {
                return;
            }
            let after = Self::agent_visible_same_law_in_state(&state);
            let mut affected = vec![chunk_id.to_string()];
            affected.extend(changed_agent_same_law_chunks(&before, &after));
            bump_versions(&mut state, affected);
        }
    }

    /// 记录 Agent 已审查某条款。
    pub fn add_reviewed_by(&self, chunk_id: &str, agent: AgentId) {
        if let Ok(mut state) = self.state.write()
            && push_unique(
                state.reviewed_by.entry(chunk_id.to_string()).or_default(),
                agent,
            )
        {
            bump_versions(&mut state, [chunk_id.to_string()]);
        }
    }

    /// 添加 linked_to 边（条款间关联）。
    pub fn add_linked_to(&self, from: &str, to: &str, reason: &str) {
        if let Ok(mut state) = self.state.write()
            && push_unique(
                state.linked_to.entry(from.to_string()).or_default(),
                LinkedChunk {
                    chunk_id: to.to_string(),
                    reason: reason.to_string(),
                },
            )
        {
            bump_versions(&mut state, [from.to_string()]);
        }
    }

    /// 写入 Agent 节点（Coordinator PRELOAD 阶段调用）。
    pub fn add_agent(&self, agent: AgentNode) {
        let agent_id = agent.agent_id.clone();
        if let Ok(mut state) = self.state.write() {
            state.agents.insert(agent_id, agent);
            bump_versions(&mut state, std::iter::empty::<String>());
        }
    }

    /// 双向写入矛盾边（Agent 调用 search_contradiction 工具时触发）。
    pub fn add_contradicts(&self, chunk_a: &str, chunk_b: &str, reason: &str) {
        if let Ok(mut state) = self.state.write() {
            let forward = push_unique(
                state.contradicts.entry(chunk_a.to_string()).or_default(),
                (chunk_b.to_string(), reason.to_string()),
            );
            let reverse = push_unique(
                state.contradicts.entry(chunk_b.to_string()).or_default(),
                (chunk_a.to_string(), reason.to_string()),
            );
            if forward || reverse {
                bump_versions(&mut state, [chunk_a.to_string(), chunk_b.to_string()]);
            }
        }
    }

    /// 查询某条款的矛盾关系。
    pub fn query_contradictions(&self, chunk_id: &str) -> Vec<(String, String)> {
        self.state
            .read()
            .ok()
            .and_then(|state| state.contradicts.get(chunk_id).cloned())
            .unwrap_or_default()
    }

    /// 查询某条款的 same_law 关联。
    pub fn query_same_law_edges(&self, chunk_id: &str) -> Vec<String> {
        self.state
            .read()
            .ok()
            .map(|state| {
                agent_visible_same_law_chunks(
                    &state.risks,
                    &state.has_risk,
                    &state.cites,
                    &state.cited_by,
                    chunk_id,
                )
            })
            .unwrap_or_default()
    }

    /// 自动推导 same_law 边：扫描 cited_by → has_risk，找到共享同一法条的 chunk。
    ///
    /// 在 `add_risk_with_edges()` 末尾调用。
    fn derive_same_law_edges_in_state(
        state: &mut GraphState,
        law_refs: &[String],
        chunk_id: &str,
    ) -> Vec<String> {
        if law_refs.is_empty() {
            return Vec::new();
        }

        // 收集引用相同法条的其他 risk_id
        let mut related_risk_ids: Vec<String> = Vec::new();
        for law_ref in law_refs {
            if let Some(risk_ids) = state.cited_by.get(law_ref) {
                for rid in risk_ids {
                    if !related_risk_ids.contains(rid) {
                        related_risk_ids.push(rid.clone());
                    }
                }
            }
        }

        if related_risk_ids.is_empty() {
            return Vec::new();
        }

        let related_chunks = related_risk_ids
            .iter()
            .filter_map(|risk_id| state.risks.get(risk_id))
            .flat_map(|risk| risk.finding.clause_ids.iter().cloned())
            .filter(|other_chunk_id| other_chunk_id != chunk_id)
            .collect::<HashSet<_>>();
        let mut affected = Vec::new();
        for other_chunk_id in related_chunks {
            let forward = push_unique(
                state.same_law.entry(chunk_id.to_string()).or_default(),
                other_chunk_id.clone(),
            );
            let reverse = push_unique(
                state.same_law.entry(other_chunk_id.clone()).or_default(),
                chunk_id.to_string(),
            );
            if forward || reverse {
                affected.push(chunk_id.to_string());
                affected.push(other_chunk_id);
            }
        }
        affected
    }

    fn add_law_edges_in_state(state: &mut GraphState, risk_id: &str, law_refs: &[String]) {
        for law_ref in law_refs {
            push_unique(
                state.cites.entry(risk_id.to_string()).or_default(),
                law_ref.clone(),
            );
            push_unique(
                state.cited_by.entry(law_ref.clone()).or_default(),
                risk_id.to_string(),
            );
            state
                .laws
                .entry(law_ref.clone())
                .or_insert_with(|| LawNode {
                    law_id: law_ref.clone(),
                    article_no: law_ref.clone(),
                    title: String::new(),
                });
        }
    }

    /// 添加 cites 边 + cited_by 反向索引（单条法条引用）。
    pub fn add_cites(&self, risk_id: &str, law_ref: &str) {
        if let Ok(mut state) = self.state.write() {
            if Self::is_terminal_risk(&state, risk_id) {
                return;
            }
            let before = Self::agent_visible_same_law_in_state(&state);
            let cites_changed = push_unique(
                state.cites.entry(risk_id.to_string()).or_default(),
                law_ref.to_string(),
            );
            let cited_by_changed = push_unique(
                state.cited_by.entry(law_ref.to_string()).or_default(),
                risk_id.to_string(),
            );
            if cites_changed || cited_by_changed {
                let affected = state
                    .risks
                    .get(risk_id)
                    .map(|risk| risk.finding.clause_ids.clone())
                    .unwrap_or_default();
                let after = Self::agent_visible_same_law_in_state(&state);
                let mut affected = affected;
                affected.extend(changed_agent_same_law_chunks(&before, &after));
                bump_versions(&mut state, affected);
            }
        }
    }

    /// 原子写入 Risk 节点 + has_risk 边 + cites 边。
    pub fn add_risk_with_edges(&self, mut risk: RiskNode, chunk_id: &str) {
        let risk_id = risk.finding.risk_id.clone();
        let law_refs = risk.finding.legal_basis.clone();
        risk.law_refs = law_refs.clone();
        if let Ok(mut state) = self.state.write() {
            if !Self::is_valid_legacy_risk_input(&risk) || Self::is_terminal_risk(&state, &risk_id)
            {
                return;
            }
            let before = Self::agent_visible_same_law_in_state(&state);
            state.risks.insert(risk_id.clone(), risk);
            push_unique(
                state.has_risk.entry(chunk_id.to_string()).or_default(),
                risk_id.clone(),
            );
            Self::add_law_edges_in_state(&mut state, &risk_id, &law_refs);
            let mut affected = vec![chunk_id.to_string()];
            affected.extend(Self::derive_same_law_edges_in_state(
                &mut state, &law_refs, chunk_id,
            ));
            let after = Self::agent_visible_same_law_in_state(&state);
            affected.extend(Self::indexed_chunks_for_changes_in_state(
                &state,
                std::slice::from_ref(&risk_id),
                &[],
            ));
            affected.extend(changed_agent_same_law_chunks(&before, &after));
            bump_versions(&mut state, affected);
        }
    }

    /// 写入 Hypothesis（不创建 Law 节点和 same_law 边）。
    pub fn add_hypothesis(&self, mut risk: RiskNode, chunk_id: &str) {
        let risk_id = risk.finding.risk_id.clone();
        let law_refs = risk.finding.legal_basis.clone();
        risk.law_refs = law_refs.clone();
        if let Ok(mut state) = self.state.write() {
            if !Self::is_valid_legacy_risk_input(&risk) || Self::is_terminal_risk(&state, &risk_id)
            {
                return;
            }
            let before = Self::agent_visible_same_law_in_state(&state);
            state.risks.insert(risk_id.clone(), risk);
            push_unique(
                state.has_risk.entry(chunk_id.to_string()).or_default(),
                risk_id.clone(),
            );
            for law_ref in law_refs {
                push_unique(state.cites.entry(risk_id.clone()).or_default(), law_ref);
            }
            let after = Self::agent_visible_same_law_in_state(&state);
            let mut affected = vec![chunk_id.to_string()];
            affected.extend(Self::indexed_chunks_for_changes_in_state(
                &state,
                std::slice::from_ref(&risk_id),
                &[],
            ));
            affected.extend(changed_agent_same_law_chunks(&before, &after));
            bump_versions(&mut state, affected);
        }
    }

    /// 查询所有 Hypothesis（BlindSpot 用）。
    pub fn get_hypotheses(&self) -> Vec<RiskFinding> {
        self.state
            .read()
            .ok()
            .map(|state| {
                state
                    .risks
                    .values()
                    .filter(|risk| risk.finding.finding_role == FindingRole::Hypothesis)
                    .map(|risk| risk.finding.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    // ── 预搜索结果缓存 ──────────────────────────────────────

    /// 批量写入条款的预搜索结果。
    pub fn cache_search_results(&self, chunk_id: &str, entries: Vec<SearchCacheEntry>) {
        if let Ok(mut cache) = self.search_results.write() {
            cache.insert(chunk_id.to_string(), entries);
        }
    }

    /// 查询条款的预搜索结果。
    pub fn get_search_results_for_clause(&self, chunk_id: &str) -> Vec<SearchCacheEntry> {
        self.search_results
            .read()
            .ok()
            .and_then(|cache| cache.get(chunk_id).cloned())
            .unwrap_or_default()
    }

    /// 检查是否有预搜索结果。
    pub fn has_search_results(&self, chunk_id: &str) -> bool {
        self.search_results
            .read()
            .ok()
            .map(|cache| cache.contains_key(chunk_id))
            .unwrap_or(false)
    }

    /// Scout 阶段是否已完成。
    pub fn is_scout_complete(&self) -> bool {
        self.scout_complete.load(Ordering::Acquire)
    }

    /// 标记 Scout 阶段已完成。
    pub fn mark_scout_complete(&self) {
        self.scout_complete.store(true, Ordering::Release);
    }

    // ── 查询 (Agent 每轮 ReAct 调用) ──────────────────────────

    fn build_clause_context(state: &GraphState, chunk_id: &str) -> ClauseContext {
        let reviewed_by = state.reviewed_by.get(chunk_id).cloned().unwrap_or_default();
        let risk_ids = state.has_risk.get(chunk_id).cloned().unwrap_or_default();
        let risks = risk_ids
            .iter()
            .filter_map(|risk_id| state.risks.get(risk_id))
            .filter(|risk| is_agent_visible_risk(risk))
            .map(|risk| risk.finding.clone())
            .collect();
        let linked_chunks = state.linked_to.get(chunk_id).cloned().unwrap_or_default();
        let same_law_chunks = agent_visible_same_law_chunks(
            &state.risks,
            &state.has_risk,
            &state.cites,
            &state.cited_by,
            chunk_id,
        );
        let contradictions = state
            .contradicts
            .get(chunk_id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|(other_id, reason)| LinkedChunk {
                chunk_id: other_id,
                reason,
            })
            .collect();

        ClauseContext {
            chunk_id: chunk_id.to_string(),
            reviewed_by,
            risks,
            linked_chunks,
            same_law_chunks,
            contradictions,
        }
    }

    /// 查询某个 Chunk 的完整上下文："谁审过这条？发现了什么风险？跟哪些条款有关联？"
    pub fn query_clause_context(&self, chunk_id: &str) -> ClauseContext {
        self.state
            .read()
            .ok()
            .map(|state| Self::build_clause_context(&state, chunk_id))
            .unwrap_or_else(|| ClauseContext {
                chunk_id: chunk_id.to_string(),
                reviewed_by: Vec::new(),
                risks: Vec::new(),
                linked_chunks: Vec::new(),
                same_law_chunks: Vec::new(),
                contradictions: Vec::new(),
            })
    }

    /// 条款版本未变化时返回 None；变化时返回同一读锁下构建的完整上下文。
    pub fn query_clause_context_since(
        &self,
        chunk_id: &str,
        known_version: Option<u64>,
    ) -> Option<VersionedClauseContext> {
        let state = self.state.read().ok()?;
        let version = state.chunk_versions.get(chunk_id).copied().unwrap_or(0);
        if known_version == Some(version) {
            return None;
        }
        Some(VersionedClauseContext {
            version,
            context: Self::build_clause_context(&state, chunk_id),
        })
    }

    /// 查询引用同一法条的所有 chunk_id，以 cites/cited_by 和 has_risk 双向事务索引为准。
    pub fn query_same_law_chunks(&self, law_ref: &str) -> Vec<String> {
        let mut result = Vec::new();
        if let Ok(state) = self.state.read()
            && let Some(risk_ids) = state.cited_by.get(law_ref)
        {
            let visible_risk_ids = risk_ids
                .iter()
                .filter(|risk_id| {
                    let cites_law = state
                        .cites
                        .get(*risk_id)
                        .is_some_and(|law_refs| law_refs.iter().any(|item| item == law_ref));
                    let is_visible = state.risks.get(*risk_id).is_some_and(|risk| {
                        is_agent_visible_risk(risk)
                            && risk.finding.finding_role != FindingRole::Hypothesis
                    });
                    cites_law && is_visible
                })
                .cloned()
                .collect::<HashSet<_>>();
            for (chunk_id, indexed_risk_ids) in &state.has_risk {
                if indexed_risk_ids
                    .iter()
                    .any(|risk_id| visible_risk_ids.contains(risk_id))
                {
                    result.push(chunk_id.clone());
                }
            }
            result.sort();
        }
        result
    }

    /// 获取完整图快照（BlindSpot / 审计用）。
    pub fn snapshot(&self) -> GraphSnapshot {
        self.state
            .read()
            .ok()
            .map(|state| GraphSnapshot {
                chunks: state.chunks.clone(),
                risks: state.risks.clone(),
                has_risk: state.has_risk.clone(),
                reviewed_by: state.reviewed_by.clone(),
                linked_to: state.linked_to.clone(),
                cites: state.cites.clone(),
                cited_by: state.cited_by.clone(),
                agents: state.agents.clone(),
                laws: state.laws.clone(),
                cases: state.cases.clone(),
                contradicts: state.contradicts.clone(),
                same_law: state.same_law.clone(),
                review_attempts: state.review_attempts.clone(),
                finding_transitions: state.finding_transitions.clone(),
                graph_version: state.graph_version,
                chunk_versions: state.chunk_versions.clone(),
            })
            .unwrap_or_default()
    }

    /// 获取图中的条款总数。
    pub fn chunk_count(&self) -> usize {
        self.state
            .read()
            .ok()
            .map(|state| state.chunks.len())
            .unwrap_or(0)
    }

    /// 获取图中的风险总数。
    pub fn risk_count(&self) -> usize {
        self.state
            .read()
            .ok()
            .map(|state| state.risks.len())
            .unwrap_or(0)
    }
}

impl Default for SessionGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_chunk(id: &str) -> ChunkNode {
        ChunkNode {
            chunk_id: id.to_string(),
            section_path: vec!["测试章节".to_string()],
            page_start: 0,
            page_end: 1,
            text_preview: "测试条款内容...".to_string(),
            tier: RiskTier::Medium,
        }
    }

    fn make_test_risk(risk_id: &str, chunk_id: &str) -> RiskNode {
        RiskNode {
            finding: RiskFinding {
                risk_id: risk_id.to_string(),
                clause_ids: vec![chunk_id.to_string()],
                block_ids: Vec::new(),
                highlight_rects: Vec::new(),
                agent: "TestAgent".to_string(),
                no_risk: false,
                severity: RiskSeverity::High,
                is_critical: false,
                critical_reason: String::new(),
                risk_type: "测试风险".to_string(),
                category_code: "TEST_RISK".to_string(),
                source_quote: "测试原文".to_string(),
                legal_basis: vec!["《测试法》第1条".to_string()],
                case_refs: Vec::new(),
                reason: "测试理由".to_string(),
                suggestion: "测试建议".to_string(),
                confidence: 0.9,
                initial_tier: RiskTier::Medium,
                final_tier: RiskTier::High,
                tier_escalated: true,
                truncated: false,
                suggested_agent: None,
                citations: Vec::new(),
                finding_role: FindingRole::default(),
                knowledge_source: String::new(),
                verification_required: Vec::new(),
                hypothesized_by: Vec::new(),
                verified_by: Vec::new(),
                evidence_verdict: None,
                verifier_reason: None,
                page_number: None,
                section_path: None,
                context: None,
            },
            law_refs: vec!["《测试法》第1条".to_string()],
            state: FindingState::Provisional,
            merged_into: None,
            decision_reason: None,
        }
    }

    #[test]
    fn test_add_chunk_and_query() {
        let g = SessionGraph::new();
        g.add_chunk(make_test_chunk("ch_001"));
        assert_eq!(g.chunk_count(), 1);

        let ctx = g.query_clause_context("ch_001");
        assert_eq!(ctx.chunk_id, "ch_001");
        assert!(ctx.reviewed_by.is_empty());
        assert!(ctx.risks.is_empty());
    }

    #[test]
    fn test_add_risk_with_edges() {
        let g = SessionGraph::new();
        g.add_chunk(make_test_chunk("ch_001"));
        let risk = make_test_risk("R_001", "ch_001");
        g.add_risk_with_edges(risk, "ch_001");

        let ctx = g.query_clause_context("ch_001");
        assert_eq!(ctx.risks.len(), 1);
        assert_eq!(ctx.risks[0].risk_id, "R_001");
        assert!(ctx.has_prior_risks());
    }

    #[test]
    fn test_reviewed_by() {
        let g = SessionGraph::new();
        g.add_chunk(make_test_chunk("ch_001"));
        g.add_reviewed_by("ch_001", AgentId::FactCheck);
        g.add_reviewed_by("ch_001", AgentId::Procedure);
        // 重复添加不应重复
        g.add_reviewed_by("ch_001", AgentId::FactCheck);

        let ctx = g.query_clause_context("ch_001");
        assert_eq!(ctx.reviewed_by.len(), 2);
    }

    #[test]
    fn test_review_attempt_completed_no_risk_counts_as_reviewed() {
        let graph = SessionGraph::new();
        graph.add_chunk(make_test_chunk("ch_001"));
        let attempt_id = graph
            .start_review_attempt(AgentId::FactCheck, "ch_001")
            .expect("应创建审查尝试");

        graph
            .complete_review_attempt(&attempt_id, ReviewAttemptOutcome::NoRisk, Vec::new())
            .expect("无风险也应正常完成");

        let snapshot = graph.snapshot();
        let attempt = &snapshot.review_attempts[&attempt_id];
        assert_eq!(attempt.status, ReviewAttemptStatus::Completed);
        assert_eq!(attempt.outcome, Some(ReviewAttemptOutcome::NoRisk));
        assert_eq!(snapshot.reviewed_by["ch_001"], vec![AgentId::FactCheck]);
    }

    #[test]
    fn complete_review_attempt_rejects_findings_without_ids() {
        let graph = SessionGraph::new();
        graph.add_chunk(make_test_chunk("ch_001"));
        let attempt_id = graph
            .start_review_attempt(AgentId::FactCheck, "ch_001")
            .expect("应创建审查尝试");

        let error = graph
            .complete_review_attempt(&attempt_id, ReviewAttemptOutcome::Findings, Vec::new())
            .expect_err("Findings 必须通过原子提交接口完成");

        assert!(error.contains("commit_review_result"));
        let snapshot = graph.snapshot();
        assert_eq!(
            snapshot.review_attempts[&attempt_id].status,
            ReviewAttemptStatus::Started
        );
        assert!(!snapshot.reviewed_by.contains_key("ch_001"));
    }

    #[test]
    fn test_review_attempt_no_risk_rejects_finding_ids() {
        let graph = SessionGraph::new();
        graph.add_chunk(make_test_chunk("ch_001"));
        let attempt_id = graph
            .start_review_attempt(AgentId::FactCheck, "ch_001")
            .expect("应创建审查尝试");

        let error = graph
            .complete_review_attempt(
                &attempt_id,
                ReviewAttemptOutcome::NoRisk,
                vec!["R_001".to_string()],
            )
            .expect_err("NoRisk 结果不得关联 finding_id");

        assert!(error.contains("finding_ids"));
        let snapshot = graph.snapshot();
        assert_eq!(
            snapshot.review_attempts[&attempt_id].status,
            ReviewAttemptStatus::Started
        );
        assert!(!snapshot.reviewed_by.contains_key("ch_001"));
    }

    #[test]
    fn complete_review_attempt_rejects_findings_and_keeps_attempt_started() {
        let graph = SessionGraph::new();
        graph.add_chunk(make_test_chunk("ch_001"));
        let attempt_id = graph
            .start_review_attempt(AgentId::FactCheck, "ch_001")
            .expect("应创建审查尝试");

        let error = graph
            .complete_review_attempt(
                &attempt_id,
                ReviewAttemptOutcome::Findings,
                vec!["R_001".to_string()],
            )
            .expect_err("Findings 必须通过原子提交接口完成");

        let snapshot = graph.snapshot();
        let attempt = &snapshot.review_attempts[&attempt_id];
        assert!(error.contains("commit_review_result"));
        assert_eq!(attempt.status, ReviewAttemptStatus::Started);
        assert!(attempt.outcome.is_none());
        assert!(attempt.finding_ids.is_empty());
        assert!(!snapshot.reviewed_by.contains_key("ch_001"));
    }

    #[test]
    fn test_failed_review_attempt_does_not_count_as_reviewed() {
        let graph = SessionGraph::new();
        graph.add_chunk(make_test_chunk("ch_001"));
        let attempt_id = graph
            .start_review_attempt(AgentId::FactCheck, "ch_001")
            .expect("应创建审查尝试");

        graph
            .fail_review_attempt(
                &attempt_id,
                ReviewAttemptErrorCode::ClauseTimeout,
                "条款审查超时",
            )
            .expect("应记录失败");

        let snapshot = graph.snapshot();
        assert_eq!(
            snapshot.review_attempts[&attempt_id].status,
            ReviewAttemptStatus::Failed
        );
        assert!(!snapshot.reviewed_by.contains_key("ch_001"));
    }

    #[test]
    fn test_reconcile_started_attempts_only_closes_selected_agent_and_chunks() {
        let graph = SessionGraph::new();
        let target = graph
            .start_review_attempt(AgentId::FactCheck, "ch_001")
            .expect("应创建目标尝试");
        let untouched = graph
            .start_review_attempt(AgentId::Procedure, "ch_002")
            .expect("应创建非目标尝试");

        let closed = graph
            .fail_started_attempts(
                &AgentId::FactCheck,
                &["ch_001".to_string()],
                ReviewAttemptErrorCode::TaskCancelled,
                "执行阶段取消",
            )
            .expect("应收口目标尝试");

        let snapshot = graph.snapshot();
        assert_eq!(closed, 1);
        assert_eq!(
            snapshot.review_attempts[&target].status,
            ReviewAttemptStatus::Failed
        );
        assert_eq!(
            snapshot.review_attempts[&untouched].status,
            ReviewAttemptStatus::Started
        );
    }

    #[test]
    fn test_query_same_law_chunks() {
        let g = SessionGraph::new();
        g.add_chunk(make_test_chunk("ch_001"));
        g.add_chunk(make_test_chunk("ch_002"));

        let risk1 = make_test_risk("R_001", "ch_001");
        g.add_risk_with_edges(risk1, "ch_001");

        // 同一个法条被另一个 risk 引用
        let mut risk2 = make_test_risk("R_002", "ch_002");
        risk2.finding.legal_basis = vec!["《测试法》第1条".to_string()];
        risk2.law_refs = vec!["《测试法》第1条".to_string()];
        g.add_risk_with_edges(risk2, "ch_002");

        let same_law = g.query_same_law_chunks("《测试法》第1条");
        assert!(same_law.contains(&"ch_001".to_string()));
        assert!(same_law.contains(&"ch_002".to_string()));
    }

    #[test]
    fn test_snapshot() {
        let g = SessionGraph::new();
        g.add_chunk(make_test_chunk("ch_001"));
        g.add_risk_with_edges(make_test_risk("R_001", "ch_001"), "ch_001");
        g.add_reviewed_by("ch_001", AgentId::FactCheck);

        let snap = g.snapshot();
        assert_eq!(snap.chunks.len(), 1);
        assert_eq!(snap.risks.len(), 1);
        assert_eq!(snap.has_risk.len(), 1);
        assert_eq!(snap.reviewed_by.len(), 1);
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
        assert_eq!(
            snapshot.review_attempts[&attempt_id].finding_ids,
            vec!["R_001"]
        );
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

    fn add_provisional_risk(graph: &SessionGraph, risk_id: &str, chunk_id: &str) {
        graph.add_chunk(make_test_chunk(chunk_id));
        graph
            .upsert_provisional_findings(&[make_test_risk(risk_id, chunk_id).finding])
            .expect("应写入 provisional finding");
    }

    fn make_finalized_graph() -> SessionGraph {
        let graph = SessionGraph::new();
        add_provisional_risk(&graph, "R_CONFIRMED", "ch_confirmed");
        add_provisional_risk(&graph, "R_MERGED", "ch_merged");
        add_provisional_risk(&graph, "R_REJECTED", "ch_rejected");
        graph
            .finalize_audit(
                &[make_test_risk("R_CONFIRMED", "ch_confirmed").finding],
                &HashMap::from([("R_MERGED".to_string(), "R_CONFIRMED".to_string())]),
                &HashMap::from([("R_REJECTED".to_string(), "证据不足".to_string())]),
            )
            .expect("测试图最终裁决应成功");
        graph
    }

    fn assert_terminal_legacy_write_is_noop(write: impl Fn(&SessionGraph, &str)) {
        let graph = make_finalized_graph();
        for risk_id in ["R_CONFIRMED", "R_MERGED", "R_REJECTED"] {
            let before = serde_json::to_value(graph.snapshot()).expect("裁决前快照应可序列化");
            write(&graph, risk_id);
            let after = serde_json::to_value(graph.snapshot()).expect("裁决后快照应可序列化");
            assert_eq!(after, before, "legacy 写入口不得修改终态 finding {risk_id}");
        }
    }

    #[test]
    fn legacy_public_writes_cannot_mutate_terminal_findings() {
        assert_terminal_legacy_write_is_noop(|graph, risk_id| {
            graph.add_risk(make_test_risk(risk_id, "ch_overwrite"));
        });
        assert_terminal_legacy_write_is_noop(|graph, risk_id| {
            graph.add_has_risk("ch_extra", risk_id);
        });
        assert_terminal_legacy_write_is_noop(|graph, risk_id| {
            graph.add_cites(risk_id, "《新增法》第9条");
        });
        assert_terminal_legacy_write_is_noop(|graph, risk_id| {
            graph.add_risk_with_edges(make_test_risk(risk_id, "ch_overwrite"), "ch_extra");
        });
        assert_terminal_legacy_write_is_noop(|graph, risk_id| {
            let mut risk = make_test_risk(risk_id, "ch_overwrite");
            risk.finding.finding_role = FindingRole::Hypothesis;
            graph.add_hypothesis(risk, "ch_extra");
        });
    }

    #[test]
    fn legacy_add_has_risk_invalidates_existing_same_law_neighbor_versions() {
        let graph = SessionGraph::new();
        graph.add_chunks(vec![
            make_test_chunk("ch_a"),
            make_test_chunk("ch_b"),
            make_test_chunk("ch_extra"),
        ]);
        graph.add_risk_with_edges(make_test_risk("R_A", "ch_a"), "ch_a");
        graph.add_risk_with_edges(make_test_risk("R_B", "ch_b"), "ch_b");
        let old_version = graph.snapshot().chunk_versions["ch_b"];
        assert_eq!(
            graph.query_clause_context("ch_b").same_law_chunks,
            vec!["ch_a"]
        );

        graph.add_has_risk("ch_extra", "R_A");

        let changed = graph
            .query_clause_context_since("ch_b", Some(old_version))
            .expect("新增 has_risk 关系必须使既有同法条邻居版本失效");
        assert_eq!(changed.context.same_law_chunks, vec!["ch_a", "ch_extra"]);
        assert!(changed.version > old_version);
    }

    #[test]
    fn legacy_add_cites_invalidates_existing_same_law_neighbor_versions() {
        let graph = SessionGraph::new();
        graph.add_chunks(vec![make_test_chunk("ch_a"), make_test_chunk("ch_b")]);
        let mut risk_a = make_test_risk("R_A", "ch_a");
        risk_a.finding.legal_basis = vec!["《另一测试法》第2条".to_string()];
        graph.add_risk_with_edges(risk_a, "ch_a");
        graph.add_risk_with_edges(make_test_risk("R_B", "ch_b"), "ch_b");
        let old_version = graph.snapshot().chunk_versions["ch_b"];
        assert!(
            graph
                .query_clause_context("ch_b")
                .same_law_chunks
                .is_empty()
        );

        graph.add_cites("R_A", "《测试法》第1条");

        let changed = graph
            .query_clause_context_since("ch_b", Some(old_version))
            .expect("新增 cites 关系必须使既有同法条邻居版本失效");
        assert_eq!(changed.context.same_law_chunks, vec!["ch_a"]);
        assert!(changed.version > old_version);
    }

    #[test]
    fn provisional_upsert_invalidates_legacy_indexed_same_law_neighbors() {
        let graph = SessionGraph::new();
        graph.add_chunks(vec![
            make_test_chunk("ch_a"),
            make_test_chunk("ch_legacy"),
            make_test_chunk("ch_b"),
        ]);
        graph.add_risk_with_edges(make_test_risk("R_A", "ch_a"), "ch_a");
        graph.add_has_risk("ch_legacy", "R_A");
        let old_version = graph.snapshot().chunk_versions["ch_legacy"];
        assert_eq!(
            graph.query_clause_context("ch_legacy").same_law_chunks,
            vec!["ch_a"]
        );

        graph
            .upsert_provisional_findings(&[make_test_risk("R_B", "ch_b").finding])
            .expect("正常 provisional 提交应成功");

        let changed = graph
            .query_clause_context_since("ch_legacy", Some(old_version))
            .expect("正常提交必须使兼容索引中的既有同法条邻居版本失效");
        assert_eq!(changed.context.same_law_chunks, vec!["ch_a", "ch_b"]);
        assert!(changed.version > old_version);
    }

    #[test]
    fn provisional_upsert_invalidates_preindexed_direct_alias_without_law() {
        let graph = SessionGraph::new();
        graph.add_chunks(vec![
            make_test_chunk("ch_primary"),
            make_test_chunk("ch_alias"),
        ]);
        graph.add_has_risk("ch_alias", "R_NEW");
        let old_version = graph.snapshot().chunk_versions["ch_alias"];
        let mut finding = make_test_risk("R_NEW", "ch_primary").finding;
        finding.legal_basis.clear();

        graph
            .upsert_provisional_findings(&[finding])
            .expect("正常 provisional 提交应成功");

        let changed = graph
            .query_clause_context_since("ch_alias", Some(old_version))
            .expect("新增风险节点必须使预建的直接索引别名版本失效");
        assert_eq!(changed.context.risks[0].risk_id, "R_NEW");
        assert!(changed.version > old_version);
    }

    fn assert_legacy_node_overwrite_invalidates_peer(
        label: &str,
        write: impl Fn(&SessionGraph, RiskNode),
    ) {
        let graph = SessionGraph::new();
        graph.add_chunks(vec![make_test_chunk("ch_a"), make_test_chunk("ch_b")]);
        graph.add_risk_with_edges(make_test_risk("R_A", "ch_a"), "ch_a");
        graph.add_risk_with_edges(make_test_risk("R_B", "ch_b"), "ch_b");
        let old_version = graph.snapshot().chunk_versions["ch_b"];
        let mut hypothesis = make_test_risk("R_A", "ch_a");
        hypothesis.finding.finding_role = FindingRole::Hypothesis;

        write(&graph, hypothesis);

        let changed = graph
            .query_clause_context_since("ch_b", Some(old_version))
            .unwrap_or_else(|| panic!("{label} 改变风险可见性后必须使邻居版本失效"));
        assert!(changed.context.same_law_chunks.is_empty(), "{label}");
        assert!(changed.version > old_version, "{label}");
    }

    #[test]
    fn legacy_node_overwrites_invalidate_same_law_neighbor_versions() {
        assert_legacy_node_overwrite_invalidates_peer("add_risk", |graph, risk| {
            graph.add_risk(risk);
        });
        assert_legacy_node_overwrite_invalidates_peer("add_risk_with_edges", |graph, risk| {
            graph.add_risk_with_edges(risk, "ch_a");
        });
        assert_legacy_node_overwrite_invalidates_peer("add_hypothesis", |graph, risk| {
            graph.add_hypothesis(risk, "ch_a");
        });
    }

    fn assert_legacy_node_overwrite_invalidates_direct_alias(
        label: &str,
        write: impl Fn(&SessionGraph, RiskNode),
    ) {
        let graph = SessionGraph::new();
        graph.add_chunks(vec![
            make_test_chunk("ch_primary"),
            make_test_chunk("ch_alias"),
        ]);
        let mut original = make_test_risk("R_ALIAS", "ch_primary");
        original.finding.legal_basis.clear();
        graph.add_risk_with_edges(original, "ch_primary");
        graph.add_has_risk("ch_alias", "R_ALIAS");
        let old_version = graph.snapshot().chunk_versions["ch_alias"];
        let mut replacement = make_test_risk("R_ALIAS", "ch_primary");
        replacement.finding.legal_basis.clear();
        replacement.finding.reason = format!("{label} 更新后的理由");

        write(&graph, replacement);

        let changed = graph
            .query_clause_context_since("ch_alias", Some(old_version))
            .unwrap_or_else(|| panic!("{label} 覆盖风险内容后必须使直接索引别名版本失效"));
        assert_eq!(
            changed.context.risks[0].reason,
            format!("{label} 更新后的理由")
        );
        assert!(changed.version > old_version, "{label}");
    }

    #[test]
    fn legacy_node_overwrites_invalidate_direct_alias_versions() {
        assert_legacy_node_overwrite_invalidates_direct_alias("add_risk", |graph, risk| {
            graph.add_risk(risk);
        });
        assert_legacy_node_overwrite_invalidates_direct_alias(
            "add_risk_with_edges",
            |graph, risk| {
                graph.add_risk_with_edges(risk, "ch_primary");
            },
        );
        assert_legacy_node_overwrite_invalidates_direct_alias(
            "add_hypothesis",
            |graph, mut risk| {
                risk.finding.finding_role = FindingRole::Hypothesis;
                graph.add_hypothesis(risk, "ch_primary");
            },
        );
    }

    fn assert_invalid_legacy_risk_node_is_noop(write: impl Fn(&SessionGraph, RiskNode)) {
        for existing_provisional in [false, true] {
            for invalid_node in [
                {
                    let mut risk = make_test_risk("R_TEST", "ch_extra");
                    risk.state = FindingState::Confirmed;
                    risk
                },
                {
                    let mut risk = make_test_risk("R_TEST", "ch_extra");
                    risk.state = FindingState::Merged;
                    risk
                },
                {
                    let mut risk = make_test_risk("R_TEST", "ch_extra");
                    risk.state = FindingState::Rejected;
                    risk
                },
                {
                    let mut risk = make_test_risk("R_TEST", "ch_extra");
                    risk.merged_into = Some("R_TARGET".to_string());
                    risk
                },
                {
                    let mut risk = make_test_risk("R_TEST", "ch_extra");
                    risk.decision_reason = Some("伪造裁决".to_string());
                    risk
                },
            ] {
                let graph = SessionGraph::new();
                graph.add_chunk(make_test_chunk("ch_extra"));
                let mut invalid_node = invalid_node;
                if existing_provisional {
                    add_provisional_risk(&graph, "R_EXISTING", "ch_existing");
                    invalid_node.finding.risk_id = "R_EXISTING".to_string();
                } else {
                    invalid_node.finding.risk_id = "R_NEW".to_string();
                }
                let before = serde_json::to_value(graph.snapshot()).expect("快照应可序列化");

                write(&graph, invalid_node);
                let after = serde_json::to_value(graph.snapshot()).expect("快照应可序列化");

                assert_eq!(
                    after, before,
                    "兼容入口不得接收携带终态或裁决元数据的 RiskNode"
                );
            }
        }
    }

    #[test]
    fn legacy_risk_node_writes_reject_forged_terminal_state_and_decision_metadata() {
        assert_invalid_legacy_risk_node_is_noop(|graph, risk| graph.add_risk(risk));
        assert_invalid_legacy_risk_node_is_noop(|graph, risk| {
            graph.add_risk_with_edges(risk, "ch_extra");
        });
        assert_invalid_legacy_risk_node_is_noop(|graph, risk| {
            graph.add_hypothesis(risk, "ch_extra");
        });
    }

    #[test]
    fn finalize_audit_confirms_latest_finding_and_rebuilds_risk_indexes() {
        let graph = SessionGraph::new();
        add_provisional_risk(&graph, "R_001", "ch_001");
        graph.add_chunk(make_test_chunk("ch_002"));
        let before = graph.snapshot();
        let mut final_finding = make_test_risk("R_001", "ch_002").finding;
        final_finding.severity = RiskSeverity::Medium;
        final_finding.reason = "最终裁决理由".to_string();
        final_finding.legal_basis = vec!["《最终法》第2条".to_string()];

        let commit = graph
            .finalize_audit(&[final_finding.clone()], &HashMap::new(), &HashMap::new())
            .expect("最终裁决应成功");
        let snapshot = graph.snapshot();
        let node = &snapshot.risks["R_001"];

        assert_eq!(node.state, FindingState::Confirmed);
        assert_eq!(node.finding.severity, RiskSeverity::Medium);
        assert_eq!(node.finding.reason, "最终裁决理由");
        assert_eq!(node.finding.legal_basis, vec!["《最终法》第2条"]);
        assert_eq!(node.law_refs, vec!["《最终法》第2条"]);
        assert!(!snapshot.has_risk.contains_key("ch_001"));
        assert_eq!(snapshot.has_risk["ch_002"], vec!["R_001"]);
        assert!(
            !snapshot.cites.contains_key("R_001")
                || !snapshot.cites["R_001"].contains(&"《测试法》第1条".to_string())
        );
        assert_eq!(snapshot.cites["R_001"], vec!["《最终法》第2条"]);
        assert_eq!(snapshot.cited_by["《最终法》第2条"], vec!["R_001"]);
        assert!(!snapshot.cited_by.contains_key("《测试法》第1条"));
        assert!(!snapshot.laws.contains_key("《测试法》第1条"));
        assert!(snapshot.laws.contains_key("《最终法》第2条"));
        assert_eq!(snapshot.finding_transitions.len(), 1);
        assert_eq!(commit.graph_version, before.graph_version + 1);
        assert_eq!(
            snapshot.chunk_versions["ch_001"],
            before.chunk_versions["ch_001"] + 1
        );
        assert_eq!(
            snapshot.chunk_versions["ch_002"],
            before.chunk_versions["ch_002"] + 1
        );
    }

    #[test]
    fn finalize_audit_preserves_independently_enriched_law_nodes() {
        let graph = SessionGraph::new();
        add_provisional_risk(&graph, "R_001", "ch_001");
        {
            let mut state = graph.state.write().expect("应取得测试写锁");
            state
                .laws
                .get_mut("《测试法》第1条")
                .expect("风险写入应创建法条节点")
                .title = "独立维护的法规元数据".to_string();
        }
        let mut final_finding = make_test_risk("R_001", "ch_001").finding;
        final_finding.legal_basis = vec!["《最终法》第2条".to_string()];

        graph
            .finalize_audit(&[final_finding], &HashMap::new(), &HashMap::new())
            .expect("最终裁决应成功");
        let snapshot = graph.snapshot();

        assert_eq!(
            snapshot.laws["《测试法》第1条"].title,
            "独立维护的法规元数据"
        );
        assert!(snapshot.laws.contains_key("《最终法》第2条"));
    }

    #[test]
    fn finalize_audit_removes_stale_same_law_edges_after_basis_changes() {
        let graph = SessionGraph::new();
        add_provisional_risk(&graph, "R_001", "ch_001");
        add_provisional_risk(&graph, "R_002", "ch_002");
        let before = graph.snapshot();
        assert_eq!(before.same_law["ch_001"], vec!["ch_002"]);
        assert_eq!(before.same_law["ch_002"], vec!["ch_001"]);
        let mut final_finding = make_test_risk("R_001", "ch_001").finding;
        final_finding.legal_basis = vec!["《最终法》第2条".to_string()];

        graph
            .finalize_audit(&[final_finding], &HashMap::new(), &HashMap::new())
            .expect("最终裁决应成功");
        let snapshot = graph.snapshot();

        assert!(!snapshot.same_law.contains_key("ch_001"));
        assert!(!snapshot.same_law.contains_key("ch_002"));
        assert_eq!(snapshot.cited_by["《测试法》第1条"], vec!["R_002"]);
        assert_eq!(snapshot.cited_by["《最终法》第2条"], vec!["R_001"]);
        assert_eq!(
            snapshot.chunk_versions["ch_001"],
            before.chunk_versions["ch_001"] + 1
        );
        assert_eq!(
            snapshot.chunk_versions["ch_002"],
            before.chunk_versions["ch_002"] + 1
        );
    }

    #[test]
    fn finalize_audit_records_merged_rejected_and_leaves_undecided_provisional() {
        let graph = SessionGraph::new();
        add_provisional_risk(&graph, "R_TARGET", "ch_target");
        add_provisional_risk(&graph, "R_MERGED", "ch_merged");
        add_provisional_risk(&graph, "R_REJECTED", "ch_rejected");
        add_provisional_risk(&graph, "R_UNDECIDED", "ch_undecided");
        let final_finding = make_test_risk("R_TARGET", "ch_target").finding;

        graph
            .finalize_audit(
                &[final_finding],
                &HashMap::from([("R_MERGED".to_string(), "R_TARGET".to_string())]),
                &HashMap::from([("R_REJECTED".to_string(), "证据不足".to_string())]),
            )
            .expect("混合裁决应成功");
        let snapshot = graph.snapshot();

        assert_eq!(snapshot.risks["R_TARGET"].state, FindingState::Confirmed);
        assert_eq!(snapshot.risks["R_MERGED"].state, FindingState::Merged);
        assert_eq!(
            snapshot.risks["R_MERGED"].merged_into.as_deref(),
            Some("R_TARGET")
        );
        assert_eq!(
            snapshot.risks["R_MERGED"].decision_reason.as_deref(),
            Some("合并至 R_TARGET")
        );
        assert_eq!(snapshot.risks["R_REJECTED"].state, FindingState::Rejected);
        assert_eq!(
            snapshot.risks["R_REJECTED"].decision_reason.as_deref(),
            Some("证据不足")
        );
        assert_eq!(
            snapshot.risks["R_UNDECIDED"].state,
            FindingState::Provisional
        );
        assert_eq!(snapshot.finding_transitions.len(), 3);
        assert!(snapshot.finding_transitions.iter().any(|transition| {
            transition.risk_id == "R_MERGED"
                && transition.from == FindingState::Provisional
                && transition.to == FindingState::Merged
                && transition.reason == "合并至 R_TARGET"
                && transition.merged_into.as_deref() == Some("R_TARGET")
                && !transition.decided_at.is_empty()
        }));
    }

    #[test]
    fn clause_context_excludes_same_law_edges_created_by_rejected_risks() {
        let graph = SessionGraph::new();
        add_provisional_risk(&graph, "R_CONFIRMED", "ch_confirmed");
        add_provisional_risk(&graph, "R_REJECTED", "ch_rejected");
        let final_finding = make_test_risk("R_CONFIRMED", "ch_confirmed").finding;

        graph
            .finalize_audit(
                &[final_finding],
                &HashMap::new(),
                &HashMap::from([("R_REJECTED".to_string(), "证据不足".to_string())]),
            )
            .expect("最终裁决应成功");

        let snapshot = graph.snapshot();
        assert_eq!(
            snapshot.same_law["ch_confirmed"],
            vec!["ch_rejected"],
            "完整审计快照必须保留终态风险产生的历史关系"
        );
        assert!(
            graph
                .query_clause_context("ch_confirmed")
                .same_law_chunks
                .is_empty(),
            "Agent 工作上下文不得暴露仅由 rejected 风险产生的同法条关系"
        );
        assert_eq!(
            graph.query_same_law_chunks("《测试法》第1条"),
            vec!["ch_confirmed"],
            "工作态法条查询不得返回 rejected 风险关联的条款"
        );
        assert!(
            graph.query_same_law_edges("ch_confirmed").is_empty(),
            "工作态边查询不得返回仅由 rejected 风险产生的关系"
        );
    }

    #[test]
    fn finalization_invalidates_working_same_law_neighbor_version() {
        let graph = SessionGraph::new();
        add_provisional_risk(&graph, "R_A", "ch_a");
        add_provisional_risk(&graph, "R_B", "ch_b");
        let before = graph.snapshot();
        let old_version = before.chunk_versions["ch_a"];
        assert_eq!(
            graph.query_clause_context("ch_a").same_law_chunks,
            vec!["ch_b"]
        );

        graph
            .finalize_audit(
                &[],
                &HashMap::new(),
                &HashMap::from([("R_B".to_string(), "证据不足".to_string())]),
            )
            .expect("最终裁决应成功");

        let changed = graph
            .query_clause_context_since("ch_a", Some(old_version))
            .expect("工作态邻居消失必须使相邻条款版本失效");
        assert!(changed.context.same_law_chunks.is_empty());
        assert!(changed.version > old_version);
    }

    #[test]
    fn working_same_law_respects_finding_lifecycle_and_role() {
        let graph = SessionGraph::new();
        add_provisional_risk(&graph, "R_A", "ch_a");
        add_provisional_risk(&graph, "R_B", "ch_b");
        add_provisional_risk(&graph, "R_TARGET", "ch_target");
        add_provisional_risk(&graph, "R_MERGED", "ch_merged");
        graph.add_chunk(make_test_chunk("ch_hypothesis"));
        let mut hypothesis = make_test_risk("R_HYPOTHESIS", "ch_hypothesis");
        hypothesis.finding.finding_role = FindingRole::Hypothesis;
        graph.add_hypothesis(hypothesis, "ch_hypothesis");

        let provisional_view = graph.snapshot().agent_visible_same_law();
        assert_eq!(
            provisional_view["ch_a"],
            vec!["ch_b", "ch_merged", "ch_target"]
        );
        assert!(!provisional_view.contains_key("ch_hypothesis"));

        graph
            .finalize_audit(
                &[
                    make_test_risk("R_A", "ch_a").finding,
                    make_test_risk("R_B", "ch_b").finding,
                    make_test_risk("R_TARGET", "ch_target").finding,
                ],
                &HashMap::from([("R_MERGED".to_string(), "R_TARGET".to_string())]),
                &HashMap::new(),
            )
            .expect("最终裁决应成功");

        let confirmed_view = graph.snapshot().agent_visible_same_law();
        assert_eq!(confirmed_view["ch_a"], vec!["ch_b", "ch_target"]);
        assert!(!confirmed_view.contains_key("ch_merged"));
        assert!(!confirmed_view.contains_key("ch_hypothesis"));
    }

    #[test]
    fn working_same_law_uses_transactional_clause_indexes_as_source_of_truth() {
        let graph = SessionGraph::new();
        graph.add_chunks(vec![
            make_test_chunk("ch_primary"),
            make_test_chunk("ch_unindexed"),
            make_test_chunk("ch_peer"),
        ]);
        let mut multi_clause_risk = make_test_risk("R_MULTI", "ch_primary");
        multi_clause_risk.finding.clause_ids =
            vec!["ch_primary".to_string(), "ch_unindexed".to_string()];
        graph.add_risk_with_edges(multi_clause_risk, "ch_primary");
        graph.add_risk_with_edges(make_test_risk("R_PEER", "ch_peer"), "ch_peer");

        let working_same_law = graph.snapshot().agent_visible_same_law();

        assert_eq!(working_same_law["ch_primary"], vec!["ch_peer"]);
        assert_eq!(working_same_law["ch_peer"], vec!["ch_primary"]);
        assert!(!working_same_law.contains_key("ch_unindexed"));
        assert!(
            !graph
                .query_same_law_chunks("《测试法》第1条")
                .contains(&"ch_unindexed".to_string()),
            "按法条查询也必须以 has_risk 事务索引为准"
        );
        assert!(
            graph
                .query_clause_context("ch_peer")
                .same_law_chunks
                .iter()
                .all(|chunk_id| chunk_id != "ch_unindexed"),
            "未进入 has_risk 事务索引的条款不得进入 Agent 工作上下文"
        );
    }

    #[test]
    fn clause_context_ignores_risks_missing_from_transactional_indexes() {
        let graph = SessionGraph::new();
        add_provisional_risk(&graph, "R_INDEXED", "ch_indexed");
        graph.add_chunk(make_test_chunk("ch_orphan"));
        let orphan = make_test_risk("R_ORPHAN", "ch_orphan");
        graph
            .state
            .write()
            .expect("测试图写锁不应中毒")
            .risks
            .insert("R_ORPHAN".to_string(), orphan);

        let context = graph.query_clause_context("ch_indexed");

        assert!(
            context.same_law_chunks.is_empty(),
            "未进入 has_risk/cited_by 事务索引的孤立节点不得参与工作态关系"
        );
    }

    #[test]
    fn finalize_audit_invalid_batch_rolls_back_all_state_and_versions() {
        let graph = SessionGraph::new();
        add_provisional_risk(&graph, "R_001", "ch_001");
        add_provisional_risk(&graph, "R_002", "ch_002");
        let final_finding = make_test_risk("R_001", "ch_001").finding;
        let before = graph.snapshot();

        let error = graph
            .finalize_audit(
                &[final_finding.clone()],
                &HashMap::from([("R_002".to_string(), "R_MISSING".to_string())]),
                &HashMap::new(),
            )
            .expect_err("非法合并目标必须失败");
        assert!(error.contains("目标"));
        let after_missing = graph.snapshot();
        assert_eq!(after_missing.graph_version, before.graph_version);
        assert_eq!(after_missing.finding_transitions.len(), 0);
        assert_eq!(
            after_missing.risks["R_001"].state,
            FindingState::Provisional
        );

        let error = graph
            .finalize_audit(
                &[final_finding],
                &HashMap::from([("R_002".to_string(), "R_001".to_string())]),
                &HashMap::from([("R_002".to_string(), "重复归类".to_string())]),
            )
            .expect_err("裁决集合冲突必须失败");
        assert!(error.contains("互斥"));
        let after_conflict = graph.snapshot();
        assert_eq!(after_conflict.graph_version, before.graph_version);
        assert!(after_conflict.finding_transitions.is_empty());
    }

    #[test]
    fn finalize_audit_rejects_final_finding_without_clause_ids_atomically() {
        let graph = SessionGraph::new();
        add_provisional_risk(&graph, "R_001", "ch_001");
        let mut final_finding = make_test_risk("R_001", "ch_001").finding;
        final_finding.clause_ids.clear();
        let before = graph.snapshot();

        let error = graph
            .finalize_audit(&[final_finding], &HashMap::new(), &HashMap::new())
            .expect_err("最终 finding 缺少 clause_ids 必须失败");
        let after = graph.snapshot();

        assert!(error.contains("clause_ids"));
        assert_eq!(after.graph_version, before.graph_version);
        assert_eq!(after.chunk_versions, before.chunk_versions);
        assert_eq!(after.risks["R_001"].state, FindingState::Provisional);
        assert!(after.finding_transitions.is_empty());
    }

    #[test]
    fn finalize_audit_rejects_blank_final_clause_id_atomically() {
        let graph = SessionGraph::new();
        add_provisional_risk(&graph, "R_001", "ch_001");
        let mut final_finding = make_test_risk("R_001", "ch_001").finding;
        final_finding.clause_ids = vec![" \t ".to_string()];
        let before = serde_json::to_value(graph.snapshot()).expect("快照应可序列化");

        let error = graph
            .finalize_audit(&[final_finding], &HashMap::new(), &HashMap::new())
            .expect_err("最终 finding 的空白 clause_id 必须失败");
        let after = serde_json::to_value(graph.snapshot()).expect("快照应可序列化");

        assert!(error.contains("clause_id"));
        assert_eq!(after, before);
    }

    #[test]
    fn finalize_audit_rejects_missing_final_clause_atomically() {
        let graph = SessionGraph::new();
        add_provisional_risk(&graph, "R_001", "ch_001");
        let mut final_finding = make_test_risk("R_001", "ch_001").finding;
        final_finding.clause_ids = vec!["ch_missing".to_string()];
        let before = serde_json::to_value(graph.snapshot()).expect("快照应可序列化");

        let error = graph
            .finalize_audit(&[final_finding], &HashMap::new(), &HashMap::new())
            .expect_err("最终 finding 引用不存在条款必须失败");
        let after = serde_json::to_value(graph.snapshot()).expect("快照应可序列化");

        assert!(error.contains("不存在"));
        assert_eq!(after, before);
    }

    #[test]
    fn finalize_audit_rejects_invalid_clause_references_on_decision_sources() {
        for (source_id, clause_id, is_merged) in [
            ("R_MERGED", " \t ", true),
            ("R_REJECTED", "ch_missing", false),
        ] {
            let graph = SessionGraph::new();
            add_provisional_risk(&graph, "R_TARGET", "ch_target");
            graph.add_risk_with_edges(make_test_risk(source_id, clause_id), clause_id);
            let before = serde_json::to_value(graph.snapshot()).expect("快照应可序列化");
            let merged = is_merged
                .then(|| HashMap::from([(source_id.to_string(), "R_TARGET".to_string())]))
                .unwrap_or_default();
            let rejected = (!is_merged)
                .then(|| HashMap::from([(source_id.to_string(), "证据不足".to_string())]))
                .unwrap_or_default();

            let error = graph
                .finalize_audit(
                    &[make_test_risk("R_TARGET", "ch_target").finding],
                    &merged,
                    &rejected,
                )
                .expect_err("裁决源 finding 的非法条款引用必须失败");
            let after = serde_json::to_value(graph.snapshot()).expect("快照应可序列化");

            assert!(error.contains("clause_id") || error.contains("不存在"));
            assert_eq!(after, before);
        }
    }

    #[test]
    fn finalize_audit_rejects_hypothesis_final_finding_atomically() {
        let graph = SessionGraph::new();
        add_provisional_risk(&graph, "R_001", "ch_001");
        let mut final_finding = make_test_risk("R_001", "ch_001").finding;
        final_finding.finding_role = FindingRole::Hypothesis;
        let before = serde_json::to_value(graph.snapshot()).expect("快照应可序列化");

        let error = graph
            .finalize_audit(&[final_finding], &HashMap::new(), &HashMap::new())
            .expect_err("Hypothesis 不得成为 confirmed finding");
        let after = serde_json::to_value(graph.snapshot()).expect("快照应可序列化");

        assert!(error.contains("Hypothesis"));
        assert_eq!(after, before);
    }

    #[test]
    fn clause_context_excludes_merged_and_rejected_terminal_history() {
        let graph = SessionGraph::new();
        graph.add_chunk(make_test_chunk("ch_shared"));
        for risk_id in ["R_CONFIRMED", "R_MERGED", "R_REJECTED", "R_PROVISIONAL"] {
            graph
                .upsert_provisional_findings(&[make_test_risk(risk_id, "ch_shared").finding])
                .expect("应写入 provisional finding");
        }
        graph
            .finalize_audit(
                &[make_test_risk("R_CONFIRMED", "ch_shared").finding],
                &HashMap::from([("R_MERGED".to_string(), "R_CONFIRMED".to_string())]),
                &HashMap::from([("R_REJECTED".to_string(), "证据不足".to_string())]),
            )
            .expect("最终裁决应成功");

        let mut risk_ids = graph
            .query_clause_context("ch_shared")
            .risks
            .into_iter()
            .map(|finding| finding.risk_id)
            .collect::<Vec<_>>();
        risk_ids.sort();

        assert_eq!(risk_ids, vec!["R_CONFIRMED", "R_PROVISIONAL"]);
    }

    #[test]
    fn finalize_audit_records_transitions_in_risk_id_order() {
        let graph = SessionGraph::new();
        for risk_id in ["R_Z", "R_A", "R_M"] {
            add_provisional_risk(&graph, risk_id, &format!("ch_{risk_id}"));
        }

        graph
            .finalize_audit(
                &[make_test_risk("R_Z", "ch_R_Z").finding],
                &HashMap::from([("R_A".to_string(), "R_Z".to_string())]),
                &HashMap::from([("R_M".to_string(), "证据不足".to_string())]),
            )
            .expect("最终裁决应成功");
        let transition_ids = graph
            .snapshot()
            .finding_transitions
            .into_iter()
            .map(|transition| transition.risk_id)
            .collect::<Vec<_>>();

        assert_eq!(transition_ids, vec!["R_A", "R_M", "R_Z"]);
    }

    #[test]
    fn finalize_audit_rejects_blank_rejection_reason_atomically() {
        let graph = SessionGraph::new();
        add_provisional_risk(&graph, "R_001", "ch_001");
        let before = graph.snapshot();

        let error = graph
            .finalize_audit(
                &[],
                &HashMap::new(),
                &HashMap::from([("R_001".to_string(), " \t ".to_string())]),
            )
            .expect_err("空白 rejected reason 必须失败");
        let after = graph.snapshot();

        assert!(error.contains("原因"));
        assert_eq!(after.graph_version, before.graph_version);
        assert_eq!(after.chunk_versions, before.chunk_versions);
        assert_eq!(after.risks["R_001"].state, FindingState::Provisional);
        assert!(after.finding_transitions.is_empty());
    }

    #[test]
    fn finalize_audit_rejects_retry_that_omits_existing_terminal_findings() {
        let graph = SessionGraph::new();
        add_provisional_risk(&graph, "R_001", "ch_001");
        add_provisional_risk(&graph, "R_002", "ch_002");
        let final_r1 = make_test_risk("R_001", "ch_001").finding;
        let final_r2 = make_test_risk("R_002", "ch_002").finding;
        graph
            .finalize_audit(
                &[final_r1.clone(), final_r2],
                &HashMap::new(),
                &HashMap::new(),
            )
            .expect("首次完整裁决应成功");
        let before = graph.snapshot();

        let error = graph
            .finalize_audit(&[final_r1], &HashMap::new(), &HashMap::new())
            .expect_err("重试遗漏已有 confirmed finding 必须失败");
        let after = graph.snapshot();

        assert!(error.contains("完整"));
        assert_eq!(after.graph_version, before.graph_version);
        assert_eq!(after.chunk_versions, before.chunk_versions);
        assert_eq!(
            after.finding_transitions.len(),
            before.finding_transitions.len()
        );
        assert_eq!(after.risks["R_001"].state, FindingState::Confirmed);
        assert_eq!(after.risks["R_002"].state, FindingState::Confirmed);
    }

    #[test]
    fn finalize_audit_terminal_conflict_rolls_back_whole_retry_batch() {
        let graph = SessionGraph::new();
        add_provisional_risk(&graph, "R_001", "ch_001");
        add_provisional_risk(&graph, "R_002", "ch_002");
        let confirmed = make_test_risk("R_001", "ch_001").finding;
        graph
            .finalize_audit(&[confirmed], &HashMap::new(), &HashMap::new())
            .expect("首次确认应成功");
        let before = graph.snapshot();
        let mut changed = make_test_risk("R_001", "ch_001").finding;
        changed.reason = "冲突的新理由".to_string();

        let error = graph
            .finalize_audit(
                &[changed],
                &HashMap::new(),
                &HashMap::from([("R_002".to_string(), "本应回滚".to_string())]),
            )
            .expect_err("已确认内容变化必须整批失败");
        assert!(error.contains("内容不同"));
        let after = graph.snapshot();
        assert_eq!(after.graph_version, before.graph_version);
        assert_eq!(
            after.finding_transitions.len(),
            before.finding_transitions.len()
        );
        assert_eq!(after.risks["R_002"].state, FindingState::Provisional);
    }

    #[test]
    fn finalize_audit_identical_retry_is_idempotent() {
        let graph = SessionGraph::new();
        add_provisional_risk(&graph, "R_TARGET", "ch_target");
        add_provisional_risk(&graph, "R_MERGED", "ch_merged");
        add_provisional_risk(&graph, "R_REJECTED", "ch_rejected");
        let final_finding = make_test_risk("R_TARGET", "ch_target").finding;
        let merged = HashMap::from([("R_MERGED".to_string(), "R_TARGET".to_string())]);
        let rejected = HashMap::from([("R_REJECTED".to_string(), "误报".to_string())]);
        graph
            .finalize_audit(std::slice::from_ref(&final_finding), &merged, &rejected)
            .expect("首次裁决应成功");
        let before_retry = graph.snapshot();

        let commit = graph
            .finalize_audit(&[final_finding], &merged, &rejected)
            .expect("相同裁决重试应成功");
        let after_retry = graph.snapshot();

        assert_eq!(commit.graph_version, before_retry.graph_version);
        assert!(commit.chunk_versions.is_empty());
        assert_eq!(after_retry.graph_version, before_retry.graph_version);
        assert_eq!(after_retry.chunk_versions, before_retry.chunk_versions);
        assert_eq!(
            after_retry.finding_transitions.len(),
            before_retry.finding_transitions.len()
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

    // ── 矛盾边 (contradicts) ──────────────────────────────────

    #[test]
    fn test_add_contradicts_bidirectional() {
        let g = SessionGraph::new();
        g.add_chunk(make_test_chunk("ch_A"));
        g.add_chunk(make_test_chunk("ch_B"));

        g.add_contradicts("ch_A", "ch_B", "条款矛盾：A允许联合体，B禁止分包");

        // 正向查询
        let a_contra = g.query_contradictions("ch_A");
        assert_eq!(a_contra.len(), 1);
        assert_eq!(a_contra[0].0, "ch_B");
        assert!(a_contra[0].1.contains("A允许联合体"));

        // 反向查询（双向写入验证）
        let b_contra = g.query_contradictions("ch_B");
        assert_eq!(b_contra.len(), 1);
        assert_eq!(b_contra[0].0, "ch_A");
        assert!(b_contra[0].1.contains("A允许联合体"));
    }

    #[test]
    fn test_add_contradicts_multi_accumulate() {
        let g = SessionGraph::new();
        g.add_chunk(make_test_chunk("ch_A"));
        g.add_chunk(make_test_chunk("ch_B"));
        g.add_chunk(make_test_chunk("ch_C"));

        g.add_contradicts("ch_A", "ch_B", "矛盾1");
        g.add_contradicts("ch_A", "ch_C", "矛盾2");

        assert_eq!(g.query_contradictions("ch_A").len(), 2);
        assert_eq!(g.query_contradictions("ch_B").len(), 1);
        assert_eq!(g.query_contradictions("ch_C").len(), 1);
    }

    #[test]
    fn test_query_contradictions_empty_for_unknown_chunk() {
        let g = SessionGraph::new();
        let result = g.query_contradictions("no_such_chunk");
        assert!(result.is_empty());
    }

    // ── AgentNode 预写入 ──────────────────────────────────────

    #[test]
    fn test_add_agent_and_snapshot() {
        let g = SessionGraph::new();
        g.add_agent(AgentNode {
            agent_id: AgentId::FactCheck,
            display_name: "事实核查Agent".into(),
            role: "事实核查".into(),
        });
        g.add_agent(AgentNode {
            agent_id: AgentId::SemanticRisk,
            display_name: "隐性风险识别Agent".into(),
            role: "隐性风险".into(),
        });

        let snap = g.snapshot();
        assert_eq!(snap.agents.len(), 2);
        assert!(snap.agents.contains_key(&AgentId::FactCheck));
        assert!(snap.agents.contains_key(&AgentId::SemanticRisk));
        assert_eq!(
            snap.agents[&AgentId::FactCheck].display_name,
            "事实核查Agent"
        );
    }

    // ── Snapshot 包含所有新字段 ───────────────────────────────

    #[test]
    fn test_snapshot_includes_all_new_fields() {
        let g = SessionGraph::new();
        g.add_chunk(make_test_chunk("ch_001"));
        g.add_risk_with_edges(make_test_risk("R_001", "ch_001"), "ch_001");
        g.add_reviewed_by("ch_001", AgentId::FactCheck);
        g.add_agent(AgentNode {
            agent_id: AgentId::FactCheck,
            display_name: "测试".into(),
            role: "测试".into(),
        });
        g.add_contradicts("ch_001", "ch_002", "测试矛盾");

        let snap = g.snapshot();
        // 原有字段
        assert_eq!(snap.chunks.len(), 1);
        assert_eq!(snap.risks.len(), 1);
        // 新增字段均应存在（至少一个为空或 1）
        assert!(!snap.agents.is_empty(), "snapshot 应包含 agents");
        assert!(
            !snap.laws.is_empty(),
            "snapshot 应包含 laws（legal_basis 自动写入）"
        );
        assert!(snap.cases.is_empty(), "snapshot cases 应为空（未写入）");
        assert!(!snap.contradicts.is_empty(), "snapshot 应包含 contradicts");
        assert!(
            snap.same_law.is_empty(),
            "snapshot 应包含 same_law 字段（可能为空）"
        );
    }

    // ── ClauseContext 包含 contradictions ─────────────────────

    #[test]
    fn test_clause_context_includes_contradictions() {
        let g = SessionGraph::new();
        g.add_chunk(make_test_chunk("ch_001"));
        g.add_chunk(make_test_chunk("ch_003"));
        g.add_contradicts("ch_001", "ch_003", "条款矛盾测试");

        let ctx = g.query_clause_context("ch_001");
        assert_eq!(ctx.contradictions.len(), 1);
        assert_eq!(ctx.contradictions[0].chunk_id, "ch_003");
        assert_eq!(ctx.contradictions[0].reason, "条款矛盾测试");
    }

    // ── Law 节点自动写入 ──────────────────────────────────────

    #[test]
    fn test_law_node_auto_created_on_risk() {
        let g = SessionGraph::new();
        g.add_chunk(make_test_chunk("ch_001"));
        let risk = make_test_risk("R_001", "ch_001");
        // risk 的 legal_basis 是 ["《测试法》第1条"]
        g.add_risk_with_edges(risk, "ch_001");

        let snap = g.snapshot();
        assert!(snap.laws.contains_key("《测试法》第1条"));
        assert_eq!(snap.laws["《测试法》第1条"].law_id, "《测试法》第1条");
    }
}
