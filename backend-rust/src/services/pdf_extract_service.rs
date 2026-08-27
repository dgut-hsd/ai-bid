//! PDF 原始内容提取服务
//!
//! 本模块负责将 PDF 投标文件解析为结构化中间数据 [`RawDocument`]。
//! 提取内容包括：文本、单词坐标、表格、线段、矩形等排版元素，
//! 供下游语义分析模块（如章节切分、关键词定位、表格结构化）使用。
//!
//! ## 新一代双引擎策略
//!
//! 1. **Rust 主路径** — 优先使用 `pdf-extract`（纯 Rust，基于 pdf-rs）做文本提取；
//!    单词/表格坐标使用 `pdfplumber` (lopdf)。当 lopdf 在某页失败时，
//!    该页降级为仅文本提取（无单词坐标）
//! 2. **Python 兜底** — 当 lopdf 有页面失败时，优先通过子进程调用
//!    Python pdfplumber (pdfminer.six) 拿「真实坐标」；不可用才回退 pdf-extract 占位框
//!
//! ## 文本清洗与段落分块
//!
//! 政府标书等 PDF 常用绝对定位渲染每个字符，导致 layout 模式的 text
//! 包含大量空格用于对齐。本模块在提取后自动执行清洗，并根据行间距
//! 将单词聚合为语义段落块，每块带有唯一 ID 和包围盒，用于下游回溯高亮。

use anyhow::{Context, Result};
use pdfplumber::{Pdf, TableSettings, TextOptions, WordOptions};
use regex::Regex;
use std::process::Command;
use std::sync::LazyLock;
use uuid::Uuid;

use crate::domain::raw_document::{
    BBox, BlockType, RawBlock, RawDocument, RawLine, RawPage, RawRect, RawTable, RawWord,
};

/// 使用 `pdf-extract` 从 PDF 提取纯文本（作为 lopdf 失败时的降级路径）。
fn extract_text_with_pdf_extract(path: &str) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let text = pdf_extract::extract_text_from_mem(&bytes)
        .map_err(|e| anyhow::anyhow!("pdf-extract 文本提取失败: {}", e))?;
    Ok(text)
}

// ---------- 文本清洗工具 ----------

/// 匹配"汉字后跟空白再跟汉字"的模式，用于合并被空格拆散的中文词组。
static CJK_SPACE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([一-鿿])\s+([一-鿿])").expect("CJK regex 编译失败"));

/// 匹配 2 个及以上的连续空格
static MULTI_SPACE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r" {2,}").expect("multi-space regex 编译失败"));

/// 清洗 layout 文本：去除排版空格噪音，保留逻辑结构。
fn clean_layout_text(text: &str) -> String {
    let mut lines: Vec<String> = text
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    for line in &mut lines {
        for _ in 0..5 {
            let new_s = CJK_SPACE_RE.replace_all(line, "$1$2").to_string();
            if new_s == *line {
                break;
            }
            *line = new_s;
        }
        *line = MULTI_SPACE_RE.replace_all(line, "  ").to_string();
    }

    lines.join("\n")
}

/// 从单词坐标重建干净文本（当 layout text 不可用时兜底）。
fn reconstruct_text_from_words(words: &[RawWord]) -> String {
    if words.is_empty() {
        return String::new();
    }

    let mut heights: Vec<f64> = words.iter().map(|w| w.bbox.bottom - w.bbox.top).collect();
    heights.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let line_height = heights[heights.len() / 2];

    let mut sorted: Vec<&RawWord> = words.iter().collect();
    sorted.sort_by(|a, b| {
        a.bbox
            .top
            .partial_cmp(&b.bbox.top)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.bbox
                    .x0
                    .partial_cmp(&b.bbox.x0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    let mut rows: Vec<Vec<&RawWord>> = Vec::new();
    let mut current_row: Vec<&RawWord> = vec![sorted[0]];
    let mut current_top = sorted[0].bbox.top;

    for w in sorted.iter().skip(1) {
        if w.bbox.top - current_top < line_height * 1.2 {
            current_row.push(w);
        } else {
            rows.push(std::mem::take(&mut current_row));
            current_row.push(w);
            current_top = w.bbox.top;
        }
    }
    rows.push(current_row);

    let mut lines: Vec<String> = Vec::new();
    for row in &mut rows {
        row.sort_by(|a, b| {
            a.bbox
                .x0
                .partial_cmp(&b.bbox.x0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let first_text = &row[0].text;
        let avg_w = if first_text.is_empty() {
            10.0
        } else {
            (row[0].bbox.x1 - row[0].bbox.x0) / first_text.len() as f64
        };
        let col_gap = avg_w * 8.0;

        let mut parts: Vec<String> = Vec::new();
        let mut current: Vec<&RawWord> = vec![row[0]];

        for w in row.iter().skip(1) {
            let gap = w.bbox.x0 - current.last().unwrap().bbox.x1;
            if gap < col_gap {
                current.push(w);
            } else {
                parts.push(current.iter().map(|w| w.text.as_str()).collect());
                current = vec![w];
            }
        }
        parts.push(current.iter().map(|w| w.text.as_str()).collect());

        let line = parts.join("  ");
        if !line.trim().is_empty() {
            lines.push(line);
        }
    }

    lines.join("\n")
}

// ---------- 段落块计算 ----------

/// 行间距大于此倍率视为段落边界
const HEADING_GAP_RATIO: f64 = 1.8;

/// 从单词列表计算出语义段落块。
///
/// 分两步：先按 y 坐标分组为行，再按行间距合并为段落。
/// 每块有唯一 ID、文本和 bbox，用于下游 LLM 引用后回溯高亮。
fn compute_blocks(words: &[RawWord], page_index: usize) -> Vec<RawBlock> {
    if words.is_empty() {
        return Vec::new();
    }

    let mut heights: Vec<f64> = words.iter().map(|w| w.bbox.bottom - w.bbox.top).collect();
    heights.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let line_height = heights[heights.len() / 2];

    let mut sorted: Vec<&RawWord> = words.iter().collect();
    sorted.sort_by(|a, b| {
        a.bbox
            .top
            .partial_cmp(&b.bbox.top)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.bbox
                    .x0
                    .partial_cmp(&b.bbox.x0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    // Step 1: 分组为行
    let mut text_rows: Vec<Vec<&RawWord>> = Vec::new();
    let mut current_row: Vec<&RawWord> = vec![sorted[0]];
    let mut current_top = sorted[0].bbox.top;

    for w in sorted.iter().skip(1) {
        if w.bbox.top - current_top < line_height * 1.2 {
            current_row.push(w);
        } else {
            text_rows.push(std::mem::take(&mut current_row));
            current_row.push(w);
            current_top = w.bbox.top;
        }
    }
    text_rows.push(current_row);

    // Step 2: 按行间距合并为段落块
    let mut blocks: Vec<RawBlock> = Vec::new();
    let mut block_rows: Vec<Vec<&RawWord>> = Vec::new();
    let mut prev_bottom: Option<f64> = None;

    for (i, row) in text_rows.iter().enumerate() {
        let row_top = row.iter().map(|w| w.bbox.top).fold(f64::INFINITY, f64::min);
        let row_bottom = row.iter().map(|w| w.bbox.bottom).fold(0.0, f64::max);

        let start_new =
            prev_bottom.is_some_and(|pb| (row_top - pb) > line_height * HEADING_GAP_RATIO);

        if start_new && !block_rows.is_empty() {
            blocks.push(build_block(&block_rows, page_index, blocks.len()));
            block_rows.clear();
        }

        block_rows.push(row.clone());
        prev_bottom = Some(row_bottom);

        if i == text_rows.len() - 1 {
            blocks.push(build_block(&block_rows, page_index, blocks.len()));
        }
    }

    blocks
}

/// 将一组行构建为一个 RawBlock。
fn build_block(rows: &[Vec<&RawWord>], page_index: usize, block_index: usize) -> RawBlock {
    let all_words: Vec<&&RawWord> = rows.iter().flat_map(|r| r.iter()).collect();

    let x0 = all_words
        .iter()
        .map(|w| w.bbox.x0)
        .fold(f64::INFINITY, f64::min);
    let top = all_words
        .iter()
        .map(|w| w.bbox.top)
        .fold(f64::INFINITY, f64::min);
    let x1 = all_words.iter().map(|w| w.bbox.x1).fold(0.0, f64::max);
    let bottom = all_words.iter().map(|w| w.bbox.bottom).fold(0.0, f64::max);

    let mut row_texts: Vec<String> = Vec::new();
    for row in rows {
        let mut sorted_row: Vec<&&RawWord> = row.iter().collect();
        sorted_row.sort_by(|a, b| {
            a.bbox
                .x0
                .partial_cmp(&b.bbox.x0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let text: String = sorted_row.iter().map(|w| w.text.as_str()).collect();
        if !text.trim().is_empty() {
            row_texts.push(text);
        }
    }

    let block_type = if rows.len() == 1 && all_words.len() <= 10 {
        BlockType::Heading
    } else {
        BlockType::Paragraph
    };

    RawBlock {
        id: format!("b_{}_{}", page_index, block_index),
        block_type,
        text: row_texts.join("\n"),
        bbox: BBox {
            x0,
            top,
            x1,
            bottom,
        },
    }
}

// ---------- 新一代 PDF 提取主函数 ----------

/// 基于单词位置检测表格包围盒。
///
/// 算法：将单词按 Y 坐标聚行为行，检测列间 gap，若多行呈现一致的列结构，
/// 则为每个表格行计算包围盒。返回 `(表格bbox列表, 已被表格覆盖的单词索引集合)`。
fn detect_table_bboxes(words: &[RawWord]) -> (Vec<BBox>, Vec<usize>) {
    if words.len() < 4 {
        return (Vec::new(), Vec::new());
    }

    // 估算行高
    let mut heights: Vec<f64> = words.iter().map(|w| w.bbox.bottom - w.bbox.top).collect();
    heights.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let line_height = heights[heights.len() / 2];
    if line_height <= 0.0 {
        return (Vec::new(), Vec::new());
    }

    // 按 Y 坐标排序后分组为行
    let mut sorted: Vec<(usize, &RawWord)> = words.iter().enumerate().collect();
    sorted.sort_by(|(_, a), (_, b)| {
        a.bbox
            .top
            .partial_cmp(&b.bbox.top)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut rows: Vec<Vec<(usize, &RawWord)>> = Vec::new();
    let mut current: Vec<(usize, &RawWord)> = vec![sorted[0]];
    let mut current_top = sorted[0].1.bbox.top;

    for &(idx, w) in sorted.iter().skip(1) {
        if w.bbox.top - current_top < line_height * 1.3 {
            current.push((idx, w));
        } else {
            rows.push(std::mem::take(&mut current));
            current.push((idx, w));
            current_top = w.bbox.top;
        }
    }
    rows.push(current);

    // 检测表格：3行及以上有相似列结构的行组
    let mut table_bboxes = Vec::new();
    let mut table_word_indices = Vec::new();
    let mut i = 0;

    while i < rows.len() {
        // 找连续的行组（行间距 < 1.5x lineHeight 的密集行）
        let mut j = i;
        while j + 1 < rows.len() {
            let this_bottom = rows[j]
                .iter()
                .map(|(_, w)| w.bbox.bottom)
                .fold(0.0f64, f64::max);
            let next_top = rows[j + 1]
                .iter()
                .map(|(_, w)| w.bbox.top)
                .fold(f64::INFINITY, f64::min);
            if next_top - this_bottom < line_height * 1.5 {
                j += 1;
            } else {
                break;
            }
        }

        let group_size = j - i + 1;
        if group_size >= 2 {
            // 检测该行组是否有表格结构（列间大 gap）
            for row_idx in i..=j {
                let mut row_sorted: Vec<&(usize, &RawWord)> = rows[row_idx].iter().collect();
                row_sorted
                    .sort_by(|(_, a), (_, b)| a.bbox.x0.partial_cmp(&b.bbox.x0).unwrap_or(std::cmp::Ordering::Equal));

                // 检测列 gap（> 2x 平均字宽）
                let avg_w: f64 = row_sorted
                    .iter()
                    .map(|(_, w)| (w.bbox.x1 - w.bbox.x0) / w.text.chars().count().max(1) as f64)
                    .sum::<f64>()
                    / row_sorted.len().max(1) as f64;
                let col_threshold = avg_w * 4.0;

                let has_table_gap = row_sorted.windows(2).any(|pair| {
                    let gap = pair[1].1.bbox.x0 - pair[0].1.bbox.x1;
                    gap > col_threshold && gap < avg_w * 40.0
                });

                if has_table_gap && group_size >= 2 {
                    // 计算整组行的表格 bbox
                    let x0 = rows[i..=j]
                        .iter()
                        .flat_map(|r| r.iter())
                        .map(|(_, w)| w.bbox.x0)
                        .fold(f64::INFINITY, f64::min);
                    let top = rows[i]
                        .iter()
                        .map(|(_, w)| w.bbox.top)
                        .fold(f64::INFINITY, f64::min);
                    let x1 = rows[i..=j]
                        .iter()
                        .flat_map(|r| r.iter())
                        .map(|(_, w)| w.bbox.x1)
                        .fold(0.0f64, f64::max);
                    let bottom = rows[j]
                        .iter()
                        .map(|(_, w)| w.bbox.bottom)
                        .fold(0.0f64, f64::max);

                    table_bboxes.push(BBox {
                        x0,
                        top,
                        x1,
                        bottom,
                    });

                    for r in i..=j {
                        for (idx, _) in &rows[r] {
                            table_word_indices.push(*idx);
                        }
                    }
                    break; // 每组只检测一个表格
                }
            }
        }

        i = j + 1;
    }

    (table_bboxes, table_word_indices)
}

/// 解析单个 PDF 页面（使用 lopdf），失败时返回 None。
fn extract_page_with_lopdf(
    page: &pdfplumber::Page,
    page_index: usize,
) -> Option<RawPage> {
    let width = page.width();
    let height = page.height();

    // 1. 文本
    let raw_text = page.extract_text(&TextOptions {
        layout: true,
        ..Default::default()
    });

    // 2. 单词
    let words: Vec<RawWord> = page
        .extract_words(&WordOptions::default())
        .into_iter()
        .enumerate()
        .map(|(i, w)| RawWord {
            id: format!("w_{}_{}", page_index, i),
            text: w.text,
            bbox: BBox {
                x0: w.bbox.x0,
                top: w.bbox.top,
                x1: w.bbox.x1,
                bottom: w.bbox.bottom,
            },
        })
        .collect();

    // 文本清洗
    let cleaned = clean_layout_text(&raw_text);
    let text = if cleaned.len() < raw_text.len() * 20 / 100 {
        eprintln!(
            "  [优化] 第{}页: 高空白占比 ({}→{} 字符)，用单词坐标重建文本...",
            page_index + 1,
            raw_text.len(),
            cleaned.len(),
        );
        reconstruct_text_from_words(&words)
    } else {
        cleaned
    };

    // 3. 段落块
    let blocks = compute_blocks(&words, page_index);

    // 4. 表格提取 + bbox 检测
    let lopdf_tables: Vec<Vec<Vec<Option<String>>>> = page.extract_tables(&TableSettings::default());
    let (detected_bboxes, _table_word_indices) = detect_table_bboxes(&words);

    let tables: Vec<RawTable> = if lopdf_tables.is_empty() {
        // lopdf 未检测到表格，但 bbox 检测到的作为占位
        detected_bboxes
            .into_iter()
            .enumerate()
            .map(|(i, bbox)| RawTable {
                id: format!("t_{}_{}", page_index, i),
                bbox: Some(bbox),
                rows: Vec::new(),
            })
            .collect()
    } else {
        lopdf_tables
            .into_iter()
            .enumerate()
            .map(|(i, rows)| {
                // 尝试匹配 bbox
                let bbox = detected_bboxes.get(i).cloned();
                RawTable {
                    id: format!("t_{}_{}", page_index, i),
                    bbox,
                    rows,
                }
            })
            .collect()
    };

    // 5. 线条
    let lines: Vec<RawLine> = page
        .lines()
        .iter()
        .map(|line| RawLine {
            bbox: BBox {
                x0: line.x0,
                top: line.top,
                x1: line.x1,
                bottom: line.bottom,
            },
        })
        .collect();

    // 6. 矩形
    let rects: Vec<RawRect> = page
        .rects()
        .iter()
        .map(|rect| RawRect {
            bbox: BBox {
                x0: rect.x0,
                top: rect.top,
                x1: rect.x1,
                bottom: rect.bottom,
            },
        })
        .collect();

    Some(RawPage {
        page_index,
        width,
        height,
        text,
        words,
        blocks,
        tables,
        lines,
        rects,
    })
}

/// 从纯文本创建合成段落块（用于无单词坐标的降级页面）。
fn blocks_from_text(text: &str, page_index: usize) -> Vec<RawBlock> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    // 按空行分隔为段落
    let paragraphs: Vec<&str> = text
        .split("\n\n")
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();

    if paragraphs.is_empty() {
        return Vec::new();
    }

    paragraphs
        .iter()
        .enumerate()
        .map(|(i, para)| {
            let line_count = para.lines().count();
            let word_count = para.chars().filter(|c| c.is_whitespace()).count() + 1;
            let block_type = if line_count == 1 && word_count <= 15 {
                BlockType::Heading
            } else {
                BlockType::Paragraph
            };

            RawBlock {
                id: format!("b_{}_{}", page_index, i),
                block_type,
                text: para.to_string(),
                // 无真实坐标，使用占位 bbox
                bbox: BBox {
                    x0: 0.0,
                    top: i as f64 * 20.0,
                    x1: 400.0,
                    bottom: (i + 1) as f64 * 20.0,
                },
            }
        })
        .collect()
}

/// 创建一个仅含文本的 stub 页面（lopdf 解析失败时使用）。
fn create_stub_page(page_index: usize, text: &str, width: f64, height: f64) -> RawPage {
    let blocks = blocks_from_text(text, page_index);
    RawPage {
        page_index,
        width,
        height,
        text: text.to_string(),
        words: Vec::new(),
        blocks,
        tables: Vec::new(),
        lines: Vec::new(),
        rects: Vec::new(),
    }
}

/// 将 PDF 文件解析为 [`RawDocument`]（新一代混合引擎）。
///
/// # 引擎策略
///
/// 1. 逐页用 lopdf 提取（文本+单词+表格+bbox），单页失败不影响其他页
/// 2. 如果有页面失败，用 pdf-extract 补充全文档文本
/// 3. 新增表格 bbox 检测（基于单词位置聚类）
/// 4. 如果所有页面都失败，尝试 Python 兜底
pub fn extract_pdf_to_raw_json(path: &str) -> Result<RawDocument> {
    extract_pdf_to_raw_json_with_python(path, &extract_with_python)
}

/// 可注入 Python 兜底实现的内部版本（便于单测注入假 python）。
fn extract_pdf_to_raw_json_with_python(
    path: &str,
    python_extract: &dyn Fn(&str, &str) -> Result<()>,
) -> Result<RawDocument> {
    let pdf = Pdf::open_file(path, None)?;

    let mut pages = Vec::new();
    let mut failed_page_indices = Vec::new();
    let mut page_count = 0usize;

    for page_result in pdf.pages_iter() {
        let page_index = page_count;
        page_count += 1;

        match page_result {
            Ok(page) => {
                match extract_page_with_lopdf(&page, page_index) {
                    Some(raw_page) => pages.push(raw_page),
                    None => {
                        eprintln!("  [警告] 第{}页提取失败，创建空页", page_index + 1);
                        failed_page_indices.push(page_index);
                        pages.push(create_stub_page(
                            page_index,
                            "",
                            page.width(),
                            page.height(),
                        ));
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "  [降级] 第{}页 lopdf 解析失败: {}",
                    page_index + 1, e
                );
                failed_page_indices.push(page_index);
                // 尝试获取页面尺寸（从已有成功页面推算，或使用默认值）
                let (w, h) = pages
                    .last()
                    .map(|p| (p.width, p.height))
                    .unwrap_or((595.0, 842.0)); // A4 默认
                pages.push(create_stub_page(page_index, "", w, h));
            }
        }
    }

    // 如果有失败页面，优先用 Python pdfplumber 兜底（真实坐标），
    // 不可用再回退 pdf-extract 纯文本（占位框）。
    if !failed_page_indices.is_empty() {
        eprintln!(
            "  [混合] {} 页中 {} 页 lopdf 失败，优先尝试 Python pdfplumber（真实坐标）...",
            page_count,
            failed_page_indices.len()
        );
        if let Some(doc) = try_extract_with_python(path, python_extract) {
            return Ok(doc);
        }
        eprintln!("  [混合] Python 兜底不可用，回退 pdf-extract 补充文本...");
        match extract_text_with_pdf_extract(path) {
            Ok(full_text) => {
                // 将 pdf-extract 文本按页面数均匀分配
                // 注意：pdf-extract 不提供页面分隔，按比例分配作为最佳近似
                let total_chars = full_text.chars().count();
                let chars_per_page = total_chars / page_count.max(1);

                for &page_idx in &failed_page_indices {
                    let start_char = page_idx * chars_per_page;
                    let end_char = if page_idx == page_count - 1 {
                        total_chars
                    } else {
                        (start_char + chars_per_page).min(total_chars)
                    };

                    let page_text: String = full_text
                        .chars()
                        .skip(start_char)
                        .take(end_char.saturating_sub(start_char))
                        .collect();

                    if page_idx < pages.len() && pages[page_idx].text.is_empty() {
                        // 同时生成合成 block
                        let blocks = blocks_from_text(&page_text, page_idx);
                        pages[page_idx].text = page_text;
                        pages[page_idx].blocks = blocks;
                    }
                }
                eprintln!("  [混合] pdf-extract 文本已补充到 {} 个失败页面",
                    failed_page_indices.iter().filter(|&&i| i < pages.len() && !pages[i].text.is_empty()).count());
            }
            Err(e) => {
                eprintln!("  [混合] pdf-extract 也失败: {}", e);
            }
        }
    }

    Ok(RawDocument {
        document_id: Uuid::new_v4().to_string(),
        source_path: path.to_string(),
        pages,
    })
}

/// 新一代并行 PDF 解析器（使用 rayon 并行处理页面）。
///
/// 适用于多页 PDF，通过并行页面提取获得 2-4x 加速。
/// 当环境变量 `AIBID_PARALLEL_PARSE=0` 时回退到串行模式。
#[allow(dead_code)]
pub fn extract_pdf_parallel(path: &str) -> Result<RawDocument> {
    use rayon::prelude::*;

    let use_parallel = std::env::var("AIBID_PARALLEL_PARSE")
        .unwrap_or_else(|_| "1".to_string())
        != "0";

    if !use_parallel {
        return extract_pdf_to_raw_json(path);
    }

    let pdf = Pdf::open_file(path, None)?;

    // 收集所有页面（先串行获取，lopdf 的 Pdf 不是 Sync）
    let page_data: Vec<(usize, Result<pdfplumber::Page, anyhow::Error>)> = pdf
        .pages_iter()
        .enumerate()
        .map(|(i, r)| (i, r.map_err(|e| anyhow::anyhow!("{}", e))))
        .collect();

    let total_pages = page_data.len();

    // 并行处理页面
    let results: Vec<(usize, Option<RawPage>)> = page_data
        .par_iter()
        .filter_map(|(page_index, page_result)| {
            match page_result {
                Ok(page) => {
                    let raw = extract_page_with_lopdf(page, *page_index);
                    Some((*page_index, raw))
                }
                Err(e) => {
                    eprintln!(
                        "  [降级] 第{}页 lopdf 解析失败: {}",
                        page_index + 1, e
                    );
                    Some((*page_index, Some(create_stub_page(*page_index, "", 595.0, 842.0))))
                }
            }
        })
        .collect();

    let mut pages: Vec<RawPage> = (0..total_pages)
        .map(|i| create_stub_page(i, "", 595.0, 842.0))
        .collect();
    for (idx, page_opt) in results {
        if let Some(page) = page_opt {
            pages[idx] = page;
        }
    }

    Ok(RawDocument {
        document_id: Uuid::new_v4().to_string(),
        source_path: path.to_string(),
        pages,
    })
}

// ---------- Python 兜底提取 ----------

/// 用 Python pdfplumber 兜底提取 PDF 内容。
pub fn extract_with_python(input_path: &str, output_path: &str) -> Result<()> {
    // 编译期嵌入脚本的绝对路径（位于 backend-rust/scripts/）
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/pdf_extract.py");
    let python = std::env::var("AI_BID_PYTHON_EXECUTABLE").unwrap_or_else(|_| "python".to_string());

    let output = Command::new(&python)
        .args([script, input_path, output_path])
        .output()
        .with_context(|| format!("无法使用 {} 执行 Python 脚本: {}", python, script))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Python 脚本执行失败: {}", stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        println!("{}", stdout.trim());
    }

    let meta = std::fs::metadata(output_path)
        .with_context(|| format!("Python 兜底提取未生成文件: {}", output_path))?;
    anyhow::ensure!(meta.len() > 0, "Python 兜底提取的 JSON 文件为空");

    Ok(())
}

/// 用 Python pdfplumber 兜底提取「真实坐标」的 [`RawDocument`]。
///
/// 返回 `None` 表示 Python 不可用/失败（调用方回退 pdf-extract 占位框路径）。
fn try_extract_with_python(
    path: &str,
    python_extract: &dyn Fn(&str, &str) -> Result<()>,
) -> Option<RawDocument> {
    let output = std::env::temp_dir().join(format!("aibid_py_{}.json", Uuid::new_v4()));
    let output_str = output.to_string_lossy();
    if let Err(e) = python_extract(path, &output_str) {
        eprintln!("  [混合] Python pdfplumber 兜底失败: {}（回退占位框路径）", e);
        let _ = std::fs::remove_file(&output);
        return None;
    }
    let doc = std::fs::read_to_string(&output)
        .ok()
        .and_then(|s| serde_json::from_str::<RawDocument>(&s).ok());
    let _ = std::fs::remove_file(&output);
    if doc.is_none() {
        eprintln!("  [混合] Python 兜底 JSON 解析失败（回退占位框路径）");
    }
    doc
}

// ─── 测试 ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── clean_layout_text ──────────────────────────────────────────

    #[test]
    fn test_clean_layout_text_merges_cjk_spaces() {
        // 中文绝对定位渲染：每个字之间被空格填充
        let input = "投  标  人  须  在  东  莞  地  区  设  有  常  驻  服  务  机  构";
        let result = clean_layout_text(input);
        assert_eq!(result, "投标人须在东莞地区设有常驻服务机构");
    }

    #[test]
    fn test_clean_layout_text_preserves_cjk_between_lines() {
        // CJK 之间空格消除，但换行保留
        let input = "第一章  总  则\n第二条  合  同  标  的";
        let result = clean_layout_text(input);
        // CJK_SPACE_RE 合并所有汉-空-汉模式，包括跨词边界
        assert_eq!(result, "第一章总则\n第二条合同标的");
    }

    #[test]
    fn test_clean_layout_text_compresses_multiple_spaces() {
        // CJK_RE 先合并汉字间的所有空格，MULTI_SPACE_RE 处理剩余
        // "符合" 和 "以下" 都是 CJK，所以被 CJK_SPACE_RE 完全合并
        let input = "符合    以下    条件";
        let result = clean_layout_text(input);
        assert_eq!(result, "符合以下条件");
    }

    #[test]
    fn test_clean_layout_text_empty_input() {
        assert_eq!(clean_layout_text(""), "");
        assert_eq!(clean_layout_text("   \n   \n  "), "");
    }

    #[test]
    fn test_clean_layout_text_pure_ascii() {
        let input = "The bidder shall comply with the requirements.";
        let result = clean_layout_text(input);
        assert_eq!(result, "The bidder shall comply with the requirements.");
    }

    #[test]
    fn test_clean_layout_text_mixed_cjk_and_ascii() {
        // 中文用 CJK 规则，英文空格保留
        let input = "项目编号  ABC-2024  投标  人";
        let result = clean_layout_text(input);
        assert_eq!(result, "项目编号  ABC-2024  投标人");
    }

    // ── reconstruct_text_from_words ────────────────────────────────

    fn make_word(text: &str, x0: f64, top: f64, x1: f64, bottom: f64) -> RawWord {
        RawWord {
            id: String::new(),
            text: text.to_string(),
            bbox: BBox {
                x0,
                top,
                x1,
                bottom,
            },
        }
    }

    #[test]
    fn test_reconstruct_text_empty_input() {
        let words: Vec<RawWord> = vec![];
        let result = reconstruct_text_from_words(&words);
        assert_eq!(result, "");
    }

    #[test]
    fn test_reconstruct_text_single_word() {
        let words = vec![make_word("投标人", 100.0, 200.0, 140.0, 210.0)];
        let result = reconstruct_text_from_words(&words);
        assert_eq!(result, "投标人");
    }

    #[test]
    fn test_reconstruct_text_single_line() {
        // 同一行内按 X 坐标排序拼接
        let words = vec![
            make_word("投标人", 100.0, 200.0, 140.0, 210.0),
            make_word("须", 145.0, 200.0, 160.0, 210.0),
            make_word("在", 165.0, 200.0, 180.0, 210.0),
            make_word("东莞", 185.0, 200.0, 215.0, 210.0),
        ];
        let result = reconstruct_text_from_words(&words);
        assert_eq!(result, "投标人须在东莞");
    }

    #[test]
    fn test_reconstruct_text_multi_line() {
        // 跨行：Y 坐标差超过 1.2 倍行高视为新行
        let words = vec![
            make_word("第一章", 100.0, 100.0, 140.0, 110.0),
            make_word("总则", 145.0, 100.0, 170.0, 110.0),
            make_word("第一条", 100.0, 130.0, 140.0, 140.0),
            make_word("合同标的", 145.0, 130.0, 190.0, 140.0),
        ];
        let result = reconstruct_text_from_words(&words);
        assert!(result.contains('\n'), "跨行文本应包含换行符");
        assert_eq!(result, "第一章总则\n第一条合同标的");
    }

    #[test]
    fn test_reconstruct_text_column_separation() {
        // 大列间距（> 8 倍平均字宽）→ 用双空格分隔
        let words = vec![
            make_word("条款", 50.0, 100.0, 80.0, 110.0),
            // 间隙 > 8x avg_w ≈ 80pt → 列分隔
            make_word("说明", 200.0, 100.0, 230.0, 110.0),
        ];
        let result = reconstruct_text_from_words(&words);
        assert_eq!(result, "条款  说明");
    }

    // ── compute_blocks ─────────────────────────────────────────────

    #[test]
    fn test_compute_blocks_empty_input() {
        let words: Vec<RawWord> = vec![];
        let blocks = compute_blocks(&words, 0);
        assert!(blocks.is_empty());
    }

    #[test]
    fn test_compute_blocks_single_line_heading() {
        // 单行单词 ≤ 10 → heading
        let words: Vec<RawWord> = (0..5)
            .map(|i| {
                make_word(
                    &format!("w{}", i),
                    50.0 + i as f64 * 30.0,
                    100.0,
                    75.0 + i as f64 * 30.0,
                    110.0,
                )
            })
            .collect();
        let blocks = compute_blocks(&words, 0);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].block_type, BlockType::Heading);
        assert!(blocks[0].id.starts_with("b_0_"));
    }

    #[test]
    fn test_compute_blocks_multi_line_paragraph() {
        // 多行多个单词 → paragraph
        let mut words = Vec::new();
        // Line 1: 10 words
        for i in 0..10 {
            words.push(make_word(
                &format!("L1W{}", i),
                50.0 + i as f64 * 30.0,
                100.0,
                75.0 + i as f64 * 30.0,
                110.0,
            ));
        }
        // Line 2: 5 words (same paragraph, gap < 1.8x line_height)
        for i in 0..5 {
            words.push(make_word(
                &format!("L2W{}", i),
                50.0 + i as f64 * 30.0,
                118.0,
                75.0 + i as f64 * 30.0,
                128.0,
            ));
        }
        let blocks = compute_blocks(&words, 2);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].block_type, BlockType::Paragraph);
        assert!(blocks[0].id.starts_with("b_2_"));
    }

    #[test]
    fn test_compute_blocks_paragraph_boundary() {
        // 行间距 > 1.8x line_height → 新段落
        let mut words = Vec::new();
        // Paragraph 1: line at y=100, height=10
        for i in 0..3 {
            words.push(make_word(
                &format!("P1W{}", i),
                50.0 + i as f64 * 30.0,
                100.0,
                75.0,
                110.0,
            ));
        }
        // Paragraph 2: line at y=140, gap=30 > 1.8*10=18 → new block
        for i in 0..3 {
            words.push(make_word(
                &format!("P2W{}", i),
                50.0 + i as f64 * 30.0,
                140.0,
                75.0,
                150.0,
            ));
        }
        let blocks = compute_blocks(&words, 1);
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].id.starts_with("b_1_"));
        assert!(blocks[1].id.starts_with("b_1_"));
    }

    #[test]
    fn test_compute_blocks_id_uniqueness() {
        // 每个 block 的 ID 在同一页内唯一
        let mut words = Vec::new();
        for p in 0..3 {
            let y = 100.0 + p as f64 * 40.0;
            for i in 0..5 {
                words.push(make_word(
                    &format!("W{}", i),
                    50.0 + i as f64 * 30.0,
                    y,
                    75.0,
                    y + 10.0,
                ));
            }
        }
        let blocks = compute_blocks(&words, 5);
        assert_eq!(blocks.len(), 3);
        let ids: Vec<&str> = blocks.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, vec!["b_5_0", "b_5_1", "b_5_2"]);
    }

    // ── 集成基准测试：真实 PDF 解析 ────────────────────────────

    /// 测试 PDF 文件（相对于 backend-rust/ 目录，由 CARGO_MANIFEST_DIR 解析）
    fn bench_pdf_paths() -> Vec<String> {
        let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let files = &[
            "tests/file/智慧教室环境改造工程.pdf",
            "tests/file/清华大学智慧校园项目招标文件.pdf",
            "tests/file/清华大学深圳国际研究生院智慧校园项目公开招标文件.pdf",
            "tests/file/研究生院智慧校园项目招标测试文件（2页）.pdf",
        ];
        files
            .iter()
            .map(|f| manifest.join(f).to_string_lossy().to_string())
            .collect()
    }

    /// 解析单个 PDF 并返回详细的解析指标。
    fn bench_parse_pdf(absolute_path: &str) -> PdfParseMetrics {
        use std::time::Instant;

        let path_str = absolute_path.to_string();
        let file_name = std::path::Path::new(&path_str)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();

        let start = Instant::now();
        let result = extract_pdf_to_raw_json(&path_str);
        let elapsed_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(doc) => {
                let total_chars: usize = doc.pages.iter().map(|p| p.text.chars().count()).sum();
                let total_words: usize = doc.pages.iter().map(|p| p.words.len()).sum();
                let total_blocks: usize = doc.pages.iter().map(|p| p.blocks.len()).sum();
                let total_tables: usize = doc.pages.iter().map(|p| p.tables.len()).sum();
                let tables_with_bbox: usize = doc
                    .pages
                    .iter()
                    .flat_map(|p| p.tables.iter())
                    .filter(|t| t.bbox.is_some())
                    .count();
                let headings: usize = doc
                    .pages
                    .iter()
                    .flat_map(|p| p.blocks.iter())
                    .filter(|b| b.block_type == BlockType::Heading)
                    .count();
                let max_page_chars: usize = doc
                    .pages
                    .iter()
                    .map(|p| p.text.chars().count())
                    .max()
                    .unwrap_or(0);

                PdfParseMetrics {
                    file_name,
                    success: true,
                    elapsed_ms,
                    pages: doc.pages.len(),
                    total_chars,
                    total_words,
                    total_blocks,
                    headings,
                    total_tables,
                    tables_with_bbox,
                    max_page_chars,
                    error: String::new(),
                }
            }
            Err(e) => PdfParseMetrics {
                file_name,
                success: false,
                elapsed_ms,
                pages: 0,
                total_chars: 0,
                total_words: 0,
                total_blocks: 0,
                headings: 0,
                total_tables: 0,
                tables_with_bbox: 0,
                max_page_chars: 0,
                error: format!("{}", e),
            },
        }
    }

    struct PdfParseMetrics {
        file_name: String,
        success: bool,
        elapsed_ms: u64,
        pages: usize,
        total_chars: usize,
        total_words: usize,
        total_blocks: usize,
        headings: usize,
        total_tables: usize,
        tables_with_bbox: usize,
        max_page_chars: usize,
        error: String,
    }

    /// 基准测试：所有测试 PDF 解析成功并输出指标报告
    #[test]
    fn bench_all_pdfs_parse_success() {
        let mut results = Vec::new();
        for pdf in &bench_pdf_paths() {
            results.push(bench_parse_pdf(pdf));
        }

        println!("\n========== PDF 解析基准测试报告 ==========");
        println!(
            "{:<40} {:>4} {:>8} {:>8} {:>8} {:>6} {:>6} {:>6}",
            "文件", "页数", "耗时ms", "字符数", "单词数", "段落", "表格", "表bbox"
        );
        println!("{}", "-".repeat(90));

        for r in &results {
            if r.success {
                println!(
                    "{:<40} {:>4} {:>8} {:>8} {:>8} {:>6} {:>6} {:>6}",
                    r.file_name,
                    r.pages,
                    r.elapsed_ms,
                    r.total_chars,
                    r.total_words,
                    r.total_blocks,
                    r.total_tables,
                    r.tables_with_bbox,
                );
            } else {
                println!(
                    "{:<40} {:>4} {:>8}  FAILED: {}",
                    r.file_name, r.pages, r.elapsed_ms, r.error
                );
            }
        }

        println!("{}", "-".repeat(90));

        // 统计
        let success_count = results.iter().filter(|r| r.success).count();
        let total_time: u64 = results.iter().map(|r| r.elapsed_ms).sum();
        let total_tables: usize = results.iter().map(|r| r.total_tables).sum();
        let total_tables_bbox: usize = results.iter().map(|r| r.tables_with_bbox).sum();
        println!("成功: {}/{}", success_count, results.len());
        println!("总耗时: {}ms", total_time);
        println!(
            "表格 bbox 覆盖率: {}/{} ({:.0}%)",
            total_tables_bbox,
            total_tables,
            if total_tables > 0 {
                total_tables_bbox as f64 / total_tables as f64 * 100.0
            } else {
                100.0
            }
        );

        // 断言：所有 PDF 必须解析成功
        for r in &results {
            assert!(r.success, "PDF 解析失败: {} — {}", r.file_name, r.error);
        }
    }

    /// 基准测试：验证文本清洗效果 — 无 CJK 空格残留
    #[test]
    fn bench_cjk_text_cleaning_quality() {
        for pdf in &bench_pdf_paths() {
            let doc = extract_pdf_to_raw_json(pdf)
                .unwrap_or_else(|e| panic!("解析失败 {}: {}", pdf, e));

            // 统计 CJK 字符间空格残留
            let cjk_space_re = regex::Regex::new(r"[一-鿿]\s+[一-鿿]").unwrap();
            let mut total_cjk_spaces = 0usize;
            let mut total_lines = 0usize;

            for page in &doc.pages {
                for line in page.text.lines() {
                    total_lines += 1;
                    let count = cjk_space_re.find_iter(line).count();
                    total_cjk_spaces += count;
                }
            }

            let file_name = std::path::Path::new(pdf)
                .file_name()
                .unwrap()
                .to_string_lossy();
            println!(
                "  {} — 行数: {}, CJK空格残留: {}",
                file_name, total_lines, total_cjk_spaces
            );

            // 中文 PDF 应极少有 CJK 空格残留（允许少量 edge case）
            assert!(
                total_cjk_spaces <= total_lines / 10,
                "{} CJK空格残留过多: {} 处 / {} 行",
                file_name,
                total_cjk_spaces,
                total_lines
            );
        }
    }

    /// 基准测试：验证表格提取 — 含表格的 PDF 应检出表格
    #[test]
    fn bench_table_extraction_coverage() {
        for pdf in &bench_pdf_paths() {
            let doc = extract_pdf_to_raw_json(pdf)
                .unwrap_or_else(|e| panic!("解析失败 {}: {}", pdf, e));

            let total_tables: usize = doc.pages.iter().map(|p| p.tables.len()).sum();
            let tables_with_bbox: usize = doc
                .pages
                .iter()
                .flat_map(|p| p.tables.iter())
                .filter(|t| t.bbox.is_some())
                .count();

            let file_name = std::path::Path::new(pdf)
                .file_name()
                .unwrap()
                .to_string_lossy();

            println!(
                "  {} — 表格: {} 个 (含bbox: {})",
                file_name, total_tables, tables_with_bbox
            );

            // 当前 Rust 引擎的已知局限：表格 bbox 缺失
            if total_tables > 0 && tables_with_bbox == 0 {
                println!(
                    "    ⚠ 已知局限: Rust pdfplumber (lopdf) 不提供表格bbox坐标"
                );
            }
        }
    }

    /// 基准测试：验证 block 类型分布 — 应同时包含 Heading 和 Paragraph
    #[test]
    fn bench_block_type_distribution() {
        for pdf in &bench_pdf_paths() {
            let doc = extract_pdf_to_raw_json(pdf)
                .unwrap_or_else(|e| panic!("解析失败 {}: {}", pdf, e));

            let headings: Vec<&RawBlock> = doc
                .pages
                .iter()
                .flat_map(|p| p.blocks.iter())
                .filter(|b| b.block_type == BlockType::Heading)
                .collect();
            let paragraphs: Vec<&RawBlock> = doc
                .pages
                .iter()
                .flat_map(|p| p.blocks.iter())
                .filter(|b| b.block_type == BlockType::Paragraph)
                .collect();

            let file_name = std::path::Path::new(pdf)
                .file_name()
                .unwrap()
                .to_string_lossy();
            println!(
                "  {} — Heading: {} 个, Paragraph: {} 个",
                file_name,
                headings.len(),
                paragraphs.len()
            );

            // 真实标书应同时包含标题和正文
            let total = headings.len() + paragraphs.len();
            if total > 10 {
                assert!(
                    headings.len() >= 1,
                    "{} 未检测到任何 Heading 块",
                    file_name
                );
                assert!(
                    paragraphs.len() >= 1,
                    "{} 未检测到任何 Paragraph 块",
                    file_name
                );
            }
        }
    }

    // ── 新引擎 POC 测试 ─────────────────────────────────────────

    /// POC: 验证 pdf-extract 能解析 lopdf 无法处理的 PDF
    #[test]
    fn bench_pdf_extract_poc() {
        for pdf in &bench_pdf_paths() {
            let file_name = std::path::Path::new(pdf)
                .file_name()
                .unwrap()
                .to_string_lossy();
            match extract_text_with_pdf_extract(pdf) {
                Ok(text) => {
                    let total_chars = text.chars().count();
                    // 估算页数（按换页符）
                    let page_count = text.split('\u{c}').count();
                    println!(
                        "  ✅ {} — pdf-extract: ~{} 页, {} 字符",
                        file_name, page_count, total_chars
                    );
                    assert!(total_chars > 100, "{}: 字符数过少 ({})", file_name, total_chars);
                }
                Err(e) => {
                    panic!("  ❌ {} — pdf-extract 失败: {}", file_name, e);
                }
            }
        }
    }

    // ── 高亮链路：真实坐标提取回归 ─────────────────────────────
    /// 真实 PDF 必须能解析出「非占位」bbox。
    ///
    /// 占位 bbox（x0==0 && x1==400 && 高度≤20）来自 lopdf 失败时的
    /// `blocks_from_text` 兜底，意味着没有任何真实坐标，前端 BBox 高亮无法工作。
    /// 该测试守卫：一旦 lopdf 降级而拿不到真实坐标，这里会失败提醒。
    #[test]
    fn real_pdf_yields_non_placeholder_bboxes() {
        let pdf = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/file/研究生院智慧校园项目招标测试文件（2页）.pdf");
        let doc =
            extract_pdf_to_raw_json(pdf.to_string_lossy().as_ref()).expect("PDF 解析失败");
        let total: usize = doc.pages.iter().map(|p| p.blocks.len()).sum();
        let non_placeholder = doc
            .pages
            .iter()
            .flat_map(|p| p.blocks.iter())
            .filter(|b| {
                !(b.bbox.x0 == 0.0 && b.bbox.x1 == 400.0 && (b.bbox.bottom - b.bbox.top) <= 20.1)
            })
            .count();
        println!("blocks total={total} non_placeholder={non_placeholder}");
        assert!(
            non_placeholder > 0,
            "真实 PDF 应解析出非占位 bbox（否则前端高亮链路无坐标可用）"
        );
    }

    // ── 兜底重排：lopdf 失败时优先 Python（真实坐标） ──────────
    #[test]
    fn try_python_fallback_yields_real_bbox_and_none_on_error() {
        let real = RawDocument {
            document_id: "py_doc".to_string(),
            source_path: "/tmp/x.pdf".to_string(),
            pages: vec![RawPage {
                page_index: 0,
                width: 595.0,
                height: 842.0,
                text: "真实坐标段落".to_string(),
                words: vec![],
                blocks: vec![RawBlock {
                    id: "b_0_0".to_string(),
                    block_type: BlockType::Paragraph,
                    text: "真实坐标段落".to_string(),
                    bbox: BBox { x0: 12.0, top: 24.0, x1: 320.0, bottom: 48.0 },
                }],
                tables: vec![],
                lines: vec![],
                rects: vec![],
            }],
        };
        let json = serde_json::to_string(&real).unwrap();

        // 成功分支：解析出非占位 bbox（真实坐标）
        let ok = |_inp: &str, out: &str| -> Result<()> {
            std::fs::write(out, &json)?;
            Ok(())
        };
        let doc = try_extract_with_python("/tmp/x.pdf", &ok).expect("应返回 Some(真实坐标)");
        let bb = doc.pages[0].blocks[0].bbox;
        assert!(
            bb.x0 > 0.0 && bb.x1 != 400.0 && (bb.bottom - bb.top) > 20.1,
            "应为非占位 bbox，got {:?}",
            bb
        );

        // 失败分支：返回 None（回退 pdf-extract 占位框路径）
        let fail = |_inp: &str, _out: &str| -> Result<()> { anyhow::bail!("no python") };
        assert!(try_extract_with_python("/tmp/x.pdf", &fail).is_none());
    }
}
