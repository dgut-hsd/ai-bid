//! 读审核结果 → 写 candidates.jsonl。
//!
//! 用法: cargo run --bin stage_a [findings文件|目录] [输出文件]
//!   findings文件: 单个 `*_findings.json`；目录: 该目录下所有 `*_findings.json`
//!   输出文件: 默认 `./candidates.jsonl`
//!
//! ⚠️ 这只是独立调试用。8/4 整合后改走 `knowledge::run::run` 的内存串联，不再落盘。

use std::path::PathBuf;

use anyhow::{Context, Result};

use ai_bid::agents::types::RiskFinding;
use ai_bid::knowledge::collect::collect_candidates;
use ai_bid::knowledge::types::Candidate;
use ai_bid::paths::data_path_str;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let input = args.next().unwrap_or_else(|| data_path_str("output/findings"));
    let out = args.next().unwrap_or_else(|| "candidates.jsonl".to_string());

    let findings = load_findings(&input)?;
    let candidates = collect_candidates(&findings);
    write_jsonl(&out, &candidates)?;
    println!("读入 {} 条 finding，挑出 {} 条精华 → {}", findings.len(), candidates.len(), out);
    Ok(())
}

/// 从单个文件或目录加载所有 RiskFinding（TODO(组员A): 按需完善路径处理）。
fn load_findings(input: &str) -> Result<Vec<RiskFinding>> {
    let path = PathBuf::from(input);
    let mut all = Vec::new();
    let files: Vec<PathBuf> = if path.is_dir() {
        std::fs::read_dir(&path)
            .with_context(|| format!("读取目录 {}", path.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension().map(|e| e == "json").unwrap_or(false)
                    && p
                        .file_name()
                        .map(|n| n.to_string_lossy().ends_with("_findings.json"))
                        .unwrap_or(false)
            })
            .collect()
    } else {
        vec![path]
    };

    for p in files {
        let data = std::fs::read(&p).with_context(|| format!("读取 {}", p.display()))?;
        let list: Vec<RiskFinding> = serde_json::from_slice(&data)?;
        all.extend(list);
    }
    Ok(all)
}

fn write_jsonl(path: &str, items: &[Candidate]) -> Result<()> {
    let mut s = String::new();
    for it in items {
        s.push_str(&serde_json::to_string(it)?);
        s.push('\n');
    }
    std::fs::write(path, s).with_context(|| format!("写入 {}", path))?;
    Ok(())
}
