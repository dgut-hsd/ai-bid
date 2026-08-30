//! `compare_versions` 工具 — 标书版本差异对比。
//!
//! 当用户上传新版标书时，与旧版逐 chunk 对比，发现新增/删除/修改的条款。
//! 核心价值：锁定资格条件变更和评分标准变更——这两类变更是投诉高发区。
//!
//! ## 使用场景
//!
//! 1. 采购人修改标书后发布澄清公告 → 对比修改前后差异
//! 2. 同一项目不同批次的标书版本 → 检查倾向性调整
//!
//! ## 算法
//!
//! - chunk 级别匹配：按 section_path + 文本相似度对齐
//! - 文本 diff：LCS（最长公共子序列）字符级差异计算
//! - 变更分类：added / removed / modified
//! - 高风险变更自动标记

use anyhow::{Result, anyhow};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::chunk::Chunk;

use super::AgentTool;

/// `compare_versions` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct CompareVersionsArgs {
    /// 旧版文档的 chunk 数据（JSON 序列化的 Chunk 列表）
    pub previous_chunks: Vec<PreviousChunk>,
    /// 新版文档的 chunk 数据（JSON 序列化的 Chunk 列表）
    pub current_chunks: Vec<CurrentChunk>,
}

/// 旧版 Chunk 的简化表示。
#[derive(Debug, Deserialize)]
pub struct PreviousChunk {
    pub chunk_id: String,
    pub section_path: Vec<String>,
    pub text: String,
}

/// 新版 Chunk 的简化表示。
#[derive(Debug, Deserialize)]
pub struct CurrentChunk {
    pub chunk_id: String,
    pub section_path: Vec<String>,
    pub text: String,
}

/// 版本对比的整体结果。
#[derive(Debug, serde::Serialize)]
struct DiffResult {
    /// 新增的条款
    added: Vec<DiffItem>,
    /// 删除的条款
    removed: Vec<DiffItem>,
    /// 内容修改的条款
    modified: Vec<ModifiedItem>,
    /// 高风险变更（资格条件/评分标准修改）
    high_risk_changes: Vec<HighRiskChange>,
    /// 统计摘要
    stats: DiffStats,
    /// 文本摘要
    summary: String,
}

#[derive(Debug, serde::Serialize)]
struct DiffItem {
    chunk_id: String,
    section_path: Vec<String>,
    text_preview: String,
    page_location: String,
}

#[derive(Debug, serde::Serialize)]
struct ModifiedItem {
    prev_chunk_id: String,
    curr_chunk_id: String,
    section_path: Vec<String>,
    prev_text_preview: String,
    curr_text_preview: String,
    change_description: String,
    change_score: f64,
}

#[derive(Debug, serde::Serialize)]
struct HighRiskChange {
    change_type: String, // "qualification" | "scoring" | "deadline" | "deposit"
    section_path: Vec<String>,
    prev_content: String,
    curr_content: String,
    risk_detail: String,
}

#[derive(Debug, serde::Serialize)]
struct DiffStats {
    added_count: usize,
    removed_count: usize,
    modified_count: usize,
    high_risk_count: usize,
    unchanged_count: usize,
}

/// `compare_versions` 工具实现。
///
/// 持有新版 Chunk 索引，LLM 传入旧版 Chunks 时对比。
pub struct CompareVersionsTool {
    /// 当前（新）版本的 Chunk 索引
    pub current_chunks: Arc<HashMap<String, Chunk>>,
    /// 当前版本有序 chunk_id 列表
    pub current_order: Arc<Vec<String>>,
}

impl CompareVersionsTool {
    pub fn new(
        current_chunks: Arc<HashMap<String, Chunk>>,
        current_order: Arc<Vec<String>>,
    ) -> Self {
        Self {
            current_chunks,
            current_order,
        }
    }

    /// LCS 字符级差异计算：返回两个字符串的差异字符数。
    fn lcs_diff(a: &str, b: &str) -> usize {
        let a_chars: Vec<char> = a.chars().collect();
        let b_chars: Vec<char> = b.chars().collect();
        let m = a_chars.len();
        let n = b_chars.len();

        if m == 0 {
            return n;
        }
        if n == 0 {
            return m;
        }

        // 简化的 LCS 计算：使用两行滚动数组
        let mut prev = vec![0u32; n + 1];
        let mut curr = vec![0u32; n + 1];

        for i in 1..=m {
            for j in 1..=n {
                if a_chars[i - 1] == b_chars[j - 1] {
                    curr[j] = prev[j - 1] + 1;
                } else {
                    curr[j] = prev[j].max(curr[j - 1]);
                }
            }
            std::mem::swap(&mut prev, &mut curr);
        }

        let lcs_len = prev[n] as usize;
        m + n - 2 * lcs_len // 编辑距离近似
    }

    /// 计算编辑占比（0-1, 0=完全相同, 1=完全不同）
    fn change_score(a: &str, b: &str) -> f64 {
        let total = a.chars().count().max(b.chars().count()).max(1) as f64;
        let diff = Self::lcs_diff(a, b) as f64;
        (diff / total).min(1.0)
    }

    /// 检测变更类型是否为高风险。
    fn detect_high_risk(prev_text: &str, curr_text: &str) -> Option<HighRiskChange> {
        let qualification_keywords = [
            "资质", "资格", "证书", "许可证", "等级", "备案", "注册",
            "业绩", "合同", "项目经验", "案例", "注册资本", "资产",
        ];
        let scoring_keywords = [
            "评分", "得分", "分值", "权重", "价格分", "技术分", "商务分",
            "评审", "评估", "综合评分", "加分", "减分", "扣分",
        ];
        let deadline_keywords = [
            "截止", "期限", "日历日", "工作日", "交付", "工期", "进度",
            "投标截止", "开标", "公告期", "等标期",
        ];
        let deposit_keywords = [
            "保证金", "保函", "履约", "投标保证金", "履约保证金",
            "比例", "退还",
        ];

        // 检查是否有高风险关键词的变化
        let prev_has = |keywords: &[&str]| -> bool {
            keywords.iter().any(|k| prev_text.contains(k))
        };
        let curr_has = |keywords: &[&str]| -> bool {
            keywords.iter().any(|k| curr_text.contains(k))
        };

        // 资格条件变更
        if prev_has(&qualification_keywords) && curr_has(&qualification_keywords) {
            if Self::change_score(prev_text, curr_text) > 0.3 {
                return Some(HighRiskChange {
                    change_type: "qualification".into(),
                    section_path: vec![],
                    prev_content: prev_text.chars().take(200).collect(),
                    curr_content: curr_text.chars().take(200).collect(),
                    risk_detail: "资格条件内容发生了变化，可能放宽或收紧供应商准入，需检查是否存在倾向性调整".into(),
                });
            }
        }
        // 资格条件新增或删除
        if !prev_has(&qualification_keywords) && curr_has(&qualification_keywords) {
            return Some(HighRiskChange {
                change_type: "qualification".into(),
                section_path: vec![],
                prev_content: "(旧版无资格条件)".into(),
                curr_content: curr_text.chars().take(200).collect(),
                risk_detail: "新版新增了资格条件，需检查是否构成额外限制".into(),
            });
        }
        if prev_has(&qualification_keywords) && !curr_has(&qualification_keywords) {
            return Some(HighRiskChange {
                change_type: "qualification".into(),
                section_path: vec![],
                prev_content: prev_text.chars().take(200).collect(),
                curr_content: "(新版移除了资格条件)".into(),
                risk_detail: "资格条件被移除，需确认是否为遗漏".into(),
            });
        }

        // 评分标准变更
        if prev_has(&scoring_keywords) && curr_has(&scoring_keywords) {
            if Self::change_score(prev_text, curr_text) > 0.3 {
                return Some(HighRiskChange {
                    change_type: "scoring".into(),
                    section_path: vec![],
                    prev_content: prev_text.chars().take(200).collect(),
                    curr_content: curr_text.chars().take(200).collect(),
                    risk_detail: "评分标准发生了变化，可能调整了竞争格局，需检查是否存在倾向性评分".into(),
                });
            }
        }

        // 截止日期变更
        if prev_has(&deadline_keywords) && curr_has(&deadline_keywords) {
            if Self::change_score(prev_text, curr_text) > 0.2 {
                return Some(HighRiskChange {
                    change_type: "deadline".into(),
                    section_path: vec![],
                    prev_content: prev_text.chars().take(200).collect(),
                    curr_content: curr_text.chars().take(200).collect(),
                    risk_detail: "关键日期发生了变化，需检查是否满足法定最短期限".into(),
                });
            }
        }

        // 保证金变更
        if prev_has(&deposit_keywords) || curr_has(&deposit_keywords) {
            if Self::change_score(prev_text, curr_text) > 0.2 {
                return Some(HighRiskChange {
                    change_type: "deposit".into(),
                    section_path: vec![],
                    prev_content: prev_text.chars().take(200).collect(),
                    curr_content: curr_text.chars().take(200).collect(),
                    risk_detail: "保证金条款发生了变化，需检查比例是否合规".into(),
                });
            }
        }

        None
    }

    /// 按 section_path 查找最匹配的当前 chunk。
    fn find_matching_chunk(&self, prev_path: &[String], prev_text: &str) -> Option<&Chunk> {
        // 1. 精确路径匹配
        for chunk_id in self.current_order.iter() {
            if let Some(chunk) = self.current_chunks.get(chunk_id) {
                if chunk.section_path == prev_path {
                    return Some(chunk);
                }
            }
        }

        // 2. 最后一段路径匹配 + 文本相似度
        let prev_last = prev_path.last().map(|s| s.as_str()).unwrap_or("");
        let mut best: Option<&Chunk> = None;
        let mut best_overlap = 0u32;

        for chunk_id in self.current_order.iter() {
            if let Some(chunk) = self.current_chunks.get(chunk_id) {
                if let Some(curr_last) = chunk.section_path.last() {
                    if curr_last == prev_last {
                        // 计算文本中共同词的数量
                        let prev_words: Vec<&str> = prev_text
                            .split(|c: char| !c.is_alphanumeric() && !('\u{4e00}'..='\u{9fff}').contains(&c))
                            .filter(|s| s.chars().count() >= 2)
                            .collect();
                        let curr_words: Vec<&str> = chunk.text
                            .split(|c: char| !c.is_alphanumeric() && !('\u{4e00}'..='\u{9fff}').contains(&c))
                            .filter(|s| s.chars().count() >= 2)
                            .collect();
                        let overlap = prev_words.iter().filter(|w| curr_words.contains(w)).count() as u32;
                        if overlap > best_overlap {
                            best_overlap = overlap;
                            best = Some(chunk);
                        }
                    }
                }
            }
        }

        best
    }
}

#[async_trait::async_trait]
impl AgentTool for CompareVersionsTool {
    fn name(&self) -> &str {
        "compare_versions"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "compare_versions",
                "description": "对比标书新旧版本差异，分类 add/remove/modified，标记资格/评分/截止/保证金等高风险变更。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "previous_chunks": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "chunk_id": {"type": "string", "description": "旧版 chunk_id"},
                                    "section_path": {
                                        "type": "array",
                                        "items": {"type": "string"},
                                        "description": "章节路径"
                                    },
                                    "text": {"type": "string", "description": "条款原文"}
                                },
                                "required": ["chunk_id", "section_path", "text"]
                            },
                            "description": "旧版标书的 Chunk 列表（由 read_section 批量读取获得）"
                        },
                        "current_chunks": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "chunk_id": {"type": "string", "description": "新版 chunk_id"},
                                    "section_path": {
                                        "type": "array",
                                        "items": {"type": "string"},
                                        "description": "章节路径"
                                    },
                                    "text": {"type": "string", "description": "条款原文"}
                                },
                                "required": ["chunk_id", "section_path", "text"]
                            },
                            "description": "新版标书的 Chunk 列表（由 read_section 批量读取获得）"
                        }
                    },
                    "required": ["previous_chunks", "current_chunks"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: CompareVersionsArgs = serde_json::from_value(args)?;

        if parsed.previous_chunks.is_empty() {
            return Err(anyhow!("previous_chunks 不能为空"));
        }

        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut modified = Vec::new();
        let mut high_risk_changes = Vec::new();

        // 1. 遍历旧版 chunks，查找新版对应项
        let mut current_matched: HashMap<String, bool> = HashMap::new();

        for prev in &parsed.previous_chunks {
            if let Some(curr_chunk) = self.find_matching_chunk(&prev.section_path, &prev.text) {
                current_matched.insert(curr_chunk.chunk_id.clone(), true);
                let score = Self::change_score(&prev.text, &curr_chunk.text);

                if score > 0.15 {
                    // 内容有变化
                    let change_desc = if score > 0.5 {
                        "实质修改".to_string()
                    } else {
                        "轻微修改".to_string()
                    };

                    let mod_item = ModifiedItem {
                        prev_chunk_id: prev.chunk_id.clone(),
                        curr_chunk_id: curr_chunk.chunk_id.clone(),
                        section_path: prev.section_path.clone(),
                        prev_text_preview: prev.text.chars().take(200).collect(),
                        curr_text_preview: curr_chunk.text.chars().take(200).collect(),
                        change_description: change_desc,
                        change_score: score,
                    };

                    // 高风险检测
                    if let Some(mut risk) =
                        Self::detect_high_risk(&prev.text, &curr_chunk.text)
                    {
                        risk.section_path = prev.section_path.clone();
                        high_risk_changes.push(risk);
                    }

                    modified.push(mod_item);
                }
                // score <= 0.15 → 视为不变
            } else {
                // 旧版有，新版无 → 删除
                removed.push(DiffItem {
                    chunk_id: prev.chunk_id.clone(),
                    section_path: prev.section_path.clone(),
                    text_preview: prev.text.chars().take(200).collect(),
                    page_location: String::new(),
                });
            }
        }

        // 2. 遍历新版 chunks，查找未匹配的（新增）
        for curr in &parsed.current_chunks {
            if !current_matched.contains_key(&curr.chunk_id) {
                // 还要检查是否作为 modified 出现了
                let is_modified_target = modified
                    .iter()
                    .any(|m| m.curr_chunk_id == curr.chunk_id);
                if !is_modified_target {
                    added.push(DiffItem {
                        chunk_id: curr.chunk_id.clone(),
                        section_path: curr.section_path.clone(),
                        text_preview: curr.text.chars().take(200).collect(),
                        page_location: String::new(),
                    });
                }
            }
        }

        // 3. 统计
        let total_prev = parsed.previous_chunks.len();
        let unchanged = total_prev.saturating_sub(
            removed.len() + modified.len()
        );

        let stats = DiffStats {
            added_count: added.len(),
            removed_count: removed.len(),
            modified_count: modified.len(),
            high_risk_count: high_risk_changes.len(),
            unchanged_count: unchanged,
        };

        // 4. 摘要
        let summary = if added.is_empty() && removed.is_empty() && modified.is_empty() {
            "✅ 两个版本无差异".to_string()
        } else {
            let mut parts = Vec::new();
            if !added.is_empty() {
                parts.push(format!("新增 {} 条", added.len()));
            }
            if !removed.is_empty() {
                parts.push(format!("删除 {} 条", removed.len()));
            }
            if !modified.is_empty() {
                parts.push(format!("修改 {} 条", modified.len()));
            }
            if !high_risk_changes.is_empty() {
                parts.push(format!("⚠️ {} 条高风险变更", high_risk_changes.len()));
            }
            parts.join("，")
        };

        let result = DiffResult {
            added,
            removed,
            modified,
            high_risk_changes,
            stats,
            summary,
        };

        Ok(serde_json::to_value(&result)?)
    }
}

// ─── 测试 ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chunk::{Chunk, ChunkType};

    fn make_chunk(id: &str, path: &[&str], text: &str) -> Chunk {
        Chunk {
            chunk_id: id.to_string(),
            chunk_type: ChunkType::Leaf,
            section_path: path.iter().map(|s| s.to_string()).collect(),
            text: text.to_string(),
            page_start: 0,
            page_end: 1,
            source_block_ids: vec![],
            bbox_refs: vec![],
        }
    }

    fn make_tool() -> CompareVersionsTool {
        let mut map = HashMap::new();
        let order = vec!["ch_001".into(), "ch_002".into(), "ch_003".into()];

        map.insert(
            "ch_001".into(),
            make_chunk("ch_001", &["第一章", "资格条件"],
                "投标人须具有独立承担民事责任的能力，提供营业执照副本。"),
        );
        map.insert(
            "ch_002".into(),
            make_chunk("ch_002", &["第二章", "评分标准"],
                "价格分权重30%，技术分权重50%，商务分权重20%。"),
        );
        map.insert(
            "ch_003".into(),
            make_chunk("ch_003", &["第三章", "合同条款"],
                "预付款比例为合同金额的30%，质保金比例为3%。"),
        );

        CompareVersionsTool::new(Arc::new(map), Arc::new(order))
    }

    #[test]
    fn test_lcs_diff_identical() {
        assert_eq!(CompareVersionsTool::lcs_diff("完全相同的文本", "完全相同的文本"), 0);
    }

    #[test]
    fn test_lcs_diff_different() {
        let diff = CompareVersionsTool::lcs_diff("文本A", "文本B");
        assert!(diff > 0);
    }

    #[test]
    fn test_change_score_same() {
        let score = CompareVersionsTool::change_score("一样", "一样");
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_change_score_completely_different() {
        let score = CompareVersionsTool::change_score("abc", "xyz");
        assert!(score > 0.5);
    }

    #[test]
    fn test_detect_high_risk_qualification_change() {
        let prev = "投标人须具备建筑工程施工总承包二级及以上资质，且注册资金不低于500万元人民币。";
        let curr = "投标人须具备市政公用工程施工总承包特级资质证书，且净资产不低于5000万元人民币，同时需提供三年审计报告。";
        let risk = CompareVersionsTool::detect_high_risk(prev, curr);
        assert!(risk.is_some(), "资质变更应被检测到（change_score={})", CompareVersionsTool::change_score(prev, curr));
        assert_eq!(risk.unwrap().change_type, "qualification");
    }

    #[test]
    fn test_detect_high_risk_scoring_change() {
        let prev = "价格分权重30%（采用最低价法），技术分权重50%，商务分权重20%。";
        let curr = "价格分权重60%（采用基准价法），技术分权重25%，商务分权重15%。";
        let risk = CompareVersionsTool::detect_high_risk(prev, curr);
        assert!(risk.is_some(), "评分变更应被检测到");
        assert_eq!(risk.unwrap().change_type, "scoring");
    }

    #[test]
    fn test_detect_high_risk_no_change() {
        let prev = "本项目位于东莞市松山湖园区。";
        let curr = "本项目位于东莞市莞城街道。";
        let risk = CompareVersionsTool::detect_high_risk(prev, curr);
        assert!(risk.is_none());
    }

    #[test]
    fn test_find_matching_chunk_exact_path() {
        let tool = make_tool();
        let found = tool.find_matching_chunk(
            &["第一章".into(), "资格条件".into()],
            "投标人须具有独立承担民事责任的能力",
        );
        assert!(found.is_some());
        assert_eq!(found.unwrap().chunk_id, "ch_001");
    }

    #[test]
    fn test_find_matching_chunk_not_found() {
        let tool = make_tool();
        let found = tool.find_matching_chunk(
            &["附录".into(), "参考资料".into()],
            "不存在的内容",
        );
        assert!(found.is_none());
    }
}
