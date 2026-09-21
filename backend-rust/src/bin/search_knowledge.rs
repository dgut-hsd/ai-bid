//!
//! 用法: cargo run --bin search_knowledge <关键词>
//!
//! 依赖 Neo4j 已启动（见方案文档 §任务单3）。

use anyhow::{Context, Result};

use ai_bid::knowledge::graph::Neo4jClient;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    let query = std::env::args()
        .nth(1)
        .context("用法: cargo run --bin search_knowledge <关键词>")?;

    let client = Neo4jClient::connect().await?;
    let hits = client.search(&query).await?;

    println!("查询『{}』命中 {} 条风险知识:", query, hits.len());
    for h in &hits {
        println!(
            "  - [{}] {}（id: {}）",
            h.risk.severity, h.risk.name, h.risk.id
        );
        for law in &h.laws {
            let short = if law.short_name.is_empty() { "" } else { &law.short_name };
            let eff = law.meta.as_ref().and_then(|m| {
                if m.effective_date.is_empty() { None } else { Some(m.effective_date.as_str()) }
            }).unwrap_or("");
            println!(
                "      法条: {}{}{}",
                if short.is_empty() { &law.law_name } else { short },
                law.article_no.as_deref().unwrap_or(""),
                if eff.is_empty() { String::new() } else { format!(" (生效:{})", eff) }
            );
            if !law.summary.is_empty() {
                println!("        摘要: {}", law.summary.chars().take(80).collect::<String>());
            }
        }
        // 处置经验（每次审核的 理由 + 建议）
        for (n, e) in h.experiences.iter().enumerate() {
            println!(
                "      经验[{}] 理由: {} | 建议: {}",
                n,
                e.reason.chars().take(60).collect::<String>(),
                e.suggestion.chars().take(60).collect::<String>()
            );
        }
        // 关联的案例（独立 Case 节点，设计文档 §9.4.4）
        for c in &h.cases {
            println!(
                "      案例: {} ({} {}{}{})",
                c.title.chars().take(60).collect::<String>(),
                c.issuing_body,
                c.case_type,
                if c.year.is_some() { format!(" {}", c.year.unwrap()) } else { String::new() },
                if c.summary.is_empty() { String::new() } else { format!(" — {}", c.summary.chars().take(60).collect::<String>()) }
            );
        }
        // 负面清单规则
        for r in &h.rules {
            println!(
                "      负面清单[{}]: {}",
                r.severity,
                r.content.chars().take(100).collect::<String>()
            );
        }
        // 审查维度
        if let Some(dim) = &h.dimension {
            println!("      维度: {}（{}）", dim.name, dim.dimension_id);
        }
    }
    Ok(())
}
