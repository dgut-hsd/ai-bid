//! 文档章节结构识别服务
//!
//! 本模块负责从 [`RawDocument`] 中识别章节层级结构。
//! 采用规则引擎方案：用正则匹配中文标书特有的标题编号模式，
//! 按模式类型确定层级，最终构建章节树。
//!
//! ## 标题模式 → 层级映射
//!
//! | 模式                          | 层级 | 示例                   |
//! |-------------------------------|------|------------------------|
//! | `第X章`                       | 1    | 第一章 磋商邀请        |
//! | `第X节`                       | 2    | 第一节 项目概况        |
//! | `一、二、三、`                 | 2    | 一、项目概述           |
//! | `（一）（二）`                 | 3    | （一）资格要求         |
//! | `1. 2. 3.` (短标题)           | 4    | 1. 供应商资格          |
//! | `(1) (2)` / `（1）（2）`       | 5    | （1）营业执照          |
//! | `第X条`                       | 4    | 第九条 工程支付        |
//!
//! ## 过滤规则
//!
//! - 纯数字行（页码）→ 剔除
//! - 匹配行过长（> 80 字）→ 降级（可能是条款正文而非标题）
//! - 同一行匹配多个模式 → 取层级最高的

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::LazyLock;

use crate::domain::raw_document::{BlockType, RawDocument, RawPage, RawTable};
#[cfg(test)]
use crate::paths::data_path_str;

// ─── 标题模式定义 ─────────────────────────────────────────────

/// 标题匹配模式。patterns 按优先级排列，靠前的优先匹配。
static HEADING_PATTERNS: LazyLock<Vec<HeadingPattern>> = LazyLock::new(|| {
    vec![
        // Level 1: 第X部分（标书顶层结构）
        HeadingPattern {
            pattern_type: "part",
            level: 1,
            regex: Regex::new(r"^第[一二三四五六七八九十百千]+部分").expect("part regex"),
        },
        // Level 1: 第X章
        HeadingPattern {
            pattern_type: "chapter",
            level: 1,
            regex: Regex::new(r"^第[一二三四五六七八九十百千]+章").expect("chapter regex"),
        },
        // Level 2: 第X节
        HeadingPattern {
            pattern_type: "section",
            level: 2,
            regex: Regex::new(r"^第[一二三四五六七八九十百千]+节").expect("section regex"),
        },
        // Level 2: 中文序号标题 (一、二、三、...)
        HeadingPattern {
            pattern_type: "cjk_numbered",
            level: 2,
            regex: Regex::new(r"^[一二三四五六七八九十百千]+[、.．]\s*\S")
                .expect("cjk_numbered regex"),
        },
        // Level 3: 括号中文序号 （一）（二）...
        HeadingPattern {
            pattern_type: "paren_cjk",
            level: 3,
            regex: Regex::new(r"^[（(][一二三四五六七八九十百千]+[）)]\s*\S")
                .expect("paren_cjk regex"),
        },
        // Level 4: 数字序号 (1. 2、3) ...) — 要求后跟非空且标题短
        HeadingPattern {
            pattern_type: "digit_dot",
            level: 4,
            regex: Regex::new(r"^\d+[.、)）]\s*\S").expect("digit_dot regex"),
        },
        // Level 5: 括号数字 （1）（2）(1) (2) ...
        HeadingPattern {
            pattern_type: "paren_digit",
            level: 5,
            regex: Regex::new(r"^[（(]\d+[）)]\s*\S").expect("paren_digit regex"),
        },
        // Level 4: 第X条 (合同条款)
        HeadingPattern {
            pattern_type: "article",
            level: 4,
            regex: Regex::new(r"^第[一二三四五六七八九十百千]+条").expect("article regex"),
        },
    ]
});

struct HeadingPattern {
    pattern_type: &'static str,
    level: u8,
    regex: Regex,
}

// ─── 行内标题拆分 ─────────────────────────────────────────────

/// 行内标题拆分正则：检测右括号后紧跟的数字编号。
///
/// 匹配 `)` 或 `）` 后紧跟的 `数字.` / `数字、` / `数字)` / `数字）` 模式。
/// 用于处理 PDF 提取中标题与前文被合并到同一行的情况。
///
/// # 示例
///
/// ```text
/// 输入: "采购包1（...二期））1.主要商务要求"
/// 输出: ["采购包1（...二期））", "1.主要商务要求"]
/// ```
static INLINE_HEADING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[)）](\d+[.、)）])").expect("inline heading regex"));

/// 将行内标题从前置内容中拆分出来。
///
/// 遍历行中每个 `)数字.` / `)数字、` 模式，在右括号后将行切开。
/// 拆分出的标题行（以数字编号开头）会被后续的 [`HEADING_PATTERNS`] 正常匹配。
///
/// 无匹配时返回原行（单元素 Vec）。
fn split_inline_headings(line: &str) -> Vec<String> {
    let matches: Vec<(usize, usize)> = INLINE_HEADING_RE
        .find_iter(line)
        .map(|m| {
            // 右括号的字节位置（也是 split 点之后的位置）
            let paren_byte_start = m.start();
            let paren_char = line[paren_byte_start..].chars().next().unwrap();
            let paren_byte_end = paren_byte_start + paren_char.len_utf8();
            // 标题数字的起始位置 = 右括号之后
            let heading_byte_start = paren_byte_end;
            (paren_byte_end, heading_byte_start)
        })
        .collect();

    if matches.is_empty() {
        return vec![line.to_string()];
    }

    let mut result = Vec::new();
    let mut last_start = 0;

    for (prefix_end, heading_start) in &matches {
        // 前缀部分（含右括号）
        if *prefix_end > last_start {
            let prefix = line[last_start..*prefix_end].to_string();
            if !prefix.trim().is_empty() {
                result.push(prefix);
            }
        }
        last_start = *heading_start;
    }

    // 最后一个标题（从 heading_start 到行尾）
    if last_start < line.len() {
        let remainder = line[last_start..].trim().to_string();
        if !remainder.is_empty() {
            result.push(remainder);
        }
    }

    if result.is_empty() {
        vec![line.to_string()]
    } else {
        result
    }
}

// ─── 输出数据结构 ─────────────────────────────────────────────

/// 一个章节节点，可嵌套子节点形成树。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    /// 层级深度 (1=章, 2=节, 3=小节, ...)
    pub level: u8,
    /// 标题文本（已清洗）
    pub title: String,
    /// 匹配的标题模式类型
    pub pattern: String,
    /// 起始页码 (0-based) — 标题所在页
    pub page_start: usize,
    /// 结束页码 (0-based，包含) — section 子树涵盖的最大页码
    pub page_end: usize,
    /// 本节包含的所有 block ID（用于回溯高亮）
    pub block_ids: Vec<String>,
    /// body_text 实际来源的起始页 (0-based)。
    /// 对于叶子 section，通常等于 page_start；
    /// 对于容器 section，是引文/说明文字的实际起始页，
    /// 可远小于 page_end（子节点页码范围）。
    #[serde(default)]
    pub body_page_start: usize,
    /// body_text 实际来源的结束页 (0-based)。
    #[serde(default)]
    pub body_page_end: usize,
    /// 本节的主体文本内容（不含标题行本身），从关联 blocks 中提取。
    /// 包含子章节标题行，但不包含子章节标题之下的正文。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body_text: String,
    /// 子章节
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Section>,
}

/// sectionize 的完整输出。
#[derive(Debug, Serialize, Deserialize)]
pub struct SectionizeOutput {
    pub document_id: String,
    pub source_path: String,
    /// 顶层章节列表
    pub sections: Vec<Section>,
    /// 统计信息
    pub stats: SectionizeStats,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SectionizeStats {
    /// 总章节数（含嵌套）
    pub total_sections: usize,
    /// 各级别数量
    pub level_counts: std::collections::HashMap<u8, usize>,
    /// 未归属到任何 section 的 block 数量
    pub orphan_blocks: usize,
}

// ─── 内部候选结构 ─────────────────────────────────────────────

/// 扫描到的标题候选（中间数据）。
#[derive(Debug, Clone)]
struct HeadingCandidate {
    /// 层级
    level: u8,
    /// 匹配的标题文本行
    title: String,
    /// 标题模式类型
    pattern: &'static str,
    /// 所在页码 (0-based)
    page: usize,
    /// 所在 block 的 ID
    block_id: String,
}

// ─── 主入口 ──────────────────────────────────────────────────

/// 从 RawDocument 中提取章节树。
pub fn sectionize(raw: &RawDocument) -> SectionizeOutput {
    // 1. 收集所有 block 及其所属页面
    let all_blocks: Vec<(&crate::domain::raw_document::RawBlock, usize)> = raw
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter().map(move |b| (b, page.page_index)))
        .collect();

    if all_blocks.is_empty() {
        return SectionizeOutput {
            document_id: raw.document_id.clone(),
            source_path: raw.source_path.clone(),
            sections: Vec::new(),
            stats: SectionizeStats {
                total_sections: 0,
                level_counts: std::collections::HashMap::new(),
                orphan_blocks: 0,
            },
        };
    }

    // 2. 扫描所有 block 的文本行，提取标题候选
    let mut candidates: Vec<HeadingCandidate> = Vec::new();

    for (block, page_idx) in &all_blocks {
        // ★ P1: 行内标题预拆分 — 将 "）1.主要商务要求" 拆为独立行
        let expanded_lines: Vec<String> =
            block.text.lines().flat_map(split_inline_headings).collect();

        let mut block_has_candidate = false;

        for line in &expanded_lines {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // 过滤 PDF 噪声行（页码、"第X页共Y页"、控制字符）
            if is_page_noise(line) {
                continue;
            }

            // 尝试匹配所有模式，取第一个命中的。
            // 若原始行无法匹配，尝试去除强调符号（★▲● 等）后再次匹配，
            // 此时标题仍取原始行，以保留重要性标记。
            let stripped = strip_emphasis_prefix(line);
            let try_lines: [(&str, bool); 2] = [
                (line, false),                           // 原始行，匹配位置即标题起始
                (stripped, stripped.len() < line.len()), // 去符号版，命中则用原始行整体作标题
            ];

            let mut found = false;
            for (test_line, use_original_as_title) in &try_lines {
                if found {
                    break;
                }
                for pattern in HEADING_PATTERNS.iter() {
                    if let Some(mat) = pattern.regex.find(test_line) {
                        let title = if *use_original_as_title {
                            // 去符号版本匹配成功 → 取原始行整体（保留 ★ 等标记）
                            line.to_string()
                        } else {
                            test_line[mat.start()..].to_string()
                        };

                        // 标题长度上限过滤：过长的"标题"大概率是正文误匹配
                        // 层级越高（数字越小）标题应越短
                        let max_title_len = match pattern.level {
                            1 => 40, // 章/部分标题 ≤ 40 字符
                            2 => {
                                // cjk_numbered 易将法律条款长句误匹配为标题
                                // （如 "一、《深圳经济特区政府采购条例》第五十七条..."）
                                // 真实的中文序号标题（"一、技术要求"）均 ≤ 25 字
                                if pattern.pattern_type == "cjk_numbered" {
                                    25
                                } else {
                                    40
                                }
                            }
                            3 => 60, // 括号中文序号 ≤ 60 字符
                            _ => 40, // Level 4+ 数字/条款序号 ≤ 40 字符
                        };
                        if title.chars().count() > max_title_len {
                            continue;
                        }

                        // 规则 A：句末标点排除 — Level 4 digit_dot 标题含 。！？ → 跳过
                        // 中文完整句子必然以句号结尾，而真实标题不会。
                        // 精确打击被误匹配的完整句子（如 "1.1本招标文件适用于..."）
                        if pattern.pattern_type == "digit_dot" && title.contains(['。', '！', '？'])
                        {
                            continue;
                        }

                        candidates.push(HeadingCandidate {
                            level: pattern.level,
                            title,
                            pattern: pattern.pattern_type,
                            page: *page_idx,
                            block_id: block.id.clone(),
                        });
                        found = true;
                        block_has_candidate = true;
                        break; // 一行只匹配一个模式
                    }
                }
            }
        }

        // ★ A2: 无编号标题 — 利用 PDF 提取器的 block type 信号
        // 如果 block 被标注为 heading 但所有行均未匹配任何编号标题模式，
        // 将首行短文本作为 plain_heading 候选。
        // 典型场景："付款方式""验收要求" 等无编号的表格列标题。
        if !block_has_candidate
            && block.block_type == BlockType::Heading
            && let Some(first_line) = block
                .text
                .lines()
                .map(|l| l.trim())
                .find(|l| !l.is_empty() && !is_page_noise(l))
        {
            let char_count = first_line.chars().count();
            // 仅接受短文本（≤ 30 字符），避免将长段落误判为标题
            if (2..=30).contains(&char_count) {
                candidates.push(HeadingCandidate {
                    level: 5, // 最低层，挂到最近的上级 section 下
                    title: first_line.to_string(),
                    pattern: "plain_heading",
                    page: *page_idx,
                    block_id: block.id.clone(),
                });
            }
        }
    }

    // 2.5 启发式过滤：排除封面法律条款/提示列表伪装成的伪章节
    //     标书封面后常出现《采购条例》等法律条文列举（如"（一）...；"），
    //     其编号模式与真实章节标题相同但语义是列表项而非章节结构。
    let candidates = filter_pseudo_section_candidates(candidates);

    // 2.6 链式验证：按编号家族+深度分组，验证编号连续性。
    //     真实章节标题（如 "1. 供应商资格"、"2. 项目概况"）形成连续编号链；
    //     正文编号内容（如 "1.项目编号：0724-..."）是孤立项，移除。
    //     参考 Oracle 专利 US 11468346 的"链式验证"方法。
    let candidates = validate_numbering_chains(candidates);

    // 2.7 TOC 目录页检测：同一页出现 ≥3 个 level-1 候选且均无 body_text
    //     → 判定为目录页 → 移除这些候选，避免产生幽灵 Section。
    //     参考 LlamaIndex 的"层级密度检测"方法。
    let candidates = filter_toc_page_candidates(candidates, &all_blocks);

    // 3. 构建章节树
    let (sections, orphan_blocks) = build_section_tree(&candidates, &all_blocks);

    // 5. 统计
    let total_sections = count_sections(&sections);
    let mut level_counts: std::collections::HashMap<u8, usize> = std::collections::HashMap::new();
    count_levels(&sections, &mut level_counts);

    SectionizeOutput {
        document_id: raw.document_id.clone(),
        source_path: raw.source_path.clone(),
        sections,
        stats: SectionizeStats {
            total_sections,
            level_counts,
            orphan_blocks,
        },
    }
}

// ─── 辅助函数 ────────────────────────────────────────────────

/// 判断一行是否为 PDF 噪声（页码行、私有区控制字符等）。
///
/// 过滤三类噪声：
/// 1. 纯数字短行（原有逻辑，如 "1"、"92"）
/// 2. "第X页共Y页" 格式的页码行（含残缺变体如 "78第72页共页"）
/// 3. 含 Unicode 私有区字符（U+E000–U+F8FF）的行，这些是 PDF 渲染产生的控制字符（如 ）
fn is_page_noise(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return true;
    }

    // 1. 纯数字短行（页码），长度 ≤ 3 且全为 ASCII 数字/空格
    if trimmed.len() <= 3 && trimmed.chars().all(|c| c.is_ascii_digit() || c == ' ') {
        return true;
    }

    // 2. "第X页共Y页" 格式页码行
    //    匹配模式: [可选前缀数字] "第" 数字 "页共" [可选数字] "页" [可选后缀]
    //    使用简单的子串匹配：含 "第" + "页" + "共" + "页" 的结构
    if trimmed.contains('第') && trimmed.contains('页') && trimmed.contains("共") {
        // 进一步确认非正文：行长度 ≤ 20 字符（正常页码行 < 15 字符）
        if trimmed.chars().count() <= 20 {
            return true;
        }
    }

    // 3. 含 Unicode 私有区字符（U+E000–U+F8FF），如 PDF bullet 符号 
    if trimmed.contains(|c: char| ('\u{E000}'..='\u{F8FF}').contains(&c)) {
        return true;
    }

    false
}

/// 启发式过滤：排除封面法律条款/提示列表伪装成的伪章节。
///
/// 标书封面后常出现《采购条例》等法律条文列举（如"（一）在采购活动中应当回避而未回避的；"），
/// 以及"温馨提示"列表（如"二、为避免因迟到而失去投标资格，请适当提前到达。"）。
/// 这些文本的编号模式与真实章节标题相同，但语义是列表项而非章节结构。
///
/// 过滤规则（仅在第一个 Level-1 候选出现之前生效）：
/// 1. 连续 ≥3 个 `paren_cjk`，全部以 `；` 或 `。` 结尾 → 法律条款列举 → 移除
/// 2. 单个 `cjk_numbered` 以 `。！？` 结尾 → 完整句子伪标题 → 移除
fn filter_pseudo_section_candidates(
    mut candidates: Vec<HeadingCandidate>,
) -> Vec<HeadingCandidate> {
    if candidates.is_empty() {
        return candidates;
    }

    // 找到第一个 level-1 候选的索引（"第X部分"/"第X章" 等）
    let first_l1 = match candidates.iter().position(|c| c.level == 1) {
        Some(idx) => idx,
        None => return candidates, // 无 level-1 章节，保守不执行过滤
    };

    // 仅对第一个 level-1 之前的候选执行过滤
    if first_l1 == 0 {
        return candidates;
    }

    let mut remove_indices: Vec<usize> = Vec::new();

    // ── 规则 1: 连续 paren_cjk 组（≥3 个），全部以 ；或。 结尾 → 法律条款枚举 ──
    let mut i = 0;
    while i < first_l1 {
        if candidates[i].pattern == "paren_cjk" {
            let group_start = i;
            let mut group_end = i;
            while group_end < first_l1 && candidates[group_end].pattern == "paren_cjk" {
                group_end += 1;
            }
            let group_size = group_end - group_start;
            if group_size >= 3 {
                // 比例匹配而非全量匹配：PDF 提取可能导致个别长标题被截断，
                // 丢失结尾的 ；或。，因此用 ≥70% 阈值容忍提取噪声。
                let clause_count = candidates[group_start..group_end]
                    .iter()
                    .filter(|c| {
                        let t = c.title.trim();
                        t.ends_with('；') || t.ends_with('。')
                    })
                    .count();
                let ratio = clause_count as f64 / group_size as f64;
                if ratio >= 0.7 {
                    for j in group_start..group_end {
                        remove_indices.push(j);
                    }
                }
            }
            i = group_end;
        } else {
            i += 1;
        }
    }

    // ── 规则 2: cjk_numbered 以 。！？ 结尾 → 完整句子，非章节标题 ──
    //           （真实标题如 "一、技术要求" 不会以句末标点结尾）
    for (idx, candidate) in candidates.iter().enumerate().take(first_l1) {
        if candidate.pattern == "cjk_numbered" {
            let t = candidate.title.trim();
            if t.ends_with('。') || t.ends_with('！') || t.ends_with('？') {
                remove_indices.push(idx);
            }
        }
    }

    // 按索引降序安全移除
    remove_indices.sort_unstable();
    remove_indices.dedup();
    for &idx in remove_indices.iter().rev() {
        candidates.remove(idx);
    }

    candidates
}

// ─── 链式验证（Oracle 专利 US 11468346 方法）────────────────────

/// 对 `digit_dot` 和 `paren_digit` 候选按编号家族+深度分组，
/// 验证编号连续性。孤立或断链的低置信度候选 → 移除。
///
/// # 核心思想
///
/// 真实章节标题形成跨页连续编号链（"1.", "2.", "3." ...），
/// 而正文编号（"1.项目编号：0724-..."）是孤立的、不成链的。
///
/// # 算法
///
/// 1. 按 (pattern_type, rank) 分组
/// 2. 每组内按文档位置排序
/// 3. 检测连续性：成员数 ≥2 且编号递增 → 保留组；孤立成员 → 移除
/// 4. 额外信号：标题过长（>35 chars for digit_dot / >50 for paren_digit）的孤立候选
///    → 确认为正文内容泄漏 → 移除
fn validate_numbering_chains(mut candidates: Vec<HeadingCandidate>) -> Vec<HeadingCandidate> {
    if candidates.is_empty() {
        return candidates;
    }

    // 只对 digit_dot (level 4) 和 paren_digit (level 5) 做链式验证
    // 更高层级的 pattern (part/chapter/cjk_numbered/paren_cjk) 由其他过滤器处理
    let target_patterns: &[&str] = &["digit_dot", "paren_digit"];

    // 为每个候选分配全局索引（用于后续稳定排序和移除）
    // 提取编号序列和 rank
    #[derive(Debug, Clone)]
    struct IndexedCandidate {
        global_idx: usize,
        rank: usize,          // 编号深度: "1." = 1, "1.1" = 2
        num_prefix: Vec<u32>, // 编号序列: "1.2.3" → [1, 2, 3]
        title_len: usize,     // 标题字符数
    }

    let mut indexed: Vec<IndexedCandidate> = Vec::new();
    for (idx, c) in candidates.iter().enumerate() {
        if !target_patterns.contains(&c.pattern) {
            continue;
        }
        let (rank, num_prefix) = extract_numbering_info(&c.title, c.pattern);
        indexed.push(IndexedCandidate {
            global_idx: idx,
            rank,
            num_prefix,
            title_len: c.title.chars().count(),
        });
    }

    if indexed.is_empty() {
        return candidates;
    }

    // 按 (pattern_type, rank) 分组
    use std::collections::HashMap;
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new(); // key → Vec<index_into_indexed>
    for (i, ic) in indexed.iter().enumerate() {
        let key = format!("{}_{}", candidates[ic.global_idx].pattern, ic.rank);
        groups.entry(key).or_default().push(i);
    }

    let mut remove_indices: Vec<usize> = Vec::new();

    for member_indices in groups.values() {
        if member_indices.len() < 2 {
            // 孤立候选（该 pattern+rank 组只有 1 个成员）→ 额外检查
            for &mi in member_indices {
                let ic = &indexed[mi];
                let c = &candidates[ic.global_idx];

                // 信号1：标题过长 → 大概率是正文内容
                let long_title = match c.pattern {
                    "digit_dot" => ic.title_len > 35,
                    "paren_digit" => ic.title_len > 50,
                    _ => false,
                };

                // 信号2：标题含冒号 → 正文特征（如 "1.项目编号：..."）
                let has_colon = c.title.contains('：') || c.title.contains(':');

                // 信号3（V6.1）：标题以介词结尾 → 半截句子，非标题
                // "2、工程交接后，供应商应按照以下要求对" → 完整句应为"对施工场地进行清理"
                let ends_with_preposition = {
                    let t = c.title.trim();
                    // 中文单字介词：对、为、在、按、由、以、与、和、从、向、被、把、将、就、至
                    t.ends_with('对')
                        || t.ends_with('为')
                        || t.ends_with('在')
                        || t.ends_with('按')
                        || t.ends_with('由')
                        || t.ends_with('以')
                        || t.ends_with('与')
                        || t.ends_with('和')
                        || t.ends_with('从')
                        || t.ends_with('向')
                        || t.ends_with('被')
                        || t.ends_with('把')
                        || t.ends_with('将')
                        || t.ends_with('就')
                        || t.ends_with('至')
                };

                if long_title || has_colon || ends_with_preposition {
                    remove_indices.push(ic.global_idx);
                }
            }
            continue;
        }

        // 组内按编号序列排序
        let mut sorted_members: Vec<usize> = member_indices.clone();
        sorted_members.sort_by_key(|&mi| &indexed[mi].num_prefix);

        // 验证编号连续性：检查相邻成员的编号是否递增
        let mut chain_breaks: Vec<usize> = Vec::new(); // 断链成员的 global_idx
        for w in sorted_members.windows(2) {
            let prev = &indexed[w[0]];
            let curr = &indexed[w[1]];

            // 检查 prev.num_prefix < curr.num_prefix (字典序)
            if prev.num_prefix >= curr.num_prefix {
                // 编号不连续或重复 → mark curr as suspicious
                chain_breaks.push(curr.global_idx);
            }
        }

        // 对断链成员做二次判断
        for &gb_idx in &chain_breaks {
            let ic = indexed.iter().find(|x| x.global_idx == gb_idx).unwrap();
            let c = &candidates[gb_idx];

            // 以句末标点结尾 → 完整句子，确认为正文泄漏
            let ends_with_sentence = {
                let t = c.title.trim();
                t.ends_with('。') || t.ends_with('！') || t.ends_with('？')
            };

            // 标题过长 → 大概率正文
            let too_long = match c.pattern {
                "digit_dot" => ic.title_len > 40,
                "paren_digit" => ic.title_len > 55,
                _ => false,
            };

            // 标题以介词结尾 → 半截句子，非标题
            // "2、工程交接后，供应商应按照以下要求对" → 完整句应为"对施工场地进行清理"
            let ends_with_preposition = {
                let t = c.title.trim();
                t.ends_with('对')
                    || t.ends_with('为')
                    || t.ends_with('在')
                    || t.ends_with('按')
                    || t.ends_with('由')
                    || t.ends_with('以')
                    || t.ends_with('与')
                    || t.ends_with('和')
                    || t.ends_with('从')
                    || t.ends_with('向')
                    || t.ends_with('被')
                    || t.ends_with('把')
                    || t.ends_with('将')
                    || t.ends_with('就')
                    || t.ends_with('至')
            };

            if ends_with_sentence || too_long || ends_with_preposition {
                remove_indices.push(gb_idx);
            }
        }
    }

    // 安全移除（降序）
    remove_indices.sort_unstable();
    remove_indices.dedup();
    for &idx in remove_indices.iter().rev() {
        candidates.remove(idx);
    }

    candidates
}

/// 从标题文本中提取编号信息：(rank, number_sequence)。
///
/// - `digit_dot`: "1." → rank=1, [1]; "1.2.3" → rank=3, [1,2,3]
/// - `paren_digit`: "（1）" → rank=2, [1]; "(2)" → rank=2, [2]
fn extract_numbering_info(title: &str, pattern: &str) -> (usize, Vec<u32>) {
    match pattern {
        "digit_dot" => {
            // 匹配开头的数字序列: "1.2.3" → [1,2,3], "1、" → [1]
            let prefix = title
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect::<String>();
            let nums: Vec<u32> = prefix
                .split('.')
                .filter_map(|s| s.parse::<u32>().ok())
                .collect();
            let rank = nums.len();
            (rank, nums)
        }
        "paren_digit" => {
            // 匹配括号内的数字: "（1）" → [1], "(2)" → [2]
            let inner: String = title
                .chars()
                .skip_while(|c| *c != '（' && *c != '(')
                .skip(1)
                .take_while(|c| c.is_ascii_digit())
                .collect();
            let num = inner.parse::<u32>().unwrap_or(0);
            (2, vec![num]) // paren_digit 默认 rank=2（嵌套在 digit_dot 之下）
        }
        _ => (1, vec![0]),
    }
}

// ─── TOC 目录页检测（LlamaIndex 层级密度检测方法）───────────────

/// 检测目录页产生的伪标题候选并移除。
///
/// # 判定条件
///
/// 同一页内，如果 level=1 的候选密度 ≥ 3 个，且这些候选的 block 均为
/// Heading 类型且该 block 是页面上唯一的 Heading（无 body text 跟随）→ TOC 页。
///
/// # 原理
///
/// 标书目录页将文档所有 "第X部分" 集中列在一页上，每个都是独立的 Heading block，
/// 不含正文。而真实章节标题分布在不同页，每页最多 1-2 个 level-1 标题，
/// 且标题后有 body text。
fn filter_toc_page_candidates(
    mut candidates: Vec<HeadingCandidate>,
    all_blocks: &[(&crate::domain::raw_document::RawBlock, usize)],
) -> Vec<HeadingCandidate> {
    if candidates.is_empty() {
        return candidates;
    }

    // 统计每页的 level-1 候选数量
    use std::collections::HashMap;
    let mut l1_per_page: HashMap<usize, Vec<usize>> = HashMap::new(); // page → Vec<candidate_idx>
    for (idx, c) in candidates.iter().enumerate() {
        if c.level == 1 {
            l1_per_page.entry(c.page).or_default().push(idx);
        }
    }

    let mut remove_indices: Vec<usize> = Vec::new();

    for (page, cand_indices) in &l1_per_page {
        // 阈值：同一页 ≥ 3 个 level-1 候选 → 疑似目录页
        if cand_indices.len() < 3 {
            continue;
        }

        // 进一步确认：检查这些候选所在的 block 周围是否有 body text
        // 目录页的 level-1 block 通常孤立（页面内无 body text 跟随）
        let page_block_ids: Vec<&str> = all_blocks
            .iter()
            .filter(|(_, p)| *p == *page)
            .map(|(b, _)| b.id.as_str())
            .collect();

        let toc_l1_count = cand_indices
            .iter()
            .filter(|&&ci| {
                let c = &candidates[ci];
                // 检查该候选的 block 在页面内是否是孤立的 Heading
                // （后续 block 都不是同级的 Paragraph body）
                let block_pos = page_block_ids.iter().position(|&id| id == c.block_id);
                match block_pos {
                    Some(pos) => {
                        // 该 block 之后还有 block → 检查是否有紧邻的 Paragraph
                        let has_body_after = page_block_ids[pos..].iter().any(|&id| {
                            all_blocks.iter().any(|(b, _)| {
                                b.id == id
                                    && b.block_type
                                        == crate::domain::raw_document::BlockType::Paragraph
                            })
                        });
                        // 目录页 level-1 标题后不应有紧邻的 Paragraph body
                        !has_body_after
                    }
                    None => true,
                }
            })
            .count();

        // 如果 ≥3 个 level-1 都是孤立的（无 body 跟随）→ 确认是目录页
        if toc_l1_count >= 3 {
            for &ci in cand_indices {
                remove_indices.push(ci);
            }
        }
    }

    // 安全移除（降序）
    remove_indices.sort_unstable();
    remove_indices.dedup();
    for &idx in remove_indices.iter().rev() {
        candidates.remove(idx);
    }

    candidates
}

/// 去除行首的强调符号（★▲●■ 等），用于标题模式匹配前的归一化。
///
/// 中文标书中常以这些符号标记重点条目，它们会干扰 `digit_dot` 等正则。
/// 去除后返回剩余文本；若无前缀符号则原样返回。
fn strip_emphasis_prefix(s: &str) -> &str {
    // 标书常见重点标注符号
    const EMPHASIS: &[char] = &[
        '★', '▲', '●', '■', '◆', '◎', '☆', '△', '▽', '◁', '▷', '◇', '□', '○', '✔', '☑', '❗', '✓',
    ];
    let trimmed = s.trim();
    match trimmed.chars().next() {
        Some(c) if EMPHASIS.contains(&c) => trimmed[c.len_utf8()..].trim_start(),
        _ => trimmed,
    }
}

/// 检查一行文本是否匹配任何标题模式（用于在正文提取中识别子标题边界）。
/// 使用与主扫描一致的过滤规则：标题过长视为正文误匹配。
fn matches_heading_pattern(line: &str) -> bool {
    for pattern in HEADING_PATTERNS.iter() {
        if let Some(mat) = pattern.regex.find(line) {
            let title = &line[mat.start()..];
            let max_title_len = match pattern.level {
                1 => 40,
                2 => {
                    if pattern.pattern_type == "cjk_numbered" {
                        25
                    } else {
                        40
                    }
                }
                3 => 60,
                _ => 40,
            };
            if title.chars().count() > max_title_len {
                continue;
            }
            // 规则 A：句末标点排除 — 完整句子误匹配的精确打击
            if pattern.pattern_type == "digit_dot" && title.contains(['。', '！', '？']) {
                continue;
            }
            return true;
        }
    }
    false
}

/// 从候选列表构建章节树。
///
/// 两阶段：先建扁平列表 + 父子索引关系，再递归组装树。
/// 返回 (root_sections, orphan_block_count)。
fn build_section_tree(
    candidates: &[HeadingCandidate],
    all_blocks: &[(&crate::domain::raw_document::RawBlock, usize)],
) -> (Vec<Section>, usize) {
    if candidates.is_empty() {
        return (Vec::new(), all_blocks.len());
    }

    // Phase 1: 创建所有 section 并记录父子关系
    let mut sections: Vec<Section> = Vec::new();
    let mut parent_of: Vec<Option<usize>> = Vec::new();
    let mut stack: Vec<usize> = Vec::new(); // 祖先链索引
    let mut assigned_blocks: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (i, candidate) in candidates.iter().enumerate() {
        let next_boundary = find_next_boundary(candidates, i);
        let block_ids =
            collect_blocks_between(all_blocks, candidate, next_boundary, &mut assigned_blocks);
        let page_end = block_ids
            .iter()
            .filter_map(|bid| find_block_page(all_blocks, bid))
            .max()
            .unwrap_or(candidate.page);

        let (body_text, body_page_start, body_page_end) =
            extract_section_body(candidate, next_boundary, all_blocks, &block_ids);
        // Level 1-2（章/节标题）本身即为完整标题，不检测截断
        let title_truncated =
            candidate.level >= 3 && is_title_truncated(&candidate.title, &body_text);

        // 如果标题被 PDF 折行截断，将续接正文合并回标题，
        // 避免"标题 + 正文"的人为割裂。
        let (final_title, final_body_text) = if title_truncated {
            let (merged_title, remaining_body) =
                merge_truncated_title(&candidate.title, &body_text);
            // 二次防御：合并后标题若含句末标点（。！？），说明"标题"
            // 实际是完整句子的前半段（如 "5）参加采购活动前3年内..." +
            // "。重大违法记录是指..."），而非真实被截断的标题。
            // 此时回退合并，保留原标题和完整 body_text。
            if merged_title.contains(['。', '！', '？']) {
                (candidate.title.clone(), body_text)
            } else {
                (merged_title, remaining_body)
            }
        } else {
            (candidate.title.clone(), body_text)
        };

        let section = Section {
            level: candidate.level,
            title: final_title,
            pattern: candidate.pattern.to_string(),
            page_start: candidate.page,
            page_end,
            block_ids,
            body_page_start,
            body_page_end,
            body_text: final_body_text,
            children: Vec::new(),
        };

        // 弹出栈中所有 level >= 当前 level 的祖先
        while let Some(&top_idx) = stack.last() {
            if sections[top_idx].level >= candidate.level {
                stack.pop();
            } else {
                break;
            }
        }

        let my_idx = sections.len();
        let my_parent = stack.last().copied();
        sections.push(section);
        parent_of.push(my_parent);
        stack.push(my_idx);
    }

    // Phase 2: 收集每个节点的子节点索引
    let n = sections.len();
    let mut children_of: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut root_indices: Vec<usize> = Vec::new();
    for (idx, p) in parent_of.iter().enumerate() {
        match p {
            Some(p) => children_of[*p].push(idx),
            None => root_indices.push(idx),
        }
    }

    // Phase 3: 递归构建树（从 Option<Section> 中 take）
    let mut opt_sections: Vec<Option<Section>> = sections.into_iter().map(Some).collect();
    let orphan_blocks = all_blocks.len() - assigned_blocks.len();

    let root_sections: Vec<Section> = root_indices
        .into_iter()
        .map(|root_idx| take_section_from_flat(&mut opt_sections, &children_of, root_idx))
        .collect();

    (root_sections, orphan_blocks)
}

/// 递归从扁平 Option 数组中取出 section 及其所有子孙。
fn take_section_from_flat(
    flat: &mut [Option<Section>],
    children_of: &[Vec<usize>],
    idx: usize,
) -> Section {
    let mut section = flat[idx].take().expect("section already taken");
    for &child_idx in &children_of[idx] {
        section
            .children
            .push(take_section_from_flat(flat, children_of, child_idx));
    }
    section
}

/// 找到下一个层级 ≤ 当前候选层级的标题候选（同级或更高级）。
/// 这标记了当前 section 的结束位置。
fn find_next_boundary(
    candidates: &[HeadingCandidate],
    current_idx: usize,
) -> Option<&HeadingCandidate> {
    let current = &candidates[current_idx];
    candidates[current_idx + 1..]
        .iter()
        .find(|c| c.level <= current.level)
}

/// 收集在两个候选之间的所有 block_ids。
fn collect_blocks_between(
    all_blocks: &[(&crate::domain::raw_document::RawBlock, usize)],
    start: &HeadingCandidate,
    end: Option<&HeadingCandidate>,
    assigned: &mut std::collections::HashSet<String>,
) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    let mut started = false;

    for (block, _page) in all_blocks {
        if block.id == start.block_id {
            started = true;
        }

        if started {
            ids.push(block.id.clone());
            assigned.insert(block.id.clone());
        }

        if let Some(end_candidate) = end
            && block.id == end_candidate.block_id
        {
            // 只有当 end 与 start 不在同一个 block 时，才弹出 end block
            // （end block 属于下一个 section）。
            // 如果 start 和 end 共享同一个 block，说明该 block 内存在多个标题，
            // 当前 section 的内容仍然包含在该 block 中，因此保留。
            if end_candidate.block_id != start.block_id {
                ids.pop();
                assigned.remove(&end_candidate.block_id);
            }
            break;
        }
    }

    ids
}

/// 从当前 section 关联的 blocks 中提取正文文本，同时追踪正文的实际页码范围。
///
/// 从 section 标题行之后开始收集，到下一个同级/上级标题出现时停止。
/// 正文中会保留子章节的标题行，但跳过页码等噪声行。
///
/// # 返回值
/// - `String`: 提取的正文文本（已 smart join）
/// - `usize`: body_text 实际起始页 (0-based)
/// - `usize`: body_text 实际结束页 (0-based)
fn extract_section_body(
    candidate: &HeadingCandidate,
    next_boundary: Option<&HeadingCandidate>,
    all_blocks: &[(&crate::domain::raw_document::RawBlock, usize)],
    block_ids: &[String],
) -> (String, usize, usize) {
    let block_id_set: std::collections::HashSet<&str> =
        block_ids.iter().map(|s| s.as_str()).collect();

    if block_id_set.is_empty() {
        return (String::new(), candidate.page, candidate.page);
    }

    let mut body_lines: Vec<String> = Vec::new();
    let mut body_page_start: usize = candidate.page;
    let mut body_page_end: usize = candidate.page;
    let mut found_title = false;
    let mut done = false;
    let mut first_body_page_set = false;

    for (block, page) in all_blocks {
        if done {
            break;
        }
        if !block_id_set.contains(block.id.as_str()) {
            continue;
        }

        for line in block.text.lines() {
            if done {
                break;
            }
            let trimmed = line.trim();

            // 跳过空行和 PDF 噪声（中文 PDF 块内换行均为物理折行伪影，不保留）
            if trimmed.is_empty() || is_page_noise(trimmed) {
                continue;
            }

            // 定位到当前 section 的标题行
            if !found_title {
                if trimmed == candidate.title.trim() {
                    found_title = true;
                }
                continue;
            }

            // 遇到任意子标题模式 → 停止（防止父 section 吞并子 section 正文）
            if matches_heading_pattern(trimmed) {
                done = true;
                break;
            }

            // 遇到下一边界标题 → 停止
            if let Some(end) = next_boundary
                && trimmed == end.title.trim()
            {
                done = true;
                break;
            }

            // 追踪正文来源的页码范围
            if !first_body_page_set {
                body_page_start = *page;
                first_body_page_set = true;
            }
            body_page_end = *page;

            body_lines.push(trimmed.to_string());
        }
    }

    // 去除尾部空行
    while body_lines.last().is_some_and(|l| l.is_empty()) {
        body_lines.pop();
    }

    let body_text = smart_join_body(&body_lines);
    (body_text, body_page_start, body_page_end)
}

/// 智能拼接 body lines。
///
/// 核心思路：用通用的"行首语义单元检测" + "行尾句子边界检测"，
/// 而非依赖特定关键词。使得方正排版 PDF 和标准 PDF 的表格/条目结构
/// 都能被正确保留，同时正常段落的 PDF 物理折行仍被无缝拼接。
///
/// 断行条件（满足任一即断）：
/// 1. 当前行是新的语义单元起始（数字开头、括号编号、特殊符号）
/// 2. 前一行以句子结束标点（。！？）结尾
///
/// 默认：PDF 物理折行，用 "" 无缝拼接。
fn smart_join_body(lines: &[String]) -> String {
    let mut result = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 && should_break_before(&lines[i - 1], line) {
            result.push('\n');
        }
        result.push_str(line);
    }
    result
}

/// 判断从前一行到当前行是否需要插入换行符。
///
/// 两套规则（满足任一即断行）：
/// - 规则1：当前行以新语义单元标记开头 → 独立条目/表行/编号条款
/// - 规则2：前一行以句子结束标点结尾 → 新句子开始
fn should_break_before(prev: &str, curr: &str) -> bool {
    let curr = curr.trim();
    if curr.is_empty() {
        return false;
    }

    // 规则1：新语义单元起始符
    if is_new_semantic_unit(curr) {
        return true;
    }

    // 规则2：前一行以句末标点结尾（仅 。！？ 三类无歧义标点；
    // 不包含 ：；，（因为它们常出现在标签-值对或从句中）
    let prev = prev.trim();
    if prev.ends_with(['。', '！', '？']) {
        return true;
    }

    // 默认：PDF 物理折行，无缝拼接
    false
}

/// 检测一行是否为"新语义单元"的起始。
///
/// 三类标记：
/// 1. ASCII 数字开头（覆盖 `digit_sep` 如 "1、" "2." 和 `digit_bare`
///    如 "1PVC管材" "1-1教育用房"）
/// 2. 括号编号开头（如 "（1）" "(2)" "（一）"）
/// 3. 特殊符号标记（▲ ★ ● ■ ◆）
fn is_new_semantic_unit(s: &str) -> bool {
    let c0 = s.chars().next().unwrap_or('\0');

    // 1. 数字开头 → 新编号条目 / 表格行
    if c0.is_ascii_digit() {
        return true;
    }

    // 2. 括号编号 → （1）（2）(1) (2)（一）（二）
    if c0 == '\u{FF08}' || c0 == '(' {
        // fullwidth left parenthesis （ 或 halfwidth (
        return true;
    }

    // 3. 特殊符号标记 → ▲ ★ ● ■ ◆
    if matches!(
        c0,
        '\u{25B2}' | '\u{2605}' | '\u{25CF}' | '\u{25A0}' | '\u{25C6}'
    ) {
        return true;
    }

    false
}

/// 检测一行是否以 ASCII 数字紧接 CJK 汉字开头（如 "4驻场要求"、"7质量保证"）。
/// 这是标书 PDF 提取中常见的独立编号条款模式。
/// 注意：此函数已不再被 `smart_join_body` 直接调用（改用通用的
/// `is_new_semantic_unit`），但保留作为独立检测工具供其他模块使用。
fn is_digit_cjk_start(s: &str) -> bool {
    let chars: Vec<char> = s.chars().take(2).collect();
    chars.len() == 2
        && chars[0].is_ascii_digit()
        && chars[1] >= '\u{4E00}'
        && chars[1] <= '\u{9FFF}'
}

/// 检测标题是否因 PDF 物理折行而被截断。
///
/// 调用方应仅对 Level >= 3 调用此函数（章/节标题本身即为完整标题）。
///
/// 判定条件：
/// 1. 标题不以句号（。！？）、冒号（：）、逗号（，）或括号结尾
/// 2. body_text 存在且首字符是 CJK 统一汉字或小写英文字母（续接特征）
/// 3. 新增：如果 title + body_text 第一个句子能拼成以 。！？ 结尾的完整句
///    （≤ 120 chars），说明 title 是完整句子的前半段而非被截断的标题 → 返回 false
fn is_title_truncated(title: &str, body_text: &str) -> bool {
    if body_text.is_empty() {
        return false;
    }

    // 短标题（< 15 字符）通常是完整标题，不需要续接检测
    if title.chars().count() < 15 {
        return false;
    }

    let title_last = title.chars().last().unwrap_or('\0');
    // 标题以这些字符结尾 → 大概率是完整句子，未截断
    // 注意：逗号（，,）不是句子结束标点，标题以逗号结尾通常是 PDF 折行截断
    if matches!(title_last, '。' | '！' | '？' | '）' | ')' | '：' | ':') {
        return false;
    }

    // ── 新增：句子完整性预判 ──
    // 如果 title + body_text 的第一个句子能拼成以 。！？ 结尾的完整句，
    // 说明 title 是完整句子的前半段（如 "5）参加采购活动前3年内..."），
    // 而非被 PDF 折行截断的真实标题 → 不触发 merge
    if let Some(end_byte) = body_text.find(['。', '！', '？']) {
        // 取到第一个句末标点（含）为止
        let end_char_len = body_text[end_byte..]
            .chars()
            .next()
            .map_or(0, |c| c.len_utf8());
        let first_sentence_end = end_byte + end_char_len;
        let combined_len = title.chars().count() + body_text[..first_sentence_end].chars().count();
        // 组合后不超过 120 字符且以句号结尾 → title 是句子前半段
        if combined_len <= 120 {
            return false;
        }
    }

    // 跳过 body_text 开头的空白字符
    let body_first = body_text
        .chars()
        .find(|c| !c.is_whitespace())
        .unwrap_or('\0');
    if body_first == '\0' {
        return false;
    }
    // body 首字符是 CJK 汉字、小写字母或 ASCII 数字 → 续接
    // ASCII 数字捕获 "4驻场要求"、"7质量保证" 等标书常见模式
    ('\u{4E00}'..='\u{9FFF}').contains(&body_first)
        || ('\u{3400}'..='\u{4DBF}').contains(&body_first)
        || body_first.is_ascii_lowercase()
        || body_first.is_ascii_digit()
}

/// 将被 PDF 物理折行截断的标题与正文首段进行合并。
///
/// 当标题行不以句子结束标点（。！？）结尾，且 body_text 首字符为 CJK
/// 续接内容时，将 body_text 中的续接行合并回标题，直到遇到：
/// 1. 句子结束标点（仅合并到第一个 。！？ 为止，不吞整行）
/// 2. 另一个标题模式匹配
/// 3. 独立编号条款（digit+CJK 开头，如 "4驻场要求"）
/// 4. 合并字符数超过上限（60 字符）
/// 5. body_text 耗尽
///
/// 返回 `(merged_title, remaining_body_text)`。
fn merge_truncated_title(title: &str, body_text: &str) -> (String, String) {
    if body_text.is_empty() {
        return (title.to_string(), String::new());
    }

    const MAX_MERGE_CHARS: usize = 60; // 合并字符上限

    let mut merged = title.to_string();
    let title_len = merged.chars().count();
    let mut remaining: Vec<&str> = Vec::new();
    let mut merge_done = false;

    for line in body_text.lines() {
        if merge_done {
            remaining.push(line);
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // 遇到另一个标题模式 → 停止合并，该行留给剩余正文
        if matches_heading_pattern(trimmed) {
            remaining.push(line);
            merge_done = true;
            continue;
        }

        // 遇到独立编号条款（digit+CJK，如 "4驻场要求"）→ 停止合并
        // 这是新的语义单元，不是标题续文
        if is_digit_cjk_start(trimmed) {
            remaining.push(line);
            merge_done = true;
            continue;
        }

        // 统一 merge cap：所有 push_str 不得超过此配额
        let cap = title_len + MAX_MERGE_CHARS - merged.chars().count();
        if cap == 0 {
            remaining.push(line);
            merge_done = true;
            continue;
        }

        // 句子级合并：只合并到第一个句末标点为止，且不超过 cap。
        // 解决 smart join 将多句话合并成一行后，push_str(line) 一次吞入全部的问题。
        if let Some(byte_pos) = trimmed.find(['。', '！', '？']) {
            let char_len = trimmed[byte_pos..].chars().next().unwrap().len_utf8();
            let end = byte_pos + char_len;
            let merge_text = &trimmed[..end]; // 到第一个句末标点（含）
            let take_count = merge_text.chars().count().min(cap);
            let split_byte: usize = merge_text
                .char_indices()
                .nth(take_count)
                .map(|(i, _)| i)
                .unwrap_or(merge_text.len());
            merged.push_str(&merge_text[..split_byte]);
            // 未合并完的部分（如果有）留在 body
            if take_count < merge_text.chars().count() {
                remaining.push(merge_text[split_byte..].trim());
            }
            // 同一行中句号之后的内容留在 body
            let rest = trimmed[end..].trim();
            if !rest.is_empty() {
                remaining.push(rest);
            }
            merge_done = true;
            continue;
        }

        // 无句末标点的行：仅合并不超过 cap 的字符数
        let take_count = line.chars().count().min(cap);
        if take_count < line.chars().count() {
            let split_byte: usize = line
                .char_indices()
                .nth(take_count)
                .map(|(i, _)| i)
                .unwrap_or(line.len());
            merged.push_str(&line[..split_byte]);
            let rest = line[split_byte..].trim();
            if !rest.is_empty() {
                remaining.push(rest);
            }
            merge_done = true;
        } else {
            merged.push_str(line);
        }
    }

    (merged, remaining.join(""))
}

/// 根据 block_id 查找页码。
fn find_block_page(
    all_blocks: &[(&crate::domain::raw_document::RawBlock, usize)],
    target_id: &str,
) -> Option<usize> {
    all_blocks
        .iter()
        .find(|(b, _)| b.id == target_id)
        .map(|(_, page)| *page)
}

/// 递归统计 section 总数。
fn count_sections(sections: &[Section]) -> usize {
    sections
        .iter()
        .map(|s| 1 + count_sections(&s.children))
        .sum()
}

/// 递归统计各级别数量。
fn count_levels(sections: &[Section], counts: &mut std::collections::HashMap<u8, usize>) {
    for s in sections {
        *counts.entry(s.level).or_insert(0) += 1;
        count_levels(&s.children, counts);
    }
}

// ─── 启发式表格检测（方案五）─────────────────────────────────

/// 从 blocks 中启发式检测纯文本型表格（`|` 分隔），补充到 raw_doc 的 tables 中。
///
/// 对每个页面，扫描 blocks 中连续含 `|` 分隔符且列数一致的段落组，
/// 每组构造一个 RawTable，追加到对应 RawPage.tables。
///
/// # 检测条件
///
/// - 至少连续 2 行含 `|` 分隔符
/// - 相邻行 `|` 分隔出的列数相同（≥2）
/// - block 类型非 heading
///
/// # 返回
///
/// 检测到的伪表格数量（用于日志输出）。
pub fn detect_pipe_tables(raw_doc: &mut RawDocument) -> usize {
    let mut total_detected = 0;

    for page in &mut raw_doc.pages {
        let mut i = 0;
        while i < page.blocks.len() {
            let block = &page.blocks[i];

            // 跳过非 paragraph 类型（降低误判）
            if block.block_type != BlockType::Paragraph {
                i += 1;
                continue;
            }

            // 检查是否含 `|` 分隔符
            let cols = block.text.split('|').count();
            if cols < 2 {
                i += 1;
                continue;
            }

            // 收集连续且列数一致的 blocks
            let mut table_block_indices: Vec<usize> = Vec::new();
            let mut j = i;
            while j < page.blocks.len() {
                let next = &page.blocks[j];
                if next.block_type != BlockType::Paragraph {
                    break;
                }
                let next_cols = next.text.split('|').count();
                if next_cols != cols {
                    break;
                }
                table_block_indices.push(j);
                j += 1;
            }

            // 至少 2 行才认为是表格
            if table_block_indices.len() < 2 {
                i = j;
                continue;
            }

            // 构造 RawTable
            let rows: Vec<Vec<Option<String>>> = table_block_indices
                .iter()
                .map(|&idx| {
                    page.blocks[idx]
                        .text
                        .split('|')
                        .map(|cell| {
                            let trimmed = cell.trim().to_string();
                            if trimmed.is_empty() {
                                None
                            } else {
                                Some(trimmed)
                            }
                        })
                        .collect()
                })
                .collect();

            let table_id = format!("t_{}_{}", page.page_index, page.tables.len());
            let table = RawTable {
                id: table_id,
                bbox: None, // 启发式表格无精确定位 bbox
                rows,
            };
            page.tables.push(table);
            total_detected += 1;

            i = j; // 跳过已消费的 blocks
        }
    }

    total_detected
}

// ─── 跨页表格合并（状态机 + 多维度签名匹配）───────────────────

/// 合并阈值：签名匹配得分 ≥ 此值即视为同一表格。
const MERGE_THRESHOLD: f64 = 0.45;

/// 表头剥离阈值：源表首行与锚点表头的相似度 ≥ 此值时剥离重复表头。
const HEADER_STRIP_THRESHOLD: f64 = 0.70;

/// 最大容忍间隙页数：超过此值链条终止。
const MAX_GAP: u32 = 2;

/// 签名各维度权重（之和为 1.0）。
const COL_COUNT_WEIGHT: f64 = 0.40;
const HEADER_WEIGHT: f64 = 0.35;
const NUMERIC_WEIGHT: f64 = 0.15;
const CELL_LEN_WEIGHT: f64 = 0.10;

/// 表格结构签名，用于跨页匹配时判断两张表是否属于同一逻辑表格。
struct TableSignature {
    col_count: usize,
    /// 首行每列归一化后的文本（trim + 小写）。
    header_fingerprint: Vec<String>,
    /// 每列是否主要为数值（>50% 非空单元格含数字）。
    numeric_cols: Vec<bool>,
    /// 每列非空单元格的平均字符长度。
    avg_cell_lens: Vec<f64>,
}

// ─── 签名计算与匹配 ──────────────────────────────────────────

fn normalize_cell(s: &str) -> String {
    s.trim().to_string()
}

fn is_numeric_cell(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    let digits = s.chars().filter(|c| c.is_ascii_digit()).count();
    let total = s.chars().count();
    total > 0 && (digits as f64 / total as f64) > 0.3
}

fn compute_signature(table: &RawTable) -> TableSignature {
    let col_count = table.rows.first().map(|r| r.len()).unwrap_or(0);
    if col_count == 0 || table.rows.is_empty() {
        return TableSignature {
            col_count: 0,
            header_fingerprint: vec![],
            numeric_cols: vec![],
            avg_cell_lens: vec![],
        };
    }

    let header_fingerprint: Vec<String> = table.rows[0]
        .iter()
        .map(|c| normalize_cell(c.as_deref().unwrap_or("")))
        .collect();

    let numeric_cols: Vec<bool> = (0..col_count)
        .map(|col| {
            let (total, numeric) = table.rows.iter().fold((0usize, 0usize), |(t, n), row| {
                match row.get(col).and_then(|c| c.as_deref()) {
                    Some(s) if !s.trim().is_empty() => {
                        (t + 1, n + if is_numeric_cell(s) { 1 } else { 0 })
                    }
                    _ => (t, n),
                }
            });
            total > 0 && (numeric as f64 / total as f64) > 0.5
        })
        .collect();

    let avg_cell_lens: Vec<f64> = (0..col_count)
        .map(|col| {
            let lens: Vec<usize> = table
                .rows
                .iter()
                .filter_map(|r| {
                    r.get(col)
                        .and_then(|c| c.as_deref())
                        .map(|s| s.chars().count())
                        .filter(|&l| l > 0)
                })
                .collect();
            if lens.is_empty() {
                0.0
            } else {
                lens.iter().sum::<usize>() as f64 / lens.len() as f64
            }
        })
        .collect();

    TableSignature { col_count, header_fingerprint, numeric_cols, avg_cell_lens }
}

/// 比较两行（表头）的相似度，按列逐一比较，返回匹配率 [0, 1]。
fn header_row_similarity(a: &[Option<String>], b: &[Option<String>]) -> f64 {
    let min_len = a.len().min(b.len());
    if min_len == 0 {
        return 0.0;
    }
    let matches = a[..min_len]
        .iter()
        .zip(&b[..min_len])
        .filter(|(ca, cb)| {
            normalize_cell(ca.as_deref().unwrap_or(""))
                == normalize_cell(cb.as_deref().unwrap_or(""))
        })
        .count();
    matches as f64 / min_len as f64
}

/// 两集合的 Jaccard 相似度。
fn jaccard_similarity(a: &[String], b: &[String]) -> f64 {
    let set_a: HashSet<&String> = a.iter().collect();
    let set_b: HashSet<&String> = b.iter().collect();
    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    if union == 0 {
        1.0
    } else {
        intersection as f64 / union as f64
    }
}

fn match_signatures(a: &TableSignature, b: &TableSignature) -> f64 {
    if a.col_count == 0 || b.col_count == 0 {
        return 0.0;
    }

    let mut score = 0.0;

    // 1. 列数匹配
    let col_diff = (a.col_count as i32 - b.col_count as i32).abs();
    if col_diff == 0 {
        score += COL_COUNT_WEIGHT;
    } else if col_diff == 1 {
        score += COL_COUNT_WEIGHT * 0.4;
    }

    // 2. 表头指纹
    let header_sim = jaccard_similarity(&a.header_fingerprint, &b.header_fingerprint);
    score += HEADER_WEIGHT * header_sim;

    // 3. 数值列模式
    let min_cols = a.numeric_cols.len().min(b.numeric_cols.len());
    if min_cols > 0 {
        let matches = a.numeric_cols[..min_cols]
            .iter()
            .zip(&b.numeric_cols[..min_cols])
            .filter(|(na, nb)| na == nb)
            .count();
        score += NUMERIC_WEIGHT * (matches as f64 / min_cols as f64);
    }

    // 4. 列均长分布
    let min_len = a.avg_cell_lens.len().min(b.avg_cell_lens.len());
    if min_len > 0 {
        let len_sim: f64 = a.avg_cell_lens[..min_len]
            .iter()
            .zip(&b.avg_cell_lens[..min_len])
            .map(|(la, lb)| {
                let max_len = la.max(*lb);
                if max_len < 1.0 { 1.0 } else { 1.0 - (la - lb).abs() / max_len }
            })
            .sum::<f64>()
            / min_len as f64;
        score += CELL_LEN_WEIGHT * len_sim.max(0.0);
    }

    score
}

// ─── 续表标记检测 ────────────────────────────────────────────

/// 扫描页面的 text 和 blocks，检测中文续表标记。
fn scan_continued_marker(page: &RawPage) -> bool {
    let patterns = ["（续上表）", "续上表", "续表", "接上页", "续前表"];
    for pat in &patterns {
        if page.text.contains(pat) {
            return true;
        }
    }
    for block in &page.blocks {
        for pat in &patterns {
            if block.text.contains(pat) {
                return true;
            }
        }
    }
    false
}

// ─── 合并主函数 ──────────────────────────────────────────────

/// 合并跨页断裂的表格（状态机 + 多维度签名匹配）。
///
/// # 改进（相比旧实现）
///
/// - **两阶段设计**：阶段1 只读扫描构建链条，阶段2 统一执行合并，消除合并-删除
///   导致的链条断裂
/// - **TableSignature 多维度匹配**：列数 + 表头指纹 + 数值列模式 + 列均长，替代
///   原先仅比较 `(0,0)` 单单元格的检测
/// - **容忍最多 2 页间隙**：pdfplumber 漏检或中间页无表格时链条不中断
/// - **重复表头自动剥离**：比较源表首行与锚点表头的相似度，≥阈值则丢弃重复表头
/// - **续表标记识别**：检测"（续上表）""续表"等标记，降低匹配合并门槛
///
/// # 返回
///
/// 成功合并的组数。
pub fn merge_cross_page_tables(raw_doc: &mut RawDocument) -> usize {
    if raw_doc.pages.len() < 2 {
        return 0;
    }

    let page_count = raw_doc.pages.len();
    let mut merge_count: usize = 0;
    let mut consumed: HashSet<(usize, usize)> = HashSet::new();

    // ── 阶段1：状态机链条发现 ──────────────────────────────────
    for start_page in 0..page_count {
        let table_count = raw_doc.pages[start_page].tables.len();
        for start_ti in 0..table_count {
            if consumed.contains(&(start_page, start_ti)) {
                continue;
            }

            let anchor_sig = compute_signature(&raw_doc.pages[start_page].tables[start_ti]);
            if anchor_sig.col_count == 0 {
                continue;
            }
            let anchor_header = raw_doc.pages[start_page].tables[start_ti]
                .rows
                .first()
                .cloned();

            let mut chain: Vec<(usize, usize)> = vec![(start_page, start_ti)];
            let mut gap_count: u32 = 0;
            let mut gap_has_marker: bool = false;

            for next_page in (start_page + 1)..page_count {
                // 检测当前页是否有续表标记
                if scan_continued_marker(&raw_doc.pages[next_page]) {
                    gap_has_marker = true;
                }

                let mut found = false;
                for (ti, table) in raw_doc.pages[next_page].tables.iter().enumerate() {
                    if consumed.contains(&(next_page, ti)) {
                        continue;
                    }
                    if table.rows.is_empty() {
                        continue;
                    }

                    let sig = compute_signature(table);
                    if sig.col_count == 0 {
                        continue;
                    }

                    let score = match_signatures(&anchor_sig, &sig);

                    // 续表标记降低合并门槛（约 0.25）
                    let threshold = if gap_has_marker {
                        MERGE_THRESHOLD * 0.55
                    } else {
                        MERGE_THRESHOLD
                    };

                    if score >= threshold {
                        chain.push((next_page, ti));
                        found = true;
                        gap_count = 0;
                        gap_has_marker = false;
                        break;
                    }
                }

                if !found {
                    gap_count += 1;
                    if gap_count > MAX_GAP {
                        break;
                    }
                }
            }

            // ── 阶段2：执行合并 ──────────────────────────────────
            if chain.len() > 1 {
                let (dest_page, dest_ti) = chain[0];

                for &(src_page, src_ti) in &chain[1..] {
                    let src_rows = std::mem::take(
                        &mut raw_doc.pages[src_page].tables[src_ti].rows,
                    );

                    if src_rows.is_empty() {
                        continue;
                    }

                    // 判断是否剥离重复表头
                    let strip = match (&anchor_header, src_rows.first()) {
                        (Some(ah), Some(sr)) => {
                            header_row_similarity(ah, sr) >= HEADER_STRIP_THRESHOLD
                        }
                        _ => false,
                    };

                    let effective_rows: Vec<Vec<Option<String>>> = if strip {
                        src_rows.into_iter().skip(1).collect()
                    } else {
                        src_rows
                    };

                    if !effective_rows.is_empty() {
                        raw_doc.pages[dest_page].tables[dest_ti]
                            .rows
                            .extend(effective_rows);
                    }

                    consumed.insert((src_page, src_ti));
                    merge_count += 1;
                }

                consumed.insert((start_page, start_ti));
            }
        }
    }

    // ── 清理：移除空壳表格 ─────────────────────────────────────
    for page in raw_doc.pages.iter_mut() {
        page.tables.retain(|t| !t.rows.is_empty());
    }

    merge_count
}

// ─── 表格内容注入（方案二）─────────────────────────────────────

/// 将 RawDocument 中的表格内容注入到 Section 树的 body_text 中。
///
/// 对每个 Section，查找其 **body 实际页码范围**（body_page_start..=body_page_end）
/// 覆盖的页面上的表格，将表格格式化为 Markdown 表格文本，追加到 body_text 末尾。
/// 同时将表格 ID 追加到 block_ids 以便回溯。
///
/// # 去重策略
///
/// 使用全局 `visited_tables: HashSet<String>` 确保每张表格只注入一次——
/// 注入到**最先遇到**的 Section（递归深度优先，即最深层级最精确的 Section）。
/// 祖先 Section 不会重复注入已被子孙 Section 消费的表格。
///
/// # Markdown 表格格式
///
/// ```markdown
/// | 品目号 | 品目名称 | 采购标的 | 数量 | 是否允许进口 |
/// |--------|----------|----------|------|-------------|
/// | 1-1    | 教育用房施工 | 东莞理工学院... | 1(项) | 否 |
/// ```
///
/// - 单元格内的换行符替换为空格
/// - 空单元格输出为空字符串
/// - 表格之间用 `\n\n` 分隔
pub fn inject_tables_into_sections(sections: &mut [Section], raw_doc: &RawDocument) {
    // 构建 page → tables 索引（只读，一次扫描）
    let page_tables: std::collections::HashMap<usize, &[RawTable]> = raw_doc
        .pages
        .iter()
        .map(|p| (p.page_index, p.tables.as_slice()))
        .collect();

    let mut visited_tables: std::collections::HashSet<String> = std::collections::HashSet::new();
    inject_tables_recursive(sections, &page_tables, &mut visited_tables);
}

fn inject_tables_recursive(
    sections: &mut [Section],
    page_tables: &std::collections::HashMap<usize, &[RawTable]>,
    visited_tables: &mut std::collections::HashSet<String>,
) {
    for section in sections.iter_mut() {
        // 先对子节点按 body_page_start 排序，确保页码小的 Section
        // 优先认领边界页表格，行为确定化。
        section.children.sort_by_key(|c| c.body_page_start);

        // 先递归处理子节点（深度优先），确保表格优先归属到最深层 Section
        inject_tables_recursive(&mut section.children, page_tables, visited_tables);

        // 收集该 Section **body 实际页码范围**内的表格
        // 使用 body_page_start..=body_page_end 而非 page_start..=page_end：
        // 容器 Section 的 page_start..=page_end 涵盖所有子孙节点页面，
        // 若用此范围会吞并属于子节点的表格。body_page 范围只反映 Section
        // 自身正文的实际页面，精确归属。
        let mut table_texts: Vec<String> = Vec::new();

        for page_idx in section.body_page_start..=section.body_page_end {
            if let Some(tables) = page_tables.get(&page_idx) {
                for table in *tables {
                    // 去重：跳过已被更深层 Section 消费的表格
                    if visited_tables.contains(&table.id) {
                        continue;
                    }
                    if let Some(md) = format_table_as_markdown(table) {
                        table_texts.push(md);
                        visited_tables.insert(table.id.clone());
                        // 将 table ID 加入追溯链
                        if !section.block_ids.contains(&table.id) {
                            section.block_ids.push(table.id.clone());
                        }
                    }
                }
            }
        }

        // ── Fallback: 纯容器 Section 的 page span 扫描 ──────────
        // 纯容器 Section（无 body_text，body_page_start/end 通常为 0）
        // 其子节点覆盖的页面范围可能存在间隙（如子节点 A 覆盖 30-35 页、
        // 子节点 B 覆盖 40-60 页，第 36-39 页为容器过渡页）。
        // 这些间隙页上的表格不被任何子节点认领，也不会被 body_page 扫描
        // （body_page 为 0..=0），导致彻底丢失。
        //
        // 此处使用容器的完整 page_start..=page_end 范围做一次兜底扫描，
        // 拾取子节点遗漏的表格。由于 visited_tables 已被子节点消费过，
        // 不会造成重复注入。
        if !section.children.is_empty() && section.body_text.is_empty() {
            for page_idx in section.page_start..=section.page_end {
                if let Some(tables) = page_tables.get(&page_idx) {
                    for table in *tables {
                        if visited_tables.contains(&table.id) {
                            continue;
                        }
                        if let Some(md) = format_table_as_markdown(table) {
                            table_texts.push(md);
                            visited_tables.insert(table.id.clone());
                            if !section.block_ids.contains(&table.id) {
                                section.block_ids.push(table.id.clone());
                            }
                        }
                    }
                }
            }
        }

        if !table_texts.is_empty() {
            let table_section = table_texts.join("\n\n");
            if section.body_text.is_empty() {
                section.body_text = table_section;
            } else {
                section.body_text = format!("{}\n\n{}", section.body_text, table_section);
            }
        }
    }
}

/// 将 RawTable 格式化为 Markdown 表格字符串。
///
/// 返回 None 如果表格为空（无行或无列）。
fn format_table_as_markdown(table: &RawTable) -> Option<String> {
    if table.rows.is_empty() {
        return None;
    }

    // 计算列数（取最大行宽）
    let col_count = table.rows.iter().map(|row| row.len()).max().unwrap_or(0);

    if col_count == 0 {
        return None;
    }

    let mut lines: Vec<String> = Vec::new();

    for (row_idx, row) in table.rows.iter().enumerate() {
        let cells: Vec<String> = (0..col_count)
            .map(|col| {
                row.get(col)
                    .and_then(|opt| opt.as_ref())
                    .map(|s| s.replace('\n', " ").trim().to_string())
                    .unwrap_or_default()
            })
            .collect();

        lines.push(format!("| {} |", cells.join(" | ")));

        // 表头后添加分隔行
        if row_idx == 0 {
            let sep: Vec<String> = (0..col_count).map(|_| "---".to_string()).collect();
            lines.push(format!("| {} |", sep.join(" | ")));
        }
    }

    Some(lines.join("\n"))
}

// ─── 测试 ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::raw_document::{BBox, RawBlock, RawPage};

    #[test]
    fn test_is_page_noise() {
        // 纯数字页码
        assert!(is_page_noise("1"));
        assert!(is_page_noise("92"));
        assert!(is_page_noise(" 5 "));
        // "第X页共Y页" 格式
        assert!(is_page_noise("第1页共78页"));
        assert!(is_page_noise("第11页共78页"));
        assert!(is_page_noise("第2页共78页温馨提示")); // 短后缀也过滤
        assert!(is_page_noise("78第72页共页")); // 残缺变体
        // Unicode 私有区控制字符 (U+F06E)
        assert!(is_page_noise(
            "系统架构要求应用系统采用浏览器/服务器架构，如无特殊原因，禁止要求终端用户安装客\u{F06E}"
        ));
        // 非噪声
        assert!(!is_page_noise("第一章"));
        assert!(!is_page_noise("1. 供应商资格"));
        assert!(!is_page_noise("1234")); // 纯数字但超过3位
        assert!(!is_page_noise("供应商应具备以下条件：")); // 正常正文
    }

    #[test]
    fn test_part_pattern() {
        let pat = &HEADING_PATTERNS[0];
        assert!(pat.regex.is_match("第一部分投标邀请函"));
        assert!(pat.regex.is_match("第五部分投标文件格式"));
        assert!(!pat.regex.is_match("第一章磋商邀请"));
        assert!(!pat.regex.is_match("一、项目概况"));
    }

    #[test]
    fn test_chapter_pattern() {
        let pat = &HEADING_PATTERNS[1];
        assert!(pat.regex.is_match("第一章磋商邀请"));
        assert!(pat.regex.is_match("第五章合同文本"));
        assert!(!pat.regex.is_match("一、项目概况"));
        assert!(!pat.regex.is_match("1. 供应商资格"));
    }

    #[test]
    fn test_cjk_numbered_pattern() {
        let pat = &HEADING_PATTERNS[3];
        assert!(pat.regex.is_match("一、项目概况"));
        assert!(pat.regex.is_match("二.供应商的资格要求"));
        assert!(!pat.regex.is_match("第一章"));
    }

    #[test]
    fn test_paren_cjk_pattern() {
        let pat = &HEADING_PATTERNS[4];
        assert!(pat.regex.is_match("（一）资格要求"));
        assert!(pat.regex.is_match("(二) 评审标准"));
    }

    #[test]
    fn test_digit_dot_pattern() {
        let pat = &HEADING_PATTERNS[5];
        assert!(pat.regex.is_match("1. 供应商资格"));
        assert!(pat.regex.is_match("2、项目概况"));
        assert!(pat.regex.is_match("3)其他要求"));
    }

    #[test]
    fn test_paren_digit_pattern() {
        let pat = &HEADING_PATTERNS[6];
        assert!(pat.regex.is_match("（1）营业执照副本"));
        assert!(pat.regex.is_match("(2) 法定代表人证明"));
    }

    #[test]
    fn test_article_pattern() {
        let pat = &HEADING_PATTERNS[7];
        assert!(pat.regex.is_match("第九条工程的支付、结算"));
    }

    // ─── A1: split_inline_headings 测试 ─────────────────────────

    #[test]
    fn test_split_inline_headings_no_match() {
        // 普通行无右括号+数字标题模式 → 原样返回
        let result = split_inline_headings("普通的正文内容");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "普通的正文内容");
    }

    #[test]
    fn test_split_inline_headings_basic() {
        // 采购包名后紧跟 "1.主要商务要求"
        let result = split_inline_headings(
            "采购包1（东莞理工学院松山湖校区智慧教室环境改造工程（二期））1.主要商务要求",
        );
        assert_eq!(
            result.len(),
            2,
            "应拆分为前缀和标题两部分，实际: {:?}",
            result
        );
        assert!(
            result[0].contains("（二期））"),
            "前缀应包含右括号，实际: {}",
            result[0]
        );
        assert_eq!(
            result[1], "1.主要商务要求",
            "标题应从数字开始，实际: {}",
            result[1]
        );
    }

    #[test]
    fn test_split_inline_headings_heading_already_at_start() {
        // 标题已在行首 → 不应被拆分（没有前置右括号）
        let result = split_inline_headings("1.具有良好的商业信誉和健全的财务会计制度；");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "1.具有良好的商业信誉和健全的财务会计制度；");
    }

    #[test]
    fn test_split_inline_headings_plain_text_no_paren() {
        // 行中包含数字编号但前面不是右括号（是冒号）
        let result = split_inline_headings("条件包括：1.具有良好的商业信誉");
        assert_eq!(result.len(), 1, "冒号不是右括号，不应拆分");
    }

    #[test]
    fn test_split_inline_headings_empty_line() {
        let result = split_inline_headings("");
        assert_eq!(result.len(), 1);
        assert!(result[0].is_empty());
    }

    // ─── A1 + A2: sectionize 集成测试 ───────────────────────────

    /// 构造包含内联标题的 RawDocument 用于集成验证。
    fn make_raw_doc_with_inline_heading() -> RawDocument {
        use crate::domain::raw_document::{BBox, RawBlock, RawPage};
        RawDocument {
            document_id: "test_inline".to_string(),
            source_path: String::new(),
            pages: vec![RawPage {
                page_index: 0,
                width: 595.0,
                height: 842.0,
                text: String::new(),
                words: vec![],
                blocks: vec![
                    RawBlock {
                        id: "b_0_0".to_string(),
                        block_type: BlockType::Heading,
                        text: "六、《资格条件承诺函》格式".to_string(),
                        bbox: BBox {
                            x0: 90.0,
                            top: 75.0,
                            x1: 350.0,
                            bottom: 100.0,
                        },
                    },
                    RawBlock {
                        id: "b_0_1".to_string(),
                        block_type: BlockType::Paragraph,
                        text: "采购包1（东莞理工学院）1.主要商务要求".to_string(),
                        bbox: BBox {
                            x0: 90.0,
                            top: 560.0,
                            x1: 500.0,
                            bottom: 580.0,
                        },
                    },
                ],
                tables: vec![],
                lines: vec![],
                rects: vec![],
            }],
        }
    }

    #[test]
    fn test_sectionize_detects_inline_heading() {
        let doc = make_raw_doc_with_inline_heading();
        let output = sectionize(&doc);

        // 应检测到 2 个标题：六、... 和 1.主要商务要求
        // 遍历树查找 "1.主要商务要求"
        let titles: Vec<String> = collect_all_titles(&output.sections);
        assert!(
            titles.iter().any(|t| t.contains("1.主要商务要求")),
            "应检测到内联标题 '1.主要商务要求'，实际标题: {:?}",
            titles
        );
    }

    #[test]
    fn test_sectionize_detects_plain_heading() {
        let doc = make_raw_doc_with_inline_heading();
        let output = sectionize(&doc);

        let orphans = output.stats.orphan_blocks;
        // "1.主要商务要求" 应被正确识别，不应有过多孤儿 block
        assert!(
            orphans <= 1,
            "孤儿 block 不应超过 1 个（可能有页码噪声），实际: {}",
            orphans
        );
    }

    /// 递归收集所有 section 的 title。
    fn collect_all_titles(sections: &[Section]) -> Vec<String> {
        let mut titles = Vec::new();
        for s in sections {
            titles.push(s.title.clone());
            titles.extend(collect_all_titles(&s.children));
        }
        titles
    }

    /// 递归收集所有 section 的 (pattern, title) 对，用于调试。
    fn collect_pattern_titles(sections: &[Section]) -> Vec<(String, String)> {
        let mut result = Vec::new();
        for s in sections {
            result.push((s.pattern.clone(), s.title.clone()));
            result.extend(collect_pattern_titles(&s.children));
        }
        result
    }

    // ─── 端到端回归测试：使用实际 PDF 的 RawDocument ─────────────

    /// 加载已有的 raw JSON，运行 sectionize，验证关键标题被正确识别。
    /// 此测试依赖 `output/raw_json/智慧教室环境改造工程_raw.json`。
    #[test]
    fn test_real_pdf_detects_inline_and_plain_headings() {
        let raw_path = data_path_str("output/raw_json/智慧教室环境改造工程_raw.json");
        let raw_json = match std::fs::read_to_string(&raw_path) {
            Ok(s) => s,
            Err(_) => {
                eprintln!("跳过: raw JSON 文件不存在 ({})", raw_path);
                return;
            }
        };
        let doc: RawDocument = match serde_json::from_str(&raw_json) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("跳过: raw JSON 解析失败: {}", e);
                return;
            }
        };

        let output = sectionize(&doc);
        let titles = collect_all_titles(&output.sections);
        let pattern_titles = collect_pattern_titles(&output.sections);

        // ★ 验证 A1: "1.主要商务要求" 被检测为独立 section
        let has_business_req = titles.iter().any(|t| t.contains("1.主要商务要求"));
        assert!(
            has_business_req,
            "A1 失败: 未检测到 '1.主要商务要求'。\n\
             实际标题列表 (共 {} 个):\n{:#?}\n\
             完整 pattern+title:\n{:#?}",
            titles.len(),
            titles,
            pattern_titles,
        );

        // ★ 验证 A2: "付款方式" 被检测为 plain_heading section
        let has_payment = titles.iter().any(|t| t == "付款方式");
        assert!(
            has_payment,
            "A2 失败: 未检测到 '付款方式' (plain_heading)。\n\
             实际标题列表:\n{:#?}",
            titles,
        );

        // ★ 验证层级关系: "付款方式" 应在 "1.主要商务要求" 下
        // （"付款方式" 的 section path 祖先应包含 "1.主要商务要求"）
        if has_business_req && has_payment {
            let payment_under_business =
                verify_child_of(&output.sections, "1.主要商务要求", "付款方式");
            assert!(
                payment_under_business,
                "层级关系错误: '付款方式' 应位于 '1.主要商务要求' 下"
            );
        }

        println!(
            "✅ 端到端验证通过: {} 个 section, {} 个孤儿 block",
            output.stats.total_sections, output.stats.orphan_blocks
        );
    }

    /// 验证 `child_title` 是否在 `parent_title_contains` 的子树中。
    fn verify_child_of(sections: &[Section], parent_contains: &str, child_title: &str) -> bool {
        for s in sections {
            if s.title.contains(parent_contains) {
                let children_titles = collect_all_titles(&s.children);
                return children_titles.iter().any(|t| t == child_title);
            }
            if verify_child_of(&s.children, parent_contains, child_title) {
                return true;
            }
        }
        false
    }

    /// 验证跨页表格合并：t_9_0（标的提供时间/地点）+ t_10_0（付款方式/验收要求）
    /// 是同一张"主要商务要求"表格，合并后 t_9_0 应包含全部 4 行。
    #[test]
    fn test_merge_cross_page_tables_real_pdf() {
        let raw_path = data_path_str("output/raw_json/智慧教室环境改造工程_raw.json");
        let raw_json = match std::fs::read_to_string(&raw_path) {
            Ok(s) => s,
            Err(_) => {
                eprintln!("跳过: raw JSON 文件不存在 ({})", raw_path);
                return;
            }
        };
        let mut doc: RawDocument = match serde_json::from_str(&raw_json) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("跳过: raw JSON 解析失败: {}", e);
                return;
            }
        };

        // 记录合并前各页表格数
        let tables_before: Vec<usize> = doc.pages.iter().map(|p| p.tables.len()).collect();

        let merged = merge_cross_page_tables(&mut doc);

        let tables_after: Vec<usize> = doc.pages.iter().map(|p| p.tables.len()).collect();

        // 验证：应该有合并发生（page 9 + page 10 的表格被合并）
        assert!(
            merged > 0,
            "应至少合并 1 组跨页表格。\n\
             合并前各页表格数: {:?}\n\
             合并后各页表格数: {:?}",
            tables_before,
            tables_after
        );

        // 验证 page 9 (index 9) 的最后一个表格包含了"付款方式"和"验收要求"
        let page_9 = &doc.pages[9];
        let last_table = page_9.tables.last().expect("page 9 应有表格");
        let all_cells: Vec<String> = last_table
            .rows
            .iter()
            .flat_map(|r| r.iter())
            .filter_map(|c| c.as_deref())
            .map(|s| s.chars().take(50).collect())
            .collect();

        let has_payment = all_cells.iter().any(|c| c.contains("付款方式"));
        let has_acceptance = all_cells.iter().any(|c| c.contains("验收要求"));
        let has_delivery_time = all_cells.iter().any(|c| c.contains("标的提供的时间"));

        assert!(
            has_delivery_time,
            "合并后表格应包含 '标的提供的时间'（来自原 t_9_0）。\n单元格: {:?}",
            all_cells
        );
        assert!(
            has_payment,
            "合并后表格应包含 '付款方式'（来自原 t_10_0）。\n单元格: {:?}",
            all_cells
        );
        assert!(
            has_acceptance,
            "合并后表格应包含 '验收要求'（来自原 t_10_0）。\n单元格: {:?}",
            all_cells
        );

        println!(
            "✅ 跨页表格合并测试通过: {} 组合并，合并前后表格数 {:?} → {:?}",
            merged, tables_before, tables_after
        );
    }

    // ═══════════════════════════════════════════════════════════════
    // 修复验证测试：跨页表格合并的 4 个已修复问题
    // ═══════════════════════════════════════════════════════════════

    // ── 辅助构造器 ──────────────────────────────────────────────

    /// 用行列字符串构造 RawTable。空字符串 → None 单元格。
    fn t(id: &str, rows: Vec<Vec<&str>>) -> RawTable {
        RawTable {
            id: id.to_string(),
            bbox: None,
            rows: rows
                .into_iter()
                .map(|r| {
                    r.into_iter()
                        .map(|c| if c.is_empty() { None } else { Some(c.to_string()) })
                        .collect()
                })
                .collect(),
        }
    }

    /// 构造只有表格的 RawPage（blocks/words 用空数组填充）。
    fn pg(index: usize, tables: Vec<RawTable>) -> RawPage {
        RawPage {
            page_index: index,
            width: 595.0,
            height: 842.0,
            text: String::new(),
            words: vec![],
            blocks: vec![],
            tables,
            lines: vec![],
            rects: vec![],
        }
    }

    /// 带 blocks 文本的页面构造器（用于测试续表标记等场景）。
    fn pg_with_blocks(index: usize, tables: Vec<RawTable>, block_texts: Vec<&str>) -> RawPage {
        let blocks: Vec<RawBlock> = block_texts
            .iter()
            .enumerate()
            .map(|(i, &text)| RawBlock {
                id: format!("b_{}_{}", index, i),
                block_type: BlockType::Paragraph,
                text: text.to_string(),
                bbox: BBox { x0: 90.0, top: 100.0, x1: 500.0, bottom: 120.0 },
            })
            .collect();
        RawPage {
            page_index: index,
            width: 595.0,
            height: 842.0,
            text: block_texts.join("\n"),
            words: vec![],
            blocks,
            tables,
            lines: vec![],
            rects: vec![],
        }
    }

    fn doc(pages: Vec<RawPage>) -> RawDocument {
        RawDocument { document_id: "test".to_string(), source_path: String::new(), pages }
    }

    /// 获取表中所有非空单元格文本，用于断言。
    fn cells(table: &RawTable) -> Vec<String> {
        table.rows.iter()
            .flat_map(|r| r.iter())
            .filter_map(|c| c.as_deref())
            .map(|s| s.to_string())
            .collect()
    }

    // ─────────────────────────────────────────────────────────────
    // 修复 1：三页及以上跨页完整合并
    // ─────────────────────────────────────────────────────────────
    //
    // 场景：一张设备清单跨 3 页：
    //   页0: [表头] + rows 1-3
    //   页1: rows 4-6（续行，无表头）
    //   页2: rows 7-9（续行，无表头）
    // ✅ 修复后：3 页合并为 1 张表，共 10 行（1 表头 + 9 数据）

    #[test]
    fn test_fix_1_three_page_chain_merged() {
        let mut doc = doc(vec![
            pg(0, vec![t("t_0_0", vec![
                vec!["序号", "设备名称", "数量"],
                vec!["1", "智慧黑板", "12"],
                vec!["2", "扩声音箱", "24"],
                vec!["3", "控制主机", "6"],
            ])]),
            pg(1, vec![t("t_1_0", vec![
                vec!["4", "无线话筒", "12"],
                vec!["5", "电源时序器", "6"],
                vec!["6", "交换机", "3"],
            ])]),
            pg(2, vec![t("t_2_0", vec![
                vec!["7", "机柜", "3"],
                vec!["8", "线材辅料", "1"],
                vec!["9", "安装调试", "1"],
            ])]),
        ]);

        let merged = merge_cross_page_tables(&mut doc);

        // ✅ 修复后：3 页全部合并（2 次合并：0+1, 0+2）
        assert_eq!(merged, 2,
            "【修复1】应有 2 次合并（页0+页1, 页0+页2），实际: {}", merged);

        // ✅ 页0 的表有完整 10 行（1 表头 + 9 数据）
        assert_eq!(doc.pages[0].tables[0].rows.len(), 10,
            "【修复1】合并后应有 10 行，实际: {}",
            doc.pages[0].tables[0].rows.len());

        // ✅ 页1 和页2 的空壳表格已被清理
        assert!(doc.pages[1].tables.is_empty(),
            "【修复1】页1 的表格应已被合并并清理");
        assert!(doc.pages[2].tables.is_empty(),
            "【修复1】页2 的表格应已被合并并清理");
    }

    // ─────────────────────────────────────────────────────────────
    // 修复 3+4：重复表头检测并剥离
    // ─────────────────────────────────────────────────────────────
    //
    // 场景：
    //   页0: [序号, 名称, 数量] [1, A, 10] [2, B, 5]
    //   页1: [序号, 名称, 数量] [3, C, 8]  ← 首行是重复表头
    // ✅ 修复后：两页正常合并，页1 的重复表头行被自动剥离

    #[test]
    fn test_fix_3_and_4_duplicate_header_stripped() {
        let mut doc = doc(vec![
            pg(0, vec![t("t_0_0", vec![
                vec!["序号", "名称", "数量"],
                vec!["1", "设备A", "10"],
                vec!["2", "设备B", "5"],
            ])]),
            pg(1, vec![t("t_1_0", vec![
                vec!["序号", "名称", "数量"],  // ← 重复表头
                vec!["3", "设备C", "8"],
            ])]),
        ]);

        let merged = merge_cross_page_tables(&mut doc);

        // ✅ 修复后：正常合并
        assert_eq!(merged, 1,
            "【修复3+4】应有 1 次合并，实际: {}", merged);

        // ✅ 合并后 4 行 = 1 表头 + 3 数据行（页1的重复表头已被剥离）
        assert_eq!(doc.pages[0].tables[0].rows.len(), 4,
            "【修复3+4】合并后应有 4 行（表头行+3数据行），重复表头已剥离，实际: {}",
            doc.pages[0].tables[0].rows.len());

        // ✅ 页1 表格已被清理
        assert!(doc.pages[1].tables.is_empty(),
            "【修复3+4】页1 的表格应已被合并并清理");

        // ✅ 验证数据完整性：所有 3 行数据都在
        let all_cells = cells(&doc.pages[0].tables[0]);
        assert!(all_cells.iter().any(|c| c == "设备A"));
        assert!(all_cells.iter().any(|c| c == "设备B"));
        assert!(all_cells.iter().any(|c| c == "设备C"));
    }

    // ─────────────────────────────────────────────────────────────
    // 修复 3：多维度签名匹配——首格相同但表头整体不同 → 应合并
    // ─────────────────────────────────────────────────────────────
    //
    // 场景：两页的首格恰好都是"东莞理工学院"，但这是数据值巧合，
    // 不是重复表头。多维度签名匹配（列数+数值模式+列长分布）应识别
    // 为同一张表。
    // ✅ 修复后：正常合并，不因首格相同而拒绝

    #[test]
    fn test_fix_3_same_first_cell_merged() {
        let mut doc = doc(vec![
            pg(0, vec![t("t_0_0", vec![
                vec!["东莞理工学院", "智慧教室改造", "500万"],
                vec!["东莞理工学院", "实验室建设", "300万"],
                vec!["东莞理工学院", "多媒体教室", "150万"],
            ])]),
            pg(1, vec![t("t_1_0", vec![
                vec!["东莞理工学院", "设备采购", "200万"],
                vec!["东莞理工学院", "网络升级", "80万"],
            ])]),
        ]);

        let merged = merge_cross_page_tables(&mut doc);

        // ✅ 修复后：多维度签名匹配通过，正常合并
        assert_eq!(merged, 1,
            "【修复3】应有 1 次合并（多维度签名匹配），实际: {}", merged);

        // ✅ 合并后 5 行
        assert_eq!(doc.pages[0].tables[0].rows.len(), 5,
            "【修复3】合并后应有 5 行，实际: {}",
            doc.pages[0].tables[0].rows.len());

        // ✅ 页1 表格已清理
        assert!(doc.pages[1].tables.is_empty());
    }

    // ─────────────────────────────────────────────────────────────
    // 修复 3+4 变体：空白差异的重复表头被归一化后正确剥离
    // ─────────────────────────────────────────────────────────────
    //
    // 场景：页1 表头有前后空格（" 序号 "），与页0 表头（"序号"）
    // 归一化后相同 → 应被检测为重复表头并剥离。
    // ✅ 修复后：合并成功，空白表头被剥离，3 行（1 表头 + 2 数据）

    #[test]
    fn test_fix_3_whitespace_header_stripped() {
        let mut doc = doc(vec![
            pg(0, vec![t("t_0_0", vec![
                vec!["序号", "名称"],
                vec!["1", "设备A"],
            ])]),
            pg(1, vec![t("t_1_0", vec![
                vec![" 序号 ", "名称"],  // ← 空白差异的重复表头
                vec!["2", "设备B"],
            ])]),
        ]);

        let merged = merge_cross_page_tables(&mut doc);

        // ✅ 修复后：合并成功
        assert_eq!(merged, 1,
            "【修复3+4变体】应有 1 次合并，实际: {}", merged);

        // ✅ 合并后 3 行（表头 + 2 数据），重复表头已剥离
        assert_eq!(doc.pages[0].tables[0].rows.len(), 3,
            "【修复3+4变体】合并后应有 3 行（表头已在归一化后剥离），实际: {}",
            doc.pages[0].tables[0].rows.len());

        // ✅ 残留检查：不应存在含空白表头的行
        let all_cells = cells(&doc.pages[0].tables[0]);
        assert!(!all_cells.iter().any(|c| c.contains(" 序号 ")),
            "【修复3+4变体】' 序号 ' 重复表头已被剥离，不应出现在数据中");
    }

    // ─────────────────────────────────────────────────────────────
    // 修复 5：续表标记识别 + 间隙容忍
    // ─────────────────────────────────────────────────────────────
    //
    // 场景：页0 有表，页1 无表但有"（续上表）"文本，页2 有表的延续。
    // ✅ 修复后：续表标记降低匹配合并门槛，页0 与页2 成功合并。

    #[test]
    fn test_fix_5_continued_marker_bridges_gap() {
        let mut doc = doc(vec![
            pg(0, vec![t("t_0_0", vec![
                vec!["序号", "名称"],
                vec!["1", "设备A"],
                vec!["2", "设备B"],
            ])]),
            // 页1：无表格，只有"（续上表）"文本
            pg_with_blocks(1, vec![], vec!["（续上表）"]),
            // 页2：表格延续
            pg(2, vec![t("t_2_0", vec![
                vec!["3", "设备C"],
                vec!["4", "设备D"],
            ])]),
        ]);

        let merged = merge_cross_page_tables(&mut doc);

        // ✅ 修复后：续表标记降低门槛 + 间隙容忍，页0 与页2 合并
        assert_eq!(merged, 1,
            "【修复5】续表标记应降低匹配合并门槛，合并页0与页2，实际: {}",
            merged);

        // ✅ 合并后 5 行（1 表头 + 4 数据）
        assert_eq!(doc.pages[0].tables[0].rows.len(), 5,
            "【修复5】合并后应有 5 行，实际: {}",
            doc.pages[0].tables[0].rows.len());

        // ✅ 页2 表格已清理
        assert!(doc.pages[2].tables.is_empty(),
            "【修复5】页2 的表格应已被合并并清理");
    }

    // ─────────────────────────────────────────────────────────────
    // 修复 5 变体：续表标记覆盖列数不匹配
    // ─────────────────────────────────────────────────────────────
    //
    // 场景：页0 表格 4 列，页1 续表被误识别为 3 列（合并单元格），
    // 但页1 有"（续上表）"标记。
    // ✅ 修复后：续表标记降低阈值 + 列数差异 ±1 容错，成功合并。

    #[test]
    fn test_fix_5_marker_overrides_col_mismatch() {
        let mut doc = doc(vec![
            pg(0, vec![t("t_0_0", vec![
                vec!["序号", "项目", "金额", "备注"],
                vec!["1", "教室改造", "500万", ""],
            ])]),
            // 页1：被误识别为 3 列，但有"（续上表）"标记
            pg_with_blocks(1, vec![
                t("t_1_0", vec![
                    vec!["2", "设备采购", "200万"],  // 只有 3 列
                ]),
            ], vec!["（续上表）"]),
        ]);

        let merged = merge_cross_page_tables(&mut doc);

        // ✅ 修复后：续表标记降低阈值 + 列数 ±1 容错 → 合并成功
        assert_eq!(merged, 1,
            "【修复5变体】续表标记+列数容错应成功合并，实际: {}",
            merged);

        // ✅ 合并后 3 行（1 表头 + 2 数据）
        assert_eq!(doc.pages[0].tables[0].rows.len(), 3,
            "【修复5变体】合并后应有 3 行，实际: {}",
            doc.pages[0].tables[0].rows.len());
    }
}
