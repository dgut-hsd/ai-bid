//! `search_knowledge_base` 工具 — 搜索本地知识库（法规/案例/负面清单/范本）。
//!
//! 与 `search_knowledge`（全网搜索 DashScope/SearXNG）互补：
//! - 本工具搜索已入库的本地知识库（Qdrant `legal_kb` collection）
//! - 返回法规原文引用（document_name + section_path + 页码）
//! - 适用于需要确切法条依据的场景

use crate::services::qdrant_store::QdrantStore;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::sync::Arc;

use super::AgentTool;

/// `search_knowledge_base` 工具参数。
#[derive(Debug, Deserialize)]
pub struct SearchKnowledgeBaseArgs {
    /// 自然语言查询（非关键词，语义搜索）
    pub question: String,
    /// 类别过滤：法规 / 案例 / 负面清单 / 范本
    #[serde(default)]
    pub category: String,
    /// 适用范围：procurement / engineering / general
    #[serde(default)]
    pub applicable_scope: String,
}

/// 本地知识库语义搜索工具。
///
/// 持有嵌入 API 客户端引用，每次调用：编码查询 → Qdrant 向量检索 → 格式化。
pub struct SearchKnowledgeBaseTool {
    /// 嵌入客户端（远程 Embedding API）
    pub embed: Arc<crate::services::embedding_api_client::EmbeddingApiClient>,
}

impl SearchKnowledgeBaseTool {
    pub fn new(embed: Arc<crate::services::embedding_api_client::EmbeddingApiClient>) -> Self {
        Self { embed }
    }
}

#[async_trait::async_trait]
impl AgentTool for SearchKnowledgeBaseTool {
    fn name(&self) -> &str {
        "search_knowledge_base"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "search_knowledge_base",
                "description": "搜索本地知识库——已入库的法规条文、案例判例、负面清单、标准范本。\n\
                    与 web_search（全网搜索）不同，本工具搜索的是系统内部已向量化的权威文档，\n\
                    能返回法规原文引用（document_name + section_path + 页码）。\n\
                    \n\
                    【使用场景】\n\
                    ① 需要引用具体法规原文（如'财库〔2020〕46号 第X条'）\n\
                    ② 查找某类条款的历史案例判例\n\
                    ③ 确认负面清单是否包含某类行为\n\
                    \n\
                    【不使用场景】\n\
                    ① 查实时新闻/最新政策 → 用 web_search\n\
                    ② 搜当前标书内部 → 用 search_document",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "question": {
                            "type": "string",
                            "description": "自然语言查询。好：'投标人须具备二级资质的具体法律依据是什么？'"
                        },
                        "category": {
                            "type": "string",
                            "enum": ["法规", "案例", "负面清单", "范本"],
                            "description": "搜索类别"
                        },
                        "applicable_scope": {
                            "type": "string",
                            "enum": ["procurement", "engineering", "general"],
                            "description": "适用范围"
                        }
                    },
                    "required": ["question"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: SearchKnowledgeBaseArgs = serde_json::from_value(args)?;

        // 1. 查询向量化
        let query_embeddings = self.embed.encode_batch(&[parsed.question.clone()])?;
        let vec = query_embeddings.into_iter().next().unwrap_or_default();

        // 2. Qdrant 向量检索
        let store = QdrantStore::from_env().context("Qdrant 连接失败")?;
        let category = if parsed.category.is_empty() {
            None
        } else {
            Some(parsed.category.clone())
        };
        let scope = if parsed.applicable_scope.is_empty() {
            None
        } else {
            Some(parsed.applicable_scope.clone())
        };
        let results = store.search(vec, 5, category, scope).await?;

        // 3. 格式化返回
        let hits: Vec<serde_json::Value> = results
            .into_iter()
            .map(|(score, payload)| {
                serde_json::json!({
                    "document_name": payload.document_name,
                    "document_id": payload.document_id,
                    "chunk_id": payload.chunk_id,
                    "relevance_score": score,
                    "snippet": payload.embed_text.chars().take(500).collect::<String>(),
                    "category": payload.category,
                    "section_path": payload.section_path,
                    "page": format!("第{}页", payload.page_start + 1),
                })
            })
            .collect();

        Ok(serde_json::json!({
            "source": "local_knowledge_base",
            "hits": hits,
            "total_hits": hits.len(),
        }))
    }
}
