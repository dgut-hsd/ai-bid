//! 知识沉淀全链路演示入口：审核结果 → 挑精华 → 查重 → 写库 → 查询。
//!
//! 用法:
//!   cargo run --release --bin knowledge_pipeline [<findings.json>]
//!
//! 参数缺省时默认 output/findings/test_3c7c15eb_findings.json（按 AIBID_DATA_DIR 解析）。
//! 从 backend-rust/ 运行请先: $env:AIBID_DATA_DIR=".."
//!
//! 整合逻辑见 `knowledge::run::run`；本入口只负责读文件 + 展示。

use std::fs;

use anyhow::{Context, Result};

use ai_bid::agents::types::RiskFinding;
use ai_bid::knowledge::graph::Neo4jClient;
use ai_bid::knowledge::run::run;
use ai_bid::paths::data_path;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();

    let path = match std::env::args().nth(1) {
        Some(p) => {
            let pb = std::path::PathBuf::from(&p);
            if pb.is_file() { pb } else { data_path(&p) }
        }
        None => data_path("output/findings/test_3c7c15eb_findings.json"),
    };

    let text = fs::read_to_string(&path).with_context(|| {
        format!(
            "读取 {} 失败。\n  提示: 从 backend-rust/ 运行请先执行 $env:AIBID_DATA_DIR=\"..\"",
            path.display()
        )
    })?;
    let findings: Vec<RiskFinding> = serde_json::from_str(&text)
        .with_context(|| format!("解析 {} 失败（应为 RiskFinding 数组 JSON）", path.display()))?;
    println!("读取审核结果: {} 条发现 <- {}", findings.len(), path.display());

    let client = Neo4jClient::connect().await?;
    let new_count = run(findings, &client).await?;
    println!("整合流水线完成：新增 {} 条（已存在自动跳过）", new_count);

    // 演示查询
    println!("\n=== 演示查询 ===");
    for kw in ["品牌", "资格", "指定"] {
        let hits = client.search(kw).await?;
        println!("查询『{}』→ {} 条风险:", kw, hits.len());
        for h in &hits {
            println!(
                "  - [{}] {}（id: {}, 候选: {:?}）",
                h.risk.severity, h.risk.name, h.risk.id, h.candidate_ids
            );
            for law in &h.laws {
                println!(
                    "      法条: {}{}",
                    law.law_name,
                    law.article_no.as_deref().unwrap_or("")
                );
            }
            if !h.snippet.is_empty() {
                let s: String = h.snippet.chars().take(60).collect();
                println!("      摘录: {}…", s);
            }
        }
    }
    Ok(())
}
