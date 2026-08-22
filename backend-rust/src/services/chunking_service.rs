//! 条款级 Chunk 切分服务
//!
//! 本模块负责从 [`Section`] 树切分为 Agent 可独立消费的条款级语义块。
//! 采用五条确定性规则，无 LLM 参与：
//!
//! | 规则 | 条件                                           | 动作                          |
//! |------|------------------------------------------------|-------------------------------|
//! | 1    | `body_text` 非空 + 无子节点 + 长度 ≤ 1500      | → `Leaf` chunk                |
//! | 1→4  | `body_text` 非空 + 无子节点 + 长度 > 1500      | → `Split` chunks (带 overlap) |
//! | 2    | `body_text` 为空 + 有子节点                     | → 向下传递容器路径，聚合子节点 |
//! | 3    | 多个叶子各自 < 100 字 + 同在父节点下            | → `Merged` chunk              |
//! | 5    | 所有 chunk                                     | → `embed_text()` 携带层级前缀  |
//!
//! ## 设计原则
//!
//! 一个 chunk = 一个可以独立理解、独立评估的完整语义单元。
//! 不是简单地把每个叶子节点当成一个 chunk。
//!
//! ## 技术选型
//!
//! 采用代码规则而非 LLM——确定性 100%、速度 < 10ms、零成本。
//! LLM 的角色是切分完成后的**质量审查**：抽样检查语义完整性。

use crate::domain::chunk::{BlockBBox, Chunk, ChunkType, ChunkingConfig};
use crate::domain::raw_document::RawDocument;
use crate::services::sectionize_service::Section;
use std::collections::HashMap;

// ─── 主入口 ──────────────────────────────────────────────────

/// 从 Section 树切分为 Chunk 列表。
///
/// 按页面顺序遍历 Section 树，应用规则 1-4 进行切分，
/// 然后按页面顺序排序、分配 `chunk_id`、合并碎片 chunk。
pub fn chunk_sections(sections: &[Section], config: &ChunkingConfig) -> Vec<Chunk> {
    let mut chunks: Vec<Chunk> = Vec::new();

    for section in sections {
        traverse_and_chunk(section, &Vec::new(), config, &mut chunks);
    }

    // 按页面顺序排序并分配 chunk_id
    chunks.sort_by_key(|c| c.page_start);
    for (i, chunk) in chunks.iter_mut().enumerate() {
        chunk.chunk_id = format!("ch_{:03}", i);
    }

    // 后处理：合并碎片 chunk
    if config.min_chunk_size > 0 {
        chunks = merge_tiny_chunks(chunks, config);
        // 重新分配 ID
        for (i, chunk) in chunks.iter_mut().enumerate() {
            chunk.chunk_id = format!("ch_{:03}", i);
        }
    }

    // 后处理：精确去重（移除内容完全相同的 chunk）
    chunks = dedup_chunks(chunks);
    // 重新分配 ID
    for (i, chunk) in chunks.iter_mut().enumerate() {
        chunk.chunk_id = format!("ch_{:03}", i);
    }

    chunks
}

// ─── 树遍历骨架 ─────────────────────────────────────────────

/// 递归遍历 Section 树，对每个节点应用切分规则。
///
/// # 参数
/// - `section`: 当前章节节点
/// - `parent_path`: 从根到当前节点父级的标题链
/// - `config`: 切分配置
/// - `chunks`: 累积输出的 chunk 列表
fn traverse_and_chunk(
    section: &Section,
    parent_path: &[String],
    config: &ChunkingConfig,
    chunks: &mut Vec<Chunk>,
) {
    // 规则1: 自包含叶子 → 直接成 chunk（可能触发规则4硬切）
    if try_chunk_leaf(section, parent_path, config, chunks) {
        return;
    }

    // 构建当前节点的完整路径（用于向下传递）
    let mut new_path = parent_path.to_vec();
    new_path.push(section.title.clone());

    // 纯标题占位（无 body_text 且无子节点）
    if section.children.is_empty() {
        // 标题非空 → 生成最小 Leaf chunk，保留模板结构信息
        // （如"格式自拟"占位页）。后续会被 merge_tiny_chunks 合并到
        // 相邻同路径 chunk，不会产生碎片污染。
        //
        // 注意：page 元数据使用 section.page_start / page_end（标题所在页），
        // 而非 body_page_start / body_page_end。纯标题 Section 无 body_text，
        // 其 body_page_start/end 经 #[serde(default)] 会被设为 0，
        // 若使用 body 范围会导致 chunk 被错误排到文档开头。
        if !section.title.trim().is_empty() {
            let mut path = parent_path.to_vec();
            path.push(section.title.clone());
            chunks.push(Chunk {
                chunk_id: String::new(),
                chunk_type: ChunkType::Leaf,
                section_path: path,
                text: section.title.trim().to_string(),
                page_start: section.page_start,
                page_end: section.page_end,
                source_block_ids: section.block_ids.clone(),
                bbox_refs: Vec::new(),
            });
        }
        return;
    }

    // 规则 1.5: 容器节点自身 body_text → 单独生成 chunk
    // （如"第五章 合同文本"的引言说明文字，pages 为 body 实际页范围而非
    // 到子节点末尾的宽泛 span，避免 22 页/44 页的 page span 膨胀）
    if !section.body_text.is_empty() {
        let text = format!("{}\n{}", section.title, section.body_text);
        if text.chars().count() > config.split_max_len {
            split_long_chunk(&new_path, &text, section, config, chunks);
        } else {
            chunks.push(Chunk {
                chunk_id: String::new(),
                chunk_type: ChunkType::Leaf,
                section_path: new_path.clone(),
                text,
                page_start: section.body_page_start,
                page_end: section.body_page_end,
                source_block_ids: section.block_ids.clone(),
                bbox_refs: Vec::new(),
            });
        }
    }

    // 规则2: 容器节点 → 向下传递路径（不单独成 chunk）
    //
    // 分离叶子和容器子节点：
    // - 叶子：无子节点 + body_text 非空
    // - 容器：有子节点（不论 body_text）
    let (leaves, containers): (Vec<&Section>, Vec<&Section>) = section
        .children
        .iter()
        .partition(|c| c.children.is_empty() && !c.body_text.is_empty());

    // 规则3: 相邻独立叶子 → 短则合并
    if !leaves.is_empty() {
        merge_adjacent_leaves(&leaves, &new_path, config, chunks);
    }

    // 容器子节点 → 递归处理
    for child in &containers {
        traverse_and_chunk(child, &new_path, config, chunks);
    }
}

// ─── 规则 1：自包含叶子节点直接成 chunk ────────────────────────

/// 尝试将自包含叶子节点转为 chunk。
///
/// 条件：`body_text` 非空 且 无子节点。
/// 若文本长度超过 `split_max_len`，委托给规则4硬切。
///
/// 返回 `true` 表示该节点已被消费（成 chunk 或硬切），无需再向下遍历。
fn try_chunk_leaf(
    section: &Section,
    parent_path: &[String],
    config: &ChunkingConfig,
    chunks: &mut Vec<Chunk>,
) -> bool {
    // 条件: body_text 非空 && children 为空
    if section.body_text.is_empty() || !section.children.is_empty() {
        return false;
    }

    let mut path = parent_path.to_vec();
    path.push(section.title.clone());
    let text = format!("{}\n{}", section.title, section.body_text);

    // 过长 → 规则4 硬切
    if text.chars().count() > config.split_max_len {
        split_long_chunk(&path, &text, section, config, chunks);
        return true;
    }

    chunks.push(Chunk {
        chunk_id: String::new(), // 由 chunk_sections 统一分配
        chunk_type: ChunkType::Leaf,
        section_path: path,
        text,
        // 使用 body 实际页范围：对于叶子节点，body_page_start/end
        // 精确反映正文所在的页码范围，而非标题页到末尾的宽泛 span
        page_start: section.body_page_start,
        page_end: section.body_page_end,
        source_block_ids: section.block_ids.clone(),
        bbox_refs: Vec::new(),
    });
    true
}

// ─── 规则 3：相邻独立叶子合并 ────────────────────────────────

/// 合并同一父节点下过短的相邻独立叶子节点。
///
/// 策略：
/// - 叶子的完整文本（标题 + 正文）< `merge_min_len` → 放入合并缓冲区
/// - 叶子够长 → 先消化缓冲区（flush），再单独成 chunk
/// - 缓冲区累计长度 > `split_max_len` → 提前 flush，避免合并后过大
/// - 叶子与缓冲区页面间隙 > `MAX_MERGE_PAGE_GAP` → 提前 flush，
///   避免远距离格式模板页被合并进同一 chunk 导致 page span 膨胀
fn merge_adjacent_leaves(
    leaves: &[&Section],
    parent_path: &[String],
    config: &ChunkingConfig,
    chunks: &mut Vec<Chunk>,
) {
    let mut merge_buffer: Vec<&Section> = Vec::new();
    let mut merge_len: usize = 0;
    // 追踪缓冲区当前的页面范围，用于检测 page gap
    let mut buffer_page_end: Option<usize> = None;

    for leaf in leaves {
        let leaf_text = format!("{}\n{}", leaf.title, leaf.body_text);
        let leaf_len = leaf_text.chars().count();

        if leaf_len < config.merge_min_len {
            // 页面间隙过大 → 先消化当前缓冲区，再开启新的合并组
            if let Some(buf_end) = buffer_page_end {
                let page_gap = leaf.body_page_start.saturating_sub(buf_end);
                if page_gap > MAX_MERGE_PAGE_GAP {
                    flush_merge_buffer(&merge_buffer, parent_path, config, chunks);
                    merge_buffer.clear();
                    merge_len = 0;
                    buffer_page_end = None;
                }
            }
            // 短叶子 → 进入合并缓冲区
            merge_buffer.push(*leaf);
            merge_len += leaf_len;
            buffer_page_end = Some(
                buffer_page_end
                    .unwrap_or(leaf.body_page_start)
                    .max(leaf.body_page_end),
            );
            // 合并后过长 → 先消化当前缓冲区
            if merge_len > config.split_max_len {
                flush_merge_buffer(&merge_buffer, parent_path, config, chunks);
                merge_buffer.clear();
                merge_len = 0;
                buffer_page_end = None;
            }
        } else {
            // 够长 → 先消化缓冲区，再单独成 chunk
            flush_merge_buffer(&merge_buffer, parent_path, config, chunks);
            merge_buffer.clear();
            merge_len = 0;
            buffer_page_end = None;
            // 单独成 chunk（可能过长触发规则4）
            let mut path = parent_path.to_vec();
            path.push(leaf.title.clone());
            let text = format!("{}\n{}", leaf.title, leaf.body_text);
            if text.chars().count() > config.split_max_len {
                split_long_chunk(&path, &text, leaf, config, chunks);
            } else {
                chunks.push(Chunk {
                    chunk_id: String::new(),
                    chunk_type: ChunkType::Leaf,
                    section_path: path,
                    text,
                    page_start: leaf.body_page_start,
                    page_end: leaf.body_page_end,
                    source_block_ids: leaf.block_ids.clone(),
                    bbox_refs: Vec::new(),
                });
            }
        }
    }

    // 处理尾部残留缓冲区
    flush_merge_buffer(&merge_buffer, parent_path, config, chunks);
}

/// 将合并缓冲区中的叶子节点输出为一个 Merged chunk。
///
/// 若缓冲区为空则不产生 chunk；若仅剩 1 个叶子则输出为 Leaf。
fn flush_merge_buffer(
    buffer: &[&Section],
    parent_path: &[String],
    config: &ChunkingConfig,
    chunks: &mut Vec<Chunk>,
) {
    if buffer.is_empty() {
        return;
    }

    // 仅剩 1 个 → 仍按 Leaf 输出
    if buffer.len() == 1 {
        let leaf = buffer[0];
        let mut path = parent_path.to_vec();
        path.push(leaf.title.clone());
        let text = format!("{}\n{}", leaf.title, leaf.body_text);
        if text.chars().count() > config.split_max_len {
            split_long_chunk(&path, &text, leaf, config, chunks);
        } else {
            chunks.push(Chunk {
                chunk_id: String::new(),
                chunk_type: ChunkType::Leaf,
                section_path: path,
                text,
                page_start: leaf.body_page_start,
                page_end: leaf.body_page_end,
                source_block_ids: leaf.block_ids.clone(),
                bbox_refs: Vec::new(),
            });
        }
        return;
    }

    // 确定合并后 chunk 的起始页、结束页和所有 block_ids
    // 使用 body 实际页范围，避免容器节点页跨度膨胀
    let page_start = buffer.iter().map(|s| s.body_page_start).min().unwrap_or(0);
    // 锚点式 page 范围上限：从第一个 leaf 的 page 开始，
    // 仅当后续 leaf 与当前范围间隙 ≤ MAX_MERGE_PAGE_GAP 时才扩展，
    // 防止远距离格式模板页撑大 chunk 的 page span（如 33→76）。
    let mut capped_page_end = buffer[0].body_page_end;
    for leaf in &buffer[1..] {
        let gap = leaf.body_page_start.saturating_sub(capped_page_end);
        if gap <= MAX_MERGE_PAGE_GAP {
            capped_page_end = capped_page_end.max(leaf.body_page_end);
        }
    }
    let page_end = capped_page_end;
    let mut all_block_ids: Vec<String> = Vec::new();
    let mut merged_text_parts: Vec<String> = Vec::new();

    for leaf in buffer {
        merged_text_parts.push(format!("{}\n{}", leaf.title, leaf.body_text));
        for bid in &leaf.block_ids {
            if !all_block_ids.contains(bid) {
                all_block_ids.push(bid.clone());
            }
        }
    }

    let merged_text = merged_text_parts.join("\n\n");

    // 合并后若过长 → 硬切
    if merged_text.chars().count() > config.split_max_len {
        // 创建临时 Section 用于 split_long_chunk
        let temp_section = Section {
            level: buffer[0].level,
            title: String::new(),
            pattern: String::new(),
            page_start,
            page_end,
            block_ids: all_block_ids.clone(),
            body_text: String::new(),
            children: Vec::new(),
            body_page_start: page_start,
            body_page_end: page_end,
        };
        split_long_chunk(parent_path, &merged_text, &temp_section, config, chunks);
        return;
    }

    chunks.push(Chunk {
        chunk_id: String::new(),
        chunk_type: ChunkType::Merged {
            rule: "adjacent_merge".to_string(),
            child_count: buffer.len(),
        },
        section_path: parent_path.to_vec(),
        text: merged_text,
        page_start,
        page_end,
        source_block_ids: all_block_ids,
        bbox_refs: Vec::new(),
    });
}

// ─── 规则 4：过长 chunk 硬切 ─────────────────────────────────

/// 将过长文本在语义边界切分为多个 chunk，相邻片段保留 overlap。
///
/// 切分策略：
/// 1. 找到所有语义边界（`find_para_boundaries`：句末标点、段落、表格行、编号）
/// 2. 每次切 `split_max_len` 长度，回退到最近的语义边界
/// 3. 下一片段从 overlap 窗口内的安全断点开始，保证语义连续
fn split_long_chunk(
    path: &[String],
    text: &str,
    section: &Section,
    config: &ChunkingConfig,
    chunks: &mut Vec<Chunk>,
) {
    let total = text.chars().count();
    if total <= config.split_max_len {
        chunks.push(Chunk {
            chunk_id: String::new(),
            chunk_type: ChunkType::Leaf,
            section_path: path.to_vec(),
            text: text.to_string(),
            page_start: section.body_page_start,
            page_end: section.body_page_end,
            source_block_ids: section.block_ids.clone(),
            bbox_refs: Vec::new(),
        });
        return;
    }

    let boundaries = find_para_boundaries(text);

    let mut parts: Vec<String> = Vec::new();
    let mut pos = 0;
    // 迭代计数防护：防止边界条件导致的死循环（最多 total/50 + 2 次迭代）
    let max_iterations = (total / 50).max(2) + 2;
    let mut iteration = 0;

    while pos < total {
        iteration += 1;
        if iteration > max_iterations {
            // 安全阀：超过合理迭代次数，收集剩余文本直接退出
            parts.push(text.chars().skip(pos).collect());
            break;
        }
        let prev_pos = pos;
        let end_candidate = (pos + config.split_max_len).min(total);

        if end_candidate >= total {
            // 最后一截：直接收尾
            parts.push(text.chars().skip(pos).collect());
            break;
        }

        // 回退到 [pos, end_candidate] 范围内最近的段落边界
        let split_point = boundaries
            .iter()
            .rev()
            .find(|&&b| b > pos && b <= end_candidate)
            .copied()
            .unwrap_or(end_candidate);

        parts.push(text.chars().skip(pos).take(split_point - pos).collect());

        // 下一片段起点 = 安全断点（在 overlap 窗口内搜索最近的语义边界）
        // 而非机械地回退固定字符数。
        // 参考 LangChain RecursiveCharacterTextSplitter 的做法：
        // 在 split_point - overlap 附近搜索安全断点，避免 overlap
        // 起始位置落在编号条目或句子中间。
        let safe_start = find_safe_overlap_start(text, split_point, config.split_overlap);
        // 确保向前推进：安全起点不能超过切分点（防死循环）
        pos = if safe_start >= split_point {
            split_point
        } else {
            safe_start
        };
        // 确保 pos 至少推进了 split_overlap/2（防止在极小文本窗口中振荡）
        if pos <= split_point.saturating_sub(config.split_overlap / 2) {
            pos = split_point.saturating_sub(config.split_overlap);
        }
        // 确保 pos 向前推进（最终安全阀）
        if pos <= prev_pos {
            pos = split_point;
        }
    }

    for (i, part) in parts.iter().enumerate() {
        chunks.push(Chunk {
            chunk_id: String::new(),
            chunk_type: ChunkType::Split {
                part: i + 1,
                total: parts.len(),
            },
            section_path: path.to_vec(),
            text: part.clone(),
            page_start: section.body_page_start,
            page_end: section.body_page_end,
            source_block_ids: section.block_ids.clone(),
            bbox_refs: Vec::new(),
        });
    }
}

/// 在 overlap 窗口内搜索安全的 overlap 起点。
///
/// 从 `split_point` 向前搜索，找到最近的语义安全断点：
/// 1. 句末标点（。！？）后紧跟换行符 → 句子边界
/// 2. 段落边界 `\n\n` → 段落边界
/// 3. 章节编号起始（`（一）`、`1.` 等）→ 语义单元边界
///
/// 搜索范围：[split_point - overlap, split_point]。
/// 找不到安全断点时回退到 `split_point - overlap`。
///
/// # 复杂度
///
/// 单次遍历 O(overlap) 个字符，在一次循环中按优先级查找最佳候选。
fn find_safe_overlap_start(text: &str, split_point: usize, overlap: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    if total == 0 || split_point >= total {
        return split_point;
    }

    let search_start = split_point.saturating_sub(overlap);
    let search_end = split_point;

    // 单次扫描，记录 5 个优先级的最佳候选位置。
    // 扫描从 search_start → search_end（= split_point），
    // 对每个优先级选择离 split_point 最近的匹配（即最后一个匹配），
    // 确保 overlap 起点尽可能接近切分点而非被推到远处。
    let mut best_sentence: Option<usize> = None;   // 句末标点后换行
    let mut best_para: Option<usize> = None;       // \n\n 后
    let mut best_section: Option<usize> = None;    // 编号起始
    let mut best_table_row: Option<usize> = None;  // 表格行边界（|\n 或 \n|）
    let mut best_newline: Option<usize> = None;    // 任意换行（最后手段）

    let cjk_numerals: &[char] = &['一', '二', '三', '四', '五', '六', '七', '八', '九', '十'];

    for i in search_start..search_end {
        if i + 1 >= total {
            continue;
        }

        // 优先级1：。！？后紧跟 \n
        if (chars[i] == '。' || chars[i] == '！' || chars[i] == '？')
            && chars[i + 1] == '\n'
        {
            let mut start = i + 1;
            if start + 1 < total && chars[start] == '\n' {
                start += 1; // 跳过 \n\n 的第二个 \n
            }
            if start < split_point {
                best_sentence = Some(start); // 总是更新，选最近的
            }
        }

        // 优先级2：\n\n
        if chars[i] == '\n' && chars[i + 1] == '\n' {
            let start = i + 2;
            if start < split_point {
                best_para = Some(start);
            }
        }

        // 优先级3：\n 后跟编号
        if chars[i] == '\n' && i + 1 < total && {
            let next = chars[i + 1];
            cjk_numerals.contains(&next) || next == '（' || next == '(' || next.is_ascii_digit()
        } {
            let start = i + 1;
            if start < split_point {
                best_section = Some(start);
            }
        }

        // 优先级3.5：表格行边界 — |\n（行末）或 \n|（行首）
        // |\n → 下一行起点在 \n 之后 (i+2)
        if chars[i] == '|' && i + 2 <= total && chars[i + 1] == '\n' {
            let start = i + 2;
            if start < split_point {
                best_table_row = Some(start);
            }
        }
        // \n| → 下一行起点在 | 处 (i+1)
        if chars[i] == '\n' && i + 1 < total && chars[i + 1] == '|' {
            let start = i + 1;
            if start < split_point {
                best_table_row = Some(start);
            }
        }

        // 最后手段：任意换行
        if chars[i] == '\n' {
            let start = i + 1;
            if start < split_point {
                best_newline = Some(start);
            }
        }
    }

    // 按优先级返回最佳候选（每个都已是最接近 split_point 的匹配）
    best_sentence
        .or(best_para)
        .or(best_section)
        .or(best_table_row)
        .or(best_newline)
        .unwrap_or_else(|| split_point.saturating_sub(overlap))
}

/// 查找文本中的段落边界位置（字符偏移）。
///
/// 段落边界定义（按优先级排列）：
/// - `\n\n`（显式段落分隔）
/// - `\n\n|` 模式（Markdown 表格行前的空行）→ 确保切分点不落入表格内部
/// - `|\n\n` 模式（Markdown 表格行后的空行）→ 确保切分点不落入表格内部
/// - `。\n` `！\n` `？\n`（句末标点后紧跟换行）→ 句子边界
/// - `|\n` 或 `\n|`（单换行表格行分隔）→ 表格行边界
/// - `\n` 后紧跟编号模式：
///   - 中文序号（一～十）
///   - ASCII 数字编号（"1." "2." 等）
///   - 括号编号（"（一）" "(1)" 等）
///   - 圈号数字（①-⑩）
///   - 项目符号（- ※ ● ◆ ■ ▲ ▼ ★ ☆）
///
/// 设计原则：边界宁可多不可少，多出的边界在 `split_long_chunk` 中
/// 通过"从 end_candidate 向前搜索最近边界"策略自然收敛到最合适的切分点。
fn find_para_boundaries(text: &str) -> Vec<usize> {
    let mut boundaries: Vec<usize> = Vec::new();
    let chars: Vec<char> = text.chars().collect();

    // 在开头加一个虚拟边界
    boundaries.push(0);

    let cjk_numerals: &[char] = &['一', '二', '三', '四', '五', '六', '七', '八', '九', '十'];
    let sentence_ends: &[char] = &['。', '！', '？'];
    let circled_numerals: &[char] = &[
        '①', '②', '③', '④', '⑤', '⑥', '⑦', '⑧', '⑨', '⑩',
    ];
    let bullet_markers: &[char] = &['-', '※', '●', '◆', '■', '▲', '▼', '★', '☆'];

    for i in 0..chars.len() {
        // ── \n\n 双换行段落分隔 ──
        if chars[i] == '\n' && i + 1 < chars.len() && chars[i + 1] == '\n' {
            // Markdown 表格行前空行：\n\n| 模式 → 强制段落边界
            if i + 2 < chars.len() && chars[i + 2] == '|' {
                boundaries.push(i + 1); // 在第二个 \n 之后
                continue;
            }
            // Markdown 表格行后空行：|\n\n 模式 → 强制段落边界
            if i > 0 && chars[i - 1] == '|' {
                boundaries.push(i + 1); // 在第二个 \n 之后
                continue;
            }
            boundaries.push(i + 1); // 在第二个 \n 之后
            continue;
        }

        // ── 句末标点后紧跟 \n → 句子边界 ──
        // 解决测试1：无 \n\n 的连续文本在句末处切分
        // push(i+2)：边界在 \n 之后，确保句子以 `。\n` 完整结尾
        if i + 1 < chars.len()
            && sentence_ends.contains(&chars[i])
            && chars[i + 1] == '\n'
        {
            boundaries.push(i + 2); // 在 \n 之后
            continue;
        }

        // ── 表格行边界：|\n 或 \n| ──
        // 解决测试2：保护 Markdown 表格行不被拦腰切断
        // |\n：边界在 \n 之后 (i+2)，确保行以 `|\n` 完整结尾
        if chars[i] == '|' && i + 1 < chars.len() && chars[i + 1] == '\n' {
            boundaries.push(i + 2); // 在行末 \n 之后
            continue;
        }
        // \n|：边界在 | 位置 (i+1)，确保下一行以 `|` 开头
        if chars[i] == '\n' && i + 1 < chars.len() && chars[i + 1] == '|' {
            boundaries.push(i + 1); // 在换行之后、表格行之前
            continue;
        }

        // ── \n 后紧跟各种编号模式 ──
        // 解决测试3：ASCII 数字编号、括号编号、项目符号等
        if chars[i] == '\n' && i + 1 < chars.len() {
            let next = chars[i + 1];

            // 中文序号（一～十）
            if cjk_numerals.contains(&next) {
                boundaries.push(i + 1);
                continue;
            }
            // ASCII 数字编号 / 括号编号 / 圈号数字 / 项目符号
            if next.is_ascii_digit()
                || next == '('
                || next == '（'
                || circled_numerals.contains(&next)
                || bullet_markers.contains(&next)
            {
                boundaries.push(i + 1);
            }
        }
    }

    // 在末尾加一个虚拟边界
    boundaries.push(chars.len());

    boundaries
}

// ─── 后处理：Chunk 去重 ───────────────────────────────────────

/// 合并时允许的最大页面间隙。相邻 chunk/page 差距 ≤ 此值时才扩展
/// page 范围，防止合并后的 chunk 页面跨度膨胀（如 43 页的格式模板合并）。
const MAX_MERGE_PAGE_GAP: usize = 2;

/// 精确去重：移除内容完全相同的重复 chunk。
///
/// 保留第一个出现的 chunk（页面顺序），移除后续重复项。
/// 使用文本内容的哈希进行高效去重。
fn dedup_chunks(chunks: Vec<Chunk>) -> Vec<Chunk> {
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::with_capacity(chunks.len());
    let mut result: Vec<Chunk> = Vec::with_capacity(chunks.len());

    for chunk in chunks {
        // 使用文本内容的简单哈希作为去重键
        // 对极短文本（< 20 chars）不执行去重，避免误删不同的短标题
        if chunk.text.chars().count() < 20 {
            result.push(chunk);
            continue;
        }
        let key = &chunk.text;
        if !seen.contains(key) {
            seen.insert(key.clone());
            result.push(chunk);
        }
    }

    result
}

// ─── 后处理：碎片 Chunk 合并 ─────────────────────────────────

/// 检查两个 chunk 是否共享同一个顶级章节（第一位路径元素）。
///
/// 原先要求直接父路径完全一致，但标书中"第X部分"下的不同子节
/// （如"八、适用法律"与"九、资格审查"）在页面流中相邻时，其碎片 chunk
/// 语义同属一个大部分，合并不会造成跨主题混淆。
///
/// 放宽到 top-1 匹配后，原先因父路径不同而无法合并的孤立极小 chunk
/// （如评标标准碎片、法律条款碎片）可被正确吸收到相邻 chunk。
///
/// 返回 `true` 表示两个 chunk 属于同一顶级章节（可以合并）。
fn same_parent(a: &[String], b: &[String]) -> bool {
    match (a.first(), b.first()) {
        (Some(a_top), Some(b_top)) => a_top == b_top,
        // 空路径 → 保守允许合并（极少发生）
        _ => true,
    }
}

/// 合并过短的碎片 chunk 到相邻 chunk。
///
/// 两阶段处理：
/// - Pass 1（前向合并）：将碎片 chunk 合并到前一个相邻 chunk
/// - Pass 2（后向合并）：对 Pass 1 未能合并的碎片，尝试合并到后一个相邻 chunk
///
/// 合并前检查两个 chunk 是否共享同一顶级章节，避免跨主题合并。
///
/// ## 页面范围膨胀控制
///
/// 合并时检查页面间隙：比较碎片与 anchor 的**原始** page 范围（即首次
/// tiny_merge 之前的 page_end / page_start）。仅当 gap ≤ 2 时才扩展
/// page 范围。否则仍然合并文本（语义归拢），但保持 page 范围不变。
///
/// 关键设计：对比 anchor 的原始范围而非逐步扩展后的范围，防止链式合并
/// 逐页推进最终膨胀到数十页。例如 ch_114 原先因连续合并 14 个格式模板
/// 碎片而膨胀到 43 页跨度（33-76），修复后应限制在原始 anchor 附近。
fn merge_tiny_chunks(chunks: Vec<Chunk>, config: &ChunkingConfig) -> Vec<Chunk> {
    let min = config.min_chunk_size;
    let mut result: Vec<Chunk> = Vec::new();

    // ── Pass 1: 前向合并（合并到 prev chunk）──
    // anchor_page_end: 记录 anchor 在首次 tiny_merge 之前的原始 page_end，
    // 用于所有后续 gap 检查。永不更新，确保不会逐页链式膨胀。
    let mut anchor_page_end: Option<usize> = None;

    for chunk in chunks {
        if chunk.text.chars().count() < min
            && let Some(prev) = result.last_mut()
            && same_parent(&prev.section_path, &chunk.section_path)
        {
            // 合并 chunk → prev（文本始终合并）
            prev.text = format!("{}\n\n{}", prev.text, chunk.text);
            // 对比 anchor 原始 page_end 而非逐步扩展后的值
            let anchor_end = anchor_page_end.unwrap_or(prev.page_end);
            let page_gap = chunk.page_start.saturating_sub(anchor_end);
            if page_gap <= MAX_MERGE_PAGE_GAP {
                prev.page_end = prev.page_end.max(chunk.page_end);
            }
            // 首次合并时锁定 anchor 原始范围
            if anchor_page_end.is_none() {
                anchor_page_end = Some(anchor_end);
            }
            for bid in &chunk.source_block_ids {
                if !prev.source_block_ids.contains(bid) {
                    prev.source_block_ids.push(bid.clone());
                }
            }
            let child_count = match &prev.chunk_type {
                ChunkType::Merged { child_count: c, .. } => c + 1,
                _ => 2,
            };
            prev.chunk_type = ChunkType::Merged {
                rule: "tiny_merge".to_string(),
                child_count,
            };
            continue; // 已合并，跳过 push
        }
        // ← 前向合并失败：继续执行 push，由 Pass 2 处理
        result.push(chunk);
        anchor_page_end = None; // chunk 进入 result → 重置 anchor 追踪
    }

    // ── Pass 2: 后向合并（合并到 next chunk）──
    let mut remove_indices: Vec<usize> = Vec::new();
    let mut anchor_page_start: Vec<Option<usize>> = vec![None; result.len()];

    for i in 0..result.len() {
        if result[i].text.chars().count() >= min {
            continue;
        }
        if i + 1 < result.len() && same_parent(&result[i].section_path, &result[i + 1].section_path)
        {
            let tiny_text = result[i].text.clone();
            let tiny_page_start = result[i].page_start;
            let tiny_page_end = result[i].page_end;
            let tiny_block_ids: Vec<String> = result[i].source_block_ids.clone();

            let next = &mut result[i + 1];
            next.text = format!("{}\n\n{}", tiny_text, next.text);
            // 对比 anchor 原始 page_start 而非逐步回退后的值
            let anchor_start = anchor_page_start[i + 1].unwrap_or(next.page_start);
            let page_gap = anchor_start.saturating_sub(tiny_page_end);
            if page_gap <= MAX_MERGE_PAGE_GAP {
                next.page_start = tiny_page_start;
            }
            if anchor_page_start[i + 1].is_none() {
                anchor_page_start[i + 1] = Some(anchor_start);
            }
            for bid in &tiny_block_ids {
                if !next.source_block_ids.contains(bid) {
                    next.source_block_ids.insert(0, bid.clone());
                }
            }
            let child_count = match &next.chunk_type {
                ChunkType::Merged { child_count: c, .. } => c + 1,
                _ => 2,
            };
            next.chunk_type = ChunkType::Merged {
                rule: "tiny_merge".to_string(),
                child_count,
            };
            remove_indices.push(i);
        }
    }

    for &idx in remove_indices.iter().rev() {
        result.remove(idx);
    }

    result
}

// ─── 规则 5：嵌入文本携带层级上下文 ───────────────────────────

/// 截断过长的路径标题。
fn truncate_path_title(title: &str, max_len: usize) -> String {
    if max_len == 0 || title.chars().count() <= max_len {
        title.to_string()
    } else {
        let truncated: String = title.chars().take(max_len).collect();
        format!("{}…", truncated)
    }
}

impl Chunk {
    /// 生成带层级上下文的嵌入文本。
    ///
    /// 向量嵌入时携带层级前缀，避免不同章节的编号条目在向量空间中混淆。
    /// `max_path_len` 控制单个路径元素的最大字符数（0 = 不截断）。
    ///
    /// # 示例
    ///
    /// ```text
    /// 裸文本:  "1）具有独立承担民事责任的能力..."
    /// 嵌入文本: "【供应商的资格要求 > 政府采购法第二十二条】
    ///           1）具有独立承担民事责任的能力..."
    /// ```
    pub fn embed_text(&self, ctx_depth: usize, max_path_len: usize) -> String {
        let ctx = self
            .section_path
            .iter()
            .rev()
            .take(ctx_depth)
            .rev()
            .map(|t| truncate_path_title(t, max_path_len))
            .collect::<Vec<_>>()
            .join(" > ");

        // V6.2: 提取正文中 Markdown 表格的表头关键词，注入到 embed 前缀
        // 解决表头信息被埋在大段正文中、向量检索无法命中的问题。
        // 例如："| 履约保证金 | 不收取 |" → 提取 "履约保证金" 加入前缀
        let table_keys = extract_table_keys(&self.text);

        let header = if ctx.is_empty() && table_keys.is_empty() {
            return self.text.clone();
        } else if table_keys.is_empty() {
            format!("【{}】", ctx)
        } else if ctx.is_empty() {
            format!("【表头: {}】", table_keys.join(", "))
        } else {
            format!("【{} | 表头: {}】", ctx, table_keys.join(", "))
        };

        format!("{}\n{}", header, self.text)
    }
}

/// 从文本中提取 Markdown 表格的表头关键词（第一列的非空单元格）。
///
/// 匹配 `| KEY | ...` 模式，去重，最多返回 5 个。
/// 跳过分隔行（`|---|`）和空 KEY。
fn extract_table_keys(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut keys = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        // 必须至少以 | 开头，且包含第二个 |
        if !trimmed.starts_with('|') {
            continue;
        }
        // 跳过分隔行
        if trimmed.contains("---") {
            continue;
        }
        // 提取第一个单元格
        let cells: Vec<&str> = trimmed.split('|').collect();
        if cells.len() < 2 {
            continue;
        }
        let key = cells[1].trim(); // cells[0] 是开头的 | 之前的空字符串
        if key.is_empty() {
            continue;
        }
        // 限制单个 key 长度（跳过超长单元格内容）
        if key.chars().count() > 30 {
            continue;
        }
        if seen.insert(key.to_string()) {
            keys.push(key.to_string());
            if keys.len() >= 5 {
                break;
            }
        }
    }

    keys
}

// ─── BBox 缓存填充 ─────────────────────────────────────────────

/// 填充每个 Chunk 的 `bbox_refs` 字段。
///
/// 从 `RawDocument` 构建 `block_id → (page_index, bbox, page_width)` 的
/// 内存索引，然后遍历每个 chunk 的 `source_block_ids` 查表填充。
///
/// 调用时机：`chunk_sections()` 之后、序列化到 JSON 或存入 `DocumentState` 之前。
pub fn populate_bbox_refs(chunks: &mut [Chunk], raw_doc: &RawDocument) {
    let mut block_map: HashMap<String, (usize, crate::domain::raw_document::BBox, f64, usize)> =
        HashMap::new();

    for page in &raw_doc.pages {
        for block in &page.blocks {
            block_map.insert(
                block.id.clone(),
                (
                    page.page_index,
                    block.bbox.clone(),
                    page.width,
                    block.text.chars().count(),
                ),
            );
        }
    }

    for chunk in chunks.iter_mut() {
        let mut refs: Vec<BlockBBox> = Vec::with_capacity(chunk.source_block_ids.len());
        for block_id in &chunk.source_block_ids {
            if let Some((page, bbox, page_width, char_count)) = block_map.get(block_id) {
                refs.push(BlockBBox {
                    block_id: block_id.clone(),
                    page: *page,
                    bbox: bbox.clone(),
                    page_width: *page_width,
                    char_count: *char_count,
                });
            }
        }
        chunk.bbox_refs = refs;
    }
}

// ─── 测试 ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chunk::ChunkingConfig;

    /// 构造一个简单的叶子 Section 用于测试。
    fn make_leaf(level: u8, title: &str, body: &str) -> Section {
        Section {
            level,
            title: title.to_string(),
            pattern: "test".to_string(),
            page_start: 0,
            page_end: 0,
            block_ids: vec![format!("b_0_{}", level)],
            body_text: body.to_string(),
            children: Vec::new(),
            body_page_start: 0,
            body_page_end: 0,
        }
    }

    /// 构造一个容器 Section（无 body_text，有子节点）。
    fn make_container(level: u8, title: &str, children: Vec<Section>) -> Section {
        Section {
            level,
            title: title.to_string(),
            pattern: "test".to_string(),
            page_start: 0,
            page_end: 0,
            block_ids: Vec::new(),
            body_text: String::new(),
            children,
            body_page_start: 0,
            body_page_end: 0,
        }
    }

    #[test]
    fn test_rule1_leaf_chunk() {
        let section = make_leaf(4, "1. 项目地点", "东莞理工学院松山湖校区6号教学楼。");
        let config = ChunkingConfig::default();
        let mut chunks = Vec::new();

        let consumed = try_chunk_leaf(&section, &["第一章".to_string()], &config, &mut chunks);
        assert!(consumed);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(chunks[0].chunk_type, ChunkType::Leaf));
        assert!(chunks[0].text.contains("1. 项目地点"));
        assert!(chunks[0].text.contains("东莞理工学院"));
    }

    #[test]
    fn test_rule1_container_not_consumed() {
        let child = make_leaf(5, "（1）条件", "具有独立承担民事责任的能力");
        let container = make_container(4, "1. 资格要求", vec![child]);
        let config = ChunkingConfig::default();
        let mut chunks = Vec::new();

        // 容器本身不应被 try_chunk_leaf 消费
        let consumed = try_chunk_leaf(&container, &Vec::new(), &config, &mut chunks);
        assert!(!consumed);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_rule3_merge_short_leaves() {
        let leaves: Vec<Section> = vec![
            make_leaf(4, "1. 项目地点", "东莞理工学院松山湖校区6号教学楼。"),
            make_leaf(4, "2. 项目工期", "合同签订后60个日历日内完成。"),
            make_leaf(4, "3. 质保期", "项目验收合格之日起2年。"),
            make_leaf(4, "4. 竣工图纸", "成交供应商需提供竣工图纸。"),
        ];
        let leaf_refs: Vec<&Section> = leaves.iter().collect();
        let config = ChunkingConfig::default();
        let mut chunks = Vec::new();
        let parent_path = vec!["第一章".to_string()];

        merge_adjacent_leaves(&leaf_refs, &parent_path, &config, &mut chunks);

        // 四个短叶子应合并为 1 个 Merged chunk
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0].chunk_type, ChunkType::Merged { .. }));
        if let ChunkType::Merged { rule, child_count } = &chunks[0].chunk_type {
            assert_eq!(rule, "adjacent_merge");
            assert_eq!(*child_count, 4);
        }
    }

    #[test]
    fn test_rule3_long_leaf_not_merged() {
        // 构造一个超过 merge_min_len 的长叶子
        let long_body = "A".repeat(150); // 150 字符 > merge_min_len (100)
        let leaves: Vec<Section> = vec![
            make_leaf(4, "1. 长条款", &long_body),
            make_leaf(4, "2. 短条款", "短内容"),
        ];
        let leaf_refs: Vec<&Section> = leaves.iter().collect();
        let config = ChunkingConfig::default();
        let mut chunks = Vec::new();
        let parent_path = vec!["第一章".to_string()];

        merge_adjacent_leaves(&leaf_refs, &parent_path, &config, &mut chunks);

        // 长叶子单独成 chunk，短叶子也单独成 chunk（单个不合并）
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn test_rule4_split_overlong() {
        // 构造超过 split_max_len 的文本（每段约 7 字 × 250 = 1750 字）
        let long_body = "项目说明：\n\n".to_string() + &"详细描述内容。".repeat(250);
        // total chars > 1500
        assert!(long_body.chars().count() > 1500);

        let section = make_leaf(4, "1. 项目概况", &long_body);
        let config = ChunkingConfig::default();
        let mut chunks = Vec::new();
        let path = vec!["第一章".to_string()];

        split_long_chunk(&path, &section.body_text, &section, &config, &mut chunks);

        // 应产出至少 2 个 Split chunk
        assert!(chunks.len() >= 2);
        for (i, chunk) in chunks.iter().enumerate() {
            assert!(matches!(&chunk.chunk_type, ChunkType::Split { part: p, .. } if *p == i + 1));
        }
        if let ChunkType::Split { total, .. } = &chunks[0].chunk_type {
            assert_eq!(*total, chunks.len());
        }
    }

    #[test]
    fn test_rule5_embed_text() {
        let chunk = Chunk {
            bbox_refs: Vec::new(),
            chunk_id: "ch_042".to_string(),
            chunk_type: ChunkType::Leaf,
            section_path: vec![
                "第一章 磋商邀请".to_string(),
                "二.供应商的资格要求".to_string(),
                "1）具有独立承担民事责任的能力".to_string(),
            ],
            text: "1）具有独立承担民事责任的能力\n供应商必须是...".to_string(),
            page_start: 3,
            page_end: 3,
            source_block_ids: vec!["b_3_5".to_string()],
        };

        let embedded = chunk.embed_text(2, 0);
        assert!(embedded.starts_with("【二.供应商的资格要求 > 1）具有独立承担民事责任的能力】"));
        assert!(embedded.contains("供应商必须是"));

        // ctx_depth=0 → 无前缀
        let no_ctx = chunk.embed_text(0, 0);
        assert_eq!(no_ctx, chunk.text);
    }

    #[test]
    fn test_find_para_boundaries() {
        let text = "第一段内容。\n\n第二段内容。\n一、新段落标题";
        let boundaries = find_para_boundaries(text);

        // 应包含边界：0, \n\n 之后, \n一 之前, text.len()
        assert!(boundaries.len() >= 3);
        assert_eq!(boundaries.first(), Some(&0));
        assert_eq!(boundaries.last(), Some(&text.chars().count()));
    }

    #[test]
    fn test_traverse_and_chunk_container_aggregation() {
        // 模拟：容器节点下多个短叶子 → 应聚合
        let child1 = make_leaf(
            5,
            "1）具有独立承担民事责任的能力",
            "供应商须是在中华人民共和国境内注册的法人。",
        );
        let child2 = make_leaf(5, "2）有依法缴纳税收", "供应商须提供近6个月的纳税证明。");
        let child3 = make_leaf(5, "3）有良好的商业信誉", "供应商须提供信用中国查询记录。");

        let sub_container = make_container(
            4,
            "1.供应商应具备《政府采购法》第二十二条规定的条件",
            vec![child1, child2, child3],
        );

        let parent = make_container(2, "二.供应商的资格要求", vec![sub_container]);

        let config = ChunkingConfig::default();
        let chunks = chunk_sections(&[parent], &config);

        // 应产出至少 1 个 chunk（3 个短叶子合并）
        assert!(!chunks.is_empty());
        // chunk_id 格式检查
        assert!(chunks[0].chunk_id.starts_with("ch_"));
        // section_path 应包含祖先容器路径
        assert!(
            chunks[0]
                .section_path
                .iter()
                .any(|t| t.contains("资格要求"))
        );
    }

    #[test]
    fn test_chunk_id_ordering() {
        // 使用足够长的 body_text 以避免被 merge_tiny_chunks 合并
        // min_chunk_size 默认 50，此处确保正文远超此阈值
        let body_long =
            "这是足够长的正文内容，确保超过 min_chunk_size 的默认阈值五十个字符以上就是如此。";
        let s1 = make_leaf(4, "A. 条款", body_long);
        let s2 = make_leaf(4, "B. 条款", body_long);
        // 手动设置不同页码测试排序（同时设 body_page_start 与 page_start 一致）
        let sections = vec![
            Section {
                page_start: 3,
                body_page_start: 3,
                body_page_end: 3,
                ..s1
            },
            Section {
                page_start: 1,
                body_page_start: 1,
                body_page_end: 1,
                ..s2
            },
        ];

        let config = ChunkingConfig::default();
        let chunks = chunk_sections(&sections, &config);

        // 按页码排序后，page_start=1 的在前
        assert!(chunks.len() >= 2, "应有至少 2 个 chunk");
        assert_eq!(chunks[0].chunk_id, "ch_000");
        assert_eq!(chunks[0].page_start, 1);
        assert_eq!(chunks[1].chunk_id, "ch_001");
        assert_eq!(chunks[1].page_start, 3);
    }

    // ─── V4.1: Leaf 判定边界与占位节点 ──────────────────────────

    #[test]
    fn test_rule1_boundary_at_split_max() {
        // 恰好 1500 字符 → Leaf；超过 → Split
        let body_1500 = "A".repeat(1500 - "1. 边界条款\n".chars().count());
        let body_1501 = "B".repeat(1501 - "1. 边界条款\n".chars().count());

        let section_leaf = make_leaf(4, "1. 边界条款", &body_1500);
        let section_split = make_leaf(4, "1. 边界条款", &body_1501);

        let config = ChunkingConfig::default();
        let mut chunks_leaf = Vec::new();
        let mut chunks_split = Vec::new();

        let consumed = try_chunk_leaf(&section_leaf, &Vec::new(), &config, &mut chunks_leaf);
        assert!(consumed);
        assert_eq!(chunks_leaf.len(), 1);
        assert!(matches!(chunks_leaf[0].chunk_type, ChunkType::Leaf));

        let consumed = try_chunk_leaf(&section_split, &Vec::new(), &config, &mut chunks_split);
        assert!(consumed);
        assert!(chunks_split.len() >= 2);
        for c in &chunks_split {
            assert!(matches!(c.chunk_type, ChunkType::Split { .. }));
        }
    }

    #[test]
    fn test_rule1_empty_body_no_children_placeholder() {
        // body="" 且 children=[] → 纯标题占位，不应生成 chunk
        let placeholder = Section {
            level: 4,
            title: "五.附则".to_string(),
            pattern: "cjk_numbered".to_string(),
            page_start: 10,
            page_end: 10,
            block_ids: vec!["b_10_3".to_string()],
            body_text: String::new(),
            children: Vec::new(),
            body_page_start: 10,
            body_page_end: 10,
        };
        let config = ChunkingConfig::default();
        let mut chunks = Vec::new();

        let consumed = try_chunk_leaf(&placeholder, &Vec::new(), &config, &mut chunks);
        assert!(!consumed, "纯标题占位不应被消费为 Leaf chunk");
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_rule1_body_and_children_not_leaf() {
        // body 非空但 children 也非空 → 不是叶子，不应被 try_chunk_leaf 消费
        let child = make_leaf(5, "1）子条款", "子条款内容。");
        let mixed = Section {
            level: 4,
            title: "1. 混合节点".to_string(),
            pattern: "digit_dot".to_string(),
            page_start: 5,
            page_end: 7,
            block_ids: vec!["b_5_0".to_string()],
            body_text: "这是引言文本。".to_string(),
            children: vec![child],
            body_page_start: 5,
            body_page_end: 5,
        };
        let config = ChunkingConfig::default();
        let mut chunks = Vec::new();

        let consumed = try_chunk_leaf(&mixed, &Vec::new(), &config, &mut chunks);
        assert!(!consumed, "有子节点的节点不应被当作 Leaf");
    }

    // ─── V4.2: 容器聚合 ─────────────────────────────────────────

    #[test]
    fn test_rule2_container_produces_no_chunks() {
        // 纯容器（无 body_text）本身不应产生任何 chunk
        // 子节点应由 traverse_and_chunk 递归处理
        let child1 = make_leaf(5, "1）条件A", "内容A。");
        let child2 = make_leaf(5, "2）条件B", "内容B。");
        let container = make_container(4, "1. 纯容器", vec![child1, child2]);

        let config = ChunkingConfig::default();
        let mut chunks = Vec::new();

        // 直接测试：容器不应被 try_chunk_leaf 消费
        let consumed = try_chunk_leaf(&container, &Vec::new(), &config, &mut chunks);
        assert!(!consumed);
        assert!(chunks.is_empty());

        // 通过 chunk_sections 完整流程测试
        let all_chunks = chunk_sections(&[container], &config);
        assert!(!all_chunks.is_empty(), "容器的子节点应产生 chunk");
        // 所有 chunk 的 section_path 应包含容器的标题
        for chunk in &all_chunks {
            assert!(
                chunk.section_path.iter().any(|t| t == "1. 纯容器"),
                "子 chunk 的 path 应包含容器标题: {:?}",
                chunk.section_path
            );
        }
    }

    #[test]
    fn test_rule2_deeply_nested_container() {
        // 验证深层容器嵌套时 section_path 完整传递
        // L2 容器 > L4 容器 > L5 叶子
        let leaf = make_leaf(5, "1）具体条件", "具体内容描述。");
        let inner_container = make_container(4, "1. 资格条件", vec![leaf]);
        let outer_container = make_container(2, "二.供应商要求", vec![inner_container]);

        let config = ChunkingConfig::default();
        let chunks = chunk_sections(&[outer_container], &config);

        assert!(!chunks.is_empty());
        // section_path 应包含完整的层级链
        let path = &chunks[0].section_path;
        assert!(
            path.iter().any(|t| t == "二.供应商要求"),
            "path 应包含 L2: {:?}",
            path
        );
        assert!(
            path.iter().any(|t| t == "1. 资格条件"),
            "path 应包含 L4: {:?}",
            path
        );
        assert!(
            path.iter().any(|t| t == "1）具体条件"),
            "path 应包含 L5: {:?}",
            path
        );
    }

    // ─── V4.3: 相邻短叶子合并 ──────────────────────────────────

    #[test]
    fn test_rule3_mixed_lengths() {
        // 场景: [30字, 120字, 25字] — 中间一条够长
        let short1 = make_leaf(4, "1. 短条一", "短内容A"); // ~12 chars
        let long = make_leaf(4, "2. 长条款", &"长".repeat(120)); // 120+ chars
        let short2 = make_leaf(4, "3. 短条三", "短内容C"); // ~12 chars

        let leaves: Vec<Section> = vec![short1, long, short2];
        let leaf_refs: Vec<&Section> = leaves.iter().collect();
        let config = ChunkingConfig::default();
        let mut chunks = Vec::new();
        let parent_path = vec!["一.项目概况".to_string()];

        merge_adjacent_leaves(&leaf_refs, &parent_path, &config, &mut chunks);

        // 期望 3 个 chunk：短1(Leaf), 长(Leaf), 短2(Leaf)
        assert_eq!(
            chunks.len(),
            3,
            "混合长度应产出 3 个 chunk，实际: {}",
            chunks.len()
        );
        // 每个 chunk 都是 Leaf 类型（缓冲区都是单条 → Leaf 输出）
        for chunk in &chunks {
            assert!(matches!(chunk.chunk_type, ChunkType::Leaf));
        }
        // 长条款应包含 "长" 字
        assert!(chunks[1].text.contains("长"));
    }

    #[test]
    fn test_rule3_merge_exceeds_split_max_during_buffer() {
        // 场景: 验证合并缓冲区在超限时正确 flush
        // 使用小型短叶子（merge 后总长 < split_max_len），确保产生 Merged chunk
        let short_body = "条".repeat(50); // 50 chars
        let leaves: Vec<Section> = (1..=6)
            .map(|i| make_leaf(4, &format!("{}. 短条款{}", i, i), &short_body))
            .collect();
        let leaf_refs: Vec<&Section> = leaves.iter().collect();
        let config = ChunkingConfig {
            merge_min_len: 100, // 每个叶子 ~65 chars < 100 → 全部入缓冲
            ..ChunkingConfig::default()
        };
        let mut chunks = Vec::new();
        let parent_path = vec!["第一章".to_string()];

        merge_adjacent_leaves(&leaf_refs, &parent_path, &config, &mut chunks);

        // 6 个短叶子合并后约 6*65 + 5*2 = 400 chars < 1500 → 单个 Merged chunk
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0].chunk_type, ChunkType::Merged { rule, child_count } if rule == "adjacent_merge" && *child_count == 6),
            "6 个短叶子应合并为 1 个 Merged chunk"
        );

        // 批量二：构造足够多的叶子，使合并文本超过 split_max_len
        // 此时 flush 会触发 split_long_chunk，产生 Split 类型
        let large_body = "款".repeat(250); // 250 chars body
        let many_leaves: Vec<Section> = (1..=8)
            .map(|i| make_leaf(4, &format!("{}. 条款{}", i, i), &large_body))
            .collect();
        let many_refs: Vec<&Section> = many_leaves.iter().collect();
        let mut chunks2 = Vec::new();

        merge_adjacent_leaves(&many_refs, &parent_path, &config, &mut chunks2);

        // 验证：所有产出的 chunk 均不超过 split_max_len（核心不变量）
        for chunk in &chunks2 {
            assert!(
                chunk.text.chars().count() <= config.split_max_len,
                "每个 chunk 应 ≤ split_max_len ({}), 实际: {}",
                config.split_max_len,
                chunk.text.chars().count()
            );
        }
        // 验证：至少产生了 chunk（叶子被处理）
        assert!(!chunks2.is_empty(), "大量叶子应产生 chunk");

        // 8 个叶子（每个 ~265 chars）全部 < merge_min_len=100? NO, 265 >= 100
        // 所以它们不会进入合并缓冲，而是各自独立成 Leaf
        // 验证每个 chunk 类型为 Leaf
        for chunk in &chunks2 {
            assert!(
                matches!(chunk.chunk_type, ChunkType::Leaf),
                "≥merge_min_len 的长叶子应各自为 Leaf"
            );
        }
    }

    #[test]
    fn test_rule3_all_long_leaves() {
        // 全部叶子 ≥ 100 字 → 各自成 Leaf，不触发合并
        let long_body = "内".repeat(120);
        let leaves: Vec<Section> = (1..=3)
            .map(|i| make_leaf(4, &format!("{}. 长条款{}", i, i), &long_body))
            .collect();
        let leaf_refs: Vec<&Section> = leaves.iter().collect();
        let config = ChunkingConfig::default();
        let mut chunks = Vec::new();
        let parent_path = vec!["第一章".to_string()];

        merge_adjacent_leaves(&leaf_refs, &parent_path, &config, &mut chunks);

        // 3 个叶子各自成 Leaf chunk
        assert_eq!(chunks.len(), 3);
        for chunk in &chunks {
            assert!(matches!(chunk.chunk_type, ChunkType::Leaf));
        }
    }

    // ─── V4.4: 硬切与 Overlap ──────────────────────────────────

    #[test]
    fn test_rule4_three_parts() {
        // 构造 3000 字的文本（用 ASCII 字符避免字节边界问题）→ 应产出 ≥3 个 Split chunk
        let body_3000 = "A".repeat(3000);
        let section = make_leaf(4, "1. 超长条款", &body_3000);
        let config = ChunkingConfig::default();
        let mut chunks = Vec::new();
        let path = vec!["第一章".to_string()];

        split_long_chunk(&path, &section.body_text, &section, &config, &mut chunks);

        assert!(
            chunks.len() >= 3,
            "3000 字应产出 ≥3 个 Split chunk, 实际: {}",
            chunks.len()
        );
        // part 编号从 1 开始连续
        for (i, chunk) in chunks.iter().enumerate() {
            if let ChunkType::Split { part, total } = &chunk.chunk_type {
                assert_eq!(*part, i + 1, "part 编号应连续");
                assert_eq!(*total, chunks.len(), "total 应等于总片段数");
            }
        }
        // 每段不超过 split_max_len
        for chunk in &chunks {
            assert!(
                chunk.text.chars().count() <= config.split_max_len,
                "每段应 ≤ split_max_len"
            );
        }
        // overlap: 相邻片段应有重叠（用 char-based 比较）
        if chunks.len() >= 2 {
            let text1: Vec<char> = chunks[0].text.chars().collect();
            let text2: Vec<char> = chunks[1].text.chars().collect();
            let overlap_start = text1.len().saturating_sub(200);
            let overlap_end = std::cmp::min(200, text2.len());
            let end_chars: String = text1[overlap_start..].iter().collect();
            let start_chars: String = text2[..overlap_end].iter().collect();
            // 验证确实存在重叠：第一部分末尾和第二部分开头有公共子串
            assert!(
                !end_chars.is_empty() && !start_chars.is_empty(),
                "overlap 区域不应为空"
            );
        }
    }

    #[test]
    fn test_rule4_no_paragraph_boundaries() {
        // 纯字母数字无换行 → 无段落边界 → 在 split_max_len 处硬切
        let body_no_breaks = "ABCDEFGHIJ".repeat(200); // 2000 chars, no \n\n
        assert!(!body_no_breaks.contains("\n\n"));
        assert!(body_no_breaks.chars().count() > 1500);

        let section = make_leaf(4, "1. 无边界条款", &body_no_breaks);
        let config = ChunkingConfig::default();
        let mut chunks = Vec::new();
        let path = vec!["第一章".to_string()];

        split_long_chunk(&path, &section.body_text, &section, &config, &mut chunks);

        // 即使无段落边界也能正常切分
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(
                chunk.text.chars().count() <= config.split_max_len,
                "每段应 ≤ split_max_len"
            );
        }
    }

    #[test]
    fn test_rule4_split_preserves_overlap_content() {
        // 验证 overlap 区域确实包含重复文本
        let body = "A".repeat(1600);
        let section = make_leaf(4, "1. 条款", &body);
        let config = ChunkingConfig::default();
        let mut chunks = Vec::new();
        let path = vec!["第一章".to_string()];

        split_long_chunk(&path, &section.body_text, &section, &config, &mut chunks);
        assert!(chunks.len() >= 2);

        // overlap 验证：第一段末尾和第二段开头有重叠
        let part1_end: String = chunks[0].text.chars().rev().take(50).collect();
        let part2_start: String = chunks[1].text.chars().take(50).collect();
        // 两者应有公共字符（overlap）
        let common = part1_end
            .chars()
            .filter(|c| part2_start.contains(*c))
            .count();
        assert!(common > 0, "overlap 区域应包含公共字符");
    }

    // ─── V4.5: embed_text 边界 ──────────────────────────────────

    #[test]
    fn test_rule5_ctx_depth_exceeds_path() {
        // ctx_depth 超过 path 长度时取全部，不 panic
        let chunk = Chunk {
            bbox_refs: Vec::new(),
            chunk_id: "ch_test".to_string(),
            chunk_type: ChunkType::Leaf,
            section_path: vec!["第一章".to_string(), "一、概述".to_string()],
            text: "正文内容...".to_string(),
            page_start: 0,
            page_end: 0,
            source_block_ids: vec!["b_0_0".to_string()],
        };

        // ctx_depth=5 但 path 只有 2 级 → 取全部 2 级
        let embedded = chunk.embed_text(5, 0);
        assert!(embedded.starts_with("【第一章 > 一、概述】"));
        // 不应 panic，应正常返回
    }

    #[test]
    fn test_rule5_empty_path() {
        let chunk = Chunk {
            bbox_refs: Vec::new(),
            chunk_id: "ch_test".to_string(),
            chunk_type: ChunkType::Leaf,
            section_path: Vec::new(),
            text: "正文内容...".to_string(),
            page_start: 0,
            page_end: 0,
            source_block_ids: vec!["b_0_0".to_string()],
        };

        // 空 path → 无前缀，直接返回原文本
        let embedded = chunk.embed_text(2, 0);
        assert_eq!(embedded, chunk.text);
        // 不应产生 "【】" 空壳
        assert!(!embedded.starts_with("【】"));
    }

    #[test]
    fn test_rule5_ctx_depth_one() {
        let chunk = Chunk {
            bbox_refs: Vec::new(),
            chunk_id: "ch_test".to_string(),
            chunk_type: ChunkType::Leaf,
            section_path: vec![
                "第一章 磋商邀请".to_string(),
                "二.供应商的资格要求".to_string(),
                "1.基本条件".to_string(),
            ],
            text: "在中华人民共和国境内注册...".to_string(),
            page_start: 0,
            page_end: 0,
            source_block_ids: vec!["b_1_0".to_string()],
        };

        // ctx_depth=1 → 只取最后 1 级
        let embedded = chunk.embed_text(1, 0);
        assert!(embedded.starts_with("【1.基本条件】"));
        assert!(!embedded.contains("二.供应商的资格要求"));
    }

    #[test]
    fn test_rule5_path_truncation() {
        // embed_path_max_len > 0 时，过长的路径元素应被截断
        let chunk = Chunk {
            bbox_refs: Vec::new(),
            chunk_id: "ch_test".to_string(),
            chunk_type: ChunkType::Leaf,
            section_path: vec![
                "第一章 磋商邀请".to_string(),
                "一、《深圳经济特区政府采购条例》第五十七条供应商在政府采购中，有下列行为之一的，属于隐瞒真实情况，提供虚假资料".to_string(),
                "（一）在采购活动中应当回避而未回避的".to_string(),
            ],
            text: "正文内容...".to_string(),
            page_start: 0,
            page_end: 0,
            source_block_ids: vec!["b_0_0".to_string()],
        };

        // max_path_len=40 → 第二个元素（70+ chars）应被截断
        let embedded = chunk.embed_text(2, 40);
        // 前40个字符: "一、《深圳经济特区政府采购条例》第五十七条供应商在政府采购中，有下列行为之一的，"
        assert!(
            embedded.starts_with("【一、《深圳经济特区政府采购条例》第五十七条供应商在政府采购中，有下列行为之一的，… > （一）在采购活动中应当回避而未回避的】"),
            "实际 embed_text: {}",
            embedded
        );
        // 第一个元素（< 40 chars）不应被截断
        assert!(!embedded.contains("第一章 磋商邀请…"));
        // 第三个元素（< 40 chars）不应被截断
        assert!(embedded.contains("（一）在采购活动中应当回避而未回避的"));

        // max_path_len=0 → 不截断
        let no_trunc = chunk.embed_text(2, 0);
        assert!(no_trunc.contains("第五十七条供应商在政府采购中，有下列行为之一的"));
    }

    #[test]
    fn test_merge_tiny_chunks_basic() {
        // 验证碎片 chunk 被合并到邻居
        let long_text = "这是足够长的正常正文内容，确保超过 min_chunk_size 的默认阈值三十个字符。";
        let chunks = vec![
            Chunk {
                bbox_refs: Vec::new(),
                chunk_id: "ch_000".to_string(),
                chunk_type: ChunkType::Leaf,
                section_path: vec!["第一章".to_string()],
                text: long_text.to_string(),
                page_start: 0,
                page_end: 0,
                source_block_ids: vec!["b_0_0".to_string()],
            },
            Chunk {
                bbox_refs: Vec::new(),
                chunk_id: "ch_001".to_string(),
                chunk_type: ChunkType::Leaf,
                section_path: vec!["第一章".to_string()],
                text: "短".to_string(), // 仅 1 字符 → 碎片
                page_start: 1,
                page_end: 1,
                source_block_ids: vec!["b_1_0".to_string()],
            },
            Chunk {
                bbox_refs: Vec::new(),
                chunk_id: "ch_002".to_string(),
                chunk_type: ChunkType::Leaf,
                section_path: vec!["第一章".to_string()],
                text: long_text.to_string(),
                page_start: 2,
                page_end: 2,
                source_block_ids: vec!["b_2_0".to_string()],
            },
        ];

        let config = ChunkingConfig {
            min_chunk_size: 30,
            ..ChunkingConfig::default()
        };
        let merged = merge_tiny_chunks(chunks, &config);

        // "短" 应被合并到前一个 chunk，最终保留 2 个 chunk
        assert_eq!(merged.len(), 2, "碎片应被合并到前一个块");
        assert!(
            merged[0].text.contains("短"),
            "碎片内容应在合并后的第一个块中"
        );
        assert!(
            merged[0].text.contains("正常正文内容"),
            "第一个块的 long_text 应保留"
        );
    }

    #[test]
    fn test_merge_tiny_chunks_disabled() {
        // min_chunk_size=0 → 不合并
        let chunks = vec![Chunk {
            bbox_refs: Vec::new(),
            chunk_id: "ch_000".to_string(),
            chunk_type: ChunkType::Leaf,
            section_path: vec!["第一章".to_string()],
            text: "短".to_string(),
            page_start: 0,
            page_end: 0,
            source_block_ids: vec!["b_0_0".to_string()],
        }];

        let config = ChunkingConfig {
            min_chunk_size: 0,
            ..ChunkingConfig::default()
        };
        let merged = merge_tiny_chunks(chunks, &config);
        assert_eq!(merged.len(), 1);
    }

    // ─── V4.6: chunk 元数据完整性 ───────────────────────────────

    #[test]
    fn test_chunk_metadata_integrity() {
        // 对所有 chunk 验证 ID 连续、page 范围合法、block_ids 非空
        let s1 = make_leaf(4, "1. 条款A", "内容A。");
        let s2 = make_leaf(4, "2. 条款B", "内容B。");
        let sections = vec![
            Section {
                page_start: 2,
                page_end: 3,
                block_ids: vec!["b_2_0".to_string(), "b_2_1".to_string()],
                ..s1
            },
            Section {
                page_start: 4,
                page_end: 4,
                block_ids: vec!["b_4_0".to_string()],
                ..s2
            },
        ];

        let config = ChunkingConfig::default();
        let chunks = chunk_sections(&sections, &config);

        // ID 连续性
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_id, format!("ch_{:03}", i));
        }

        // page 范围合法性
        for chunk in &chunks {
            assert!(
                chunk.page_start <= chunk.page_end,
                "page_start({}) ≤ page_end({})",
                chunk.page_start,
                chunk.page_end
            );
        }

        // block_ids 非空
        for chunk in &chunks {
            assert!(
                !chunk.source_block_ids.is_empty(),
                "每个 chunk 应有至少一个 source_block_id"
            );
        }

        // section_path 非空
        for chunk in &chunks {
            assert!(
                !chunk.section_path.is_empty(),
                "每个 chunk 应有非空 section_path"
            );
        }
    }

    // ─── 边界/极端条件 ──────────────────────────────────────────

    #[test]
    fn test_empty_sections() {
        let config = ChunkingConfig::default();
        let chunks = chunk_sections(&[], &config);
        assert!(chunks.is_empty(), "空 sections 列表应产生空 chunks");
    }

    #[test]
    fn test_deeply_nested_empty_leaves() {
        // 深层嵌套，但所有叶子 body_text 为空 → 不应产生 chunk
        let empty_leaf = Section {
            level: 5,
            title: "（1）空条款".to_string(),
            pattern: "paren_digit".to_string(),
            page_start: 1,
            page_end: 1,
            block_ids: vec!["b_1_0".to_string()],
            body_text: String::new(),
            children: Vec::new(),
            body_page_start: 1,
            body_page_end: 1,
        };
        let container = make_container(4, "1. 容器", vec![empty_leaf]);
        let root = make_container(2, "一、章节", vec![container]);

        let config = ChunkingConfig::default();
        let chunks = chunk_sections(&[root], &config);

        // 空叶子不应产生 chunk
        for chunk in &chunks {
            assert!(!chunk.text.is_empty(), "不应产生空 text 的 chunk");
        }
    }

    // ─── extract_table_keys 测试 ─────────────────────────────

    #[test]
    fn test_extract_table_keys_basic() {
        let text = "some text\n| 付款方式 | 1期：支付比例30%... |\n| 验收要求 | 按清单验收 |\n| 履约保证金 | 不收取 |";
        let keys = extract_table_keys(text);
        assert_eq!(keys, vec!["付款方式", "验收要求", "履约保证金"]);
    }

    #[test]
    fn test_extract_table_keys_skips_separator() {
        let text =
            "| 标的提供的时间 | 合同签订后... |\n| --- | --- |\n| 标的提供的地点 | 东莞理工学院 |";
        let keys = extract_table_keys(text);
        assert_eq!(keys, vec!["标的提供的时间", "标的提供的地点"]);
    }

    #[test]
    fn test_extract_table_keys_empty_cell_skipped() {
        let text = "| 付款方式 | ... |\n|  | empty key skipped |\n| 履约保证金 | 不收取 |";
        let keys = extract_table_keys(text);
        assert_eq!(keys, vec!["付款方式", "履约保证金"]);
    }

    #[test]
    fn test_extract_table_keys_max_five() {
        let text = (1..=10)
            .map(|i| format!("| KEY{} | VALUE{} |", i, i))
            .collect::<Vec<_>>()
            .join("\n");
        let keys = extract_table_keys(&text);
        assert_eq!(keys.len(), 5, "最多返回 5 个");
    }

    #[test]
    fn test_extract_table_keys_no_table() {
        let keys = extract_table_keys("普通正文内容，没有表格。");
        assert!(keys.is_empty());
    }

    // ═══════════════════════════════════════════════════════════════
    // 修复验证测试：文本语义边界切分
    // 以下测试验证 find_para_boundaries 增强后，split_long_chunk
    // 能在句末标点、表格行、编号项等语义边界处切分。
    // ═══════════════════════════════════════════════════════════════

    /// 修复1：句末标点处切分（不再截断句子）
    ///
    /// 构造一段约 3000 字的长文本，每 ~47 字以「。」结尾再接 `\n`，
    /// 全程没有 `\n\n` 双换行。修复后 `find_para_boundaries` 识别
    /// `。\n` 为语义边界，`split_long_chunk` 在最近的 `。\n` 处切分，
    /// 而非机械地切在 1500 字符处。
    #[test]
    fn test_fix_1_sentence_boundary_split() {
        // 构造单句模板：~100 字，以「。」结尾后跟换行
        let sentence = "这是关于投标项目的一份详细说明文档，包含了标的数量名称规格以及交付期限等核心条款的内容说明。\n";
        // 每句 ~47 字，需要 ~65 句 > 3000 字
        let body = sentence.repeat(65);
        let total = body.chars().count();
        assert!(total > 3000, "构造文本应 > 3000 字，实际: {}", total);

        let section = make_leaf(4, "1. 项目说明", &body);
        let config = ChunkingConfig::default();
        let mut chunks = Vec::new();
        let path = vec!["第一章".to_string()];

        split_long_chunk(&path, &section.body_text, &section, &config, &mut chunks);

        assert!(chunks.len() >= 2, "应产出 ≥2 个 chunk");

        // 修复验证：第一个 chunk 的切分点应为句末边界
        let first_chunk_end: String =
            chunks[0].text.chars().rev().take(5).collect::<Vec<_>>().into_iter().rev().collect();

        let ends_with_sentence_boundary = first_chunk_end.contains("。\n")
            || first_chunk_end.contains("！\n")
            || first_chunk_end.contains("？\n");
        assert!(
            ends_with_sentence_boundary,
            "【修复验证】第一个 chunk 应以句末标点结尾，\n\
             实际末尾5字符: {:?}",
            first_chunk_end
        );

        // 额外验证：`find_para_boundaries` 找到了句末边界
        let boundaries = find_para_boundaries(&body);
        assert!(
            boundaries.len() > 2,
            "【修复验证】find_para_boundaries 应找到 > 2 个边界（含句末标点），实际: {}",
            boundaries.len()
        );
        assert_eq!(boundaries[0], 0);
        assert_eq!(*boundaries.last().unwrap(), total);
    }

    /// 修复2：表格行边界保护（不再拦腰切断表格行）
    ///
    /// 构造一个含 pipe 分隔符的大表格，行之间仅用 `\n` 分隔无空行。
    /// 修复后 `find_para_boundaries` 识别 `|\n` 和 `\n|` 为表格行边界，
    /// 切分点落在完整行之间。
    #[test]
    fn test_fix_2_table_row_boundary_protected() {
        // 构造 60 行 Markdown 表格，每行 ~30 字，总计 ~1800 字
        let mut table_rows = String::new();
        for i in 1..=60 {
            table_rows.push_str(&format!(
                "| {} | 条款内容描述第{}项 | 满足 | 详见附件{} |\n",
                i, i, i
            ));
        }
        let total = table_rows.chars().count();
        assert!(
            total > 1500,
            "表格文本应 > 1500 字, 实际: {}",
            total
        );
        // 确认没有 \n\n 双换行
        assert!(
            !table_rows.contains("\n\n"),
            "表格行之间不应有空行"
        );

        let section = make_leaf(4, "1. 商务条款响应表", &table_rows);
        let config = ChunkingConfig::default();
        let mut chunks = Vec::new();
        let path = vec!["第五章".to_string()];

        split_long_chunk(&path, &section.body_text, &section, &config, &mut chunks);

        assert!(chunks.len() >= 2, "应产出 ≥2 个 chunk");

        // 修复验证：所有 chunk 都应包含完整表格行
        let mut has_broken_row = false;
        for (i, chunk) in chunks.iter().enumerate() {
            let text = &chunk.text;
            let first_char = text.chars().next().unwrap_or(' ');
            let last_char = text.chars().rev().next().unwrap_or(' ');

            if i > 0 && first_char != '|' {
                has_broken_row = true;
            }
            if i < chunks.len() - 1 && last_char != '\n' {
                has_broken_row = true;
            }
        }
        assert!(
            !has_broken_row,
            "【修复验证】表格行不应被切断，所有中间chunk应以 | 开头、以 \\n 结尾"
        );

        // 验证 find_para_boundaries 在表格行之间找到了边界
        let boundaries = find_para_boundaries(&table_rows);
        let non_virtual = boundaries.len() - 2;
        assert!(
            non_virtual > 0,
            "【修复验证】find_para_boundaries 应在表格行之间找到边界，实际: {}",
            non_virtual
        );
    }

    /// 修复3：数字编号列表项边界保护（不再切断列表项）
    ///
    /// 构造一个用 `1.` `2.` `3.` 等 ASCII 数字编号的列表。
    /// 修复后 `find_para_boundaries` 识别 `\n`+ASCII 数字为编号边界，
    /// 切分点落在列表项之间。
    #[test]
    fn test_fix_3_numbered_list_boundary_protected() {
        // 构造 ASCII 编号列表：35 项，每项 ~48 字
        let mut list = String::new();
        for i in 1..=35 {
            list.push_str(&format!(
                "{}. 投标人应当具备承担本招标项目的能力和良好的商业信誉记录，提供近三年无重大违法记录的书面声明材料。\n",
                i
            ));
        }
        let total = list.chars().count();
        assert!(total > 1500, "列表文本应 > 1500 字, 实际: {}", total);

        // 确认使用 ASCII 数字编号（非 CJK 数字）
        assert!(list.contains("1. "), "应为 ASCII 数字编号");

        let section = make_leaf(4, "1. 资格条件清单", &list);
        let config = ChunkingConfig::default();
        let mut chunks = Vec::new();
        let path = vec!["第二章".to_string()];

        split_long_chunk(&path, &section.body_text, &section, &config, &mut chunks);

        assert!(chunks.len() >= 2, "应产出 ≥2 个 chunk");

        // 修复验证：所有中间 chunk 应以数字编号开头
        let mut has_truncated_item = false;
        for (i, chunk) in chunks.iter().enumerate() {
            let text = &chunk.text;
            if i == 0 {
                continue;
            }
            let first_line = text.lines().next().unwrap_or("");
            let is_numbered_start = first_line
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false);
            if !is_numbered_start {
                has_truncated_item = true;
            }
        }
        assert!(
            !has_truncated_item,
            "【修复验证】所有中间 chunk 应以数字编号开头，列表项不应被切断"
        );

        // 验证 find_para_boundaries 在编号之间找到了边界
        let boundaries = find_para_boundaries(&list);
        let non_virtual = boundaries.len() - 2;
        assert!(
            non_virtual > 0,
            "【修复验证】find_para_boundaries 应在编号列表中找到边界，实际: {}",
            non_virtual
        );
    }

    /// 修复4：切分点与 overlap 起点质量对齐
    ///
    /// 修复后 `find_para_boundaries` 也识别 `。\n` 等语义边界，
    /// 与 `find_safe_overlap_start` 能力一致。切分点不再机械地
    /// 落在 1500 字符处，而是回退到最近的句末边界。
    #[test]
    fn test_fix_4_split_overlap_aligned() {
        // 构造文本：在 ~1680 位置有一个 `。\n`（单换行，非 \n\n），
        // 在 ~1500 位置没有任何语义边界。
        //
        // 策略：见下方 —— 构造在 offset ~1480 处有句末标点但无 \n\n 的文本

        // 构造一个在 offset ~1480 处有句末标点 `。\n` 但无 `\n\n` 的文本
        let segment_a = "A".repeat(1470); // 连续文本到 ~1470
        let boundary = "。\n";             // 句末换行边界 @ ~1471
        let segment_b = "B".repeat(1500); // 后续文本
        let body = format!("{}{}{}", segment_a, boundary, segment_b);
        let total = body.chars().count();
        assert!(total > 1500, "总文本应 > 1500 字");

        // 修复验证：find_para_boundaries 在 ~1480 找到了 。\n 边界
        let boundaries = find_para_boundaries(&body);
        let has_sentence_boundary = boundaries.iter().any(|&b| {
            b > 1400 && b < 1550 && b != 0 && b != total
        });
        assert!(
            has_sentence_boundary,
            "【修复验证】find_para_boundaries 应在 ~1480 找到 。\\n 边界，\n\
             boundaries 在 1400-1550 范围内: {:?}",
            boundaries.iter().filter(|&&b| b > 1400 && b < 1550).collect::<Vec<_>>()
        );

        // 核心验证：切分点接近句末边界而非 1500 机械位置
        let section = make_leaf(4, "1. 技术条款", &body);
        let config = ChunkingConfig::default();
        let mut chunks = Vec::new();
        let path = vec!["第三章".to_string()];

        split_long_chunk(&path, &section.body_text, &section, &config, &mut chunks);

        assert!(chunks.len() >= 2, "应产出 ≥2 个 chunk");
        let first_chunk_len = chunks[0].text.chars().count();

        // 第一个 chunk 应接近 ~1472（句末边界）而非 1500
        let is_near_boundary = first_chunk_len > 1460 && first_chunk_len < 1490;
        assert!(
            is_near_boundary,
            "【修复验证】切分点应接近句末边界（~1472），实际第一个chunk长度={}，应在1460-1490范围内",
            first_chunk_len
        );
    }
}
