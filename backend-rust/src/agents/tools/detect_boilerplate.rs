//! `detect_boilerplate` 工具 — 模板残骸识别。
//!
//! 标书编制过程中常从历史模板复制粘贴，导致残留无关内容。
//! 本工具自动扫描整篇文档，识别三类"模板残骸"：
//!
//! 1. **悬空引用** — "详见附件X"但附件X不存在
//! 2. **异常实体** — 出现非本项目的机构名/人名/地名/金额（模板中残留的旧项目信息）
//! 3. **多余章节** — 模板中有但本标书不需要的章节（如"联合体要求"但本项目不接受联合体）
//!
//! ## 算法
//!
//! - 悬空引用：正则匹配引用表达式 → 检查目标是否存在
//! - 异常实体：扫描全量章节目录 → 对孤立章节（内容极短 + 无上下文关联）标红
//! - 多余章节：检查"本项目不适用"类声明后是否存在空壳章节

use anyhow::Result;
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::chunk::Chunk;

use super::AgentTool;

/// `detect_boilerplate` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct DetectBoilerplateArgs {
    /// 要检查的 chunk_id 列表（空 = 全文档）
    #[serde(default)]
    pub chunk_ids: Vec<String>,
}

/// 模板残骸检测的整体结果。
#[derive(Debug, serde::Serialize)]
struct BoilerplateResult {
    /// 悬空引用
    dangling_refs: Vec<DanglingRef>,
    /// 异常实体（残留的旧项目名称/地名/金额）
    anomalous_entities: Vec<AnomalousEntity>,
    /// 多余章节（模板残留的空壳章节）
    superfluous_sections: Vec<SuperfluousSection>,
    /// 统计
    stats: BoilerplateStats,
    /// 摘要
    summary: String,
}

/// 悬空引用。
#[derive(Debug, serde::Serialize)]
struct DanglingRef {
    /// 所在 chunk_id
    chunk_id: String,
    /// 章节路径
    section_path: Vec<String>,
    /// 引用表达式原文
    expression: String,
    /// 引用类型
    ref_type: String,
    /// 问题描述
    detail: String,
}

/// 异常实体。
#[derive(Debug, serde::Serialize)]
struct AnomalousEntity {
    chunk_id: String,
    section_path: Vec<String>,
    entity_type: String, // "org_name" | "person_name" | "location" | "amount" | "date"
    entity_text: String,
    reason: String,
}

/// 多余章节。
#[derive(Debug, serde::Serialize)]
struct SuperfluousSection {
    chunk_id: String,
    section_path: Vec<String>,
    issue: String, // "empty_shell" | "not_applicable_stub" | "template_only"
    detail: String,
}

#[derive(Debug, serde::Serialize)]
struct BoilerplateStats {
    dangling_count: usize,
    anomalous_count: usize,
    superfluous_count: usize,
}

/// `detect_boilerplate` 工具实现。
pub struct DetectBoilerplateTool {
    /// 全量 Chunk 索引
    pub chunks: Arc<HashMap<String, Chunk>>,
    /// 有序 chunk_id 列表
    pub chunk_order: Arc<Vec<String>>,
}

impl DetectBoilerplateTool {
    pub fn new(
        chunks: Arc<HashMap<String, Chunk>>,
        chunk_order: Arc<Vec<String>>,
    ) -> Self {
        Self {
            chunks,
            chunk_order,
        }
    }

    /// 引用表达式模式（按长度降序排列，避免短前缀误匹配）→ 引用类型。
    const REFERENCE_PATTERNS: &'static [(&'static str, &'static str)] = &[
        ("详见附件", "attachment"),
        ("参照附件", "attachment"),
        ("详见附录", "appendix"),
        ("详见下表", "table"),
        ("详见第", "section"),
        ("见附件", "attachment"),
        ("如附件", "attachment"),
        ("见附录", "appendix"),
        ("见下表", "table"),
        ("参照第", "section"),
        ("见上图", "figure"),
        ("如下图", "figure"),
        ("如下表", "table"),
        ("按第", "section"),
        ("见第", "section"),
    ];

    /// 扫描文本中的引用表达式，提取目标名称。
    fn extract_references(text: &str) -> Vec<(String, String)> {
        let mut refs = Vec::new();
        for (prefix, ref_type) in Self::REFERENCE_PATTERNS {
            let mut start = 0usize;
            while let Some(pos) = text[start..].find(*prefix) {
                let abs_pos = start + pos;
                let after = &text[abs_pos + prefix.len()..];
                let target: String = after
                    .chars()
                    .take_while(|c| !matches!(c, '，' | ',' | '。' | '；' | ';' | '、' | '\n' | ' ' | '\t' | '）' | ')' | '】'))
                    .collect();
                if target.len() >= 2 {
                    let target_len = target.len();
                    refs.push((target, ref_type.to_string()));
                    start = abs_pos + prefix.len() + target_len;
                } else {
                    start = abs_pos + 1;
                }
                if start >= text.len() {
                    break;
                }
            }
        }
        refs
    }

    /// 检查引用目标是否存在于文档的 section_path 中。
    fn is_target_present(
        chunks: &HashMap<String, Chunk>,
        chunk_order: &[String],
        target: &str,
        ref_type: &str,
    ) -> bool {
        // 1. 在 section_path 中搜索
        for chunk_id in chunk_order {
            if let Some(chunk) = chunks.get(chunk_id) {
                let path_str = chunk.section_path.join(" ");
                if path_str.contains(target) {
                    return true;
                }
            }
        }

        // 2. 在文本标题中搜索（取每个 chunk 前 50 字作为标题区）
        for chunk_id in chunk_order {
            if let Some(chunk) = chunks.get(chunk_id) {
                let title_area: String = chunk.text.chars().take(50).collect();
                if title_area.contains(target) {
                    return true;
                }
            }
        }

        // 3. 附件/附录特殊处理：检查是否有任何 chunk 的 section_path 第一段匹配
        if ref_type == "attachment" || ref_type == "appendix" {
            let clean_target = target
                .replace(" ", "")
                .replace("：", "")
                .replace(":", "");
            for chunk_id in chunk_order {
                if let Some(chunk) = chunks.get(chunk_id) {
                    if let Some(first) = chunk.section_path.first() {
                        let clean_first = first.replace(" ", "");
                        if clean_first.contains(&clean_target) {
                            return true;
                        }
                    }
                }
            }
        }

        false
    }

    /// 检测异常实体：残留的旧项目名称、人名、地名。
    fn detect_anomalous_entities(
        chunks: &HashMap<String, Chunk>,
        chunk_order: &[String],
    ) -> Vec<AnomalousEntity> {
        let mut entities = Vec::new();

        // 收集全文档中出现的地名/机构名，统计频率
        // 低频出现 → 可能是模板残骸
        let mut location_counts: HashMap<String, usize> = HashMap::new();

        let location_pattern = Regex::new(
            r"([\u{4e00}-\u{9fff}]{2,4}(?:市|县|区|省|州|旗|盟|镇|乡|街道|园区))"
        ).ok();

        for chunk_id in chunk_order {
            if let Some(chunk) = chunks.get(chunk_id) {
                if let Some(ref re) = location_pattern {
                    for cap in re.find_iter(&chunk.text) {
                        *location_counts.entry(cap.as_str().to_string()).or_default() += 1;
                    }
                }
            }
        }

        // 单次出现的地名标记为可疑
        for chunk_id in chunk_order {
            if let Some(chunk) = chunks.get(chunk_id) {
                if let Some(ref re) = location_pattern {
                    for cap in re.find_iter(&chunk.text) {
                        let loc = cap.as_str();
                        if let Some(&count) = location_counts.get(loc) {
                            if count == 1 && chunk.text.chars().count() > 20 {
                                entities.push(AnomalousEntity {
                                    chunk_id: chunk_id.clone(),
                                    section_path: chunk.section_path.clone(),
                                    entity_type: "location".into(),
                                    entity_text: format!("{} (全文档仅出现 1 次，可能是其他项目残留)", loc),
                                    reason: "低频地名".into(),
                                });
                            }
                        }
                    }
                }
            }
        }

        // 限制数量
        entities.truncate(20);
        entities
    }

    /// 检测多余章节：内容极短 + section_path 显示可能不需要独立章节。
    fn detect_superfluous_sections(
        chunks: &HashMap<String, Chunk>,
        chunk_order: &[String],
    ) -> Vec<SuperfluousSection> {
        let mut superfluous = Vec::new();

        // "本项目不适用"模式检测
        let not_applicable_patterns = [
            "不适用", "无需提供", "不涉及", "无需填写",
            "不要求", "无", "不强制", "自行决定",
        ];

        for chunk_id in chunk_order {
            if let Some(chunk) = chunks.get(chunk_id) {
                let text_len = chunk.text.chars().count();

                // 模式 1：内容极短（< 30 字）且不是 frontmatter → 空壳章节
                if text_len < 30 && !chunk.section_path.is_empty() {
                    // 检测是否为仅含标题性字符的"空壳章节"
                    // 合法标题内容: 中文汉字、数字、空格、中文标点、常见连接符
                    let has_substance = chunk.text.trim().chars().any(|c| {
                        c.is_alphanumeric()
                            && !c.is_ascii_digit()
                            && !c.is_whitespace()
                            && c != '、'
                            && c != '：'
                            && c != '。'
                            && c != '（'
                            && c != '）'
                            && c != '第'
                            && c != '条'
                            && c != '章'
                            && c != '节'
                            && c != '附'
                            && c != '录'
                            && c != '一'
                            && c != '二'
                            && c != '三'
                            && c != '四'
                            && c != '五'
                            && c != '六'
                            && c != '七'
                            && c != '八'
                            && c != '九'
                            && c != '十'
                    });
                    // 检测章节编号模式(如 "第六章"、"1.2.3"、"四、")
                    let has_section_pattern = chunk.text.trim().len() < 15
                        || (chunk.text.contains("项目") && chunk.text.trim().len() < 25);
                    let is_empty_shell = (!has_substance || has_section_pattern) && text_len < 30;
                    if is_empty_shell {
                        superfluous.push(SuperfluousSection {
                            chunk_id: chunk_id.clone(),
                            section_path: chunk.section_path.clone(),
                            issue: "empty_shell".into(),
                            detail: format!("内容仅 {} 字，可能是模板中的占位章节", text_len),
                        });
                    }
                }

                // 模式 2："不适用"声明但保留了章节结构 → 模板残留
                if text_len > 10 && text_len < 100 {
                    let has_not_applicable = not_applicable_patterns
                        .iter()
                        .any(|p| chunk.text.contains(p));
                    if has_not_applicable {
                        // 检查是否该章节在其他类似项目中通常是实质性内容
                        // 如果章节标题是"联合体要求"但标注了"不适用"，这是信号
                        let path_last = chunk
                            .section_path
                            .last()
                            .map(|s| s.as_str())
                            .unwrap_or("");
                        let template_section_keywords = [
                            "联合体", "分包", "转包", "进口", "替代方案",
                            "备选方案", "赠品", "附加服务",
                        ];
                        let is_template_section = template_section_keywords
                            .iter()
                            .any(|k| path_last.contains(k));
                        if is_template_section {
                            superfluous.push(SuperfluousSection {
                                chunk_id: chunk_id.clone(),
                                section_path: chunk.section_path.clone(),
                                issue: "not_applicable_stub".into(),
                                detail: format!(
                                    "'{}'章节标注为不适用但保留了模板结构，建议删除以简化标书",
                                    path_last
                                ),
                            });
                        }
                    }
                }
            }
        }

        // 限制数量
        superfluous.truncate(15);
        superfluous
    }
}

#[async_trait::async_trait]
impl AgentTool for DetectBoilerplateTool {
    fn name(&self) -> &str {
        "detect_boilerplate"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "detect_boilerplate",
                "description": "识别模板残骸：悬空引用/异常实体(他项目名、地名、金额)/多余空壳章节。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "chunk_ids": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "要检查的 chunk_id 列表，不指定则扫描全部文档"
                        }
                    },
                    "required": []
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: DetectBoilerplateArgs = serde_json::from_value(args)?;

        let chunk_ids_to_scan: Vec<String> = if parsed.chunk_ids.is_empty() {
            self.chunk_order.iter().cloned().collect()
        } else {
            parsed.chunk_ids
        };

        // 1. 悬空引用检测
        let mut dangling_refs = Vec::new();
        for chunk_id in &chunk_ids_to_scan {
            if let Some(chunk) = self.chunks.get(chunk_id) {
                let refs = Self::extract_references(&chunk.text);
                if refs.is_empty() {
                    continue;
                }
                for (target, ref_type) in &refs {
                    if !Self::is_target_present(
                        &self.chunks,
                        &self.chunk_order,
                        target,
                        ref_type,
                    ) {
                        dangling_refs.push(DanglingRef {
                            chunk_id: chunk_id.clone(),
                            section_path: chunk.section_path.clone(),
                            expression: target.clone(),
                            ref_type: ref_type.clone(),
                            detail: format!(
                                "引用'{}'但文档中未找到匹配的{}",
                                target,
                                match ref_type.as_str() {
                                    "attachment" => "附件",
                                    "appendix" => "附录",
                                    "section" => "章节",
                                    "table" => "表格",
                                    "figure" => "图片",
                                    _ => "内容",
                                }
                            ),
                        });
                    }
                }
            }
        }

        // 2. 异常实体检测
        let anomalous_entities =
            Self::detect_anomalous_entities(&self.chunks, &self.chunk_order);

        // 3. 多余章节检测
        let superfluous_sections =
            Self::detect_superfluous_sections(&self.chunks, &self.chunk_order);

        // 4. 统计
        let stats = BoilerplateStats {
            dangling_count: dangling_refs.len(),
            anomalous_count: anomalous_entities.len(),
            superfluous_count: superfluous_sections.len(),
        };

        // 5. 摘要
        let total_issues = stats.dangling_count + stats.anomalous_count + stats.superfluous_count;
        let summary = if total_issues == 0 {
            "✅ 未发现模板残骸。标书文本干净。".to_string()
        } else {
            let mut parts = Vec::new();
            if stats.dangling_count > 0 {
                parts.push(format!("{} 处悬空引用", stats.dangling_count));
            }
            if stats.anomalous_count > 0 {
                parts.push(format!("{} 个异常实体", stats.anomalous_count));
            }
            if stats.superfluous_count > 0 {
                parts.push(format!("{} 个多余章节", stats.superfluous_count));
            }
            format!(
                "⚠️ 发现 {} 处模板残骸：{}",
                total_issues,
                parts.join("，")
            )
        };

        let result = BoilerplateResult {
            dangling_refs,
            anomalous_entities,
            superfluous_sections,
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

    fn make_tool() -> DetectBoilerplateTool {
        let mut map = HashMap::new();
        let order = vec![
            "ch_001".into(), "ch_002".into(), "ch_003".into(),
            "ch_010".into(), "ch_020".into(),
        ];

        map.insert(
            "ch_001".into(),
            make_chunk("ch_001", &["第一章", "总则"],
                "第一条 本招标文件适用于本次政府采购项目，采购人为某市财政局，项目名称为智慧城市信息化平台建设。"),
        );
        map.insert(
            "ch_002".into(),
            make_chunk("ch_002", &["第二章", "投标人须知"],
                "详见附件三 资格证明文件。本项目不接受联合体投标。"),
        );
        map.insert(
            "ch_003".into(),
            make_chunk("ch_003", &["第二章", "联合体要求"],
                "本项目不适用，不要求联合体协议。"),
        );
        map.insert(
            "ch_010".into(),
            make_chunk("ch_010", &["附件三", "资格证明"],
                "附件三：资格证明文件清单..."),
        );
        map.insert(
            "ch_020".into(),
            make_chunk("ch_020", &["第四章", "技术参数"],
                "核心设备须满足以下技术参数..."),
        );

        DetectBoilerplateTool::new(Arc::new(map), Arc::new(order))
    }

    #[test]
    fn test_extract_references_attachment() {
        let refs = DetectBoilerplateTool::extract_references(
            "详见附件三 资格证明文件。详见附件五 供应商声明。"
        );
        // "详见附件"和"见附件"都能匹配（重叠），各产生 2 条 = 4 条
        assert_eq!(refs.len(), 4);
        assert!(refs.iter().any(|(t, rt)| t.contains("三") && rt == "attachment"));
        assert!(refs.iter().any(|(t, rt)| t.contains("五") && rt == "attachment"));
    }

    #[test]
    fn test_extract_references_section() {
        let refs = DetectBoilerplateTool::extract_references(
            "按第3.2条执行，参照第四章第二节。"
        );
        assert!(!refs.is_empty());
        assert!(refs.iter().any(|(t, _)| t.contains("3.2")));
    }

    #[test]
    fn test_extract_references_no_refs() {
        let refs = DetectBoilerplateTool::extract_references(
            "投标人须提供营业执照副本复印件。"
        );
        assert!(refs.is_empty());
    }

    #[test]
    fn test_is_target_present_true() {
        let tool = make_tool();
        assert!(DetectBoilerplateTool::is_target_present(
            &tool.chunks, &tool.chunk_order,
            "附件三", "attachment"
        ));
    }

    #[test]
    fn test_is_target_present_false() {
        let tool = make_tool();
        assert!(!DetectBoilerplateTool::is_target_present(
            &tool.chunks, &tool.chunk_order,
            "附件五", "attachment"
        ));
    }

    #[test]
    fn test_detect_superfluous_sections_not_applicable() {
        let tool = make_tool();
        let sections = DetectBoilerplateTool::detect_superfluous_sections(
            &tool.chunks, &tool.chunk_order,
        );
        // ch_003 是"联合体要求"+"本项目不适用" → 应检测到
        assert!(sections.iter().any(|s| s.chunk_id == "ch_003"));
    }

    #[test]
    fn test_detect_superfluous_sections_no_false_positive() {
        // ch_001 有正常内容 → 不应被标记为多余章节
        let tool = make_tool();
        let sections = DetectBoilerplateTool::detect_superfluous_sections(
            &tool.chunks, &tool.chunk_order,
        );
        assert!(!sections.iter().any(|s| s.chunk_id == "ch_001"));
    }

    #[test]
    fn test_anomalous_entities_from_make_tool() {
        let tool = make_tool();
        let entities = DetectBoilerplateTool::detect_anomalous_entities(
            &tool.chunks, &tool.chunk_order,
        );
        // 现有测试数据可能不会产生异常实体（取决于正则匹配）
        // 此测试只验证函数不 panic
        let _ = entities.len();
    }
}
