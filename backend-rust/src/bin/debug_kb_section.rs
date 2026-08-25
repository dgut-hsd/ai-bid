//! 调试工具：打印 PDF 的 block 划分 + sectionize 结果（定位法条分块问题）。
//! 用法: cargo run --bin debug_kb_section -- <pdf路径>

use ai_bid::domain::chunk::ChunkingConfig;
use ai_bid::services::chunking_service::chunk_sections;
use ai_bid::services::pdf_extract_service::extract_pdf_to_raw_json;
use ai_bid::services::sectionize_service::{self, Section};

fn dump_sections(sections: &[Section], depth: usize) {
    for s in sections {
        let indent = "  ".repeat(depth);
        println!(
            "{}[L{} {}] title={:?}",
            indent, s.level, s.pattern, s.title
        );
        let body: String = s.body_text.chars().take(140).collect();
        println!("{}   body={:?}", indent, body);
        if !s.children.is_empty() {
            dump_sections(&s.children, depth + 1);
        }
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: debug_kb_section <pdf>");
    let raw = extract_pdf_to_raw_json(&path).expect("extract failed");

    if let Some(page) = raw.pages.first() {
        println!("===== PAGE {} blocks ======", page.page_index);
        for b in &page.blocks {
            println!("[{}] {:?} text={:?}", b.id, b.block_type, b.text);
        }
    }

    let out = sectionize_service::sectionize(&raw);
    println!(
        "\n===== SECTIONIZE total={} orphans={} =====",
        out.stats.total_sections, out.stats.orphan_blocks
    );
    dump_sections(&out.sections, 0);

    let config = ChunkingConfig::default();
    let chunks = chunk_sections(&out.sections, &config);
    println!("\n===== CHUNKS count={} =====", chunks.len());
    for c in &chunks {
        let path = c.section_path.join(" > ");
        println!(
            "[{}] {} path={}\n      text={:?}",
            c.chunk_id,
            format!("{:?}", c.chunk_type),
            path,
            c.text.chars().take(80).collect::<String>()
        );
    }
}