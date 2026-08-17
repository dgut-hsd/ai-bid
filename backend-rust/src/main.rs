use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::Path;

use ai_bid::domain::chunk::{BlockBBox, Chunk, ChunkType, ChunkingConfig};
use ai_bid::domain::raw_document::RawDocument;
use ai_bid::paths::data_path_str;
use ai_bid::services::chunking_service::{chunk_sections, populate_bbox_refs};
use ai_bid::services::docx_convert_service::convert_docx_to_pdf;
use ai_bid::services::pdf_extract_service::{extract_pdf_to_raw_json, extract_with_python};
use ai_bid::services::sectionize_service::sectionize;
use serde::Serialize;

// Agent 框架
use ai_bid::agents::bus::AgentBus;
use ai_bid::agents::chat_agent::ChatAgent;
use ai_bid::agents::coordinator::Coordinator;
use ai_bid::agents::fact_check::create_fact_check_agent;
use ai_bid::agents::registry::AgentRegistry;
use ai_bid::agents::session_graph::SessionGraph;
use ai_bid::agents::tools::ToolRegistry;
use ai_bid::agents::tools::answer_user::AnswerUserTool;
use ai_bid::agents::tools::output_finding::OutputFindingTool;
use ai_bid::agents::tools::read_section::ReadSectionTool;
use ai_bid::agents::tools::search_document::SearchDocumentTool;
use ai_bid::agents::tools::search_knowledge::{
    DashScopeSearchBackend, SearchBuffer, SearchKnowledgeTool,
};
use ai_bid::agents::tools::search_knowledge_base::SearchKnowledgeBaseTool;
// V2+ 工具
use ai_bid::agents::tools::compare_versions::CompareVersionsTool;
use ai_bid::agents::tools::detect_boilerplate::DetectBoilerplateTool;
// 零依赖计算/检查工具
use ai_bid::agents::tools::calculate_timeline::CalculateTimelineTool;
// 依赖 chunk 数据的工具
use ai_bid::agents::tools::check_cross_reference::CheckCrossReferenceTool;
use ai_bid::agents::tools::extract_obligations::ExtractObligationsTool;
use ai_bid::agents::tools::compare_with_template::{CompareWithTemplateTool, ChunkTextProvider, TemplateStore};
use ai_bid::agents::tools::validate_calculation::ValidateCalculationTool;
use ai_bid::agents::tools::search_contradiction::SearchContradictionTool;
// V3 采购程序合规审查
use ai_bid::agents::tools::verify_procurement_method::VerifyProcurementMethodTool;
use ai_bid::agents::tools::verify_bid_deposit::VerifyBidDepositTool;
use ai_bid::agents::tools::verify_announcement_period::VerifyAnnouncementPeriodTool;
use ai_bid::agents::tools::verify_bid_preparation_period::VerifyBidPreparationPeriodTool;
// V4 评审标准审查
use ai_bid::agents::tools::validate_scoring_formula::ValidateScoringFormulaTool;
use ai_bid::agents::tools::validate_weight_distribution::ValidateWeightDistributionTool;
use ai_bid::agents::tools::detect_subjective_scoring::DetectSubjectiveScoringTool;
use ai_bid::agents::tools::check_scoring_completeness::CheckScoringCompletenessTool;
use ai_bid::agents::tools::check_imported_products::CheckImportedProductsTool;
use ai_bid::agents::tools::verify_consortium_rules::VerifyConsortiumRulesTool;
use ai_bid::agents::trace::TraceLog;
use ai_bid::agents::types::{ChatAgentConfig, CoordinatorConfig, CoordinatorOutput, ReviewClause};
use ai_bid::services::llm_client::create_llm_client;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Chunk 切分完整输出（对应验证.md V4.8 格式）。
#[derive(Debug, Serialize)]
struct ChunkingOutput {
    document_id: String,
    source_path: String,
    config: ChunkingConfigInfo,
    stats: ChunkingStats,
    chunks: Vec<ChunkOutputItem>,
}

#[derive(Debug, Serialize)]
struct ChunkingConfigInfo {
    merge_min_len: usize,
    split_max_len: usize,
    split_overlap: usize,
    embed_ctx_depth: usize,
    min_chunk_size: usize,
    embed_path_max_len: usize,
}

#[derive(Debug, Serialize)]
struct ChunkingStats {
    total_chunks: usize,
    #[serde(rename = "type_counts")]
    type_counts: TypeCounts,
    total_chars: usize,
    avg_chunk_size: f64,
    max_chunk_size: usize,
    min_chunk_size: usize,
}

#[derive(Debug, Serialize)]
struct TypeCounts {
    #[serde(rename = "Leaf")]
    leaf: usize,
    #[serde(rename = "Merged")]
    merged: usize,
    #[serde(rename = "Split")]
    split: usize,
}

/// 单个 chunk 的输出格式（含 embed_text）。
#[derive(Debug, Serialize)]
struct ChunkOutputItem {
    chunk_id: String,
    chunk_type: serde_json::Value,
    section_path: Vec<String>,
    text: String,
    page_start: usize,
    page_end: usize,
    source_block_ids: Vec<String>,
    bbox_refs: Vec<BlockBBox>,
    embed_text: String,
}

/// 递归收集 Section 树中所有的 block_id。
fn collect_all_block_ids(section: &ai_bid::services::sectionize_service::Section) -> Vec<&str> {
    let mut ids: Vec<&str> = section.block_ids.iter().map(|s| s.as_str()).collect();
    for child in &section.children {
        ids.extend(collect_all_block_ids(child));
    }
    ids
}

impl ChunkingOutput {
    fn new(
        document_id: String,
        source_path: String,
        config: &ChunkingConfig,
        chunks: &[Chunk],
    ) -> Self {
        let leaf_count = chunks
            .iter()
            .filter(|c| matches!(c.chunk_type, ChunkType::Leaf))
            .count();
        let merged_count = chunks
            .iter()
            .filter(|c| matches!(c.chunk_type, ChunkType::Merged { .. }))
            .count();
        let split_count = chunks
            .iter()
            .filter(|c| matches!(c.chunk_type, ChunkType::Split { .. }))
            .count();

        let sizes: Vec<usize> = chunks.iter().map(|c| c.text.chars().count()).collect();
        let total_chars: usize = sizes.iter().sum();
        let max_size = sizes.iter().copied().max().unwrap_or(0);
        let min_size = sizes.iter().copied().min().unwrap_or(0);
        let avg_size = if chunks.is_empty() {
            0.0
        } else {
            total_chars as f64 / chunks.len() as f64
        };

        let chunk_items: Vec<ChunkOutputItem> = chunks
            .iter()
            .map(|c| {
                let chunk_type_value = match &c.chunk_type {
                    ChunkType::Leaf => {
                        serde_json::json!({ "type": "Leaf" })
                    }
                    ChunkType::Merged { rule, child_count } => {
                        serde_json::json!({
                            "type": "Merged",
                            "rule": rule,
                            "child_count": child_count
                        })
                    }
                    ChunkType::Split { part, total } => {
                        serde_json::json!({
                            "type": "Split",
                            "part": part,
                            "total": total
                        })
                    }
                };
                ChunkOutputItem {
                    chunk_id: c.chunk_id.clone(),
                    chunk_type: chunk_type_value,
                    section_path: c.section_path.clone(),
                    text: c.text.clone(),
                    page_start: c.page_start,
                    page_end: c.page_end,
                    source_block_ids: c.source_block_ids.clone(),
                    bbox_refs: c.bbox_refs.clone(),
                    embed_text: c.embed_text(config.embed_ctx_depth, config.embed_path_max_len),
                }
            })
            .collect();

        ChunkingOutput {
            document_id,
            source_path,
            config: ChunkingConfigInfo {
                merge_min_len: config.merge_min_len,
                split_max_len: config.split_max_len,
                split_overlap: config.split_overlap,
                embed_ctx_depth: config.embed_ctx_depth,
                min_chunk_size: config.min_chunk_size,
                embed_path_max_len: config.embed_path_max_len,
            },
            stats: ChunkingStats {
                total_chunks: chunks.len(),
                type_counts: TypeCounts {
                    leaf: leaf_count,
                    merged: merged_count,
                    split: split_count,
                },
                total_chars,
                avg_chunk_size: (avg_size * 10.0).round() / 10.0,
                max_chunk_size: max_size,
                min_chunk_size: min_size,
            },
            chunks: chunk_items,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // 加载 .env 文件（LLM API 密钥等配置）
    // 依次尝试：当前目录 → data_dir（支持从 backend-rust/ 子目录运行时找到根级 .env）
    dotenv::dotenv().ok();
    let data_env = ai_bid::paths::data_dir().join(".env");
    if data_env.exists() {
        dotenv::from_path(data_env).ok();
    }

    // ── 指标采集器 ─────────────────────────────────────────────
    let llm_model = std::env::var("DASHSCOPE_MODEL")
        .unwrap_or_else(|_| std::env::var("LLM_MODEL").unwrap_or_else(|_| "qwen-plus".to_string()));
    let metrics = Arc::new(Mutex::new(ai_bid::metrics::MetricsCollector::new(
        ai_bid::metrics::SCHEMA_VERSION,
        &llm_model,
    )));
    let pipeline_start = std::time::Instant::now();
    let mut phase_start = pipeline_start;

    // 解析命令行参数：flag 参数（如 --chat）与位置参数（文件路径）分离
    let args: Vec<String> = env::args().collect();
    let chat_mode = args.iter().any(|a| a == "--chat");
    // 取第一个不以 -- 开头的参数作为文件路径
    let input_path = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| data_path_str("tests/file/智慧教室环境改造工程.pdf"));

    let input = Path::new(&input_path);
    anyhow::ensure!(input.exists(), "文件不存在: {}", input.display());

    // 根据扩展名确定处理路径
    let pdf_path: String;
    let stem: String;

    let ext = input
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "docx" | "doc" => {
            let dir = input
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or(".")
                .to_string();
            let converted = convert_docx_to_pdf(&input_path, &dir)?;
            pdf_path = converted.to_string_lossy().to_string();
            stem = input.file_stem().unwrap().to_string_lossy().to_string();
        }
        "pdf" => {
            pdf_path = input_path.clone();
            stem = input.file_stem().unwrap().to_string_lossy().to_string();
        }
        other => anyhow::bail!("不支持的文件格式: .{}，仅支持 .pdf / .docx / .doc", other),
    }

    println!("输入文件: {}", input_path);
    println!("PDF 路径: {}", pdf_path);

    // ─── 阶段 1: PDF → RawDocument ───────────────────────────

    let raw_json_dir = data_path_str("output/raw_json");
    fs::create_dir_all(&raw_json_dir)
        .with_context(|| format!("无法创建输出目录: {}", raw_json_dir))?;
    let raw_json_path = format!("{}/{}_raw.json", raw_json_dir, stem);

    let mut raw_doc: RawDocument = match extract_pdf_to_raw_json(&pdf_path) {
        Ok(doc) => {
            println!("Rust pdfplumber 解析成功");
            let json = serde_json::to_string_pretty(&doc)?;
            fs::write(&raw_json_path, json)?;
            println!("Raw JSON 已生成: {}", raw_json_path);
            doc
        }
        Err(e) => {
            println!("Rust pdfplumber 失败: {}", e);
            println!("切换到 Python pdfplumber 兜底提取...");
            extract_with_python(&pdf_path, &raw_json_path)?;
            println!("Raw JSON 已生成: {}", raw_json_path);
            // Python 兜底后，读回 RawDocument
            let json_str = fs::read_to_string(&raw_json_path)
                .with_context(|| "无法读取 Python 兜底输出的 JSON")?;
            serde_json::from_str(&json_str).with_context(|| "Python 兜底输出的 JSON 解析失败")?
        }
    };

    // ── 指标：阶段 1 文档摄取 ──
    {
        let stage_duration = phase_start.elapsed().as_millis() as u64;
        let mut collector = metrics.lock().await;
        collector.record_stage(
            ai_bid::metrics::SemanticStage::DocumentIngestion,
            stage_duration,
            ai_bid::metrics::StageDetail::DocumentIngestion {
                pages: raw_doc.pages.len(),
                engine: "pdfplumber".to_string(),
            },
        );
        phase_start = std::time::Instant::now();
    }

    // ─── 阶段 2: RawDocument → Sections ──────────────────────

    println!("正在进行章节结构识别 (sectionize)...");
    let sections_output = sectionize(&raw_doc);

    let sections_dir = data_path_str("output/sections");
    fs::create_dir_all(&sections_dir)
        .with_context(|| format!("无法创建输出目录: {}", sections_dir))?;

    let sections_path = format!("{}/{}_sections.json", sections_dir, stem);
    let sections_json = serde_json::to_string_pretty(&sections_output)?;
    fs::write(&sections_path, sections_json)?;

    println!("Sections JSON 已生成: {}", sections_path);
    println!(
        "  总章节数: {} (orphan blocks: {})",
        sections_output.stats.total_sections, sections_output.stats.orphan_blocks
    );
    for (level, count) in sections_output.stats.level_counts.iter() {
        println!("    Level {}: {} 个", level, count);
    }

    // ─── 阶段 2.4: 启发式表格检测（`|` 分隔符）─────────────────
    // 在 sectionize 之后、表格注入之前执行，确保后续 inject_tables_into_sections
    // 能消费这些补充表格。对于 pdfplumber 已检测到表格的标书，此步骤通常为无操作。

    println!("正在启发式检测纯文本表格...");
    let pipe_table_count = ai_bid::services::sectionize_service::detect_pipe_tables(&mut raw_doc);
    println!("  已启发式检测到 {} 张纯文本表格", pipe_table_count);

    // ─── 阶段 2.5: Orphan blocks → Section（在表格注入之前）──────
    // 将孤儿块按连续页码分组构造临时 Section，追加到 sections 列表，
    // 使其走完整的 chunking 管线（含 split_long_chunk + merge_tiny_chunks）。

    let chunking_config = ChunkingConfig::default();

    // 收集所有已分配的 block_id
    let assigned: std::collections::HashSet<&str> = sections_output
        .sections
        .iter()
        .flat_map(|s| collect_all_block_ids(s))
        .collect();

    // 找出未分配的 orphan blocks
    let orphan_blocks: Vec<&ai_bid::domain::raw_document::RawBlock> = raw_doc
        .pages
        .iter()
        .flat_map(|p| p.blocks.iter())
        .filter(|b| !assigned.contains(b.id.as_str()))
        .collect();

    let mut all_sections = sections_output.sections.clone();

    if !orphan_blocks.is_empty() {
        // 构建 block_id → page_index 索引（避免 O(P×B×O) 嵌套扫描）
        let block_page: std::collections::HashMap<&str, usize> = raw_doc
            .pages
            .iter()
            .flat_map(|p| p.blocks.iter().map(move |b| (b.id.as_str(), p.page_index)))
            .collect();

        // 按页码分组 orphan blocks
        let mut page_to_blocks: std::collections::BTreeMap<
            usize,
            Vec<&ai_bid::domain::raw_document::RawBlock>,
        > = std::collections::BTreeMap::new();
        for block in &orphan_blocks {
            if let Some(&page_idx) = block_page.get(block.id.as_str()) {
                page_to_blocks.entry(page_idx).or_default().push(*block);
            }
        }

        // 将连续页码合并为组（相邻页码 → 同一组）
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

        // 每组构造一个临时 Section
        for group in &page_groups {
            let group_start = *group.first().unwrap();
            let group_end = *group.last().unwrap();
            let group_blocks: Vec<&&ai_bid::domain::raw_document::RawBlock> = group
                .iter()
                .flat_map(|p| page_to_blocks[p].iter())
                .collect();
            let orphan_ids: Vec<String> = group_blocks.iter().map(|b| b.id.clone()).collect();
            let orphan_text = group_blocks
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");

            let orphan_section = ai_bid::services::sectionize_service::Section {
                level: 0, // 特殊层级：未归类内容
                title: format!("未归类内容 (第{}-{}页)", group_start + 1, group_end + 1),
                pattern: "orphan".to_string(),
                page_start: group_start,
                page_end: group_end,
                block_ids: orphan_ids,
                body_text: orphan_text,
                children: Vec::new(),
                body_page_start: group_start,
                body_page_end: group_end,
            };

            all_sections.push(orphan_section);
        }

        println!(
            "  已补充 {} 个 orphan block（{} 个连续页码组）兜底 Section",
            orphan_blocks.len(),
            page_groups.len()
        );
    }

    // ─── 阶段 2.6: 表格内容注入 Sections ───────────────────────
    // 注意：此处使用 all_sections（含孤儿 Section），而非 sections_output.sections，
    // 确保孤儿 Section 也能接收表格内容注入。

    println!("正在注入表格内容到章节结构...");
    // ★ 跨页表格合并：同一逻辑表格跨页时被 PDF 提取器拆散，先合并再注入
    let cross_page_merged =
        ai_bid::services::sectionize_service::merge_cross_page_tables(&mut raw_doc);
    if cross_page_merged > 0 {
        println!("  已合并 {} 组跨页表格", cross_page_merged);
    }
    ai_bid::services::sectionize_service::inject_tables_into_sections(&mut all_sections, &raw_doc);
    // 递归统计所有 section（含子节点）中 t_ 前缀的 block_id 数量
    fn count_table_ids(sections: &[ai_bid::services::sectionize_service::Section]) -> usize {
        sections
            .iter()
            .map(|s| {
                s.block_ids.iter().filter(|id| id.starts_with("t_")).count()
                    + count_table_ids(&s.children)
            })
            .sum()
    }
    let injected_table_count = count_table_ids(&all_sections);
    println!("  已注入 {} 张表格到对应章节", injected_table_count);

    // ── 指标：阶段 2 文档结构化 ──
    {
        let stage_duration = phase_start.elapsed().as_millis() as u64;
        let mut collector = metrics.lock().await;
        collector.record_stage(
            ai_bid::metrics::SemanticStage::DocumentStructure,
            stage_duration,
            ai_bid::metrics::StageDetail::DocumentStructure {
                section_count: all_sections.len(),
            },
        );
        phase_start = std::time::Instant::now();
    }

    // ─── 阶段 3: Sections → Chunks ────────────────────────────

    println!("正在进行条款级 Chunk 切分 (chunking)...");
    let mut chunks = chunk_sections(&all_sections, &chunking_config);
    populate_bbox_refs(&mut chunks, &raw_doc);

    let chunks_dir = data_path_str("output/chunks");
    fs::create_dir_all(&chunks_dir).with_context(|| format!("无法创建输出目录: {}", chunks_dir))?;

    let chunks_path = format!("{}/{}_chunks.json", chunks_dir, stem);

    let chunking_output = ChunkingOutput::new(
        sections_output.document_id.clone(),
        sections_output.source_path.clone(),
        &chunking_config,
        &chunks,
    );
    let chunks_json = serde_json::to_string_pretty(&chunking_output)?;
    fs::write(&chunks_path, chunks_json)?;

    println!("Chunks JSON 已生成: {}", chunks_path);
    let stats = &chunking_output.stats;
    println!("  总 Chunk 数: {}", stats.total_chunks);
    println!(
        "  类型分布 — Leaf: {}, Merged: {}, Split: {}",
        stats.type_counts.leaf, stats.type_counts.merged, stats.type_counts.split
    );
    println!(
        "  大小 — 总计 {} 字符, 平均 {:.1}, 最小 {}, 最大 {}",
        stats.total_chars, stats.avg_chunk_size, stats.min_chunk_size, stats.max_chunk_size
    );

    // ── 指标：阶段 3 Chunking ──
    {
        let stage_duration = phase_start.elapsed().as_millis() as u64;
        let mut collector = metrics.lock().await;
        collector.record_stage(
            ai_bid::metrics::SemanticStage::Chunking,
            stage_duration,
            ai_bid::metrics::StageDetail::Chunking {
                chunk_count: stats.total_chunks,
                total_chars: stats.total_chars,
            },
        );
        phase_start = std::time::Instant::now();
    }

    // ─── 阶段 4: Chunks → Embedding → DocumentVectorIndex ─────
    //
    // 通过 .env 中的 EMBED_ENGINE 切换嵌入引擎：
    //   EMBED_ENGINE=local  → 本地 BGE-M3 数据并行（默认，需 models/ 缓存）
    //   EMBED_ENGINE=remote → 远程 text-embedding-v4（需 DASHSCOPE_API_KEY）
    //
    // 本地模式 K 值（数据并行实例数，由 AIBID_EMBED_PARALLELISM 覆盖）：
    //   K=1 → ~1.2GB          K=2 → ~2.4GB, ~1.8×（推荐）
    //   K=3 → ~3.6GB, ~2.5×   K=4 → ~4.8GB, ~3.2×
    // 低内存机器（<16GB）建议 K=1，避免与 Ollama + Neo4j 等并发时 OOM。
    let embed_parallelism: usize = env::var("AIBID_EMBED_PARALLELISM")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);

    let embed_engine = env::var("EMBED_ENGINE").unwrap_or_else(|_| "local".to_string());
    let is_remote = embed_engine == "remote";
    println!(
        "嵌入引擎: {}",
        if is_remote {
            "远程 DashScope (text-embedding-v4)"
        } else {
            "本地 BGE-M3（数据并行）"
        }
    );

    let doc_index = if is_remote {
        let api_client = ai_bid::services::embedding_api_client::EmbeddingApiClient::from_env()?;
        ai_bid::services::embedding_service::embed_chunks_remote(
            &chunks,
            &chunking_config,
            &sections_output.document_id,
            &api_client,
        )?
    } else {
        println!(
            "正在生成 BGE-M3 Embedding（数据并行: {} 实例）...",
            embed_parallelism
        );
        ai_bid::services::embedding_service::embed_chunks_parallel(
            &chunks,
            &chunking_config,
            &sections_output.document_id,
            embed_parallelism,
        )?
    };

    let embeddings_dir = data_path_str("output/embeddings");
    ai_bid::services::embedding_service::save_index(&doc_index, &embeddings_dir, &stem)?;
    println!(
        "  索引完成: {} 条向量, 维度 {}",
        doc_index.len(),
        doc_index.embeddings.first().map(|v| v.len()).unwrap_or(0)
    );

    // ── 指标：阶段 4 Embedding ──
    let embed_dimension = doc_index.embeddings.first().map(|v| v.len()).unwrap_or(0);
    {
        let stage_duration = phase_start.elapsed().as_millis() as u64;
        let mut collector = metrics.lock().await;
        collector.set_embedding_stats(doc_index.len(), embed_dimension, &embed_engine);
        collector.record_stage(
            ai_bid::metrics::SemanticStage::Embedding,
            stage_duration,
            ai_bid::metrics::StageDetail::Embedding {
                chunk_count: doc_index.len(),
                dimension: embed_dimension,
            },
        );
    }

    // ─── 阶段 5: 语义搜索验证（V5.6）──────────────────────────

    println!();
    println!("正在运行语义搜索验证（V5.6）...");

    let queries: &[(&str, &str)] = &[
        ("Q1", "供应商需要具备哪些资格条件？"),
        ("Q2", "本项目不接受联合体投标的要求"),
        ("Q3", "投标文件的密封和递交要求"),
        ("Q4", "付款方式和结算条件是什么"),
        ("Q5", "技术服务的验收标准和流程"),
    ];

    // 创建嵌入客户端（查询 + Agent 复用，根据 EMBED_ENGINE 自动选择引擎）
    let embed_client = ai_bid::services::embedding_service::EmbeddingClient::from_env()?;

    let query_texts: Vec<&str> = queries.iter().map(|(_, t)| *t).collect();
    let query_embs = embed_client.encode_queries(&query_texts)?;

    println!("  已编码 {} 条查询", query_embs.len());
    println!();

    for (i, (label, query)) in queries.iter().enumerate() {
        // query_embs 已由 EmbeddingClient 做 L2 归一化，直接使用
        let hits = doc_index.search(&query_embs[i], 5);
        println!("━━━ {}: \"{}\" ━━━", label, query);
        for (rank, hit) in hits.iter().enumerate() {
            let marker = if rank == 0 { "★" } else { " " };
            println!(
                "  {} #{}. [{}] cos={:.4}  p.{}  \"{}\"",
                marker,
                rank + 1,
                hit.chunk_id,
                hit.score,
                hit.page_start,
                hit.title,
            );
            // 显示 snippet 前 120 字符
            let snippet: String = hit.snippet.chars().take(120).collect();
            println!("       ↳ {}", snippet);
        }
        println!();
    }

    // ─── 阶段 6: Multi-Agent 合规审查 (Phase 2 Coordinator) ───

    println!();
    println!("══════════════════════════════════════════════");
    println!("  阶段 6: Multi-Agent 合规审查 (Phase 2)");
    println!("══════════════════════════════════════════════");
    println!();

    // 检查是否启用 Agent 模式（环境变量 AIBID_AGENT=1）
    let agent_enabled = env::var("AIBID_AGENT").unwrap_or_default() == "1";
    if !agent_enabled {
        println!("  Agent 模式未启用。设置 AIBID_AGENT=1 以启用。");
        println!("  用法: $env:AIBID_AGENT=1; cargo run -- <文件路径>");
        println!();
        println!("  注意：Agent 模式需要设置 OPENAI_API_KEY 环境变量。");
        return Ok(());
    }

    // ── 阶段 6b: ChatAgent 交互模式（--chat 标志）──
    if chat_mode {
        println!();
        println!("══════════════════════════════════════════════");
        println!("  阶段 6b: ChatAgent 交互模式");
        println!("══════════════════════════════════════════════");
        println!();

        // 1. 构建 Chunk 查找表（read_section 工具复用）
        let chunk_map: HashMap<String, Chunk> = chunks
            .iter()
            .map(|c| (c.chunk_id.clone(), c.clone()))
            .collect();
        let chunk_map = Arc::new(chunk_map);
        let chunk_order: Vec<String> = chunks.iter().map(|c| c.chunk_id.clone()).collect();
        let chunk_order = Arc::new(chunk_order);

        // 2. 共享 DocumentVectorIndex（search_document 工具复用）
        let doc_index = Arc::new(doc_index);

        // 3. 共享嵌入客户端（search_document 查询编码用）
        let agent_embed = Arc::new(embed_client);

        // 4. 创建 LLM 客户端（Arc 共享）
        let llm: Arc<dyn ai_bid::agents::react_loop::LlmClient> = create_llm_client()?.into();

        // 5. 搜索后端选择（复用阶段 6 逻辑）
        let search_backend =
            env::var("AIBID_SEARCH_BACKEND").unwrap_or_else(|_| "dashscope".to_string());

        let shared_search_buffer: Option<Arc<SearchBuffer>> = if search_backend == "searxng" {
            let url =
                env::var("SEARXNG_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
            Some(SearchBuffer::new(url, None))
        } else {
            None
        };

        let shared_dashscope_search: Option<Arc<DashScopeSearchBackend>> =
            if search_backend == "dashscope" {
                let ds = DashScopeSearchBackend::from_env()
                    .expect("DashScope 搜索后端初始化失败。请设置 DASHSCOPE_API_KEY");
                Some(Arc::new(ds))
            } else {
                None
            };

        // 6. 构建 ChatAgent 工具注册表（不含 output_finding，含 answer_user）
        let mut chat_tools = ToolRegistry::new();
        if search_backend == "dashscope" {
            chat_tools.register(Box::new(SearchKnowledgeTool::with_dashscope(
                shared_dashscope_search.expect("DashScope 未初始化"),
            )));
        } else {
            chat_tools.register(Box::new(SearchKnowledgeTool::with_buffer(
                shared_search_buffer.expect("SearchBuffer 未初始化"),
            )));
        }
        chat_tools.register(Box::new(SearchDocumentTool::new(
            Arc::clone(&doc_index),
            Arc::clone(&agent_embed),
        )));
        chat_tools.register(Box::new(ReadSectionTool::new(
            Arc::clone(&chunk_map),
            Arc::clone(&chunk_order),
        )));
        chat_tools.register(Box::new(AnswerUserTool));

        // 7. 创建 ChatAgent 并进入交互循环
        let chat_config = ChatAgentConfig::default();
        let chat_agent = ChatAgent::new(
            chat_config,
            llm,
            chat_tools,
            Some(Arc::clone(&doc_index)),   // P0: 自动 RAG 注入
            Some(Arc::clone(&agent_embed)), // P0: 查询编码
            Some(Arc::clone(&chunk_map)),   // P3: 引用验证
        )?;

        println!("  已加载 {} 个 Chunk", chunk_order.len());
        println!("  搜索后端: {}", search_backend);
        println!();

        chat_agent.chat_loop().await?;
        return Ok(());
    }

    // 1. 构建 Chunk 查找表（read_section 工具）
    let chunk_map: HashMap<String, Chunk> = chunks
        .iter()
        .map(|c| (c.chunk_id.clone(), c.clone()))
        .collect();
    let chunk_map = Arc::new(chunk_map);

    // 构建有序 chunk_id 列表（用于 read_section 的相邻上下文查询）
    let chunk_order: Vec<String> = chunks.iter().map(|c| c.chunk_id.clone()).collect();
    let chunk_order = Arc::new(chunk_order);
    println!("  已加载 {} 个 Chunk 到内存索引", chunk_order.len());

    // 2. 共享 DocumentVectorIndex（search_document 工具）
    let doc_index = Arc::new(doc_index);

    // 3. 共享嵌入客户端（search_document 查询编码用，与验证阶段共用同一实例）
    let agent_embed = Arc::new(embed_client);
    println!("  嵌入客户端已共享给 Agent");

    // 4. 构建审查条款列表（不再限制数量，全量审查）
    let max_parallel: usize = env::var("AIBID_MAX_PARALLEL_CLAUSES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    println!(
        "  最大并行条款: {} (设置 AIBID_MAX_PARALLEL_CLAUSES 调整)",
        max_parallel
    );

    let review_clauses: Vec<ReviewClause> = chunks
        .iter()
        .map(|c| {
            ReviewClause::from_chunk(
                c,
                chunking_config.embed_ctx_depth,
                chunking_config.embed_path_max_len,
            )
        })
        .collect();
    println!("  待审查条款: {} 条", review_clauses.len());

    // 5. Phase 2 共享基础设施
    // AgentBus — Agent 间实时广播（capacity=32）
    let bus = Arc::new(AgentBus::new(32));

    // SessionGraph — Blackboard 核心（中期记忆）
    let graph = Arc::new(SessionGraph::new());

    // TraceLog — 审查追溯日志
    let trace = Arc::new(Mutex::new(TraceLog::new()));

    // AgentRegistry — 8 Agent 内置定义
    let registry = AgentRegistry::builtin();

    // 6. 工厂函数（避免 clone_box 传染）
    let llm_factory = {
        move || {
            create_llm_client()
                .expect("创建 LLM 客户端失败。请检查 AIBID_LLM_PROTOCOL 及相关 API 密钥环境变量")
        }
    };

    // 6. 搜索后端选择
    let search_backend =
        env::var("AIBID_SEARCH_BACKEND").unwrap_or_else(|_| "dashscope".to_string());

    // SearXNG 仅在明确配置时初始化
    let shared_search_buffer: Option<Arc<SearchBuffer>> = if search_backend == "searxng" {
        let searxng_url =
            env::var("SEARXNG_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
        println!("  SearXNG 搜索后端: {} (SearchBuffer 已启用)", searxng_url);
        Some(SearchBuffer::new(searxng_url, None))
    } else {
        None
    };

    // DashScope 搜索后端
    let shared_dashscope_search: Option<Arc<DashScopeSearchBackend>> =
        if search_backend == "dashscope" {
            let ds = DashScopeSearchBackend::from_env()
                .expect("DashScope 搜索后端初始化失败。请设置 DASHSCOPE_API_KEY");
            println!(
                "  DashScope 联网搜索后端已启用 (model={})",
                std::env::var("DASHSCOPE_SEARCH_MODEL")
                    .or_else(|_| std::env::var("DASHSCOPE_MODEL"))
                    .unwrap_or_else(|_| "qwen-plus".to_string())
            );
            Some(Arc::new(ds))
        } else {
            None
        };

    // 验证
    if search_backend != "dashscope" && search_backend != "searxng" {
        anyhow::bail!(
            "未知的 AIBID_SEARCH_BACKEND: '{}'。支持: dashscope, searxng",
            search_backend
        );
    }

    let tools_factory = {
        let doc_index = doc_index.clone();
        let agent_embed = agent_embed.clone();
        let chunk_map = chunk_map.clone();
        let chunk_order = chunk_order.clone();
        let ds_search = shared_dashscope_search.clone();
        let buffer = shared_search_buffer.clone();
        move || {
            eprintln!("[main] ── 创建 Agent 工具集 ToolRegistry ──");
            let mut registry = ToolRegistry::new();
            registry.register(Box::new(SearchDocumentTool::new(
                doc_index.clone(),
                agent_embed.clone(),
            )));
            registry.register(Box::new(ReadSectionTool::new(
                chunk_map.clone(),
                chunk_order.clone(),
            )));
            // 根据搜索后端选择工具变体
            if let Some(ref ds) = ds_search {
                registry.register(Box::new(SearchKnowledgeTool::with_dashscope(ds.clone())));
            } else if let Some(ref buf) = buffer {
                registry.register(Box::new(SearchKnowledgeTool::with_buffer(buf.clone())));
            } else {
                panic!("搜索后端未初始化");
            }
            // 本地知识库检索（与入库共享 EmbeddingClient，保证向量空间一致）
            registry.register(Box::new(SearchKnowledgeBaseTool::new(
                agent_embed.clone(),
            )));
            registry.register(Box::new(OutputFindingTool));
            // V2+ 工具
            registry.register(Box::new(CompareVersionsTool::new(
                chunk_map.clone(),
                chunk_order.clone(),
            )));
            registry.register(Box::new(DetectBoilerplateTool::new(
                chunk_map.clone(),
                chunk_order.clone(),
            )));
            // V3 采购程序合规审查
            registry.register(Box::new(VerifyProcurementMethodTool));
            registry.register(Box::new(VerifyBidDepositTool));
            registry.register(Box::new(VerifyAnnouncementPeriodTool));
            registry.register(Box::new(VerifyBidPreparationPeriodTool));
            // V4 评审标准审查
            registry.register(Box::new(ValidateScoringFormulaTool));
            registry.register(Box::new(ValidateWeightDistributionTool));
            registry.register(Box::new(DetectSubjectiveScoringTool));
            registry.register(Box::new(CheckScoringCompletenessTool));
            registry.register(Box::new(CheckImportedProductsTool));
            registry.register(Box::new(VerifyConsortiumRulesTool));
            // 零依赖计算工具
            registry.register(Box::new(CalculateTimelineTool));
            // 依赖 chunk 数据的工具
            registry.register(Box::new(CheckCrossReferenceTool::new(
                chunk_map.clone(),
                chunk_order.clone(),
            )));
            registry.register(Box::new(ExtractObligationsTool::new(
                chunk_map.clone(),
                chunk_order.clone(),
            )));
            // 模板比对（需要 ChunkTextProvider）
            let template_text_provider = Arc::new(ChunkTextProvider {
                chunks: chunk_map.clone(),
            });
            registry.register(Box::new(CompareWithTemplateTool::new(
                Arc::new(TemplateStore::with_builtin_templates()),
                template_text_provider,
            )));
            // 数值计算校验
            registry.register(Box::new(ValidateCalculationTool));
            // 矛盾检测
            registry.register(Box::new(SearchContradictionTool::new(
                chunk_map.clone(),
                chunk_order.clone(),
                None,
            )));
            eprintln!(
                "[main] ── 工具集注册完成: 共 {} 个工具 ──",
                registry.len()
            );
            registry
        }
    };

    // 7. 检查是否使用 Coordinator 模式（环境变量 AIBID_COORDINATOR=1）
    let use_coordinator = env::var("AIBID_COORDINATOR").unwrap_or_default() == "1";

    let output: CoordinatorOutput = if use_coordinator {
        // ── Coordinator 模式（Phase 2 完整管线）─────────────────

        println!("  模式: Coordinator (Multi-Agent)");
        println!("  搜索后端: {}", search_backend);

        let config = CoordinatorConfig {
            max_parallel_clauses: max_parallel,
            ..Default::default()
        };
        let coordinator = Coordinator::new(
            config,
            registry,
            Arc::new(llm_factory),
            Arc::new(tools_factory),
            bus,
            graph,
            trace,
        )
        .with_metrics(metrics.clone());

        // ── 指标：占位 AgentReview 阶段（让 Coordinator 子阶段能追加进去）──
        {
            let mut collector = metrics.lock().await;
            collector.record_stage(
                ai_bid::metrics::SemanticStage::AgentReview,
                0, // 占位，后续 update_last_stage_duration 填充
                ai_bid::metrics::StageDetail::AgentReview {
                    clause_count: review_clauses.len(),
                    coordinator_phases: Some(vec![]),
                },
            );
        }
        let review_phase_start = std::time::Instant::now();

        let output = coordinator.review(&review_clauses).await?;

        // ── 指标：回填 AgentReview 耗时 ──
        {
            let review_ms = review_phase_start.elapsed().as_millis() as u64;
            let mut collector = metrics.lock().await;
            collector.update_last_stage_duration(review_ms);
        }

        // BlindSpot: 后台异步执行（CLI 模式下 await 确保完成后再退出）
        coordinator.run_blind_spot().await;

        output
    } else {
        // ── 单 Agent 模式（向后兼容 MVP）─────────────────────────

        println!("  模式: 单 Agent (向后兼容 MVP)");
        println!("  提示: 设置 AIBID_COORDINATOR=1 启用 Multi-Agent 模式");

        let findings = ai_bid::agents::react_loop::review_clauses_parallel(
            &review_clauses,
            create_fact_check_agent,
            &llm_factory,
            &tools_factory,
            max_parallel,
            None,
            None,
            "FactCheckAgent",
        )
        .await;

        CoordinatorOutput {
            findings,
            routing_summary: ai_bid::agents::types::RoutingSummary {
                total_clauses: review_clauses.len(),
                agent_clause_counts: {
                    let mut m = HashMap::new();
                    m.insert("FactCheckAgent".to_string(), review_clauses.len());
                    m
                },
                high_risk_count: 0,
                legal_verify_count: 0,
                blind_spot_findings: 0,
            },
            graph_snapshot: None,
        }
    };

    // 8. 输出结果
    let findings = &output.findings;
    let findings_dir = data_path_str("output/findings");
    fs::create_dir_all(&findings_dir)
        .with_context(|| format!("无法创建输出目录: {}", findings_dir))?;
    let findings_path = format!("{}/{}_findings.json", findings_dir, stem);
    let findings_json = serde_json::to_string_pretty(&findings)?;
    fs::write(&findings_path, findings_json)?;

    // 输出 routing_summary
    let summary_path = format!("{}/{}_routing_summary.json", findings_dir, stem);
    let summary_json = serde_json::to_string_pretty(&output.routing_summary)?;
    fs::write(&summary_path, summary_json)?;

    // 输出 graph_snapshot（审计追溯）
    if let Some(ref snap) = output.graph_snapshot {
        let snap_path = format!("{}/{}_graph_snapshot.json", findings_dir, stem);
        let snap_json = serde_json::to_string_pretty(snap)?;
        fs::write(&snap_path, snap_json)?;
        println!("  Graph 快照已写入: {}", snap_path);
    }

    // 8.5 知识沉淀：审核结果 → 挑精华 → 查重 → 写 Neo4j（默认开启，可用 AIBID_WRITE_NEO4J=0 关闭）
    if std::env::var("AIBID_WRITE_NEO4J").unwrap_or_else(|_| "1".into()) != "0" {
        match ai_bid::knowledge::graph::Neo4jClient::connect().await {
            Ok(client) => match ai_bid::knowledge::run::run(output.findings.clone(), &client).await {
                Ok(written) => {
                    println!("  知识沉淀: 写入 Neo4j {} 条新风险/法条", written);
                }
                Err(e) => {
                    eprintln!("  知识沉淀警告: 写 Neo4j 失败（不影响审核结果）: {}", e);
                }
            },
            Err(e) => {
                eprintln!("  知识沉淀警告: 连接 Neo4j 失败（不影响审核结果）: {}", e);
            }
        }
    }

    println!();
    println!("══════════════════════════════════════════════");
    println!("  审查完成");
    println!("══════════════════════════════════════════════");
    println!("  审查条款: {} 条", output.routing_summary.total_clauses);
    println!(
        "  发现风险: {} 条",
        findings.iter().filter(|f| !f.no_risk).count()
    );
    println!(
        "  合规条款: {} 条",
        findings
            .iter()
            .filter(|f| f.no_risk && !f.truncated)
            .count()
    );
    println!(
        "  截断(需人工): {} 条",
        findings.iter().filter(|f| f.truncated).count()
    );
    println!("  🔴 High: {} 条", output.routing_summary.high_risk_count);
    println!("  结果文件: {}", findings_path);

    // 打印 Agent 分布
    println!();
    println!("  Agent 条款分配:");
    for (agent, count) in &output.routing_summary.agent_clause_counts {
        println!("    {} : {} 条", agent, count);
    }

    // 打印风险摘要
    for f in findings {
        if !f.no_risk {
            println!();
            println!(
                "  ┌─ {} [{}] confidence={:.2}",
                f.risk_id, f.severity, f.confidence
            );
            println!("  │  条款: {}", f.clause_ids.join(", "));
            println!("  │  类型: {}", f.risk_type);
            println!("  │  法条: {}", f.legal_basis.join("; "));
            println!(
                "  │  理由: {}",
                f.reason.chars().take(200).collect::<String>()
            );
            println!("  └──────────────────────────────");
        }
    }

    // ── 指标：写盘 ─────────────────────────────────────────
    {
        let _total_duration = pipeline_start.elapsed().as_millis() as u64;
        let mut collector = metrics.lock().await;
        collector.set_findings_detail(&output.findings);

        // 构建元数据
        let git_commit = std::process::Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let git_branch = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let run_id = chrono::Local::now().format("%Y%m%dT%H%M%S").to_string();
        let use_coordinator = env::var("AIBID_COORDINATOR").unwrap_or_default() == "1";
        let search_backend =
            env::var("AIBID_SEARCH_BACKEND").unwrap_or_else(|_| "dashscope".to_string());

        let meta = ai_bid::metrics::RunMeta {
            run_id: run_id.clone(),
            title: None,
            notes: None,
            experiment_group: std::env::var("AIBID_RUN_GROUP").ok(),
            timestamp: chrono::Local::now().to_rfc3339(),
            git_commit,
            git_branch,
            tags: vec!["auto".to_string()],
            description: format!("CLI run: {}", stem),
            document: ai_bid::metrics::schema::DocumentInfo {
                name: format!("{}.pdf", stem),
                pages: raw_doc.pages.len(),
                file_size_kb: 0, // not tracked in CLI mode
            },
            config: ai_bid::metrics::schema::RunConfig {
                coordinator_enabled: use_coordinator,
                agent_count: if use_coordinator { 7 } else { 1 },
                embed_engine: embed_engine.clone(),
                llm_model,
                search_backend,
                max_parallel_clauses: max_parallel,
            },
        };

        let run_metrics = collector.finalize(meta);

        // 写盘
        let runs_dir = if let Ok(f) = std::env::var("AIBID_RUN_FOLDER") {
            let d = format!("{}/{}", data_path_str("output/runs"), f);
            fs::create_dir_all(&d).ok();
            d
        } else {
            data_path_str("output/runs")
        };
        fs::create_dir_all(&runs_dir).ok();
        let run_path = format!("{}/{}.json", runs_dir, run_id);
        if let Ok(json) = serde_json::to_string_pretty(&run_metrics)
            && fs::write(&run_path, &json).is_ok()
        {
            eprintln!("\n📊 指标已写入: {}", run_path);
            eprintln!(
                "   总耗时 {:.1}s | Token {} in + {} out | 成本 ¥{:.2}",
                run_metrics.latency.total_wall_clock_secs,
                run_metrics.llm_efficiency.totals.tokens_input,
                run_metrics.llm_efficiency.totals.tokens_output,
                run_metrics.llm_efficiency.totals.cost_cny,
            );
        }
    }

    Ok(())
}
