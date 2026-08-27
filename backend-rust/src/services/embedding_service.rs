//! BGE-M3 嵌入服务
//!
//! 本模块封装 fastembed-rs，将 Chunk 列表批量编码为 1024 维稠密向量，
//! 构建 [`DocumentVectorIndex`] 供 Agent 的 `search_document` 工具语义搜索。
//!
//! ## 技术选型
//!
//! - **fastembed-rs** 而非 Ollama：纯 Rust 进程内 ONNX 推理，零外部服务，零网络延迟
//! - **BGE-M3** 而非 mxbai-embed-large：中文 C-MTEB 顶级，支持 dense+sparse+ColBERT
//! - **BGEM3Q**（Q4 量化）默认模型：CPU 优化，1.2GB 磁盘，首次自动下载
//!
//! ## 首次运行
//!
//! 首次调用 `embed_chunks()` 时，fastembed-rs 自动从 HuggingFace 下载
//! BGE-M3 量化 ONNX 模型到 `models/`（~568MB）。
//! 后续运行直接从缓存加载。
//!
//! ## 管线位置
//!
//! ```text
//! chunking_service → Chunk[]
//!   → embedding_service::embed_chunks(&chunks, &config, &mut model)
//!   → DocumentVectorIndex
//! ```
//!
//! ## 模型生命周期
//!
//! Bgem3Embedding 实例由调用侧（main.rs）创建并传入引用，全管线复用同一个实例，
//! 避免反复加载模型（每次 ~560MB 磁盘读取 + ONNX Runtime 初始化）。

use crate::domain::chunk::{Chunk, ChunkingConfig};
use crate::domain::vector_index::{ChunkMeta, DocumentVectorIndex};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

// ─── 嵌入服务 ──────────────────────────────────────────────────

/// 从 Chunk 列表批量生成 BGE-M3 嵌入向量，构建 DocumentVectorIndex。
///
/// 每个 Chunk 先调 `embed_text(ctx_depth, max_path_len)` 携带层级前缀，
/// 再由 BGE-M3 编码为 1024 维稠密向量。
///
/// # 参数
///
/// * `chunks` - 已切分的 Chunk 列表
/// * `config` - ChunkingConfig，用于 ctx_depth 和 max_path_len
/// * `document_id` - 文档 UUID（来自 SectionizeOutput）
/// * `model` - 已加载的 BGE-M3 模型实例（由调用侧创建并复用）
///
/// # 返回值
///
/// 构建好的 DocumentVectorIndex，embeddings 已 L2 归一化。
pub fn embed_chunks(
    chunks: &[Chunk],
    config: &ChunkingConfig,
    document_id: &str,
    model: &mut fastembed::Bgem3Embedding,
) -> Result<DocumentVectorIndex> {
    if chunks.is_empty() {
        return Ok(DocumentVectorIndex {
            document_id: document_id.to_string(),
            chunks: Vec::new(),
            embeddings: Vec::new(),
        });
    }

    // 1. 生成带层级上下文的嵌入文本
    let texts: Vec<String> = chunks
        .iter()
        .map(|c| c.embed_text(config.embed_ctx_depth, config.embed_path_max_len))
        .collect();

    println!(
        "  已生成 {} 条 embed_text（ctx_depth={}, max_path_len={}）",
        texts.len(),
        config.embed_ctx_depth,
        config.embed_path_max_len
    );

    // 2. BGE-M3 批量推理（分批编码 + 进度提示）
    const BATCH_SIZE: usize = 32;
    let total = texts.len();
    let total_batches = total.div_ceil(BATCH_SIZE);

    let mut all_dense: Vec<Vec<f32>> = Vec::with_capacity(total);
    let start = std::time::Instant::now();

    for (batch_idx, chunk_batch) in texts.chunks(BATCH_SIZE).enumerate() {
        let batch_n = batch_idx + 1;
        let batch_refs: Vec<&str> = chunk_batch.iter().map(|s| s.as_str()).collect();

        let batch_output = model.embed(batch_refs, None).with_context(|| {
            format!("BGE-M3 批量编码失败 (batch {}/{})", batch_n, total_batches)
        })?;

        all_dense.extend(batch_output.dense);

        let processed = total.min(batch_n * BATCH_SIZE);
        let elapsed = start.elapsed().as_secs_f64();
        let progress = processed as f64 / total as f64;
        let eta = if progress > 0.0 {
            elapsed / progress * (1.0 - progress)
        } else {
            0.0
        };
        println!(
            "  编码进度: {}/{} chunks (batch {}/{}), 已耗时 {:.1}s, 预计剩余 {:.1}s",
            processed, total, batch_n, total_batches, elapsed, eta
        );
    }

    println!(
        "  编码完成: {} 条向量, 维度 {}, 总耗时 {:.1}s",
        all_dense.len(),
        all_dense.first().map(|v| v.len()).unwrap_or(0),
        start.elapsed().as_secs_f64()
    );

    // 3. 构建 ChunkMeta 列表
    let metas: Vec<ChunkMeta> = chunks
        .iter()
        .zip(texts.iter())
        .map(|(c, embed_text)| ChunkMeta {
            chunk_id: c.chunk_id.clone(),
            section_path: c.section_path.clone(),
            embed_text: embed_text.clone(),
            text_len: c.text.chars().count(),
            page_start: c.page_start,
            page_end: c.page_end,
        })
        .collect();

    // 4. 构建 DocumentVectorIndex（内部执行 L2 归一化）
    let mut index = DocumentVectorIndex::new(metas, all_dense);
    index.document_id = document_id.to_string();

    Ok(index)
}

/// 数据并行版: 将 chunks 分为 K 组，每组由独立的 BGE-M3 实例并行编码。
///
/// K=1 等价于串行模式（单实例 + 分批进度提示）。K≥2 启动多线程，每个线程持有
/// 独立的 ONNX 模型实例，并行编码各自的分区后合并结果（保持原始顺序）。
///
/// ## 内存开销
///
/// 每个 BGE-M3 实例约占用 1.2GB 内存（Q4 量化 ONNX 模型 ~568MB + 推理缓存）。
/// K=2 约 2.4GB，K=4 约 4.8GB。建议根据可用内存选择 K 值。
///
/// ## 加速比
///
/// 非完美线性，受 CPU 核心数 / 内存带宽 / ONNX Runtime 内部线程数共同制约。
/// 参考值：K=2 约 1.8×，K=4 约 3.2×（视硬件和 chunk 数量而定）。
///
/// ## 与 [`embed_chunks`] 的区别
///
/// [`embed_chunks`] 接受外部预创建的 `&mut model` 引用，适合调用方已持有模型
/// 的场景（如 Agent 查询编码复用）。本函数自行管理模型生命周期——并行编码时
/// 创建 K 个临时实例，编码完成后释放；K=1 时创建 1 个临时实例。
///
/// # 参数
///
/// * `chunks` - 已切分的 Chunk 列表
/// * `config` - ChunkingConfig
/// * `document_id` - 文档 UUID
/// * `k` - 并行实例数。建议 2–4。实际值会 clamp 到 [1, chunks.len()]
pub fn embed_chunks_parallel(
    chunks: &[Chunk],
    config: &ChunkingConfig,
    document_id: &str,
    k: usize,
) -> Result<DocumentVectorIndex> {
    if chunks.is_empty() {
        return Ok(DocumentVectorIndex {
            document_id: document_id.to_string(),
            chunks: Vec::new(),
            embeddings: Vec::new(),
        });
    }

    // 1. 生成 embed texts
    let texts: Vec<String> = chunks
        .iter()
        .map(|c| c.embed_text(config.embed_ctx_depth, config.embed_path_max_len))
        .collect();

    println!(
        "  已生成 {} 条 embed_text（ctx_depth={}, max_path_len={}）",
        texts.len(),
        config.embed_ctx_depth,
        config.embed_path_max_len
    );

    // 2. 编码：K=1 串行（分批 + 进度），K≥2 数据并行
    let effective_k = k.min(texts.len()).max(1);
    let all_dense = if effective_k == 1 {
        encode_serial_with_progress(&texts)?
    } else {
        encode_data_parallel(&texts, effective_k)?
    };

    // 3. 构建 ChunkMeta 列表
    let metas: Vec<ChunkMeta> = chunks
        .iter()
        .zip(texts.iter())
        .map(|(c, embed_text)| ChunkMeta {
            chunk_id: c.chunk_id.clone(),
            section_path: c.section_path.clone(),
            embed_text: embed_text.clone(),
            text_len: c.text.chars().count(),
            page_start: c.page_start,
            page_end: c.page_end,
        })
        .collect();

    // 4. 构建 DocumentVectorIndex（内部执行 L2 归一化）
    let mut index = DocumentVectorIndex::new(metas, all_dense);
    index.document_id = document_id.to_string();
    Ok(index)
}

// ─── 内部编码辅助 ──────────────────────────────────────────────

/// K=1 串行路径：创建单个模型实例，分批编码 + 进度提示。
fn encode_serial_with_progress(texts: &[String]) -> Result<Vec<Vec<f32>>> {
    let options = fastembed::Bgem3InitOptions::default()
        .with_cache_dir(std::path::PathBuf::from("models"))
        .with_show_download_progress(false);
    let mut model = fastembed::Bgem3Embedding::try_new(options).context("无法加载 BGE-M3 模型")?;

    const BATCH_SIZE: usize = 32;
    let total = texts.len();
    let total_batches = total.div_ceil(BATCH_SIZE);
    let mut all_dense: Vec<Vec<f32>> = Vec::with_capacity(total);
    let start = std::time::Instant::now();

    for (batch_idx, chunk_batch) in texts.chunks(BATCH_SIZE).enumerate() {
        let batch_n = batch_idx + 1;
        let batch_refs: Vec<&str> = chunk_batch.iter().map(|s| s.as_str()).collect();

        let batch_output = model
            .embed(batch_refs, None)
            .with_context(|| format!("BGE-M3 编码失败 (batch {}/{})", batch_n, total_batches))?;

        all_dense.extend(batch_output.dense);

        let processed = total.min(batch_n * BATCH_SIZE);
        let elapsed = start.elapsed().as_secs_f64();
        let progress = processed as f64 / total as f64;
        let eta = if progress > 0.0 {
            elapsed / progress * (1.0 - progress)
        } else {
            0.0
        };
        println!(
            "  编码进度: {}/{} chunks (batch {}/{}), 已耗时 {:.1}s, 预计剩余 {:.1}s",
            processed, total, batch_n, total_batches, elapsed, eta
        );
    }

    println!(
        "  编码完成: {} 条向量, 维度 {}, 总耗时 {:.1}s",
        all_dense.len(),
        all_dense.first().map(|v| v.len()).unwrap_or(0),
        start.elapsed().as_secs_f64()
    );
    Ok(all_dense)
}

/// K≥2 数据并行路径：将 texts 分为 K 个分区，每个分区在独立线程中由独立
/// BGE-M3 实例编码，最后按原始索引合并。
///
/// 每个线程持有独立的 ONNX 模型实例，避免跨线程共享 ONNX Runtime 会话。
/// 线程数 = 分区数（通常 = K，最后一个分区可能合并）。
fn encode_data_parallel(texts: &[String], k: usize) -> Result<Vec<Vec<f32>>> {
    let total = texts.len();
    let chunk_size = total.div_ceil(k);

    // 将 texts 分为 K 个分区，每个分区记录 (起始索引, 文本)
    let partitions: Vec<(usize, Vec<String>)> = texts
        .chunks(chunk_size)
        .enumerate()
        .map(|(i, chunk)| (i * chunk_size, chunk.to_vec()))
        .collect();

    let actual_k = partitions.len();
    let start = std::time::Instant::now();
    println!(
        "  并行编码: {} 实例（每实例独立加载 ONNX 模型 ~1.2GB，合计 ~{:.1}GB）",
        actual_k,
        actual_k as f64 * 1.2
    );

    // ── Step A: 主线程串行预加载 K 个模型 ──
    // ONNX Runtime 环境初始化不是线程安全的，多线程同时创建会死锁/阻塞。
    // 必须串行创建完成后再分发到各线程。
    println!(
        "  预加载 {} 个模型实例（串行初始化，避免 ONNX Runtime 并发死锁）...",
        actual_k
    );
    let t_load = std::time::Instant::now();
    let mut models: Vec<fastembed::Bgem3Embedding> = Vec::with_capacity(actual_k);
    for i in 0..actual_k {
        let t_one = std::time::Instant::now();
        let options = fastembed::Bgem3InitOptions::default()
            .with_cache_dir(std::path::PathBuf::from("models"))
            .with_show_download_progress(false);
        let model = fastembed::Bgem3Embedding::try_new(options)
            .with_context(|| format!("无法加载 BGE-M3 模型 (实例 {}/{})", i + 1, actual_k))?;
        models.push(model);
        println!(
            "    [实例 {}/{}] 加载完成, 耗时 {:.1}s",
            i + 1,
            actual_k,
            t_one.elapsed().as_secs_f64()
        );
    }
    println!(
        "  全部模型加载完成, 总耗时 {:.1}s",
        t_load.elapsed().as_secs_f64()
    );

    // ── Step B: 分发模型到各线程，并行编码（每线程内分批处理）──
    println!("  开始并行编码...");
    std::thread::scope(|s| -> Result<Vec<Vec<f32>>> {
        let handles: Vec<_> = partitions
            .into_iter()
            .zip(models)
            .enumerate()
            .map(|(thread_idx, ((start_idx, text_batch), mut model))| {
                s.spawn(move || -> Result<(usize, Vec<Vec<f32>>)> {
                    let thread_n = thread_idx + 1;
                    let thread_total = text_batch.len();
                    let t_thread = std::time::Instant::now();
                    println!(
                        "    [线程 {}/{}] 开始编码 {} chunks（分批处理）...",
                        thread_n, actual_k, thread_total
                    );

                    const SUB_BATCH: usize = 32;
                    let mut dense = Vec::with_capacity(thread_total);

                    for (sub_idx, sub_chunk) in text_batch.chunks(SUB_BATCH).enumerate() {
                        let sub_refs: Vec<&str> = sub_chunk.iter().map(|s| s.as_str()).collect();
                        let sub_output = model.embed(sub_refs, None).with_context(|| {
                            format!(
                                "BGE-M3 编码失败 (线程 {}, sub-batch {})",
                                thread_n,
                                sub_idx + 1
                            )
                        })?;
                        dense.extend(sub_output.dense);

                        let done = thread_total.min((sub_idx + 1) * SUB_BATCH);
                        let elapsed = t_thread.elapsed().as_secs_f64();
                        let progress = done as f64 / thread_total as f64;
                        let eta = if progress > 0.0 {
                            elapsed / progress * (1.0 - progress)
                        } else {
                            0.0
                        };
                        println!(
                            "    [线程 {}/{}] {}/{} chunks, 已耗时 {:.1}s, 预计剩余 {:.1}s",
                            thread_n, actual_k, done, thread_total, elapsed, eta
                        );
                    }

                    println!(
                        "    [线程 {}/{}] 完成, 总耗时 {:.1}s",
                        thread_n,
                        actual_k,
                        t_thread.elapsed().as_secs_f64()
                    );
                    Ok((start_idx, dense))
                })
            })
            .collect();

        // 收集各线程结果，按起始索引排序恢复原始 chunk 顺序
        let mut results: Vec<(usize, Vec<Vec<f32>>)> = Vec::with_capacity(handles.len());
        for h in handles {
            match h.join().unwrap() {
                Ok(r) => results.push(r),
                Err(e) => return Err(e),
            }
        }
        results.sort_by_key(|(idx, _)| *idx);

        let all_dense: Vec<Vec<f32>> = results.into_iter().flat_map(|(_, dense)| dense).collect();

        anyhow::ensure!(
            all_dense.len() == total,
            "并行编码结果数量不匹配: 期望 {} 条, 实际 {} 条",
            total,
            all_dense.len()
        );

        println!(
            "  并行编码完成: {} 条向量, {} 实例, 总耗时 {:.1}s",
            all_dense.len(),
            actual_k,
            start.elapsed().as_secs_f64()
        );
        Ok(all_dense)
    })
}

// ─── 嵌入客户端（统一本地/远程查询接口）─────────────────────

/// 统一嵌入客户端：封装本地 BGE-M3 和远程 API 的差异。
///
/// 用于**查询编码**（Agent `search_document` 工具 + 阶段 5 验证查询）。
/// Chunk 批量编码走独立函数 [`embed_chunks_parallel`] / [`embed_chunks_remote`]。
///
/// 创建方式：`EmbeddingClient::from_env()` 读取 `EMBED_ENGINE` 自动选择引擎。
pub enum EmbeddingClient {
    /// 本地 BGE-M3 ONNX 模型（fastembed-rs, Mutex 保护 &mut 访问）
    Local {
        model: Box<std::sync::Mutex<fastembed::Bgem3Embedding>>,
    },
    /// 远程 text-embedding-v4 API（DashScope）
    Remote {
        client: super::embedding_api_client::EmbeddingApiClient,
    },
}

impl EmbeddingClient {
    /// 从环境变量创建客户端。
    ///
    /// `EMBED_ENGINE=local`（默认）→ 加载本地 BGE-M3 模型
    /// `EMBED_ENGINE=remote` → 初始化 DashScope API 客户端（需 `DASHSCOPE_API_KEY`）
    pub fn from_env() -> Result<Self> {
        let engine = std::env::var("EMBED_ENGINE").unwrap_or_else(|_| "local".to_string());
        match engine.as_str() {
            "remote" => {
                let client = super::embedding_api_client::EmbeddingApiClient::from_env()?;
                println!("  嵌入引擎: 远程 DashScope (text-embedding-v4)");
                Ok(Self::Remote { client })
            }
            _ => {
                let options = fastembed::Bgem3InitOptions::default()
                    .with_cache_dir(std::path::PathBuf::from("models"))
                    .with_show_download_progress(false);
                let model =
                    fastembed::Bgem3Embedding::try_new(options).context("无法加载 BGE-M3 模型")?;
                println!("  嵌入引擎: 本地 BGE-M3");
                Ok(Self::Local {
                    model: Box::new(std::sync::Mutex::new(model)),
                })
            }
        }
    }

    /// 批量编码查询文本，返回 **L2 归一化后** 的向量。
    ///
    /// 用于阶段 5 验证查询（5 条 query 一次编码）。
    pub fn encode_queries(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        match self {
            Self::Local { model } => {
                let mut model = model.lock().unwrap();
                let output = model.embed(texts, None).context("BGE-M3 查询编码失败")?;
                Ok(output
                    .dense
                    .into_iter()
                    .map(l2_normalize_in_place)
                    .collect())
            }
            Self::Remote { client } => {
                let owned: Vec<String> = texts.iter().map(|s| s.to_string()).collect();
                // 查询文本很短、不含敏感信息，不脱敏
                let embs = client.encode_batch(&owned)?;
                Ok(embs.into_iter().map(l2_normalize_in_place).collect())
            }
        }
    }

    /// H3：本地引擎复用启动时已加载的 BGE-M3 实例批量编码 chunks，
    /// 避免每次上传都重新串行加载 ~1.2GB×2 的模型实例。
    pub fn embed_chunks_local_reuse(
        &self,
        chunks: &[Chunk],
        config: &ChunkingConfig,
        document_id: &str,
    ) -> Result<DocumentVectorIndex> {
        match self {
            Self::Local { model } => {
                let mut guard = model
                    .lock()
                    .map_err(|_| anyhow::anyhow!("嵌入模型锁被毒化（poisoned）"))?;
                embed_chunks(chunks, config, document_id, &mut *guard)
            }
            Self::Remote { .. } => {
                anyhow::bail!("embed_engine 与嵌入客户端不一致：期望 local，实际 remote")
            }
        }
    }
}

/// L2 归一化（原地修改，返回原 Vec 避免分配）。
fn l2_normalize_in_place(mut v: Vec<f32>) -> Vec<f32> {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

// ─── 远程 API Chunk 嵌入 ────────────────────────────────────

/// 远程 API 版 Chunk 嵌入：生成 embed_text → 正则脱敏 → 调用 text-embedding-v4。
///
/// 与本地版 [`embed_chunks_parallel`] 返回相同格式的 [`DocumentVectorIndex`]。
/// 脱敏在发送 API 前执行（正则替换 PII），不影响本地原始数据。
///
/// # 参数
///
/// * `chunks` - 已切分的 Chunk 列表
/// * `config` - ChunkingConfig
/// * `document_id` - 文档 UUID
/// * `api_client` - 已初始化的 DashScope 客户端
pub fn embed_chunks_remote(
    chunks: &[Chunk],
    config: &ChunkingConfig,
    document_id: &str,
    api_client: &super::embedding_api_client::EmbeddingApiClient,
) -> Result<DocumentVectorIndex> {
    if chunks.is_empty() {
        return Ok(DocumentVectorIndex {
            document_id: document_id.to_string(),
            chunks: Vec::new(),
            embeddings: Vec::new(),
        });
    }

    // 1. 生成 embed texts
    let texts: Vec<String> = chunks
        .iter()
        .map(|c| c.embed_text(config.embed_ctx_depth, config.embed_path_max_len))
        .collect();

    println!(
        "  已生成 {} 条 embed_text（ctx_depth={}, max_path_len={}）",
        texts.len(),
        config.embed_ctx_depth,
        config.embed_path_max_len
    );

    // 2. 脱敏（发送 API 前必须；正则替换，不影响本地文件）
    println!("  正在脱敏（正则替换手机/金额/日期/邮箱/身份证）...");
    let desensitized: Vec<String> = texts
        .iter()
        .map(|t| super::desensitize_service::desensitize(t))
        .collect();
    println!("  脱敏完成: {} 条文本", desensitized.len());

    // 3. 远程 API 编码（分批 + 进度）
    println!("  正在调用远程 Embedding API (text-embedding-v4)...");
    let all_dense = api_client.encode_batch(&desensitized)?;

    // 4. 构建 ChunkMeta（使用脱敏后的 texts，保持 embed_text 与向量一致）
    let metas: Vec<ChunkMeta> = chunks
        .iter()
        .zip(desensitized.iter())
        .map(|(c, embed_text)| ChunkMeta {
            chunk_id: c.chunk_id.clone(),
            section_path: c.section_path.clone(),
            embed_text: embed_text.clone(),
            text_len: c.text.chars().count(),
            page_start: c.page_start,
            page_end: c.page_end,
        })
        .collect();

    // 5. 构建 DocumentVectorIndex（内部执行 L2 归一化）
    let mut index = DocumentVectorIndex::new(metas, all_dense);
    index.document_id = document_id.to_string();
    Ok(index)
}

// ─── 序列化 / 反序列化 ────────────────────────────────────────

/// 将 DocumentVectorIndex 保存到磁盘。
///
/// 输出目录结构：
/// ```text
/// output/embeddings/{stem}_embedding_index/
///     chunk_meta.json          — document_id + ChunkMeta[]
///     vectors_1024d_f32le.bin  — N×1024 f32 原始二进制 (little-endian)
/// ```
pub fn save_index(index: &DocumentVectorIndex, dir: &str, stem: &str) -> Result<()> {
    let index_dir = format!("{}/{}_embedding_index", dir, stem);
    fs::create_dir_all(&index_dir).with_context(|| format!("无法创建输出目录: {}", index_dir))?;

    // chunk_meta.json
    let meta = index.to_meta();
    let meta_path = format!("{}/chunk_meta.json", index_dir);
    let meta_json = serde_json::to_string_pretty(&meta).context("序列化 IndexMeta 失败")?;
    fs::write(&meta_path, meta_json).with_context(|| format!("无法写入: {}", meta_path))?;

    // vectors_1024d_f32le.bin（原始 f32 二进制，无序列化开销）
    let bin_path = format!("{}/vectors_1024d_f32le.bin", index_dir);
    let mut buf = Vec::with_capacity(
        index.embeddings.len() * index.embeddings.first().map(|v| v.len()).unwrap_or(0) * 4,
    );
    for emb in &index.embeddings {
        for &val in emb {
            buf.extend_from_slice(&val.to_le_bytes());
        }
    }
    fs::write(&bin_path, buf).with_context(|| format!("无法写入: {}", bin_path))?;

    println!(
        "  DocumentVectorIndex 已保存: {}/ ({} 条向量, {} 维)",
        index_dir,
        index.embeddings.len(),
        index.embeddings.first().map(|v| v.len()).unwrap_or(0)
    );
    println!("    chunk_meta.json          — 元数据");
    println!(
        "    vectors_1024d_f32le.bin  — {:.1} KB",
        index.embeddings.len() * index.embeddings.first().map(|v| v.len()).unwrap_or(0) * 4 / 1024
    );

    Ok(())
}

/// 从磁盘加载 DocumentVectorIndex。
///
/// 期望目录结构：
/// ```text
/// output/embeddings/{stem}_embedding_index/
///     chunk_meta.json
///     vectors_1024d_f32le.bin
/// ```
///
/// 加载的向量**不重新执行 L2 归一化**（假定保存时已归一化）。
pub fn load_index(dir: &str, stem: &str) -> Result<DocumentVectorIndex> {
    let index_dir = format!("{}/{}_embedding_index", dir, stem);
    let index_path = Path::new(&index_dir);

    // chunk_meta.json
    let meta_path = index_path.join("chunk_meta.json");
    let meta_json = fs::read_to_string(&meta_path)
        .with_context(|| format!("无法读取: {}", meta_path.display()))?;
    let meta: crate::domain::vector_index::IndexMeta =
        serde_json::from_str(&meta_json).context("解析 IndexMeta 失败")?;

    // vectors_1024d_f32le.bin
    let bin_path = index_path.join("vectors_1024d_f32le.bin");
    let bin = fs::read(&bin_path).with_context(|| format!("无法读取: {}", bin_path.display()))?;

    let dim = meta.dimension;
    let count = meta.chunk_count;
    anyhow::ensure!(
        bin.len() == count * dim * 4,
        "vectors_1024d_f32le.bin 大小不匹配: 期望 {} bytes ({} × {} × 4), 实际 {} bytes",
        count * dim * 4,
        count,
        dim,
        bin.len()
    );

    let embeddings: Vec<Vec<f32>> = (0..count)
        .map(|i| {
            let start = i * dim * 4;
            let end = start + dim * 4;
            let slice = &bin[start..end];
            (0..dim)
                .map(|j| {
                    let off = j * 4;
                    f32::from_le_bytes([slice[off], slice[off + 1], slice[off + 2], slice[off + 3]])
                })
                .collect()
        })
        .collect();

    Ok(DocumentVectorIndex {
        document_id: meta.document_id,
        chunks: meta.chunks,
        embeddings,
    })
}

// ─── 测试 ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── l2_normalize_in_place ──────────────────────────────────────

    #[test]
    fn test_l2_normalize_unit_vector() {
        // 已经是单位向量 → 不变
        let v = vec![1.0, 0.0, 0.0];
        let result = l2_normalize_in_place(v);
        for (i, &val) in result.iter().enumerate() {
            let expected = if i == 0 { 1.0 } else { 0.0 };
            assert!((val - expected).abs() < 1e-6);
        }
    }

    #[test]
    fn test_l2_normalize_scale_to_unit() {
        // 向量 [3.0, 4.0] 的 L2 norm = 5 → [0.6, 0.8]
        let v = vec![3.0, 4.0];
        let result = l2_normalize_in_place(v);
        assert!((result[0] - 0.6).abs() < 1e-6);
        assert!((result[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn test_l2_normalize_preserves_direction() {
        let v = vec![2.0, 2.0, 2.0];
        let result = l2_normalize_in_place(v);
        // 归一化后的各分量应相等
        assert!((result[0] - result[1]).abs() < 1e-6);
        assert!((result[1] - result[2]).abs() < 1e-6);
        // L2 norm 应为 1
        let norm: f32 = result.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_l2_normalize_zero_vector() {
        // 零向量 → 保持为零向量（不除零）
        let v = vec![0.0, 0.0, 0.0];
        let result = l2_normalize_in_place(v);
        assert_eq!(result, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_l2_normalize_empty_vector() {
        let v: Vec<f32> = vec![];
        let result = l2_normalize_in_place(v);
        assert!(result.is_empty());
    }

    #[test]
    fn test_l2_normalize_high_dimensional() {
        // 1024 维随机向量 → L2 norm = 1
        let v: Vec<f32> = (0..1024).map(|i| (i as f32).sin()).collect();
        let result = l2_normalize_in_place(v);
        let norm: f32 = result.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_l2_normalize_negative_values() {
        let v = vec![-1.0, -2.0, 3.0];
        let result = l2_normalize_in_place(v);
        let norm: f32 = result.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
        // 符号应保留（方向不变）
        assert!(result[0] < 0.0);
        assert!(result[1] < 0.0);
        assert!(result[2] > 0.0);
    }
}
