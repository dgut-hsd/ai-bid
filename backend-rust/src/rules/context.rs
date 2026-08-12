//! Agent 上下文构建 — build_agent_context。
//!
//! 本模块产出 AgentContext，但**不直接注入 prompts.rs**（硬约束：禁改其他模块）。
//! 上下文通过 `risk_taxonomy::review_candidates_for_agent` 间接流入 Agent（候选码更精准）。
//! 同时序列化为 JSON 供 `bin/test_rules.rs` 离线验证。

use crate::rules::schema::Rule;
use serde::{Deserialize, Serialize};

/// 一条规则的命中详情。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleHit {
    pub rule_id: String,
    pub category: String,
    pub severity: String,
    pub law_ref: String,
    pub evidence_hint: String,
}

/// Agent 上下文（规则引擎产出）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentContext {
    /// 命中的规则列表
    pub hits: Vec<RuleHit>,
    /// 已确认的问题数（critical）
    pub critical_count: usize,
    /// 待 Agent 深度语义审查的候选 canonical code
    pub candidate_categories: Vec<String>,
}

/// 根据命中的规则构建 Agent 上下文。
///
/// `matched_rules`: 对条款文本求值后命中的规则列表。
/// `agent`: 责任 Agent 名（用于过滤该 Agent 应关注的候选）。
pub fn build_agent_context(
    matched_rules: &[&Rule],
    agent: &str,
) -> AgentContext {
    let mut hits = Vec::new();
    let mut critical_count = 0;
    let mut candidates = Vec::new();

    for rule in matched_rules {
        if !rule.enabled {
            continue;
        }
        // 只纳入该 Agent 责任范围内的规则（owner_agent 一致，或 agent == RuleEngineAgent）
        let owner = crate::rules::catalog::owner_agent(&rule.category);
        if owner != agent && agent != "RuleEngineAgent" {
            continue;
        }
        hits.push(RuleHit {
            rule_id: rule.id.clone(),
            category: rule.category.clone(),
            severity: rule.severity.clone(),
            law_ref: rule.law_ref.clone(),
            evidence_hint: rule.source.excerpt.clone(),
        });
        if rule.severity.eq_ignore_ascii_case("critical") {
            critical_count += 1;
        }
        if !candidates.contains(&rule.category) {
            candidates.push(rule.category.clone());
        }
    }

    AgentContext {
        hits,
        critical_count,
        candidate_categories: candidates,
    }
}
