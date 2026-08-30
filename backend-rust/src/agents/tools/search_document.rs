//! `search_document` 工具 — 在待审标书内部做语义搜索。
//!
//! 底层使用嵌入模型编码查询文本，在 [`DocumentVectorIndex`] 上执行暴力 KNN。
//! 返回 Top-5 匹配 Chunk 的摘要，供 Agent 发现关联章节后精读确认。
//!
//! 嵌入模型通过 [`EmbeddingClient`] 抽象，支持本地 BGE-M3 和远程 text-embedding-v4。

use crate::domain::vector_index::{DocumentVectorIndex, SearchHit};
use crate::services::embedding_service::EmbeddingClient;
use anyhow::Result;
use serde::Deserialize;
use std::sync::Arc;

use super::AgentTool;

/// `search_document` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct SearchDocumentArgs {
    /// 提炼后的关键词组合（不是完整条款原文）
    pub query: String,
}

/// `search_document` 工具实现。
///
/// 持有共享的 DocumentVectorIndex 和嵌入客户端引用（本地/远程透明切换）。
/// 每次调用：编码 query → L2 归一化 → 向量搜索 → 返回 Top-5。
pub struct SearchDocumentTool {
    /// 共享的文档向量索引（Arc 保证线程安全）
    pub index: Arc<DocumentVectorIndex>,
    /// 共享的嵌入客户端（本地 BGE-M3 或远程 text-embedding-v4）
    pub embed: Arc<EmbeddingClient>,
}

impl SearchDocumentTool {
    pub fn new(index: Arc<DocumentVectorIndex>, embed: Arc<EmbeddingClient>) -> Self {
        Self { index, embed }
    }

    /// 编码查询文本并 L2 归一化（归一化由 EmbeddingClient 内部处理）。
    fn encode_query(&self, query: &str) -> Result<Vec<f32>> {
        let results = self.embed.encode_queries(&[query])?;
        Ok(results.into_iter().next().unwrap_or_default())
    }
}

#[async_trait::async_trait]
impl AgentTool for SearchDocumentTool {
    fn name(&self) -> &str {
        "search_document"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "search_document",
                "description": "在招标文件内部语义搜索，返回最相关chunk及摘要(score排序)；用提炼关键词。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "提炼后的关键词组合。好: '品牌 型号 指定'；坏: 粘贴整个条款原文。"
                        }
                    },
                    "required": ["query"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: SearchDocumentArgs = serde_json::from_value(args)?;

        // 编码查询
        let query_emb = self.encode_query(&parsed.query)?;

        // 搜索
        let hits: Vec<SearchHit> = self.index.search(&query_emb, 5);

        // 如果所有命中 score < 0.5，附加提示
        let all_low = hits.iter().all(|h| h.score < 0.5);
        let result = if all_low && !hits.is_empty() {
            serde_json::json!({
                "hits": hits,
                "warning": "所有结果的相似度均低于 0.5，搜索方向可能不对，建议换搜索词。"
            })
        } else {
            serde_json::json!({ "hits": hits })
        };

        Ok(result)
    }
}
