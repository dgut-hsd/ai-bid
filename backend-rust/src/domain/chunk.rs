//! 条款级语义切块数据模型
//!
//! 本模块定义了从 [`Section`] 树切分为 Agent 可独立消费的
//! 条款级语义块（Chunk）的数据结构。每个 Chunk 是一个可以
//! 独立理解、独立评估的完整语义单元。
//!
//! ## 切分原则
//!
//! 一个 chunk = 一个可以独立理解、独立评估的完整语义单元。
//! 不是简单地把每个叶子节点当成一个 chunk —— 容器节点向上聚合，
//! 短叶子相邻合并，过长 chunk 在段落边界硬切。
//!
//! ## Chunk 类型
//!
//! - [`ChunkType::Leaf`] — 自包含叶子节点直接成 chunk（规则1）
//! - [`ChunkType::Merged`] — 容器聚合或短叶子合并（规则2+3）
//! - [`ChunkType::Split`] — 过长硬切（规则4）

use serde::{Deserialize, Serialize};

use crate::domain::raw_document::BBox;

/// 条款级语义切块，是 Agent 审查的最小单位。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    /// 唯一标识，格式 `"ch_042"`
    pub chunk_id: String,
    /// 切分方式
    pub chunk_type: ChunkType,
    /// 从根章节到当前节点的标题链（层级路径）
    pub section_path: Vec<String>,
    /// chunk 完整文本（含标题行）
    pub text: String,
    /// 起始页码 (0-based)
    pub page_start: usize,
    /// 结束页码 (0-based，包含)
    pub page_end: usize,
    /// 来源 block ID（用于回溯高亮）
    pub source_block_ids: Vec<String>,
    /// 预计算的 BBox 缓存（block_id → 页面坐标）。
    /// 由 `populate_bbox_refs()` 在 chunk 切分完成后统一填充。
    /// 空 Vec 表示尚未填充（如 CLI 路径中未调用 populate）。
    #[serde(default)]
    pub bbox_refs: Vec<BlockBBox>,
}

/// 切分方式枚举，记录 chunk 是如何从 Section 树产生的。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ChunkType {
    /// 规则1：自包含叶子节点直接成 chunk
    Leaf,
    /// 规则2（容器聚合）或规则3（短叶子相邻合并）
    Merged {
        /// 聚合规则描述："container_aggregation" 或 "adjacent_merge"
        rule: String,
        /// 合并的子节点数量
        child_count: usize,
    },
    /// 规则4：过长 chunk 在段落边界硬切，保留 overlap
    Split {
        /// 当前片段编号 (1-based)
        part: usize,
        /// 总片段数
        total: usize,
    },
}

/// Chunk 切分配置，控制切分粒度与行为。
pub struct ChunkingConfig {
    /// 规则3：短叶子合并阈值（字符数），低于此长度的独立叶子将被合并。
    /// 默认：100
    pub merge_min_len: usize,
    /// 规则4：过长硬切阈值（字符数），超过此长度的 chunk 将被切分。
    /// 默认：1500
    pub split_max_len: usize,
    /// 规则4：硬切时前后片段的重叠字符数。
    /// 默认：200
    pub split_overlap: usize,
    /// 规则5：嵌入文本中携带的层级前缀深度（祖先标题层数）。
    /// 默认：2
    pub embed_ctx_depth: usize,
    /// 后处理：低于此字符数的碎片 chunk 将被合并到相邻 chunk。
    /// 设为 0 禁用。默认：30
    pub min_chunk_size: usize,
    /// 规则5：embed_text 中单个路径元素的最大字符数，超出截断。
    /// 设为 0 禁用截断。默认：40
    pub embed_path_max_len: usize,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            merge_min_len: 120,
            split_max_len: 1500,
            split_overlap: 200,
            embed_ctx_depth: 2,
            min_chunk_size: 50,
            embed_path_max_len: 40,
        }
    }
}

/// 预计算的 BBox 信息，用于前端 bbox-based PDF 精确高亮。
///
/// 每个 `BlockBBox` 对应 `Chunk.source_block_ids` 中的一个 block，
/// 包含其在原始 PDF 中的精确坐标和页面宽度（用于前端 scale 计算）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockBBox {
    /// Block ID（如 "b_5_3"）
    pub block_id: String,
    /// 所在页码 (0-based)
    pub page: usize,
    /// 包围盒坐标（PDF points，原点左上角）
    pub bbox: BBox,
    /// 原始 PDF 页面宽度 (pt)，用于前端 scale = renderedWidth / pageWidth
    pub page_width: f64,
    /// block 文本的字符数，用于按真实文本长度估算其在 chunk.text 中的偏移，
    /// 替代按 block 序号做 index 比例估算（后者在 block 长度差异大时偏移严重）。
    #[serde(default)]
    pub char_count: usize,
}
