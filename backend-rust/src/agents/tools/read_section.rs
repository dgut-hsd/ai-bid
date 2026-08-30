//! `read_section` 工具 — 按 chunk_id 精读条款原文。
//!
//! 底层在 `HashMap<String, Chunk>` 上做 O(1) 查找。
//! 支持单个 chunk_id 或数组形式批量读取。
//! 返回完整条款文本 + 页码 + 父章节路径。
//!
//! ## 相邻 Chunk 上下文（V5.7）
//!
//! 每次读取同时返回前一个和后一个 chunk 的摘要信息
//! （chunk_id、章节路径、文本前 200 字符），帮助 Agent
//! 判断当前 chunk 的内容是否被截断、是否有关联内容在相邻位置。
//! 这能减少类似"编号 4 出现在 3 之前"的误报——
//! 因为 Agent 能意识到可能只是 PDF 提取的版式问题。

use crate::domain::chunk::Chunk;
use anyhow::{Result, anyhow};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

use super::AgentTool;

/// `read_section` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct ReadSectionArgs {
    /// 要精读的 chunk_id（如 "ch_042"），或数组 ["ch_042", "ch_015"]
    pub chunk_id: serde_json::Value,
}

/// 单个 chunk 的完整信息（返回给 LLM）。
#[derive(Debug, serde::Serialize)]
struct SectionDetail {
    chunk_id: String,
    section_path: Vec<String>,
    text: String,
    page_start: usize,
    page_end: usize,
    char_count: usize,
    /// 上一个 chunk 的摘要（如果存在）
    #[serde(skip_serializing_if = "Option::is_none")]
    prev_chunk: Option<NeighborChunk>,
    /// 下一个 chunk 的摘要（如果存在）
    #[serde(skip_serializing_if = "Option::is_none")]
    next_chunk: Option<NeighborChunk>,
}

/// 相邻 chunk 的简要信息（帮助 Agent 感知上下文连续性）。
#[derive(Debug, serde::Serialize)]
struct NeighborChunk {
    chunk_id: String,
    section_path: Vec<String>,
    /// 文本前 200 字符的摘要
    text_snippet: String,
    page_start: usize,
    page_end: usize,
}

/// `read_section` 工具实现。
///
/// 持有所有 Chunk 的内存索引（Arc<HashMap>），零 I/O 延迟。
/// 同时持有有序 chunk_id 列表以支持相邻上下文查询。
pub struct ReadSectionTool {
    /// Chunk ID → Chunk 映射表
    pub chunks: Arc<HashMap<String, Chunk>>,
    /// 有序 chunk_id 列表（按文档出现顺序），用于查找相邻 chunk
    pub chunk_order: Arc<Vec<String>>,
}

impl ReadSectionTool {
    pub fn new(chunks: Arc<HashMap<String, Chunk>>, chunk_order: Arc<Vec<String>>) -> Self {
        Self {
            chunks,
            chunk_order,
        }
    }

    fn read_one(&self, chunk_id: &str) -> Result<SectionDetail> {
        let chunk = self
            .chunks
            .get(chunk_id)
            .ok_or_else(|| anyhow!("chunk_id 不存在: {}", chunk_id))?;

        // 查找当前 chunk 在有序列表中的位置
        let pos = self.chunk_order.iter().position(|id| id == chunk_id);

        let (prev_chunk, next_chunk) = if let Some(idx) = pos {
            let prev = if idx > 0 {
                self.chunks
                    .get(&self.chunk_order[idx - 1])
                    .map(neighbor_from_chunk)
            } else {
                None
            };
            let next = if idx + 1 < self.chunk_order.len() {
                self.chunks
                    .get(&self.chunk_order[idx + 1])
                    .map(neighbor_from_chunk)
            } else {
                None
            };
            (prev, next)
        } else {
            // chunk 不在有序列表中（理论上不应发生）
            (None, None)
        };

        Ok(SectionDetail {
            chunk_id: chunk.chunk_id.clone(),
            section_path: chunk.section_path.clone(),
            text: chunk.text.clone(),
            page_start: chunk.page_start,
            page_end: chunk.page_end,
            char_count: chunk.text.chars().count(),
            prev_chunk,
            next_chunk,
        })
    }
}

/// 从 Chunk 构造 NeighborChunk 摘要。
fn neighbor_from_chunk(chunk: &Chunk) -> NeighborChunk {
    let snippet: String = chunk.text.chars().take(200).collect();
    NeighborChunk {
        chunk_id: chunk.chunk_id.clone(),
        section_path: chunk.section_path.clone(),
        text_snippet: snippet,
        page_start: chunk.page_start,
        page_end: chunk.page_end,
    }
}

#[async_trait::async_trait]
impl AgentTool for ReadSectionTool {
    fn name(&self) -> &str {
        "read_section"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_section",
                "description": "按 chunk_id 精确读取某个章节的完整原文 + 上下文信息。\
                    【关键】你收到的审查任务消息中已包含当前条款的完整原文——\
                    如果只是审查当前条款，无需为了「看到原文」而调用此工具；\
                    原文已经在你的上下文中了，直接分析即可。\
                    \
                    【仅在以下情况调用】\
                    ① search_document 返回了感兴趣的关联 chunk_id 和摘要，你需要读全文确认证据；\
                    ② Session Graph 显示某关联条款与当前条款有关联（linked_to），你需要交叉验证；\
                    ③ 准备 output_finding 前，需要精确截取 source_quote 的原文措辞（L2/L3 条款）；\
                    ④ 你怀疑当前条款文本不完整（被截断），需要借助相邻上下文验证；\
                    ⑤ 你需要读取当前条款以外的其他条款（跨条款比对）。\
                    \
                    【不要调用的情况】\
                    ① 刚开始审查一个条款就想「看到原文」——原文已在你的任务消息中，直接分析即可；\
                    ② 已经读过同一 chunk_id ≥2 次且没有新疑点——不要重复读取，浪费轮次；\
                    ③ 纯信息/格式类条款（编号、联系方式、项目名称）——已知全文，直接 output_finding，\
                    无需 read_section\
                    【相邻上下文】返回结果中包含 prev_chunk 和 next_chunk 字段：\
                    提供前后相邻 chunk 的摘要（标题 + 文本前 200 字符）。\
                    如果你发现当前 chunk 的编号、格式看起来有问题，\
                    先检查相邻 chunk 的内容——很可能只是 PDF 提取的版式失真或内容被分到了相邻 chunk。\
                    不要因为当前 chunk 内部看起来'不完整'就判定为格式缺失。\
                    【注意】source_quote 必须从原文中逐字截取——不要凭记忆、不要改写条款原文。\
                    优先从 read_section 返回的完整原文中截取（最可靠）；\
                    对于纯信息/格式类条款（L1），可从任务消息中的条款原文逐字截取。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "chunk_id": {
                            "type": "string",
                            "description": "要精读的条款 chunk_id，如 'ch_042'。也支持数组形式: ['ch_042', 'ch_015']——用于对比两个关联条款。"
                        }
                    },
                    "required": ["chunk_id"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: ReadSectionArgs = serde_json::from_value(args)?;

        match &parsed.chunk_id {
            // 单个 chunk_id
            serde_json::Value::String(id) => {
                let detail = self.read_one(id)?;
                Ok(serde_json::to_value(&detail)?)
            }
            // 数组形式：批量读取
            serde_json::Value::Array(ids) => {
                let mut results = Vec::new();
                let mut errors = Vec::new();
                for id_val in ids {
                    if let Some(id) = id_val.as_str() {
                        match self.read_one(id) {
                            Ok(detail) => results.push(detail),
                            Err(e) => errors.push(format!("{}: {}", id, e)),
                        }
                    }
                }
                Ok(serde_json::json!({
                    "sections": results,
                    "errors": errors
                }))
            }
            _ => Err(anyhow!(
                "chunk_id 必须是字符串或字符串数组，收到: {}",
                parsed.chunk_id
            )),
        }
    }
}
