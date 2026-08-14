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
            println!(
                "      法条: {}{}",
                law.law_name,
                law.article_no.as_deref().unwrap_or("")
            );
        }
        if !h.snippet.is_empty() {
            println!("      摘录: {}", h.snippet.chars().take(80).collect::<String>());
        }
    }
    Ok(())
}
