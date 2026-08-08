//! 法规/标准库文件入库服务
//!
//! 复用现有管线：PDF提取 → 章节化 → 切分 → 嵌入 → 写入 Qdrant。
//! 与标书链路的唯一区别：最后一步从 save_index（落盘）改为 upsert 到 Qdrant。

use crate::domain::chunk::ChunkingConfig;
use crate::domain::raw_document::RawDocument;
use crate::domain::vector_index::DocumentVectorIndex;
use crate::paths::data_path_str;
use crate::services::chunking_service::{chunk_sections, populate_bbox_refs};
use crate::services::docx_convert_service::convert_docx_to_pdf;
use crate::services::pdf_extract_service::{extract_pdf_to_raw_json, extract_with_python};
use crate::services::qdrant_store::{KnowledgePayload, QdrantStore, KB_COLLECTION, KB_VECTOR_DIM};
use crate::services::sectionize_service::{self, Section};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use uuid::Uuid;

/// 单份文件入库结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IngestResult {
    pub document_id: String,
    pub document_name: String,
    pub category: String,
    pub applicable_scope: String,
    pub chunk_count: usize,
    pub dimension: u64,
    pub collection: String,
    pub elapsed_ms: u64,
}

/// 递归收集 Section 树中的全部子章节（对齐 main.rs 的 orphan 处理）
fn collect_all_block_ids(section: &Section) -> Vec<&str> {
    let mut ids: Vec<&str> = section.block_ids.iter().map(|s| s.as_str()).collect();
    for child in &section.children {
        ids.extend(collect_all_block_ids(child));
    }
    ids
}

/// 将文件字节入库（同步耗时管线，调用方建议放入 tokio::task::spawn_blocking 或独立异步任务）
pub async fn ingest_bytes(
    file_bytes: Vec<u8>,
    filename: &str,
    category: &str,
    applicable_scope: &str,
) -> Result<IngestResult> {
    let start = std::time::Instant::now();
    let store = QdrantStore::from_env().context("Qdrant 初始化失败")?;
    store.ensure_collection().await?;

    // ── 1. 落盘临时文件（对齐 process_document L336-344）──
    let tmp_dir = data_path_str("tmp");
    fs::create_dir_all(&tmp_dir).context("创建临时目录失败")?;
    let stem = Uuid::new_v4().to_string();
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("pdf");
    let tmp_path = format!("{}/{}.{}", tmp_dir, stem, ext);
    fs::write(&tmp_path, &file_bytes).context("写入临时文件失败")?;
    let document_id = stem.clone();

    // ── 2. DOCX → PDF（可选）──
    let pdf_path = if ext == "docx" || ext == "doc" {
        convert_docx_to_pdf(&tmp_path, &tmp_dir).context("DOCX 转 PDF 失败")?
    } else {
        std::path::PathBuf::from(&tmp_path)
    };
    let pdf_path_str = pdf_path.to_str().unwrap_or(&tmp_path).to_string();

    // ── 3. PDF → RawDocument（Rust 主 + Python 兜底）──
    let raw_doc: RawDocument = match extract_pdf_to_raw_json(&pdf_path_str) {
        Ok(doc) => doc,
        Err(e) => {
            println!("[INGEST] Rust 解析失败: {}, 切换 Python 兜底", e);
            let fallback = format!("{}/{}_fallback_raw.json", tmp_dir, stem);
            extract_with_python(&pdf_path_str, &fallback)
                .context("PDF 解析失败（Rust 与 Python 均失败）")?;
            serde_json::from_str(&fs::read_to_string(&fallback)?.as_str())
                .context("读取 Python 兜底 JSON 失败")?
        }
    };

    // ── 4. 章节化 + 表格处理（对齐 process_document L398-451）──
    let sections_output = sectionize_service::sectionize(&raw_doc);
    let mut raw_doc_mut = {
        // Re-serialize and deserialize to get a mutable copy
        // (RawDocument doesn't implement Clone)
        let json = serde_json::to_value(&raw_doc).context("序列化 RawDocument 失败")?;
        serde_json::from_value(json).context("反序列化 RawDocument 失败")?
    };
    // 4a. 纯文本表格检测
    sectionize_service::detect_pipe_tables(&mut raw_doc_mut);

    // 4b. orphan block 兜底（对齐 main.rs L338-426：孤儿块按连续页码分组构造临时 Section）
    let assigned: HashSet<&str> = sections_output
        .sections
        .iter()
        .flat_map(|s| collect_all_block_ids(s))
        .collect();
    let orphan_blocks: Vec<&crate::domain::raw_document::RawBlock> = raw_doc_mut
        .pages
        .iter()
        .flat_map(|p| p.blocks.iter())
        .filter(|b| !assigned.contains(b.id.as_str()))
        .collect();

    let mut all_sections = sections_output.sections.clone();
    if !orphan_blocks.is_empty() {
        let block_page: HashMap<&str, usize> = raw_doc_mut
            .pages
            .iter()
            .flat_map(|p| p.blocks.iter().map(move |b| (b.id.as_str(), p.page_index)))
            .collect();
        let mut page_to_blocks: BTreeMap<usize, Vec<&crate::domain::raw_document::RawBlock>> =
            BTreeMap::new();
        for block in &orphan_blocks {
            if let Some(&page_idx) = block_page.get(block.id.as_str()) {
                page_to_blocks.entry(page_idx).or_default().push(*block);
            }
        }
        let sorted_pages: Vec<usize> = page_to_blocks.keys().copied().collect();
        let mut page_groups: Vec<Vec<usize>> = Vec::new();
        let mut current_group: Vec<usize> = Vec::new();
        for &p in &sorted_pages {
            if current_group.is_empty() || p == current_group.last().unwrap() + 1 {
                current_group.push(p);
            } else {
                page_groups.push(std::mem::take(&mut current_group));
                current_group.push(p);
            }
        }
        if !current_group.is_empty() {
            page_groups.push(current_group);
        }
        for group in &page_groups {
            let group_start = *group.first().unwrap();
            let group_end = *group.last().unwrap();
            let group_blocks: Vec<&&crate::domain::raw_document::RawBlock> = group
                .iter()
                .flat_map(|p| page_to_blocks[p].iter())
                .collect();
            let orphan_ids: Vec<String> = group_blocks.iter().map(|b| b.id.clone()).collect();
            let orphan_text = group_blocks
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            all_sections.push(Section {
                level: 0,
                title: format!("未归类内容 (第{}-{}页)", group_start + 1, group_end + 1),
                pattern: "orphan".to_string(),
                page_start: group_start,
                page_end: group_end,
                block_ids: orphan_ids,
                body_text: orphan_text,
                children: Vec::new(),
                body_page_start: group_start,
                body_page_end: group_end,
            });
        }
        println!(
            "[INGEST] 已补充 {} 个 orphan block（{} 个连续页码组）兜底 Section",
            orphan_blocks.len(),
            page_groups.len()
        );
    }

    // 4c. 跨页表合并 + 表格内容注入
    sectionize_service::merge_cross_page_tables(&mut raw_doc_mut);
    sectionize_service::inject_tables_into_sections(&mut all_sections, &raw_doc_mut);

    // ── 5. 切分 ──
    let config = ChunkingConfig::default();
    let mut chunks = chunk_sections(&all_sections, &config);
    populate_bbox_refs(&mut chunks, &raw_doc);

    // ── 6. 嵌入（按 EMBED_ENGINE 切换，对齐 handlers.rs L529-546）──
    let embed_engine = std::env::var("EMBED_ENGINE").unwrap_or_else(|_| "local".to_string());
    let doc_index: DocumentVectorIndex = if embed_engine == "remote" {
        let api_client =
            crate::services::embedding_api_client::EmbeddingApiClient::from_env()?;
        crate::services::embedding_service::embed_chunks_remote(
            &chunks,
            &config,
            &document_id,
            &api_client,
        )?
    } else {
        crate::services::embedding_service::embed_chunks_parallel(
            &chunks,
            &config,
            &document_id,
            2,
        )?
    };

    // ── 7. 组装 payload → upsert 到 Qdrant ──
    let now = chrono::Utc::now().to_rfc3339();
    let payloads: Vec<KnowledgePayload> = doc_index
        .chunks
        .iter()
        .map(|meta| KnowledgePayload {
            document_id: document_id.clone(),
            document_name: filename.to_string(),
            category: category.to_string(),
            applicable_scope: applicable_scope.to_string(),
            chunk_id: meta.chunk_id.clone(),
            section_path: meta.section_path.clone(),
            embed_text: meta.embed_text.clone(),
            text_len: meta.text_len,
            page_start: meta.page_start,
            page_end: meta.page_end,
            ingested_at: now.clone(),
        })
        .collect();
    store.upsert_chunks(payloads, doc_index.embeddings).await?;

    // ── 8. 清理临时文件 ──
    let _ = fs::remove_file(&tmp_path);
    if pdf_path_str != tmp_path {
        let _ = fs::remove_file(&pdf_path_str);
    }

    Ok(IngestResult {
        document_id,
        document_name: filename.to_string(),
        category: category.to_string(),
        applicable_scope: applicable_scope.to_string(),
        chunk_count: chunks.len(),
        dimension: KB_VECTOR_DIM,
        collection: KB_COLLECTION.to_string(),
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ingest_result_fields_complete() {
        let r = IngestResult {
            document_id: "doc-1".into(),
            document_name: "某办法.pdf".into(),
            category: "regulation".into(),
            applicable_scope: "engineering".into(),
            chunk_count: 42,
            dimension: KB_VECTOR_DIM,
            collection: KB_COLLECTION.to_string(),
            elapsed_ms: 1234,
        };
        assert_eq!(r.document_id, "doc-1");
        assert_eq!(r.document_name, "某办法.pdf");
        assert_eq!(r.category, "regulation");
        assert_eq!(r.applicable_scope, "engineering");
        assert_eq!(r.chunk_count, 42);
        assert_eq!(r.dimension, KB_VECTOR_DIM);
        assert_eq!(r.collection, KB_COLLECTION);
        assert!(r.elapsed_ms > 0);
    }

    #[test]
    fn test_ingest_result_empty_chunk_count_ok() {
        // 空 chunk 时 chunk_count == 0 仍正常返回（不 panic）
        let r = IngestResult {
            document_id: "doc-empty".into(),
            document_name: "empty.pdf".into(),
            category: "regulation".into(),
            applicable_scope: "general".into(),
            chunk_count: 0,
            dimension: KB_VECTOR_DIM,
            collection: KB_COLLECTION.to_string(),
            elapsed_ms: 10,
        };
        assert_eq!(r.chunk_count, 0);
        // 序列化/反序列化往返正常
        let json = serde_json::to_string(&r).unwrap();
        let back: IngestResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.document_id, "doc-empty");
        assert_eq!(back.chunk_count, 0);
    }

    #[test]
    fn test_unknown_extension_falls_back_to_pdf() {
        // 非 PDF/Word 扩展名不应 panic：按 pdf 逻辑处理（此处仅验证扩展名解析逻辑）
        let ext = std::path::Path::new("file.unknown")
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("pdf");
        assert_eq!(ext, "unknown");
        // ingest_bytes 对未知扩展名会走 pdf 分支（不提前报错），
        // 实际解析失败由 extract_pdf_to_raw_json 返回 Err，调用方可感知。
        // 此处仅验证扩展名提取不会 panic 即可。
    }

    #[test]
    fn test_collect_all_block_ids_recursive() {
        let section = Section {
            level: 1,
            title: "第三章".into(),
            pattern: "chapter".into(),
            page_start: 1,
            page_end: 3,
            block_ids: vec!["b_1_0".into(), "b_1_1".into()],
            body_text: "正文".into(),
            children: vec![Section {
                level: 2,
                title: "第十条".into(),
                pattern: "article".into(),
                page_start: 2,
                page_end: 2,
                block_ids: vec!["b_2_0".into()],
                body_text: "条款正文".into(),
                children: Vec::new(),
                body_page_start: 2,
                body_page_end: 2,
            }],
            body_page_start: 1,
            body_page_end: 3,
        };
        let ids = collect_all_block_ids(&section);
        assert_eq!(ids, vec!["b_1_0", "b_1_1", "b_2_0"]);
    }
}
