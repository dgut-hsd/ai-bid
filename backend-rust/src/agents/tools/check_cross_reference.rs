//! `check_cross_reference` 工具 — 交叉引用完整性检查。
//!
//! 条款中出现"详见附件X""按第Y章第Z条""参照前款"等引用表达式时，
//! 验证引用目标是否存在且内容匹配。这是 Cursor 'Find References' 的标书版。
//!
//! ## 核心逻辑
//!
//! 1. 解析 `reference_expression`，识别引用类型（附件/章节/条款/表格/附录）
//! 2. 在所有 Chunk 的 section_path 和文本中搜索引用目标
//! 3. 返回验证结果（valid / dangling / ambiguous / content_mismatch）

use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::chunk::Chunk;

use super::AgentTool;

/// `check_cross_reference` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct CheckCrossReferenceArgs {
    /// 包含引用表达式的 chunk_id
    pub source_chunk: String,
    /// 引用表达式原文，如"详见附件三""按第三章第二节"
    pub reference_expression: String,
    /// 引用类型
    #[serde(default)]
    pub reference_type: Option<String>,
    /// 引用方声称的关键词列表（用于验证内容是否匹配）
    /// 如"详见附件三 技术参数表" → claims=["技术参数", "参数表"]
    #[serde(default)]
    pub reference_claims: Vec<String>,
}

/// 交叉引用检查结果。
#[derive(Debug, serde::Serialize)]
struct CrossReferenceResult {
    /// 验证状态
    status: RefStatus,
    /// 引用表达式原文
    reference_expression: String,
    /// 引用类型
    reference_type: String,
    /// 找到的目标 chunk_id（如有）
    target_chunk: Option<String>,
    /// 目标章节路径（如有）
    target_section: Option<Vec<String>>,
    /// 不匹配的详细说明
    mismatch_detail: Option<String>,
    /// 所有可能的候选目标
    candidates: Vec<CandidateRef>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum RefStatus {
    Valid,
    Dangling,
    Ambiguous,
    ContentMismatch,
}

#[derive(Debug, serde::Serialize)]
struct CandidateRef {
    chunk_id: String,
    section_path: Vec<String>,
    match_reason: String,
    match_score: u32,
    text_preview: String,
}

/// `check_cross_reference` 工具实现。
///
/// 持有所有 Chunk 的内存索引 (Arc<HashMap>)，零 I/O 延迟。
pub struct CheckCrossReferenceTool {
    /// Chunk ID → Chunk 映射表
    pub chunks: Arc<HashMap<String, Chunk>>,
    /// 有序 chunk_id 列表（按文档出现顺序）
    pub chunk_order: Arc<Vec<String>>,
}

impl CheckCrossReferenceTool {
    pub fn new(chunks: Arc<HashMap<String, Chunk>>, chunk_order: Arc<Vec<String>>) -> Self {
        Self {
            chunks,
            chunk_order,
        }
    }

    /// 从引用表达式中提取搜索关键词。
    ///
    /// 支持的引用模式：
    /// - "详见附件三" → type=attachment, keywords=["附件三", "附件3"]
    /// - "按第三章第二节" → type=section, keywords=["第三章", "第二节", "第三章第二节"]
    /// - "参照第3.2条" → type=clause, keywords=["3.2", "第3.2条"]
    /// - "见附表二" → type=table, keywords=["附表二", "附表2"]
    fn parse_reference(expr: &str, hint: Option<&str>) -> (String, Vec<String>) {
        let ref_type = if let Some(h) = hint {
            h.to_string()
        } else if expr.contains("附件") || expr.contains("附录") {
            if expr.contains("附件") {
                "attachment"
            } else {
                "appendix"
            }
            .to_string()
        } else if expr.contains("章") || expr.contains("节") || expr.contains("条") {
            "section".to_string()
        } else if expr.contains("表") || expr.contains("图") {
            "table".to_string()
        } else {
            "clause".to_string()
        };

        // 提取核心关键词
        let mut keywords = Vec::new();
        keywords.push(expr.to_string());

        // 中文数字转阿拉伯数字（状态机解析，"十二"→"12"、"二十"→"20"、"十一"→"11"）
        let cn_digits: Vec<char> = expr.chars().filter(|c| {
            matches!(c, '一'..='九' | '十' | '百' | '零')
        }).collect();
        let mut cn_to_ar = String::new();
        let mut last_digit = 0u32;
        let mut acc = 0u32;
        let mut in_num = false;
        for &c in &cn_digits {
            let val = match c {
                '一' => 1, '二' => 2, '三' => 3, '四' => 4,
                '五' => 5, '六' => 6, '七' => 7, '八' => 8, '九' => 9,
                '十' => { last_digit = if last_digit == 0 { 1 } else { last_digit }; acc += last_digit * 10; last_digit = 0; in_num = true; continue; }
                '百' => { last_digit = if last_digit == 0 { 1 } else { last_digit }; acc += last_digit * 100; last_digit = 0; in_num = true; continue; }
                '零' => { in_num = true; continue; }
                _ => continue,
            };
            last_digit = val;
            in_num = true;
        }
        acc += last_digit;
        if in_num {
            cn_to_ar = acc.to_string();
        }

        let has_ar_num: String = expr.chars().filter(|c| c.is_ascii_digit()).collect();
        if !has_ar_num.is_empty() && !cn_to_ar.is_empty() {
            keywords.push(cn_to_ar);
        } else if !has_ar_num.is_empty() && cn_to_ar.is_empty() {
            // 只有阿拉伯数字无中文数字,不需要额外变体
        } else if !cn_to_ar.is_empty() {
            keywords.push(cn_to_ar);
        }

        (ref_type, keywords)
    }

    /// 在所有 chunk 中搜索引用目标。
    fn search_targets(
        &self,
        _expr: &str,
        ref_type: &str,
        keywords: &[String],
    ) -> Vec<CandidateRef> {
        let mut candidates = Vec::new();

        // 扩展搜索词：除了原有关键词，还尝试提取数字子模式（如"附件三"、"第三章"等）
        let mut all_search_terms = keywords.to_vec();
        for kw in keywords {
            // 从复合表达式中提取可能的子模式
            for sub in &["附件", "附录", "附表", "第", "表"] {
                if let Some(pos) = kw.find(sub) {
                    let snippet: String = kw[pos..].chars().take(10).collect();
                    if snippet.len() >= 3 && !all_search_terms.contains(&snippet) {
                        all_search_terms.push(snippet);
                    }
                }
            }
        }

        for chunk_id in self.chunk_order.iter() {
            if let Some(chunk) = self.chunks.get(chunk_id) {
                let search_text = format!(
                    "{} {}",
                    chunk.section_path.join(" "),
                    chunk.text.chars().take(500).collect::<String>()
                );

                let mut best_match_reason = String::new();
                let mut best_score = 0u32;

                for kw in &all_search_terms {
                    if search_text.contains(kw.as_str()) {
                        let score = 3u32;
                        if score > best_score {
                            best_score = score;
                            best_match_reason = format!("文本包含'{}'", kw);
                        }
                    }
                }

                // 对于 section 类型，还检查 section_path 的精确匹配
                if ref_type == "section" && best_score == 0 {
                    for kw in &all_search_terms {
                        // 尝试多种匹配方式
                        let path_str_spaced = chunk.section_path.join(" ");
                        let path_str_no_space: String = chunk.section_path.concat();

                        if path_str_spaced.contains(kw.as_str())
                            || path_str_no_space.contains(kw.as_str())
                        {
                            best_score = 2;
                            best_match_reason = format!("章节路径包含'{}'", kw);
                            break;
                        }
                        // 尝试逐个 segment 匹配
                        for seg in &chunk.section_path {
                            if seg.contains(kw.as_str()) {
                                best_score = 2;
                                best_match_reason = format!("章节路径片段'{}' 包含'{}'", seg, kw);
                                break;
                            }
                        }
                    }
                }

                if best_score > 0 {
                    candidates.push(CandidateRef {
                        chunk_id: chunk_id.clone(),
                        section_path: chunk.section_path.clone(),
                        match_reason: best_match_reason,
                        match_score: best_score,
                        text_preview: chunk.text.chars().take(200).collect(),
                    });
                }
            }
        }

        // 按匹配分数降序排列（文本精确匹配 > section_path 匹配）
        candidates.sort_by(|a, b| {
            b.match_score
                .cmp(&a.match_score)
                .then_with(|| a.chunk_id.cmp(&b.chunk_id))
        });

        candidates
    }
}

#[async_trait::async_trait]
impl AgentTool for CheckCrossReferenceTool {
    fn name(&self) -> &str {
        "check_cross_reference"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "check_cross_reference",
                "description": "【使用场景】条款中出现'详见附件X''按第Y章第Z条''参照前款'等引用表达式时，\
                    验证引用目标是否存在且内容匹配。这是 Cursor 'Find References' 的标书版。\
                    【不使用场景】纯语义关联查询（如'找相似条款'）——用 search_document。\
                    【验证内容】\
                    ① 引用目标是否存在（附件是否存在、章节是否在文档中）；\
                    ② 引用目标的内容是否与引用上下文匹配（附件标题是否对得上、条款内容是否矛盾）。\
                    【注意】reference_type 可选，如不提供则自动从 expression 中推断。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "source_chunk": {
                            "type": "string",
                            "description": "包含引用表达式的 chunk_id，如 'ch_042'"
                        },
                        "reference_expression": {
                            "type": "string",
                            "description": "引用表达式原文，如'详见附件三''按第三章第二节'"
                        },
                        "reference_type": {
                            "type": "string",
                            "enum": ["attachment", "section", "clause", "table", "appendix"],
                            "description": "引用类型，可选。不提供则自动推断。"
                        },
                        "reference_claims": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "引用方声称的内容关键词列表，用于验证目标内容是否匹配。如'详见附件3 技术参数表' → [\"技术参数\", \"参数表\"]"
                        }
                    },
                    "required": ["source_chunk", "reference_expression"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: CheckCrossReferenceArgs = serde_json::from_value(args)?;

        // 1. 解析引用表达式
        let (ref_type, keywords) = Self::parse_reference(
            &parsed.reference_expression,
            parsed.reference_type.as_deref(),
        );

        // 2. 搜索候选目标
        let candidates = self.search_targets(&parsed.reference_expression, &ref_type, &keywords);

        // 3. 判定状态
        let (status, target_chunk, target_section, mismatch_detail) = if candidates.is_empty() {
            (
                RefStatus::Dangling,
                None,
                None,
                Some(format!(
                    "未找到引用目标 '{}'。文档中不存在匹配的{}。",
                    parsed.reference_expression,
                    ref_type_name(&ref_type)
                )),
            )
        } else if candidates.len() == 1 {
            // 唯一候选 → 先检查 source_chunk 是否存在
            if !self.chunks.contains_key(&parsed.source_chunk) {
                (
                    RefStatus::Dangling,
                    None,
                    None,
                    Some(format!("源 chunk '{}' 不存在", parsed.source_chunk)),
                )
            } else if !parsed.reference_claims.is_empty() {
                // ★ ContentMismatch 检测：有唯一候选目标，但内容是否匹配引用方声称的关键词？
                let c = &candidates[0];
                let target_text = self
                    .chunks
                    .get(&c.chunk_id)
                    .map(|ch| &ch.text)
                    .unwrap_or(&c.text_preview);

                let mut matched_claims = Vec::new();
                let mut unmatched_claims = Vec::new();
                for claim in &parsed.reference_claims {
                    if target_text.contains(claim.as_str()) {
                        matched_claims.push(claim.clone());
                    } else {
                        unmatched_claims.push(claim.clone());
                    }
                }

                if unmatched_claims.is_empty() {
                    // 所有声称关键词都匹配 → Valid
                    (
                        RefStatus::Valid,
                        Some(c.chunk_id.clone()),
                        Some(c.section_path.clone()),
                        None,
                    )
                } else {
                    // 部分／全部关键词不匹配 → ContentMismatch
                    let detail = if matched_claims.is_empty() {
                        format!(
                            "引用声称的内容（{}）在目标中均未找到。目标实际标题为 '{}'，内容摘要：'{}'",
                            unmatched_claims.join("、"),
                            c.section_path.join(" > "),
                            c.text_preview
                        )
                    } else {
                        format!(
                            "部分声称匹配({})，但以下关键词未找到：{}。请确认引用目标是否正确。",
                            matched_claims.join("、"),
                            unmatched_claims.join("、")
                        )
                    };
                    (
                        RefStatus::ContentMismatch,
                        Some(c.chunk_id.clone()),
                        Some(c.section_path.clone()),
                        Some(detail),
                    )
                }
            } else {
                let c = &candidates[0];
                (
                    RefStatus::Valid,
                    Some(c.chunk_id.clone()),
                    Some(c.section_path.clone()),
                    None,
                )
            }
        } else {
            // 多个候选 → ambiguous，返回所有候选让 LLM 判断
            let summary = candidates
                .iter()
                .map(|c| format!("{}({})", c.chunk_id, c.section_path.join(" > ")))
                .collect::<Vec<_>>()
                .join(", ");
            (
                RefStatus::Ambiguous,
                None,
                None,
                Some(format!(
                    "找到 {} 个可能的目标: {}。请用 read_section 确认。",
                    candidates.len(),
                    summary
                )),
            )
        };

        let result = CrossReferenceResult {
            status,
            reference_expression: parsed.reference_expression,
            reference_type: ref_type,
            target_chunk,
            target_section,
            mismatch_detail,
            candidates,
        };

        Ok(serde_json::to_value(&result)?)
    }
}

fn ref_type_name(t: &str) -> &str {
    match t {
        "attachment" => "附件",
        "section" => "章节",
        "clause" => "条款",
        "table" => "表格",
        "appendix" => "附录",
        _ => "文档",
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

    fn make_tool() -> CheckCrossReferenceTool {
        let mut chunks_map = HashMap::new();
        let chunk_order = vec![
            "ch_001".to_string(),
            "ch_002".to_string(),
            "ch_003".to_string(),
            "ch_010".to_string(),
            "ch_020".to_string(),
        ];

        chunks_map.insert(
            "ch_001".to_string(),
            make_chunk("ch_001", &["第一章", "总则"], "第一条 本招标文件适用于..."),
        );
        chunks_map.insert(
            "ch_002".to_string(),
            make_chunk(
                "ch_002",
                &["第二章", "投标人须知"],
                "详见附件三 资格证明文件",
            ),
        );
        chunks_map.insert(
            "ch_003".to_string(),
            make_chunk("ch_003", &["第三章", "评标办法"], "按第三章第二节执行"),
        );
        chunks_map.insert(
            "ch_010".to_string(),
            make_chunk(
                "ch_010",
                &["附件三", "资格证明"],
                "附件三：资格证明文件清单...",
            ),
        );
        chunks_map.insert(
            "ch_020".to_string(),
            make_chunk(
                "ch_020",
                &["第三章", "第二节", "评分标准"],
                "第二节 评分标准细则...",
            ),
        );

        CheckCrossReferenceTool {
            chunks: Arc::new(chunks_map),
            chunk_order: Arc::new(chunk_order),
        }
    }

    #[test]
    fn test_parse_reference_attachment() {
        let (ref_type, keywords) = CheckCrossReferenceTool::parse_reference("详见附件三", None);
        assert_eq!(ref_type, "attachment");
        assert!(keywords.iter().any(|k| k.contains("附件三")));
    }

    #[test]
    fn test_parse_reference_section() {
        let (ref_type, keywords) = CheckCrossReferenceTool::parse_reference("按第三章第二节", None);
        assert_eq!(ref_type, "section");
        assert!(keywords.iter().any(|k| k.contains("第三章")));
    }

    #[test]
    fn test_search_targets_found() {
        let tool = make_tool();
        let (ref_type, keywords) = CheckCrossReferenceTool::parse_reference("详见附件三", None);
        let candidates = tool.search_targets("详见附件三", &ref_type, &keywords);
        assert!(!candidates.is_empty());
        // 应该至少包含附件三的 chunk
        assert!(candidates.iter().any(|c| c.chunk_id == "ch_010"));
    }

    #[test]
    fn test_search_targets_section_found() {
        let tool = make_tool();
        let (ref_type, keywords) =
            CheckCrossReferenceTool::parse_reference("第三章第二节", Some("section"));
        let candidates = tool.search_targets("第三章第二节", &ref_type, &keywords);
        assert!(!candidates.is_empty());
        assert!(candidates.iter().any(|c| c.chunk_id == "ch_020"));
    }

    #[test]
    fn test_search_targets_dangling() {
        let tool = make_tool();
        let candidates =
            tool.search_targets("附件五 供应商声明", "attachment", &["附件五".to_string()]);
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_content_mismatch_detection() {
        let tool = make_tool();
        let (ref_type, keywords) =
            CheckCrossReferenceTool::parse_reference("附件三", Some("attachment"));
        let candidates = tool.search_targets("附件三", &ref_type, &keywords);
        assert!(!candidates.is_empty(), "应找到附件三");

        // 验证 ContentMismatch 逻辑：附件三的实际内容是"资格证明文件清单"
        // 如果引用方声称它包含"技术参数表"，应当检测为不匹配
        let target_text = tool.chunks.get(&candidates[0].chunk_id).unwrap();
        let has_tech_param = target_text.text.contains("技术参数");
        // 附件三是资格证明，不应该包含"技术参数"
        assert!(!has_tech_param, "附件三不应包含'技术参数'");
        // 但确认附件三确实存在且包含"资格证明"
        assert!(target_text.text.contains("资格证明"));
    }
}
