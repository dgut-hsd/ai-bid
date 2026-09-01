//! 知识检索「埋入 → 召回」闭环冒烟测试（方式 B：直连 QdrantStore + EmbeddingClient）
//!
//! 不经过 HTTP / HMAC 中间件，直接复用生产代码里的两个核心函数：
//! - 埋入：`QdrantStore::upsert_chunks`
//! - 召回：`EmbeddingClient::encode_queries` + `QdrantStore::search`
//!
//! 前置依赖：
//! - Qdrant 服务已启动（`QDRANT_URL`，默认 http://localhost:6334 gRPC）
//! - 嵌入服务可用（`EMBED_ENGINE`，本测试按根级 .env 用 remote / text-embedding-v4）
//!
//! 运行：
//!   cargo test --test kb_roundtrip -- --nocapture
//!
//! 判定标准：
//! - collection `legal_kb` 存在且 count >= 3
//! - 查询「资质要求」的 top-1 命中「…二级及以上资质」那条（语义排序正确）

use ai_bid::services::embedding_service::EmbeddingClient;
use ai_bid::services::qdrant_store::{KnowledgePayload, QdrantStore};

/// 加载两层 .env：backend-rust/.env（AIBID_DATA_DIR=..）→ 根级 .env（密钥/EMBED_ENGINE）。
/// 对齐 bin/server.rs 的加载顺序。
fn load_env() {
    dotenv::dotenv().ok();
    if let Ok(data_dir) = std::env::var("AIBID_DATA_DIR") {
        let root_env = std::path::Path::new(&data_dir).join(".env");
        dotenv::from_path(root_env).ok();
    }
    // 兜底：直接尝试上级目录的 .env
    dotenv::from_path("../.env").ok();
}

#[tokio::test]
async fn kb_embed_recall_roundtrip() -> anyhow::Result<()> {
    load_env();

    let engine = std::env::var("EMBED_ENGINE").unwrap_or_else(|_| "local".to_string());
    println!("=== 知识检索「埋入→召回」闭环验证 ===");
    println!("EMBED_ENGINE = {engine}");

    // ── 1. 连接 Qdrant + 确保 collection ─────────────────────────
    let store = QdrantStore::from_env()?;
    store.ensure_collection().await?;
    println!("✅ Qdrant 连接成功，collection `legal_kb` 就绪");

    // ── 2. 嵌入客户端（与查询同一模型，保证向量空间一致）────────
    let embed = EmbeddingClient::from_env()?;

    // ── 3. 埋入 3 条法规片段 ──────────────────────────────────────
    let docs = [
        "投标人须具备工程招标代理机构二级及以上资质",
        "投标人提交投标文件截止日前三年内不得有重大违法记录",
        "投标人应提供近六个月依法缴纳税收的证明材料",
    ];
    let vecs = embed.encode_queries(&docs)?;
    assert_eq!(vecs.len(), 3, "埋入侧应编码出 3 条向量");
    let dim = vecs[0].len();
    println!("✅ 埋入侧编码完成：3 条向量，维度 {dim}");
    assert_eq!(dim, 1024, "text-embedding-v4 应为 1024 维");

    let document_id = "kb-roundtrip-smoke";
    let tenant_id = "smoke-tenant";
    let payloads: Vec<KnowledgePayload> = docs
        .iter()
        .enumerate()
        .map(|(i, t)| KnowledgePayload {
            document_id: document_id.into(),
            document_name: "闭环冒烟.pdf".into(),
            category: "regulation".into(),
            applicable_scope: "general".into(),
            chunk_id: format!("ch_{:03}", i),
            section_path: vec![],
            embed_text: (*t).into(),
            text_len: t.chars().count(),
            page_start: 0,
            page_end: 0,
            ingested_at: "2026-01-01T00:00:00Z".into(),
            tenant_id: tenant_id.into(),
        })
        .collect();
    store.upsert_chunks(payloads, vecs).await?;
    println!("✅ 埋入完成：document_id={document_id}, tenant={tenant_id}");

    // ── 4. 召回 ───────────────────────────────────────────────────
    let query_vec = embed.encode_queries(&["资质要求"])?.remove(0);
    let hits = store
        .search(query_vec, 3, Some("regulation".into()), None, Some(tenant_id.into()))
        .await?;
    assert!(!hits.is_empty(), "召回为空，Qdrant 中无该租户数据");
    println!("✅ 召回完成，命中 {} 条：", hits.len());
    for (score, p) in &hits {
        println!("    score={score:.4}  chunk={}  text={}", p.chunk_id, p.embed_text);
    }

    // ── 5. 断言：语义排序正确 + count > 0 ─────────────────────────
    let top = &hits[0];
    assert_eq!(
        top.1.embed_text, docs[0],
        "查询『资质要求』的 top-1 应为『…二级及以上资质』，实际为『{}』",
        top.1.embed_text
    );
    assert!(top.0 > 0.3, "语义相似度明显偏低：{}", top.0);

    let count = store.count().await?;
    println!("✅ collection `legal_kb` 当前向量总数 = {count}");
    assert!(count >= 3, "collection 向量数应 >= 3，实际 {count}");

    // ── 6. 清理冒烟数据，避免污染真实知识库 ──────────────────────
    store.delete_by_document(document_id, Some(tenant_id)).await?;
    println!("✅ 已清理冒烟数据（document_id={document_id}）");

    println!();
    println!("🎉 闭环验证通过：埋入 → 召回，语义排序正确。");
    Ok(())
}