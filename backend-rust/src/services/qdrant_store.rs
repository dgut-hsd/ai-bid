//! Qdrant 共享接入层（全组共用：入库写入 + 检索读取）
//!
//! 本模块是知识检索组两侧（入库 / 检索）的**字段契约载体**：
//! - 入库成员：写入时按 [`KnowledgePayload`] 组装 point
//! - 检索成员：查询时按 [`KnowledgePayload`] 解析 payload、按 KB_COLLECTION 过滤
//!
//! 环境变量：QDRANT_URL（默认 http://localhost:6334，gRPC）

use anyhow::{Context, Result};
use qdrant_client::qdrant::points_selector::PointsSelectorOneOf;
use qdrant_client::qdrant::r#match::MatchValue;
use qdrant_client::qdrant::{
    Condition, CreateCollectionBuilder, DeletePointsBuilder, Distance, Filter, PointStruct,
    SearchPointsBuilder, UpsertPointsBuilder, VectorParamsBuilder,
};
use qdrant_client::Qdrant;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 知识库 collection 名称（法规/标准库专用，与标书 collection 隔离）
pub const KB_COLLECTION: &str = "legal_kb";
/// 向量维度：BGE-M3 dense / text-embedding-v4 均为 1024
pub const KB_VECTOR_DIM: u64 = 1024;

/// Qdrant Point 的 payload 结构 —— 入库与检索两侧的字段契约
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgePayload {
    pub document_id: String,
    pub document_name: String,
    pub category: String,          // regulation / price / supplier / contract / case / other
    pub applicable_scope: String,  // procurement / engineering / general
    pub chunk_id: String,
    pub section_path: Vec<String>,
    pub embed_text: String,        // 携带章节层级前缀的嵌入文本（供摘要回显）
    pub text_len: usize,
    pub page_start: usize,
    pub page_end: usize,
    pub ingested_at: String,       // RFC3339，入库时间
}

impl KnowledgePayload {
    /// 全局唯一 Point ID。
    ///
    /// 注意：qdrant-client 1.x 的 `From<String> for PointId` 会把字符串**直接当作 UUID**
    /// 发送（不自动转换），因此不能用 `{document_id}_{chunk_id}` 拼接格式（Qdrant 会报
    /// "Unable to parse UUID"）。这里用 UUID v5 从 (document_id, chunk_id) 确定性派生：
    /// 保证全局唯一、幂等可重试，且满足 Qdrant 对 Point ID 的格式要求。
    pub fn point_id(&self) -> String {
        Uuid::new_v5(
            &Uuid::NAMESPACE_DNS,
            format!("{}_{}", self.document_id, self.chunk_id).as_bytes(),
        )
        .to_string()
    }

    /// 转为 qdrant-client 的 Payload（JSON Map）
    pub fn into_payload(self) -> qdrant_client::Payload {
        qdrant_client::Payload::try_from(
            serde_json::to_value(self).expect("KnowledgePayload 序列化失败"),
        )
        .expect("KnowledgePayload 转 Qdrant Payload 失败")
    }
}

/// 共享 Qdrant 客户端封装
pub struct QdrantStore {
    client: Qdrant,
}

impl QdrantStore {
    /// 从环境变量 QDRANT_URL 创建客户端（默认 http://localhost:6334）
    pub fn from_env() -> Result<Self> {
        let url = std::env::var("QDRANT_URL")
            .unwrap_or_else(|_| "http://localhost:6334".to_string());
        let client = Qdrant::from_url(&url)
            .build()
            .context("Qdrant 连接失败，请确认 docker compose 已启动")?;
        Ok(Self { client })
    }

    /// 确保知识库 collection 存在（幂等）
    pub async fn ensure_collection(&self) -> Result<()> {
        if self.client.collection_exists(KB_COLLECTION).await? {
            return Ok(());
        }
        self.client
            .create_collection(
                CreateCollectionBuilder::new(KB_COLLECTION)
                    .vectors_config(VectorParamsBuilder::new(KB_VECTOR_DIM, Distance::Cosine)),
            )
            .await
            .context("创建 collection legal_kb 失败")?;
        Ok(())
    }

    /// 批量写入向量点
    pub async fn upsert_chunks(
        &self,
        payloads: Vec<KnowledgePayload>,
        embeddings: Vec<Vec<f32>>,
    ) -> Result<()> {
        anyhow::ensure!(
            payloads.len() == embeddings.len(),
            "payload 数量与向量数量不一致: {} vs {}",
            payloads.len(),
            embeddings.len()
        );
        let points: Vec<PointStruct> = payloads
            .into_iter()
            .zip(embeddings)
            .map(|(p, vec)| PointStruct::new(p.point_id(), vec, p.into_payload()))
            .collect();
        // 分批 upsert（每批 128 条，法规文件 chunk 通常 < 1000）
        for batch in points.chunks(128) {
            self.client
                .upsert_points(UpsertPointsBuilder::new(KB_COLLECTION, batch.to_vec()))
                .await
                .context("upsert 到 Qdrant 失败")?;
        }
        Ok(())
    }

    /// 语义检索（供检索成员使用）：query 必须已 L2 归一化（复用 EmbeddingClient::encode_queries）
    pub async fn search(
        &self,
        query_vector: Vec<f32>,
        top_k: u64,
        category: Option<String>,
        applicable_scope: Option<String>,
    ) -> Result<Vec<(f32, KnowledgePayload)>> {
        let mut builder = SearchPointsBuilder::new(KB_COLLECTION, query_vector, top_k)
            .with_payload(true);
        // 可选过滤：category / applicable_scope
        let mut must: Vec<Condition> = Vec::new();
        if let Some(c) = category {
            must.push(Condition::matches("category", MatchValue::Keyword(c)));
        }
        if let Some(s) = applicable_scope {
            must.push(Condition::matches("applicable_scope", MatchValue::Keyword(s)));
        }
        if !must.is_empty() {
            builder = builder.filter(Filter::must(must));
        }
        let resp = self.client.search_points(builder).await?;
        Ok(resp
            .result
            .into_iter()
            .filter_map(|hit| {
                let payload: KnowledgePayload = serde_json::from_value(
                    serde_json::to_value(&hit.payload).ok()?,
                )
                .ok()?;
                Some((hit.score as f32, payload))
            })
            .collect())
    }

    /// 按 document_id 删除某份文件的所有向量（Java 组删除标准库文件时调用）
    pub async fn delete_by_document(&self, document_id: &str) -> Result<()> {
        let filter = Filter::must([Condition::matches(
            "document_id",
            MatchValue::Keyword(document_id.to_string()),
        )]);
        self.client
            .delete_points(
                DeletePointsBuilder::new(KB_COLLECTION)
                    .points(PointsSelectorOneOf::Filter(filter)),
            )
            .await
            .context("按 document_id 删除失败")?;
        Ok(())
    }

    /// 集合内向量总数（测试/监控用）
    pub async fn count(&self) -> Result<u64> {
        let info = self.client.collection_info(KB_COLLECTION).await?;
        Ok(info.result.and_then(|r| r.points_count).unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_payload() -> KnowledgePayload {
        KnowledgePayload {
            document_id: "abc-123".into(),
            chunk_id: "ch_000".into(),
            document_name: "某办法.pdf".into(),
            category: "regulation".into(),
            applicable_scope: "general".into(),
            section_path: vec!["第三章".into(), "第X条".into()],
            embed_text: "【第三章 | 第X条】正文".into(),
            text_len: 24,
            page_start: 3,
            page_end: 4,
            ingested_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn test_point_id_is_valid_deterministic_uuid() {
        let p = sample_payload();
        let id = p.point_id();
        // 必须是合法 UUID（Qdrant 要求 Point ID 为 UUID 或整数）
        let parsed = Uuid::parse_str(&id).expect("point_id 必须是合法 UUID");
        assert_eq!(parsed.get_version_num(), 5, "point_id 应为 UUID v5");
        // 确定性：同 (document_id, chunk_id) 产生相同 ID（幂等 upsert）
        assert_eq!(id, sample_payload().point_id());
        // 不同 chunk_id 产生不同 ID
        let other = KnowledgePayload {
            chunk_id: "ch_001".into(),
            ..sample_payload()
        };
        assert_ne!(id, other.point_id());
    }

    #[test]
    fn test_payload_json_roundtrip() {
        let p = sample_payload();
        let v = serde_json::to_value(&p).unwrap();
        let back: KnowledgePayload = serde_json::from_value(v).unwrap();
        assert_eq!(back.point_id(), p.point_id());
        assert_eq!(back.section_path, p.section_path);
        assert_eq!(back.embed_text, p.embed_text);
    }

    #[test]
    fn test_payload_into_payload_has_required_fields() {
        let qdrant_payload = sample_payload().into_payload();
        let map = qdrant_payload.into();
        let map: serde_json::Value = map;
        for key in ["document_id", "chunk_id", "category", "applicable_scope", "embed_text", "section_path"] {
            assert!(map.get(key).is_some(), "payload 缺少字段: {}", key);
        }
    }

    #[test]
    fn test_payload_fields_complete() {
        // 字段契约完整性：入库与检索两侧共用的字段一个都不能少
        let p = sample_payload();
        assert_eq!(p.document_name, "某办法.pdf");
        assert_eq!(p.text_len, 24);
        assert_eq!(p.page_start, 3);
        assert_eq!(p.page_end, 4);
        assert_eq!(p.ingested_at, "2026-01-01T00:00:00Z");
        assert_eq!(p.section_path.len(), 2);
    }
}
