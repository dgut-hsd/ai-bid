//! `search_knowledge_base` 工具 — 搜索本地知识库（法规/案例/负面清单/范本）。
//!
//! 与 `search_knowledge`（全网搜索 DashScope/SearXNG）互补：
//! - 本工具搜索已入库的本地知识库（Qdrant `legal_kb` collection）
//! - 返回法规原文引用（document_name + section_path + 页码）

use crate::services::embedding_service::EmbeddingClient;
use crate::services::qdrant_store::QdrantStore;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::sync::Arc;

use super::AgentTool;

#[derive(Debug, Deserialize)]
pub struct SearchKnowledgeBaseArgs {
    pub question: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub applicable_scope: String,
}

/// 工具 definition 里 LLM 看到的中文类别 → Qdrant payload 里存的英文值。
///
/// 入库时 category 字段按 KnowledgePayload 契约存英文（regulation/case/...），
/// 而工具 definition 面向 LLM 用中文枚举。这里做映射，否则中文值永远
/// 匹配不到 Qdrant 的 payload 过滤条件。
fn map_category(category: &str) -> Option<String> {
    match category {
        "法规" => Some("regulation".to_string()),
        "案例" => Some("case".to_string()),
        "负面清单" => Some("negative_list".to_string()),
        "范本" => Some("template".to_string()),
        "" => None, // 未指定 → 不过滤
        other => Some(other.to_string()), // 英文直传（兼容直接传英文的调用方）
    }
}

pub struct SearchKnowledgeBaseTool {
    /// 嵌入客户端（与入库共享同一 EmbeddingClient，保证向量空间一致）
    pub embed: Arc<EmbeddingClient>,
}

impl SearchKnowledgeBaseTool {
    pub fn new(embed: Arc<EmbeddingClient>) -> Self {
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
                            "description": "自然语言查询"
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
        let question = parsed.question.clone();
        let embed = self.embed.clone();

        // 同步嵌入调用移出 Tokio worker，避免阻塞 reactor
        let query_embeddings = tokio::task::spawn_blocking(move || {
            embed.encode_queries(&[question.as_str()])
        })
        .await
        .context("嵌入任务执行失败")??;
        let vec = query_embeddings.into_iter().next().unwrap_or_default();

        let store = QdrantStore::from_env().context("Qdrant 连接失败")?;
        let cat = map_category(&parsed.category);
        let scope = if parsed.applicable_scope.is_empty() {
            None
        } else {
            Some(parsed.applicable_scope)
        };
        // Agent 工具在单租户审核会话内使用，不传 tenant 过滤
        let results = store.search(vec, 5, cat, scope, None).await?;

        let hits: Vec<serde_json::Value> = results
            .into_iter()
            .map(|(score, payload)| serde_json::json!({
                "document_name": payload.document_name,
                "document_id": payload.document_id,
                "chunk_id": payload.chunk_id,
                "relevance_score": score,
                "snippet": payload.embed_text.chars().take(500).collect::<String>(),
                "category": payload.category,
                "section_path": payload.section_path,
                "page": format!("第{}页", payload.page_start + 1),
            }))
            .collect();

        Ok(serde_json::json!({
            "source": "local_knowledge_base",
            "hits": hits,
            "total_hits": hits.len(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_category_chinese_to_english() {
        assert_eq!(map_category("法规"), Some("regulation".to_string()));
        assert_eq!(map_category("案例"), Some("case".to_string()));
        assert_eq!(map_category("负面清单"), Some("negative_list".to_string()));
        assert_eq!(map_category("范本"), Some("template".to_string()));
    }

    #[test]
    fn test_map_category_empty_means_no_filter() {
        assert_eq!(map_category(""), None);
    }

    #[test]
    fn test_map_category_english_passthrough() {
        assert_eq!(map_category("regulation"), Some("regulation".to_string()));
    }
}
