//! 组长：整合主流程 — 内存函数串联，无中间文件。
//!
//! 两条落库通道：
//! - `run`：Neo4j 图库沉淀（实体图 + RiskExperience 经验节点）
//! - `store_experiences`：Qdrant 向量库沉淀（同批经验的语义向量，供检索召回）
//! 调用方（CLI / HTTP）按开关分别执行；任一条失败仅告警，不影响审核结果。

use anyhow::{Context, Result};
use std::sync::Arc;

use crate::agents::types::RiskFinding;
use crate::knowledge::collect::collect_candidates;
use crate::knowledge::extract::extract_and_dedup;
use crate::knowledge::graph::Neo4jClient;
use crate::knowledge::types::{Decision, EntityDecision};
use crate::services::embedding_service::EmbeddingClient;
use crate::services::qdrant_store::{KnowledgePayload, QdrantStore};

/// 一次沉淀的产物：供调用方决定后续以何种方式落库。
#[derive(Debug, Clone)]
pub struct RunOutcome {
    /// 本次新增（New）的决策数
    pub new_count: usize,
    /// 全部决策（含 Exists），含每次必产的处置经验
    pub decisions: Vec<EntityDecision>,
}

/// 审核结果 → 挑精华 → 查重 → 写入 Neo4j。
///
/// `batch_key`：当前审核批次的确定性标识（CLI 传文件标识、HTTP 传 tenant/doc_id），
/// 作为 experience_id 的批次盐，保证跨文档同类型风险不撞 ID（P1 修复）。
pub async fn run(
    findings: Vec<RiskFinding>,
    client: &Neo4jClient,
    batch_key: &str,
) -> Result<RunOutcome> {
    let candidates = collect_candidates(&findings);
    let existing = client.all_law_ids().await?;
    let decisions = extract_and_dedup(candidates, &existing, batch_key);
    let new_count = decisions
        .iter()
        .filter(|d| d.decision == Decision::New)
        .count();
    client.write(decisions.clone()).await?;
    Ok(RunOutcome {
        new_count,
        decisions,
    })
}

/// 将本次审核沉淀的处置经验写入 Qdrant（DO 要求"沉淀到 Neo4j 和 Qdrant"双库）。
///
/// 复用 `legal_kb` 集合 + `KnowledgePayload` 契约（category=case），向量与
/// `search_knowledge_base` 工具同一 EmbeddingClient / 同一 1024 维，检索侧零改动。
/// 幂等：Point ID 由 QdrantStore 以 UUIDv5(document_id, chunk_id) 派生，同文档重跑覆盖。
///
/// 返回写入的向量点数量。
pub async fn store_experiences(
    decisions: &[EntityDecision],
    batch_key: &str,
    tenant_id: &str,
    embed: Arc<EmbeddingClient>,
) -> Result<usize> {
    let doc_ref = format!("audit/{}", batch_key);
    let now = chrono::Utc::now().to_rfc3339();

    let texts: Vec<String> = decisions
        .iter()
        .map(|d| {
            let exp = &d.risk_experience;
            let law_names: Vec<String> = d
                .laws
                .iter()
                .map(|l| {
                    let short = if l.short_name.is_empty() {
                        l.law_name.clone()
                    } else {
                        l.short_name.clone()
                    };
                    match &l.article_no {
                        Some(no) => format!("{}{}", short, no),
                        None => short,
                    }
                })
                .collect();
            format!(
                "【历史审核经验】\n风险类型：{}\n严重度：{}\n依据条款：{}\n事实锚点：{}\n论证理由：{}\n处置建议：{}\n置信度：{:.2}",
                d.risk.name,
                d.risk.severity,
                law_names.join("、"),
                exp.source_quote,
                exp.reason,
                exp.suggestion,
                exp.confidence,
            )
        })
        .collect();

    if texts.is_empty() {
        return Ok(0);
    }

    let texts_for_task = texts.clone();
    let embed_for_task = std::sync::Arc::clone(&embed);
    let embeddings =
        tokio::task::spawn_blocking(move || {
            let refs: Vec<&str> = texts_for_task.iter().map(|s| s.as_str()).collect();
            embed_for_task.encode_queries(&refs)
        })
        .await
        .context("嵌入任务执行失败")??;
    debug_assert_eq!(embeddings.len(), decisions.len());

    let payloads: Vec<KnowledgePayload> = decisions
        .iter()
        .zip(texts.iter())
        .map(|(d, text)| {
            let exp = &d.risk_experience;
            KnowledgePayload {
                document_id: doc_ref.clone(),
                document_name: d.risk.name.clone(),
                category: "case".to_string(),
                applicable_scope: "general".to_string(),
                chunk_id: exp.experience_id.clone(),
                section_path: vec!["知识沉淀".to_string(), "处置经验".to_string()],
                embed_text: text.clone(),
                text_len: text.chars().count(),
                page_start: 0,
                page_end: 0,
                ingested_at: now.clone(),
                tenant_id: tenant_id.to_string(),
            }
        })
        .collect();

    let store = QdrantStore::from_env().context("Qdrant 初始化失败")?;
    store.ensure_collection().await?;
    store.upsert_chunks(payloads, embeddings).await?;
    Ok(texts.len())
}