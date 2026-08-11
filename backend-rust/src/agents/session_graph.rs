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
//! SessionGraph 的读写是 **eventually consistent**：
//! - 每个字段独立 `RwLock`，不保证跨字段的 ACID 事务
//! - Agent 不应假设"读到 Risk 节点就一定有关联的 has_risk 边"
//! - 写入用 `add_risk_with_edges()` 在一次写锁内完成 Risk + has_risk + cites，
//!   减少（但不消除）中间态窗口
//! - BlindSpot 在所有 Agent 完成后串行读取，此时图已静止，无并发问题
//!
//! ## 生命周期
//!
//! Session 结束销毁，不持久化。长期记忆（Neo4j + Qdrant）在 Phase 3+ 实现。

use crate::agents::types::*;
use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// 中期记忆：Session Knowledge Graph。
///
/// 线程安全的内存图，Agent 在审查过程中读写。
pub struct SessionGraph {
    /// 条款节点 (chunk_id → metadata)
    chunks: RwLock<HashMap<String, ChunkNode>>,
    /// 风险节点 (risk_id → RiskNode)
    risks: RwLock<HashMap<String, RiskNode>>,
    /// has_risk 边: chunk_id → Vec<risk_id>
    has_risk: RwLock<HashMap<String, Vec<String>>>,
    /// reviewed_by 边: chunk_id → Vec<AgentId>
    reviewed_by: RwLock<HashMap<String, Vec<AgentId>>>,
    /// linked_to 边: chunk_id → Vec<LinkedChunk>
    linked_to: RwLock<HashMap<String, Vec<LinkedChunk>>>,
    /// cites 边: risk_id → Vec<law_ref>（"哪些风险引用了此法条？"）
    cites: RwLock<HashMap<String, Vec<String>>>,
    /// cited_by 反向索引: law_ref → Vec<risk_id>（"此法条被哪些风险引用？"）
    cited_by: RwLock<HashMap<String, Vec<String>>>,
    /// contradicts 边: chunk_id → Vec<(other_chunk_id, reason)>
    contradicts: RwLock<HashMap<String, Vec<(String, String)>>>,
    /// same_law 物化边: chunk_id → Vec<other_chunk_id>
    same_law: RwLock<HashMap<String, Vec<String>>>,
    /// Agent 节点: agent_id → AgentNode
    agents: RwLock<HashMap<AgentId, AgentNode>>,
    /// Law 节点: law_id → LawNode
    laws: RwLock<HashMap<String, LawNode>>,
    /// Case 节点: case_id → CaseNode
    cases: RwLock<HashMap<String, CaseNode>>,
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
            chunks: RwLock::new(HashMap::new()),
            risks: RwLock::new(HashMap::new()),
            has_risk: RwLock::new(HashMap::new()),
            reviewed_by: RwLock::new(HashMap::new()),
            linked_to: RwLock::new(HashMap::new()),
            cites: RwLock::new(HashMap::new()),
            cited_by: RwLock::new(HashMap::new()),
            contradicts: RwLock::new(HashMap::new()),
            same_law: RwLock::new(HashMap::new()),
            agents: RwLock::new(HashMap::new()),
            laws: RwLock::new(HashMap::new()),
            cases: RwLock::new(HashMap::new()),
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

    // ── 写入 (Agent 调用) ──────────────────────────────────────

    /// 添加条款节点。
    pub fn add_chunk(&self, chunk: ChunkNode) {
        if let Ok(mut chunks) = self.chunks.write() {
            chunks.insert(chunk.chunk_id.clone(), chunk);
        }
    }

    /// 批量添加条款节点（Coordinator PRELOAD 阶段）。
    pub fn add_chunks(&self, chunks: Vec<ChunkNode>) {
        if let Ok(mut map) = self.chunks.write() {
            for c in chunks {
                map.insert(c.chunk_id.clone(), c);
            }
        }
    }

    /// 添加风险节点。
    pub fn add_risk(&self, mut risk: RiskNode) {
        // 从 RiskFinding.legal_basis 提取法条引用
        risk.law_refs = risk.finding.legal_basis.clone();
        if let Ok(mut risks) = self.risks.write() {
            risks.insert(risk.finding.risk_id.clone(), risk);
        }
    }

    /// 添加 has_risk 边（chunk → risk）。
    pub fn add_has_risk(&self, chunk_id: &str, risk_id: &str) {
        if let Ok(mut edges) = self.has_risk.write() {
            edges
                .entry(chunk_id.to_string())
                .or_default()
                .push(risk_id.to_string());
        }
    }

    /// 记录 Agent 已审查某条款。
    pub fn add_reviewed_by(&self, chunk_id: &str, agent: AgentId) {
        if let Ok(mut edges) = self.reviewed_by.write() {
            let entry = edges.entry(chunk_id.to_string()).or_default();
            if !entry.contains(&agent) {
                entry.push(agent);
            }
        }
    }

    /// 添加 linked_to 边（条款间关联）。
    pub fn add_linked_to(&self, from: &str, to: &str, reason: &str) {
        if let Ok(mut edges) = self.linked_to.write() {
            edges
                .entry(from.to_string())
                .or_default()
                .push(LinkedChunk {
                    chunk_id: to.to_string(),
                    reason: reason.to_string(),
                });
        }
    }

    /// 写入 Agent 节点（Coordinator PRELOAD 阶段调用）。
    pub fn add_agent(&self, agent: AgentNode) {
        let agent_id = agent.agent_id.clone();
        if let Ok(mut agents) = self.agents.write() {
            agents.insert(agent_id, agent);
        }
    }

    /// 双向写入矛盾边（Agent 调用 search_contradiction 工具时触发）。
    pub fn add_contradicts(&self, chunk_a: &str, chunk_b: &str, reason: &str) {
        if let Ok(mut edges) = self.contradicts.write() {
            edges
                .entry(chunk_a.to_string())
                .or_default()
                .push((chunk_b.to_string(), reason.to_string()));
            edges
                .entry(chunk_b.to_string())
                .or_default()
                .push((chunk_a.to_string(), reason.to_string()));
        }
    }

    /// 查询某条款的矛盾关系。
    pub fn query_contradictions(&self, chunk_id: &str) -> Vec<(String, String)> {
        self.contradicts
            .read()
            .ok()
            .and_then(|edges| edges.get(chunk_id).cloned())
            .unwrap_or_default()
    }

    /// 查询某条款的 same_law 关联。
    pub fn query_same_law_edges(&self, chunk_id: &str) -> Vec<String> {
        self.same_law
            .read()
            .ok()
            .and_then(|edges| edges.get(chunk_id).cloned())
            .unwrap_or_default()
    }

    /// 自动推导 same_law 边：扫描 cited_by → has_risk，找到共享同一法条的 chunk。
    ///
    /// 在 `add_risk_with_edges()` 末尾调用。
    fn derive_same_law_edges(&self, law_refs: &[String], chunk_id: &str) {
        if law_refs.is_empty() {
            return;
        }

        // 收集引用相同法条的其他 risk_id
        let mut related_risk_ids: Vec<String> = Vec::new();
        if let Ok(cited_by) = self.cited_by.read() {
            for law_ref in law_refs {
                if let Some(risk_ids) = cited_by.get(law_ref) {
                    for rid in risk_ids {
                        if !related_risk_ids.contains(rid) {
                            related_risk_ids.push(rid.clone());
                        }
                    }
                }
            }
        }

        if related_risk_ids.is_empty() {
            return;
        }

        // 通过 has_risk 反查 chunk_id → 写入 same_law 边
        if let (Ok(_has_risk), Ok(risks_map)) = (self.has_risk.read(), self.risks.read()) {
            for rid in &related_risk_ids {
                if let Some(rn) = risks_map.get(rid) {
                    for other_cid in &rn.finding.clause_ids {
                        if other_cid == chunk_id {
                            continue;
                        }
                        // 双向写入
                        if let Ok(mut same_law_edges) = self.same_law.write() {
                            let entry = same_law_edges.entry(chunk_id.to_string()).or_default();
                            if !entry.contains(other_cid) {
                                entry.push(other_cid.clone());
                            }
                            let reverse = same_law_edges.entry(other_cid.clone()).or_default();
                            if !reverse.contains(&chunk_id.to_string()) {
                                reverse.push(chunk_id.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    /// 添加 cites 边 + cited_by 反向索引（单条法条引用）。
    pub fn add_cites(&self, risk_id: &str, law_ref: &str) {
        // cites: risk_id → [law_ref]
        if let Ok(mut cites) = self.cites.write() {
            cites
                .entry(risk_id.to_string())
                .or_default()
                .push(law_ref.to_string());
        }
        // cited_by 反向索引: law_ref → [risk_id]
        if let Ok(mut cited_by) = self.cited_by.write() {
            cited_by
                .entry(law_ref.to_string())
                .or_default()
                .push(risk_id.to_string());
        }
    }

    /// 原子写入 Risk 节点 + has_risk 边 + cites 边。
    ///
    /// 在一次写锁内完成 Risk + has_risk + cites，减少（但不消除）中间态窗口。
    /// 这是 Agent 审查完成后的主要写入入口。
    pub fn add_risk_with_edges(&self, risk: RiskNode, chunk_id: &str) {
        let risk_id = risk.finding.risk_id.clone();
        let law_refs = risk.finding.legal_basis.clone();

        // 1. 写入 Risk 节点
        {
            if let Ok(mut risks) = self.risks.write() {
                risks.insert(risk_id.clone(), risk);
            }
        }

        // 2. 写入 has_risk 边
        {
            if let Ok(mut edges) = self.has_risk.write() {
                edges
                    .entry(chunk_id.to_string())
                    .or_default()
                    .push(risk_id.clone());
            }
        }

        // 3. 写入 cites + cited_by 边
        {
            if let Ok(mut cites) = self.cites.write() {
                for law_ref in &law_refs {
                    cites
                        .entry(risk_id.clone())
                        .or_default()
                        .push(law_ref.clone());
                }
            }
            if let Ok(mut cited_by) = self.cited_by.write() {
                for law_ref in &law_refs {
                    cited_by
                        .entry(law_ref.clone())
                        .or_default()
                        .push(risk_id.clone());
                }
            }
        }

        // 4. 存储 Law 节点（如果还不存在）
        {
            if let Ok(mut laws) = self.laws.write() {
                for law_ref in &law_refs {
                    // 用 law_ref 作为 law_id
                    if !laws.contains_key(law_ref) {
                        laws.insert(
                            law_ref.clone(),
                            LawNode {
                                law_id: law_ref.clone(),
                                article_no: law_ref.clone(),
                                title: String::new(),
                            },
                        );
                    }
                }
            }
        }

        // 5. 自动推导 same_law 边
        self.derive_same_law_edges(&law_refs, chunk_id);
    }

    /// 写入 Hypothesis（轻量版 add_risk_with_edges）。
    ///
    /// 与 add_risk_with_edges 的区别:
    /// - 不创建 Law 节点（Hypothesis 的法规名未验证，可能是幻觉）
    /// - 不触发 same_law 推导（避免未验证信息污染图拓扑）
    /// - 只写入 Risk 节点 + has_risk 边 + cites 边（单向）
    pub fn add_hypothesis(&self, risk: RiskNode, chunk_id: &str) {
        let risk_id = risk.finding.risk_id.clone();
        let law_refs = risk.finding.legal_basis.clone();

        // 1. Risk 节点
        if let Ok(mut risks) = self.risks.write() {
            risks.insert(risk_id.clone(), risk);
        }

        // 2. has_risk 边
        if let Ok(mut edges) = self.has_risk.write() {
            edges
                .entry(chunk_id.to_string())
                .or_default()
                .push(risk_id.clone());
        }

        // 3. cites 边（仅单向，不建 cited_by 反向索引，不触发 same_law）
        if let Ok(mut cites) = self.cites.write() {
            cites.entry(risk_id).or_default().extend(law_refs);
        }
    }

    /// 查询所有 Hypothesis（BlindSpot 用）。
    pub fn get_hypotheses(&self) -> Vec<RiskFinding> {
        self.risks
            .read()
            .ok()
            .map(|m| {
                m.values()
                    .filter(|r| r.finding.finding_role == FindingRole::Hypothesis)
                    .map(|r| r.finding.clone())
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

    /// 查询某个 Chunk 的完整上下文："谁审过这条？发现了什么风险？跟哪些条款有关联？"
    pub fn query_clause_context(&self, chunk_id: &str) -> ClauseContext {
        let reviewed_by = self
            .reviewed_by
            .read()
            .ok()
            .and_then(|edges| edges.get(chunk_id).cloned())
            .unwrap_or_default();

        // 通过 has_risk 边获取关联的 risk_id 列表，再查找 RiskNode
        let risk_ids: Vec<String> = self
            .has_risk
            .read()
            .ok()
            .and_then(|edges| edges.get(chunk_id).cloned())
            .unwrap_or_default();

        let risks: Vec<RiskFinding> = if let Ok(risks_map) = self.risks.read() {
            risk_ids
                .iter()
                .filter_map(|rid| risks_map.get(rid))
                .map(|rn| rn.finding.clone())
                .collect()
        } else {
            Vec::new()
        };

        let linked_chunks = self
            .linked_to
            .read()
            .ok()
            .and_then(|edges| edges.get(chunk_id).cloned())
            .unwrap_or_default();

        // 通过 cited_by 反向索引查找引用相同法条的其他 chunk
        let same_law_chunks: Vec<String> = {
            let mut result = Vec::new();
            if let (Ok(cited_by), Ok(_has_risk), Ok(risks_map)) = (
                self.cited_by.read(),
                self.has_risk.read(),
                self.risks.read(),
            ) {
                // 获取当前条款的所有风险的法条引用
                let mut all_law_refs: Vec<String> = Vec::new();
                for rid in &risk_ids {
                    if let Some(rn) = risks_map.get(rid) {
                        all_law_refs.extend(rn.law_refs.clone());
                    }
                }
                // 对每条法条，查找引用它的其他风险
                for law_ref in &all_law_refs {
                    if let Some(citing_risk_ids) = cited_by.get(law_ref) {
                        for citing_rid in citing_risk_ids {
                            // 找到引用此法条的风险节点，再通过 has_risk 反查 chunk
                            if let Some(citing_rn) = risks_map.get(citing_rid) {
                                for cid in &citing_rn.finding.clause_ids {
                                    if cid != chunk_id && !result.contains(cid) {
                                        result.push(cid.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            result
        };

        let contradictions = self
            .contradicts
            .read()
            .ok()
            .and_then(|edges| edges.get(chunk_id).cloned())
            .map(|pairs| {
                pairs
                    .into_iter()
                    .map(|(other_id, reason)| LinkedChunk {
                        chunk_id: other_id,
                        reason,
                    })
                    .collect()
            })
            .unwrap_or_default();

        ClauseContext {
            chunk_id: chunk_id.to_string(),
            reviewed_by,
            risks,
            linked_chunks,
            same_law_chunks,
            contradictions,
        }
    }

    /// 查询引用同一法条的所有 chunk_id（通过 cited_by 反向索引 O(1) 查询）。
    pub fn query_same_law_chunks(&self, law_ref: &str) -> Vec<String> {
        let mut result = Vec::new();
        if let (Ok(cited_by), Ok(_has_risk), Ok(risks_map)) = (
            self.cited_by.read(),
            self.has_risk.read(),
            self.risks.read(),
        ) && let Some(risk_ids) = cited_by.get(law_ref)
        {
            for rid in risk_ids {
                if let Some(rn) = risks_map.get(rid) {
                    for cid in &rn.finding.clause_ids {
                        if !result.contains(cid) {
                            result.push(cid.clone());
                        }
                    }
                }
            }
        }
        result
    }

    /// 获取完整图快照（BlindSpot / 审计用）。
    ///
    /// 在所有 Agent 完成后串行调用，此时图已静止。
    pub fn snapshot(&self) -> GraphSnapshot {
        GraphSnapshot {
            chunks: self
                .chunks
                .read()
                .ok()
                .map(|g| g.clone())
                .unwrap_or_default(),
            risks: self
                .risks
                .read()
                .ok()
                .map(|g| g.clone())
                .unwrap_or_default(),
            has_risk: self
                .has_risk
                .read()
                .ok()
                .map(|g| g.clone())
                .unwrap_or_default(),
            reviewed_by: self
                .reviewed_by
                .read()
                .ok()
                .map(|g| g.clone())
                .unwrap_or_default(),
            linked_to: self
                .linked_to
                .read()
                .ok()
                .map(|g| g.clone())
                .unwrap_or_default(),
            cites: self
                .cites
                .read()
                .ok()
                .map(|g| g.clone())
                .unwrap_or_default(),
            cited_by: self
                .cited_by
                .read()
                .ok()
                .map(|g| g.clone())
                .unwrap_or_default(),
            agents: self
                .agents
                .read()
                .ok()
                .map(|g| g.clone())
                .unwrap_or_default(),
            laws: self.laws.read().ok().map(|g| g.clone()).unwrap_or_default(),
            cases: self
                .cases
                .read()
                .ok()
                .map(|g| g.clone())
                .unwrap_or_default(),
            contradicts: self
                .contradicts
                .read()
                .ok()
                .map(|g| g.clone())
                .unwrap_or_default(),
            same_law: self
                .same_law
                .read()
                .ok()
                .map(|g| g.clone())
                .unwrap_or_default(),
            review_attempts: HashMap::new(),
        }
    }

    /// 获取图中的条款总数。
    pub fn chunk_count(&self) -> usize {
        self.chunks.read().ok().map(|c| c.len()).unwrap_or(0)
    }

    /// 获取图中的风险总数。
    pub fn risk_count(&self) -> usize {
        self.risks.read().ok().map(|r| r.len()).unwrap_or(0)
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
