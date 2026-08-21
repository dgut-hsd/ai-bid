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
            ReviewAttemptOutcome::Findings if finding_ids.is_empty() => {
                return Err("Findings 结果的 finding_ids 不得为空".to_string());
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

    fn upsert_provisional_node_in_state(
        state: &mut GraphState,
        node: &RiskNode,
    ) -> (Vec<String>, bool) {
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
        for law_ref in &node.law_refs {
            if push_unique(
                state.cites.entry(risk_id.clone()).or_default(),
                law_ref.clone(),
            ) {
                changed = true;
            }
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
        for chunk_id in &node.finding.clause_ids {
            let same_law_affected =
                Self::derive_same_law_edges_in_state(state, &node.law_refs, chunk_id);
            if !same_law_affected.is_empty() {
                changed = true;
                affected.extend(same_law_affected);
            }
        }
        if changed {
            affected.extend(node.finding.clause_ids.clone());
        }
        (deduplicate_strings(&affected), changed)
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
        for node in &nodes {
            let (node_affected, _) = Self::upsert_provisional_node_in_state(&mut state, node);
            affected.extend(node_affected);
        }
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
        let mut changed = false;
        for node in &nodes {
            let (node_affected, node_changed) =
                Self::upsert_provisional_node_in_state(&mut state, node);
            affected.extend(node_affected);
            changed |= node_changed;
        }
        if !changed {
            return Ok(GraphCommit {
                graph_version: state.graph_version,
                chunk_versions: HashMap::new(),
            });
        }
        let affected = deduplicate_strings(&affected);
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
        if let Ok(mut state) = self.state.write() {
            let chunk_ids = risk.finding.clause_ids.clone();
            state.risks.insert(risk.finding.risk_id.clone(), risk);
            bump_versions(&mut state, chunk_ids);
        }
    }

    /// 添加 has_risk 边（chunk → risk）。
    pub fn add_has_risk(&self, chunk_id: &str, risk_id: &str) {
        if let Ok(mut state) = self.state.write()
            && push_unique(
                state.has_risk.entry(chunk_id.to_string()).or_default(),
                risk_id.to_string(),
            )
        {
            bump_versions(&mut state, [chunk_id.to_string()]);
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
            .and_then(|state| state.same_law.get(chunk_id).cloned())
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
            bump_versions(&mut state, affected);
        }
    }

    /// 写入 Hypothesis（不创建 Law 节点和 same_law 边）。
    pub fn add_hypothesis(&self, mut risk: RiskNode, chunk_id: &str) {
        let risk_id = risk.finding.risk_id.clone();
        let law_refs = risk.finding.legal_basis.clone();
        risk.law_refs = law_refs.clone();
        if let Ok(mut state) = self.state.write() {
            state.risks.insert(risk_id.clone(), risk);
            push_unique(
                state.has_risk.entry(chunk_id.to_string()).or_default(),
                risk_id.clone(),
            );
            for law_ref in law_refs {
                push_unique(state.cites.entry(risk_id.clone()).or_default(), law_ref);
            }
            bump_versions(&mut state, [chunk_id.to_string()]);
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
            .map(|risk| risk.finding.clone())
            .collect();
        let linked_chunks = state.linked_to.get(chunk_id).cloned().unwrap_or_default();
        let same_law_chunks = state.same_law.get(chunk_id).cloned().unwrap_or_default();
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

    /// 查询引用同一法条的所有 chunk_id（通过 cited_by 反向索引 O(1) 查询）。
    pub fn query_same_law_chunks(&self, law_ref: &str) -> Vec<String> {
        let mut result = Vec::new();
        if let Ok(state) = self.state.read()
            && let Some(risk_ids) = state.cited_by.get(law_ref)
        {
            for risk_id in risk_ids {
                if let Some(risk) = state.risks.get(risk_id) {
                    for chunk_id in &risk.finding.clause_ids {
                        if !result.contains(chunk_id) {
                            result.push(chunk_id.clone());
                        }
                    }
                }
            }
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
                page_number: None,
                section_path: None,
                context: None,
            },
            law_refs: vec!["《测试法》第1条".to_string()],
            state: FindingState::Provisional,
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
    fn test_review_attempt_findings_requires_non_empty_finding_ids() {
        let graph = SessionGraph::new();
        graph.add_chunk(make_test_chunk("ch_001"));
        let attempt_id = graph
            .start_review_attempt(AgentId::FactCheck, "ch_001")
            .expect("应创建审查尝试");

        let error = graph
            .complete_review_attempt(&attempt_id, ReviewAttemptOutcome::Findings, Vec::new())
            .expect_err("Findings 结果必须关联至少一个 finding_id");

        assert!(error.contains("finding_ids"));
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
    fn test_review_attempt_completed_findings_records_ids_once() {
        let graph = SessionGraph::new();
        graph.add_chunk(make_test_chunk("ch_001"));
        let attempt_id = graph
            .start_review_attempt(AgentId::FactCheck, "ch_001")
            .expect("应创建审查尝试");

        graph
            .complete_review_attempt(
                &attempt_id,
                ReviewAttemptOutcome::Findings,
                vec!["R_001".to_string()],
            )
            .expect("Findings 应正常完成");
        assert!(
            graph
                .complete_review_attempt(
                    &attempt_id,
                    ReviewAttemptOutcome::Findings,
                    vec!["R_002".to_string()],
                )
                .is_err()
        );
        assert!(
            graph
                .fail_review_attempt(
                    &attempt_id,
                    ReviewAttemptErrorCode::TaskCancelled,
                    "重复失败",
                )
                .is_err()
        );

        let snapshot = graph.snapshot();
        let attempt = &snapshot.review_attempts[&attempt_id];
        assert_eq!(attempt.status, ReviewAttemptStatus::Completed);
        assert_eq!(attempt.outcome, Some(ReviewAttemptOutcome::Findings));
        assert_eq!(attempt.finding_ids, vec!["R_001"]);
        assert_eq!(snapshot.reviewed_by["ch_001"], vec![AgentId::FactCheck]);
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
