//! 组长：写库调试入口 — 读 entities_decisions.jsonl → 写入 Neo4j。
//!
//! 用法: cargo run --bin graph_write <entities_decisions.jsonl>
//!   默认样例: output/findings/sample_entities_decisions.jsonl
//!
//! ⚠️ 这只是独立调试用。8/4 整合后改走 `knowledge::run::run`，输入变为内存 Vec。

use std::fs;

use anyhow::{Context, Result};

use ai_bid::knowledge::graph::Neo4jClient;
use ai_bid::knowledge::types::{Decision, EntityDecision};
use ai_bid::paths::data_path;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    let path = resolve_path(std::env::args().nth(1));

    let decisions = read_jsonl(&path.to_string_lossy())?;
    let new_count = decisions
        .iter()
        .filter(|d| d.decision == Decision::New)
        .count();
    let exists_count = decisions.len() - new_count;

    let client = Neo4jClient::connect().await?;
    client.write(decisions).await?;

    println!("写入 Neo4j 完成：new={}, exists(跳过)={}", new_count, exists_count);
    Ok(())
}

/// 解析输入文件路径：
/// 1. 无参数 → 默认 `output/findings/sample_entities_decisions.jsonl`（按 AIBID_DATA_DIR 解析）
/// 2. 有参数 → 若文件存在直接用；否则回退到 AIBID_DATA_DIR 下解析
fn resolve_path(arg: Option<String>) -> std::path::PathBuf {
    match arg {
        Some(p) => {
            let pb = std::path::PathBuf::from(&p);
            if pb.is_file() {
                pb
            } else {
                data_path(&p)
            }
        }
        None => data_path("output/findings/sample_entities_decisions.jsonl"),
    }
}

fn read_jsonl(path: &str) -> Result<Vec<EntityDecision>> {
    let text = fs::read_to_string(path).with_context(|| {
        format!(
            "读取 {} 失败。\n  提示：样例数据在项目根的 output/findings/ 下。\n  从 backend-rust/ 运行请先执行: $env:AIBID_DATA_DIR=\"..\"",
            path
        )
    })?;
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).with_context(|| format!("解析 {} 失败", path)))
        .collect()
}
