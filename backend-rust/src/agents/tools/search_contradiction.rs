//! `search_contradiction` 工具 — 检测文档中两个条款之间是否存在矛盾或隐性升级。
//!
//! ## 典型矛盾类型
//!
//! ① 资格门槛二级但评分给一级额外加分 → 隐性升级门槛 (implicit_upgrade)
//! ② 技术需求要求'兼容现有系统'但未说明现有系统的技术规格 → 悬空引用 (dangling_reference)
//! ③ 付款条款与预算条款总金额不一致 → 数据矛盾 (data_inconsistency)
//! ④ 资格要求'不接受联合体'但合同条款要求'联合体成员承担连带责任' → 逻辑矛盾 (logic_conflict)
//!
//! ## 核心逻辑
//!
//! 1. 如果 clause_b 为 null → 自动搜索可能的矛盾对（语义相似 + 可能矛盾）
//! 2. 如果 clause_b 提供 → 精确对比两个条款
//! 3. 结果写入 SessionGraph 的 contradicts 边

use anyhow::{Result, anyhow};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::agents::session_graph::SessionGraph;
use crate::domain::chunk::Chunk;

use super::AgentTool;

/// `search_contradiction` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct SearchContradictionArgs {
    /// 第一个条款 chunk_id
    pub clause_a: String,
    /// 第二个条款 chunk_id，或 null 表示自动搜索
    #[serde(default)]
    pub clause_b: Option<String>,
}

/// 矛盾检测结果。
#[derive(Debug, serde::Serialize)]
struct ContradictionResult {
    /// 矛盾类型
    contradiction_type: ContradictionType,
    /// 涉及的两个条款
    clause_pair: (String, Option<String>),
    /// 关键差异对比
    contrast: String,
    /// 严重程度
    severity: String,
    /// 法条依据（如有）
    legal_basis: Option<String>,
    /// 自动发现的候选矛盾对列表（clause_b = null 时返回）
    candidates: Vec<ContradictionCandidate>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ContradictionType {
    /// 隐性升级：低门槛进 → 高门槛赢
    ImplicitUpgrade,
    /// 悬空引用：引用不存在的实体
    DanglingReference,
    /// 数据矛盾：数值不一致
    DataInconsistency,
    /// 逻辑矛盾：语义冲突
    LogicConflict,
    /// 未发现矛盾
    NoContradiction,
}

impl ContradictionType {
    fn from_str(s: &str) -> Self {
        match s {
            "implicit_upgrade" => ContradictionType::ImplicitUpgrade,
            "dangling_reference" => ContradictionType::DanglingReference,
            "data_inconsistency" => ContradictionType::DataInconsistency,
            "logic_conflict" => ContradictionType::LogicConflict,
            _ => ContradictionType::NoContradiction,
        }
    }

    fn as_str(&self) -> &str {
        match self {
            ContradictionType::ImplicitUpgrade => "implicit_upgrade",
            ContradictionType::DanglingReference => "dangling_reference",
            ContradictionType::DataInconsistency => "data_inconsistency",
            ContradictionType::LogicConflict => "logic_conflict",
            ContradictionType::NoContradiction => "no_contradiction",
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct ContradictionCandidate {
    chunk_id: String,
    section_path: Vec<String>,
    contradiction_type: String,
    contrast: String,
    text_preview: String,
}

/// `search_contradiction` 工具实现。
pub struct SearchContradictionTool {
    /// Chunk ID → Chunk 映射表
    pub chunks: Arc<HashMap<String, Chunk>>,
    /// 有序 chunk_id 列表（用于自动搜索时的遍历）
    pub chunk_order: Arc<Vec<String>>,
    /// SessionGraph 引用（用于读写矛盾边）
    pub graph: Option<Arc<SessionGraph>>,
}

impl SearchContradictionTool {
    pub fn new(
        chunks: Arc<HashMap<String, Chunk>>,
        chunk_order: Arc<Vec<String>>,
        graph: Option<Arc<SessionGraph>>,
    ) -> Self {
        Self {
            chunks,
            chunk_order,
            graph,
        }
    }

    /// 对比两个条款，检测矛盾。
    fn compare_pair(
        chunk_a: &Chunk,
        chunk_b: &Chunk,
    ) -> Option<(ContradictionType, String, Option<String>)> {
        let text_a = &chunk_a.text.to_lowercase();
        let text_b = &chunk_b.text.to_lowercase();

        // ① 隐性升级：资格门槛低但评分给高
        let implicit_upgrade = Self::check_implicit_upgrade(text_a, text_b);
        if let Some(contrast) = implicit_upgrade {
            return Some((
                ContradictionType::ImplicitUpgrade,
                contrast,
                Some("《政府采购法实施条例》第20条：不得以不合理条件对供应商实行差别待遇或者歧视待遇".into()),
            ));
        }

        // ② 逻辑矛盾：正反互斥
        let logic_conflict = Self::check_logic_conflict(text_a, text_b);
        if let Some(contrast) = logic_conflict {
            return Some((ContradictionType::LogicConflict, contrast, None));
        }

        // ③ 数据矛盾：数字不一致
        let data_inconsistency = Self::check_data_inconsistency(text_a, text_b);
        if let Some(contrast) = data_inconsistency {
            return Some((ContradictionType::DataInconsistency, contrast, None));
        }

        // ④ 悬空引用
        let dangling = Self::check_dangling_reference(text_a, text_b);
        if let Some(contrast) = dangling {
            return Some((ContradictionType::DanglingReference, contrast, None));
        }

        None
    }

    /// 检测隐性升级模式：
    /// A 说"资格：二级资质即可" → B 说"有一级资质加 5 分"
    fn check_implicit_upgrade(text_a: &str, text_b: &str) -> Option<String> {
        let qual_keywords = ["资质", "等级", "资格", "门槛"];
        let score_keywords = ["加分", "评分", "优先", "额外"];

        let a_has_qual = qual_keywords.iter().any(|k| text_a.contains(k));
        let b_has_score = score_keywords.iter().any(|k| text_b.contains(k));

        if a_has_qual && b_has_score {
            // 检测数值模式：A 中的低数值 vs B 中的高数值
            let extract_level = |t: &str| -> Option<u32> {
                for kw in &["一级", "二级", "三级", "甲级", "乙级", "丙级"] {
                    if t.contains(kw) {
                        return Some(match *kw {
                            "一级" | "甲级" => 1,
                            "二级" | "乙级" => 2,
                            "三级" | "丙级" => 3,
                            _ => 0,
                        });
                    }
                }
                None
            };

            if let (Some(level_a), Some(level_b)) = (extract_level(text_a), extract_level(text_b))
                && level_a > level_b
            {
                // A 要求较低（二级=2），B 奖励较高（一级=1）
                return Some(format!(
                    "隐性升级：条款 A 接受低资质（第{}级），但条款 B 给高资质（第{}级）额外加分——形成事实上的资质升级",
                    level_a, level_b
                ));
            }
            // 通用隐性升级提示
            return Some(
                "可能存在隐性升级：一个条款设定了资格门槛，另一个条款在评分中给超出门槛的条件额外加分".into()
            );
        }

        None
    }

    /// 检测逻辑矛盾：A 说 X，B 说 非X
    fn check_logic_conflict(text_a: &str, text_b: &str) -> Option<String> {
        let conflict_pairs: &[(&str, &str, &str)] = &[
            (
                "不接受联合体",
                "联合体",
                "联合体投标矛盾：一个条款不接受，另一个条款涉及联合体",
            ),
            (
                "不允许分包",
                "分包",
                "分包矛盾：一个条款不允许，另一个条款涉及分包",
            ),
            ("不允许转包", "转包", "转包矛盾"),
            ("资格后审", "资格预审", "资格审查方式矛盾"),
            ("最低评标价法", "综合评分法", "评标方法矛盾"),
            ("不接受替代方案", "备选方案", "备选方案矛盾"),
            ("不接受进口", "进口产品", "进口产品政策矛盾"),
        ];

        for (neg_pattern, pos_pattern, desc) in conflict_pairs {
            if text_a.contains(neg_pattern) && text_b.contains(pos_pattern) {
                return Some(desc.to_string());
            }
            if text_b.contains(neg_pattern) && text_a.contains(pos_pattern) {
                return Some(desc.to_string());
            }
        }

        None
    }

    /// 检测数据矛盾：相同数据项的不同数值
    fn check_data_inconsistency(text_a: &str, text_b: &str) -> Option<String> {
        // 提取文本中的金额/比例数值
        let extract_numbers = |t: &str| -> Vec<f64> {
            let mut nums = Vec::new();
            let chars: Vec<char> = t.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                if chars[i].is_ascii_digit() {
                    let start = i;
                    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                        i += 1;
                    }
                    if let Ok(n) = chars[start..i].iter().collect::<String>().parse::<f64>()
                        && n > 0.0
                    {
                        nums.push(n);
                    }
                } else {
                    i += 1;
                }
            }
            nums
        };

        // 查找共同的关键上下文词
        let context_words = [
            "保证金",
            "合同金额",
            "预算",
            "付款",
            "预付款",
            "质保金",
            "履约",
        ];
        let has_shared_context = context_words
            .iter()
            .any(|w| text_a.contains(w) && text_b.contains(w));

        if has_shared_context {
            let nums_a = extract_numbers(text_a);
            let nums_b = extract_numbers(text_b);

            // 对比相邻数量级的数值
            for &na in &nums_a {
                for &nb in &nums_b {
                    if (na - nb).abs() > 1.0 && (na / nb).max(nb / na) < 100.0 {
                        return Some(format!(
                            "数据不一致：相同上下文中的数值差异。A={}, B={}",
                            na, nb
                        ));
                    }
                }
            }
        }

        None
    }

    /// 检测悬空引用
    fn check_dangling_reference(text_a: &str, text_b: &str) -> Option<String> {
        let ref_patterns = ["详见", "参照", "按照", "参见", "见附件", "见附表"];
        for pat in &ref_patterns {
            if text_a.contains(pat) && !text_b.contains(pat) {
                return Some(format!(
                    "可能的悬空引用：条款 A 包含'{}'引用，但关联条款中未找到对应内容",
                    pat
                ));
            }
        }
        None
    }

    /// 自动搜索可能的矛盾对（当 clause_b = null 时）。
    fn find_candidates(&self, clause_a_id: &str) -> Vec<ContradictionCandidate> {
        let mut candidates = Vec::new();

        let chunk_a = match self.chunks.get(clause_a_id) {
            Some(c) => c,
            None => return candidates,
        };

        // 遍历所有其他 chunk，检测矛盾信号
        for other_id in self.chunk_order.iter() {
            if other_id == clause_a_id {
                continue;
            }
            if let Some(chunk_b) = self.chunks.get(other_id)
                && let Some((ct, contrast, _legal)) = Self::compare_pair(chunk_a, chunk_b)
            {
                candidates.push(ContradictionCandidate {
                    chunk_id: other_id.clone(),
                    section_path: chunk_b.section_path.clone(),
                    contradiction_type: ct.as_str().to_string(),
                    contrast,
                    text_preview: chunk_b.text.chars().take(200).collect(),
                });
            }
        }

        // 限制最多返回 10 个候选
        candidates.truncate(10);
        candidates
    }
}

#[async_trait::async_trait]
impl AgentTool for SearchContradictionTool {
    fn name(&self) -> &str {
        "search_contradiction"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "search_contradiction",
                "description": "【使用场景】检测文档中两个条款之间是否存在矛盾或隐性升级。\
                    ① 资格门槛二级但评分给一级额外加分 → 隐性升级门槛；\
                    ② 技术需求要求'兼容现有系统'但未说明现有系统的技术规格 → 悬空引用；\
                    ③ 付款条款（进度款80%+尾款20%）与预算条款（总金额不一致）→ 数据矛盾；\
                    ④ 资格要求'不接受联合体'但合同条款要求'联合体成员承担连带责任'→ 逻辑矛盾。\
                    【不使用场景】\
                    ① 单条条款本身的合规性判断——用 ReAct 审查流程 + web_search；\
                    ② 两个条款只是语义相近但没有矛盾——'相似'≠'矛盾'；\
                    ③ 没有具体疑点的全局扫描——这会返回大量噪音，浪费轮次。\
                    矛盾检测的前提是你已经怀疑两个条款之间存在冲突。\
                    【注意】如果 clause_b 为 null，工具在文档中自动搜索可能的矛盾对——\
                    但需谨慎使用，优先传入具体怀疑的条款对。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "clause_a": {
                            "type": "string",
                            "description": "第一个条款 chunk_id，如 'ch_042'"
                        },
                        "clause_b": {
                            "type": "string",
                            "description": "第二个条款 chunk_id，或 null 表示自动搜索可能的矛盾对"
                        }
                    },
                    "required": ["clause_a"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: SearchContradictionArgs = serde_json::from_value(args)?;

        let chunk_a = self
            .chunks
            .get(&parsed.clause_a)
            .ok_or_else(|| anyhow!("chunk_id 不存在: {}", parsed.clause_a))?;

        // 如果提供了 clause_b，精确对比
        if let Some(ref clause_b_id) = parsed.clause_b {
            let chunk_b = self
                .chunks
                .get(clause_b_id)
                .ok_or_else(|| anyhow!("chunk_id 不存在: {}", clause_b_id))?;

            let (ct, contrast, legal_basis) =
                Self::compare_pair(chunk_a, chunk_b).unwrap_or_else(|| {
                    (
                        ContradictionType::NoContradiction,
                        "未发现明显矛盾".to_string(),
                        None,
                    )
                });

            // 如果发现矛盾，写入 SessionGraph
            if !matches!(ct, ContradictionType::NoContradiction)
                && let Some(ref graph) = self.graph
            {
                graph.add_contradicts(&parsed.clause_a, clause_b_id, &contrast);
            }

            let result = ContradictionResult {
                contradiction_type: ct,
                clause_pair: (parsed.clause_a.clone(), parsed.clause_b.clone()),
                contrast,
                severity: "medium".to_string(),
                legal_basis,
                candidates: Vec::new(),
            };

            Ok(serde_json::to_value(&result)?)
        } else {
            // 自动搜索矛盾对
            let candidates = self.find_candidates(&parsed.clause_a);

            // 如果找到候选，取第一个作为主要矛盾
            let (ct, contrast, severity) = if candidates.is_empty() {
                (
                    ContradictionType::NoContradiction,
                    "未发现与其他条款的明显矛盾".to_string(),
                    "info".to_string(),
                )
            } else {
                // 将第一个候选的矛盾写入 SessionGraph
                if let Some(ref graph) = self.graph {
                    graph.add_contradicts(
                        &parsed.clause_a,
                        &candidates[0].chunk_id,
                        &candidates[0].contrast,
                    );
                }
                (
                    ContradictionType::from_str(&candidates[0].contradiction_type),
                    candidates[0].contrast.clone(),
                    "medium".to_string(),
                )
            };

            let result = ContradictionResult {
                contradiction_type: ct,
                clause_pair: (
                    parsed.clause_a.clone(),
                    candidates.first().map(|c| c.chunk_id.clone()),
                ),
                contrast,
                severity: severity.to_string(),
                legal_basis: None,
                candidates,
            };

            Ok(serde_json::to_value(&result)?)
        }
    }
}

// ─── 测试 ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chunk::{Chunk, ChunkType};

    fn make_chunk(id: &str, text: &str) -> Chunk {
        Chunk {
            chunk_id: id.to_string(),
            chunk_type: ChunkType::Leaf,
            section_path: vec!["测试".to_string()],
            text: text.to_string(),
            page_start: 0,
            page_end: 1,
            source_block_ids: vec![],
            bbox_refs: vec![],
        }
    }

    #[test]
    fn test_implicit_upgrade_detected() {
        // "资质"关键词命中qual_keywords, "加分"关键词命中score_keywords
        let a = make_chunk("ch_A", "投标人须具备二级及以上资质");
        let b = make_chunk("ch_B", "具有一级资质的投标人额外加分");
        let result = SearchContradictionTool::compare_pair(&a, &b);
        assert!(result.is_some());
        assert!(matches!(
            result.unwrap().0,
            ContradictionType::ImplicitUpgrade
        ));
    }

    #[test]
    fn test_logic_conflict_joint_venture() {
        let a = make_chunk("ch_A", "本项目不接受联合体投标");
        let b = make_chunk("ch_B", "联合体各方须承担连带责任");
        let result = SearchContradictionTool::compare_pair(&a, &b);
        assert!(result.is_some());
        assert!(matches!(
            result.unwrap().0,
            ContradictionType::LogicConflict
        ));
    }

    #[test]
    fn test_no_contradiction_for_similar_clauses() {
        let a = make_chunk("ch_A", "投标人须具备建筑工程施工总承包资质");
        let b = make_chunk("ch_B", "投标人须具备有效的安全生产许可证");
        let result = SearchContradictionTool::compare_pair(&a, &b);
        assert!(result.is_none());
    }

    #[test]
    fn test_data_inconsistency() {
        let a = make_chunk("ch_A", "合同金额为500万元，履约保证金为50万元");
        let b = make_chunk("ch_B", "合同金额为480万元，履约保证金为48万元");
        let result = SearchContradictionTool::compare_pair(&a, &b);
        assert!(result.is_some());
        assert!(matches!(
            result.unwrap().0,
            ContradictionType::DataInconsistency
        ));
    }
}
