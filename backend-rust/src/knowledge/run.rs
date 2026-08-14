//! 组长：整合主流程 — 内存函数串联，无中间文件。

use anyhow::Result;

use crate::agents::types::RiskFinding;
use crate::knowledge::collect::collect_candidates;
use crate::knowledge::extract::extract_and_dedup;
use crate::knowledge::graph::Neo4jClient;
use crate::knowledge::types::Decision;

/// 审核结果 → 挑精华 → 查重 → 写入 Neo4j，返回新写入数量。
pub async fn run(findings: Vec<RiskFinding>, client: &Neo4jClient) -> Result<usize> {
    let candidates = collect_candidates(&findings);
    let existing = client.all_law_ids().await?;
    let decisions = extract_and_dedup(candidates, &existing);
    let new_count = decisions
        .iter()
        .filter(|d| d.decision == Decision::New)
        .count();
    client.write(decisions).await?;
    Ok(new_count)
}
