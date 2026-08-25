//! 知识库「埋入 → 召回」直观演示 CLI
//!
//! 一条命令喂文档入库，一条命令语义检索，肉眼判断召回内容是否相关。
//! 直接复用生产函数（knowledge_ingest_service::ingest_file + QdrantStore + EmbeddingClient），
//! 不经过 HTTP / HMAC 中间件。
//!
//! ## 用法（在 backend-rust 目录下，Qdrant 需已启动）
//!
//! ```text
//! cargo run --bin kb_demo -- ingest <pdf或docx路径> [--category regulation] [--scope general] [--tenant demo] [--name 显示名]
//! cargo run --bin kb_demo -- search <查询词> [--top-k 5] [--category 法规] [--scope general] [--tenant demo]
//! cargo run --bin kb_demo -- count
//! cargo run --bin kb_demo -- delete <document_id> [--tenant demo]
//! ```
//!
//! 注意：
//! - `--tenant` 检索过滤键，search 必须与 ingest 一致，否则搜不到。
//! - `--category` 支持中文（法规/案例/负面清单/范本）或英文（regulation/case/negative_list/template）。
//! - 嵌入引擎由根 .env 的 `EMBED_ENGINE` 决定（默认 remote / text-embedding-v4）。

use std::collections::HashMap;
use std::path::PathBuf;

use ai_bid::services::embedding_service::EmbeddingClient;
use ai_bid::services::qdrant_store::{KnowledgePayload, QdrantStore};
use anyhow::{Context, Result};

fn load_env() {
    dotenv::dotenv().ok();
    if let Ok(data_dir) = std::env::var("AIBID_DATA_DIR") {
        dotenv::from_path(std::path::Path::new(&data_dir).join(".env")).ok();
    }
    dotenv::from_path("../.env").ok();
}

/// 极小命令行解析：位置参数 + `--flag value`。
struct Opts {
    positional: Vec<String>,
    flags: HashMap<String, String>,
}

fn parse(args: &[String]) -> Opts {
    let mut positional = Vec::new();
    let mut flags = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        if let Some(name) = args[i].strip_prefix("--") {
            if let Some(val) = args.get(i + 1) {
                flags.insert(name.to_string(), val.clone());
                i += 2;
            } else {
                flags.insert(name.to_string(), String::new());
                i += 1;
            }
        } else {
            positional.push(args[i].clone());
            i += 1;
        }
    }
    Opts { positional, flags }
}

/// LLM 可见的中文类别 → Qdrant payload 存的英文值（对齐 search_knowledge_base 工具）。
fn map_category(c: &str) -> Option<String> {
    match c {
        "法规" => Some("regulation".into()),
        "案例" => Some("case".into()),
        "负面清单" => Some("negative_list".into()),
        "范本" => Some("template".into()),
        "" => None,
        other => Some(other.to_string()),
    }
}

fn truncate(s: &str, n: usize) -> String {
    let count = s.chars().count();
    let head: String = s.chars().take(n).collect();
    if count > n {
        format!("{}…", head)
    } else {
        head
    }
}

fn usage() {
    eprintln!(
        "\
知识库「埋入→召回」演示：
  cargo run --bin kb_demo -- ingest <pdf/docx路径> [--category regulation] [--scope general] [--tenant demo] [--name 显示名]
  cargo run --bin kb_demo -- search <查询词> [--top-k 5] [--category 法规] [--scope general] [--tenant demo]
  cargo run --bin kb_demo -- count
  cargo run --bin kb_demo -- delete <document_id> [--tenant demo]
  cargo run --bin kb_demo -- text-ingest <法规txt> [--category regulation] [--tenant kb] [--name 显示名]"
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    load_env();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("ingest") => cmd_ingest(&args[1..]).await,
        Some("search") => cmd_search(&args[1..]).await,
        Some("count") => cmd_count().await,
        Some("delete") => cmd_delete(&args[1..]).await,
        Some("text-ingest") => cmd_text_ingest(&args[1..]).await,
        _ => {
            usage();
            Ok(())
        }
    }
}

async fn cmd_ingest(args: &[String]) -> Result<()> {
    let o = parse(args);
    let path = o
        .positional
        .first()
        .context("用法: kb_demo ingest <pdf/docx路径> [--category ...] [--tenant ...]")?;
    let filename = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path.as_str())
        .to_string();
    let category = o.flags.get("category").cloned().unwrap_or_else(|| "regulation".into());
    let scope = o.flags.get("scope").cloned().unwrap_or_else(|| "general".into());
    let tenant = o.flags.get("tenant").cloned().unwrap_or_else(|| "demo".into());
    let name = o.flags.get("name").cloned().unwrap_or_else(|| filename.clone());

    println!("[*] 正在入库: {path}");
    let result = ai_bid::services::knowledge_ingest_service::ingest_file(
        PathBuf::from(path.as_str()),
        &name,
        &category,
        &scope,
        &tenant,
    )
    .await?;

    println!(
        "✅ 入库完成: document_id={}, chunks={}, collection={}, 耗时 {}ms",
        result.document_id, result.chunk_count, result.collection, result.elapsed_ms
    );
    println!("   连库后检索: cargo run --bin kb_demo -- search \"<查询词>\" --tenant {tenant}");
    println!("   想删掉它:   cargo run --bin kb_demo -- delete {} --tenant {tenant}", result.document_id);
    Ok(())
}

/// 直接入库纯文本「法规条文」知识库（标题：正文逐行），模拟入库组的条文级切分。
/// 不走 PDF 解析，专用于快速搭建事实知识库做检索演示。
async fn cmd_text_ingest(args: &[String]) -> Result<()> {
    let o = parse(args);
    let path = o
        .positional
        .first()
        .context("用法: kb_demo text-ingest <txt路径> [--category regulation] [--tenant kb] [--name 显示名]")?;
    let filename = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path.as_str())
        .to_string();
    let category = o.flags.get("category").cloned().unwrap_or_else(|| "regulation".into());
    let scope = o.flags.get("scope").cloned().unwrap_or_else(|| "general".into());
    let tenant = o.flags.get("tenant").cloned().unwrap_or_else(|| "kb".into());
    let name = o
        .flags
        .get("name")
        .cloned()
        .unwrap_or_else(|| filename.trim_end_matches(".txt").to_string());

    // 读文本 → 按空行切段 → 去掉换行压成一行；`#` 开头为注释，跳过
    let raw = std::fs::read_to_string(path.as_str()).with_context(|| format!("读取失败: {path}"))?;
    let mut labels: Vec<String> = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    for para in raw.split("\n\n") {
        let one: String = para.chars().filter(|&c| c != '\n' && c != '\r').collect();
        let one = one.trim().to_string();
        if one.is_empty() || one.starts_with('#') {
            continue;
        }
        match one.find('：') {
            Some(pos) => {
                labels.push(one[..pos].to_string());
                texts.push(one[pos + '：'.len_utf8()..].to_string());
            }
            None => {
                labels.push(String::new());
                texts.push(one);
            }
        }
    }
    anyhow::ensure!(!texts.is_empty(), "文本里没有可入库的法条（请用「标题：正文」逐行写）");

    // 与查询同模型编码（保证向量空间一致）
    let embed = EmbeddingClient::from_env()?;
    let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
    let vecs = embed.encode_queries(&refs)?;
    println!("✅ 已编码 {} 条法条，维度 {}", vecs.len(), vecs.first().map(|v| v.len()).unwrap_or(0));

    // 组装 payload + upsert（chunk_id 用 art_000 递增保持稳定）
    let document_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let payloads: Vec<KnowledgePayload> = texts
        .iter()
        .enumerate()
        .map(|(i, t)| KnowledgePayload {
            document_id: document_id.clone(),
            document_name: name.clone(),
            category: category.clone(),
            applicable_scope: scope.clone(),
            chunk_id: format!("art_{:03}", i),
            section_path: if labels[i].is_empty() { vec![] } else { vec![labels[i].clone()] },
            embed_text: t.clone(),
            text_len: t.chars().count(),
            page_start: 0,
            page_end: 0,
            ingested_at: now.clone(),
            tenant_id: tenant.clone(),
        })
        .collect();

    let store = QdrantStore::from_env()?;
    store.ensure_collection().await?;
    store.upsert_chunks(payloads, vecs).await?;

    println!("✅ 事实知识库入库完成: {} 条法条, document_id={document_id}, 来源={name}", texts.len());
    println!("   检索: cargo run --bin kb_demo -- search \"<事实问题>\" --tenant {tenant}");
    Ok(())
}

async fn cmd_search(args: &[String]) -> Result<()> {
    let o = parse(args);
    let query = o.positional.first().context("用法: kb_demo search <查询词> [--top-k ...]")?;
    let top_k: u64 = o.flags.get("top-k").and_then(|v| v.parse().ok()).unwrap_or(5).clamp(1, 50);
    let category = o.flags.get("category").and_then(|c| map_category(c));
    let scope = o.flags.get("scope").cloned();
    let tenant = o.flags.get("tenant").cloned();

    let embed = EmbeddingClient::from_env()?;
    let query_vec = embed.encode_queries(&[query.as_str()])?.remove(0);
    let store = QdrantStore::from_env()?;
    let hits = store.search(query_vec, top_k, category, scope.clone(), tenant.clone()).await?;

    println!("查询「{query}」→ 命中 {} 条（top_k={}）:", hits.len(), top_k);
    for (i, (score, p)) in hits.iter().enumerate() {
        println!(
            "  #{}. score={:.4}  {}\n      类别={}  条款={}  第{}页\n      原文: {}",
            i + 1,
            score,
            p.document_name,
            p.category,
            if p.section_path.is_empty() { "-".into() } else { p.section_path.join(" / ") },
            p.page_start + 1,
            truncate(&p.embed_text, 120),
        );
    }
    if hits.is_empty() {
        println!("  （无结果：请先 ingest，且确认 --tenant 与入库时一致、--category 与入库值匹配）");
    }
    Ok(())
}

async fn cmd_count() -> Result<()> {
    let store = QdrantStore::from_env()?;
    println!("collection `legal_kb` 向量总数 = {}", store.count().await?);
    Ok(())
}

async fn cmd_delete(args: &[String]) -> Result<()> {
    let o = parse(args);
    let doc_id = o.positional.first().context("用法: kb_demo delete <document_id> [--tenant ...]")?;
    let tenant = o.flags.get("tenant").map(String::as_str);
    let store = QdrantStore::from_env()?;
    store.delete_by_document(doc_id, tenant).await?;
    println!("✅ 已删除 document_id={doc_id} 的全部向量");
    Ok(())
}