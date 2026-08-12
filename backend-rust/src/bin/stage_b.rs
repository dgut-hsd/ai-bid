//! 读 candidates.jsonl + existing_ids.txt → 写 entities_decisions.jsonl。
//!
//! 用法: cargo run --bin stage_b <candidates.jsonl> [existing_ids.txt] [输出文件]
//!   默认: candidates.jsonl / existing_ids.txt / entities_decisions.jsonl
//!
//! ⚠️ 这只是独立调试用。8/4 整合后改走 `knowledge::run::run`，
//!    查重集合由组长从 Neo4j 查，不再读文件。

use std::collections::HashSet;
use std::fs;

use anyhow::{Context, Result};

use ai_bid::knowledge::extract::extract_and_dedup;
use ai_bid::knowledge::types::Candidate;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let cand_path = args.next().unwrap_or_else(|| "candidates.jsonl".to_string());
    let existing_path = args.next().unwrap_or_else(|| "existing_ids.txt".to_string());
    let out = args.next().unwrap_or_else(|| "entities_decisions.jsonl".to_string());

    let candidates = read_jsonl::<Candidate>(&cand_path)?;
    let existing = read_id_lines(&existing_path)?;
    let decisions = extract_and_dedup(candidates, &existing);
    write_jsonl(&out, &decisions)?;
    let new = decisions.iter().filter(|d| d.decision == ai_bid::knowledge::types::Decision::New).count();
    println!(
        "共 {} 条决策（new={} / exists={}）→ {}",
        decisions.len(),
        new,
        decisions.len() - new,
        out
    );
    Ok(())
}

fn read_jsonl<T: serde::de::DeserializeOwned>(path: &str) -> Result<Vec<T>> {
    let text = fs::read_to_string(path).with_context(|| format!("读取 {}", path))?;
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).with_context(|| format!("解析 {} 失败", path)))
        .collect()
}

fn read_id_lines(path: &str) -> Result<HashSet<String>> {
    let text = fs::read_to_string(path).with_context(|| format!("读取 {}", path))?;
    Ok(text
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

fn write_jsonl<T: serde::Serialize>(path: &str, items: &[T]) -> Result<()> {
    let mut s = String::new();
    for it in items {
        s.push_str(&serde_json::to_string(it)?);
        s.push('\n');
    }
    fs::write(path, s).with_context(|| format!("写入 {}", path))?;
    Ok(())
}
