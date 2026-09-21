//! `search_graph_knowledge` 工具 — 检索沉淀在 Neo4j 图库中的历史审核经验。
//!
//! 与 `search_knowledge_base`（Qdrant 向量库，法规/案例原文）互补：
//! - 本工具连 Neo4j，返回**结构化图知识**：风险实体 + 关联法条/条款 + 历史处置经验
//!   （RiskExperience：事实锚点 source_quote / 论证理由 reason / 处置建议 suggestion / 置信度）
//! - 对应设计文档 §9.4 运行时检索 Step 1「图查询（精确匹配）」：
//!   同一类风险上次是怎么判的、建议怎么改。
//!
//! 图库数据来源：`knowledge::run::run`（审核完成后自动沉淀，CLI 与 HTTP 两条链路均已接入）。

use crate::agents::tools::AgentTool;
use crate::knowledge::graph::Neo4jClient;
use anyhow::{Context, Result};
use serde::Deserialize;

const MAX_RESULTS: usize = 5;

#[derive(Debug, Deserialize)]
pub struct SearchGraphKnowledgeArgs {
    /// 风险关键词 / 风险类型（如 "地域限制"、"指定品牌"）
    pub risk_keyword: String,
    /// 返回最多命中数（默认 5）
    #[serde(default)]
    pub limit: Option<usize>,
}

pub struct SearchGraphKnowledgeTool;

impl SearchGraphKnowledgeTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl AgentTool for SearchGraphKnowledgeTool {
    fn name(&self) -> &str {
        "search_graph_knowledge"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "search_graph_knowledge",
                "description": "检索历史审核沉淀 —— 从 Neo4j 知识图谱取回同类风险实体的结构化信息，\n\
                    包括：风险名称/严重度、关联法条与条款号、负面清单规则、所属审查维度、\n\
                    以及最重要的【历史处置经验】（同一类风险上一次是怎么论证的、建议怎么改）。\n\
                    \n\
                    【使用场景】\n\
                    ① 当前发现某类风险，想借鉴此前同类风险的处置经验和论证理由\n\
                    ② 需要快速确认某风险关联了哪些法条/条款/负面清单规则\n\
                    ③ 复核环节核对上次审核的处置建议\n\
                    \n\
                    【不使用场景】\n\
                    ① 查法规/判例原文 → 用 search_knowledge_base（Qdrant）\n\
                    ② 查标书内条款 → 用 search_document",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "risk_keyword": {
                            "type": "string",
                            "description": "风险关键词或风险类型名，如 '地域限制'、'指定品牌'、'资格条件'"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "最多返回命中条数（默认 5）"
                        }
                    },
                    "required": ["risk_keyword"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: SearchGraphKnowledgeArgs = serde_json::from_value(args)?;
        let q = parsed.risk_keyword.clone();
        let limit = parsed.limit.unwrap_or(MAX_RESULTS).min(MAX_RESULTS);

        let client = Neo4jClient::connect()
            .await
            .context("Neo4j 连接失败（知识图谱不可用）")?;
        let hits = client.search(&q).await?;

        eprintln!(
            "[search_graph_knowledge] keyword={:?} => {} hits",
            q,
            hits.len()
        );

        let hits: Vec<serde_json::Value> = hits
            .into_iter()
            .take(limit)
            .map(|h| {
                serde_json::json!({
                    "risk": {
                        "name": h.risk.name,
                        "risk_id": h.risk.id,
                        "severity": h.risk.severity,
                    },
                    "dimension": h.dimension.as_ref().map(|d| serde_json::json!({
                        "name": d.name,
                        "description": d.description,
                    })),
                    "laws": h.laws.iter().map(|l| serde_json::json!({
                        "law_id": l.law_id,
                        "name": if l.short_name.is_empty() { l.law_name.clone() } else { l.short_name.clone() },
                        "article_no": l.article_no,
                        "summary": l.summary.chars().take(200).collect::<String>(),
                    })).collect::<Vec<_>>(),
                    "rules": h.rules.iter().map(|r| serde_json::json!({
                        "rule_id": r.rule_id,
                        "content": r.content.chars().take(200).collect::<String>(),
                        "severity": r.severity,
                    })).collect::<Vec<_>>(),
                    "experiences": h.experiences.iter().map(|e| serde_json::json!({
                        "source_quote": e.source_quote.chars().take(200).collect::<String>(),
                        "reason": e.reason.chars().take(300).collect::<String>(),
                        "suggestion": e.suggestion.chars().take(300).collect::<String>(),
                        "confidence": e.confidence,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();

        Ok(serde_json::json!({
            "source": "neo4j_knowledge_graph",
            "query": q,
            "total_hits": hits.len(),
            "hits": hits,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::tools::AgentTool;
    use crate::knowledge::graph::Neo4jClient;
    use crate::knowledge::types::{Decision, EntityDecision, LawArticleEntity, RiskEntity, RiskExperience};

    /// 预置一条带有唯一风险名的决策，供 live round-trip 测试检索。
    fn sample_decision() -> EntityDecision {
        EntityDecision {
            candidate_id: "candidate_graph_tool_001".to_string(),
            decision: Decision::New,
            risk: RiskEntity {
                id: "risk_graph_tool_001".to_string(),
                name: "G6图库工具测试-指定品牌唯一来源".to_string(),
                severity: "HIGH".to_string(),
            },
            laws: vec![LawArticleEntity {
                law_id: "law_graph_tool_001".to_string(),
                law_name: "中华人民共和国政府采购法".to_string(),
                short_name: "采购法".to_string(),
                article_id: Some("article_graph_tool_001".to_string()),
                article_no: Some("第八十六条".to_string()),
                summary: "测试条款摘要".to_string(),
                meta: None,
            }],
            cases: vec![],
            rules: vec![],
            dimension: None,
            risk_experience: RiskExperience {
                experience_id: "exp_graph_tool_001".to_string(),
                candidate_id: "candidate_graph_tool_001".to_string(),
                source_quote: "招标文件第七条指定唯一品牌".to_string(),
                reason: "图库工具测试论证理由：指定品牌排斥竞争".to_string(),
                suggestion: "图库工具测试处置建议：改为三项及以上品牌或技术参数描述".to_string(),
                confidence: 0.9,
            },
        }
    }

    /// LLM 看到的工具契约必须自洽：工具名唯一、risk_keyword 必填、limit 可选整数。
    #[test]
    fn test_name_and_definition_schema() {
        let tool = SearchGraphKnowledgeTool::new();
        assert_eq!(tool.name(), "search_graph_knowledge");
        let def = tool.definition();
        assert_eq!(def["function"]["name"], "search_graph_knowledge");
        let required = def["function"]["parameters"]["required"]
            .as_array()
            .expect("required 应为数组");
        assert!(required.iter().any(|v| v == "risk_keyword"), "risk_keyword 必须为必填");
        assert!(!required.iter().any(|v| v == "limit"), "limit 应可选");
        assert_eq!(def["function"]["parameters"]["properties"]["risk_keyword"]["type"], "string");
        assert_eq!(def["function"]["parameters"]["properties"]["limit"]["type"], "integer");
    }

    /// 缺 risk_keyword 必须在连库之前就参数校验失败。
    #[tokio::test]
    async fn test_execute_rejects_missing_keyword() {
        let tool = SearchGraphKnowledgeTool::new();
        let res = tool.execute(serde_json::json!({})).await;
        assert!(res.is_err(), "缺 risk_keyword 必须报参数错误");
    }

    /// 真实验证（需 Neo4j，与 test_law_round_trip 同款跳过策略）：
    /// 预置一条风险+法条+处置经验 → 调工具 execute() → 断言结构化结果取回了经验/法条/置信度。
    /// 这是 P0-3「工具真的能从图库取回经验」的直接证据，不是接线断言。
    #[tokio::test]
    async fn test_execute_round_trip_vs_graph() {
        let client = match Neo4jClient::connect().await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[test_execute_round_trip_vs_graph] 跳过：无法连接 Neo4j — {e}");
                return;
            }
        };

        let decision = sample_decision();
        client
            .write(vec![decision])
            .await
            .expect("预置图库测试数据失败");

        let tool = SearchGraphKnowledgeTool::new();
        let resp = tool
            .execute(serde_json::json!({
                "risk_keyword": "G6图库工具测试-指定品牌唯一来源",
                "limit": 5,
            }))
            .await
            .expect("工具执行应成功");

        assert_eq!(resp["source"], "neo4j_knowledge_graph");
        assert!(resp["total_hits"].as_u64().unwrap_or(0) >= 1, "必须命中预置的风险");

        let hit = resp["hits"]
            .as_array()
            .expect("hits 应为数组")
            .iter()
            .find(|h| h["risk"]["risk_id"] == "risk_graph_tool_001")
            .unwrap_or_else(|| panic!("必须能查到预置的风险节点"));

        let exps = hit["experiences"].as_array().expect("experiences 应为数组");
        assert!(!exps.is_empty(), "预置的处置经验必须随工具返回");
        assert_eq!(exps[0]["suggestion"], "图库工具测试处置建议：改为三项及以上品牌或技术参数描述");
        assert_eq!(exps[0]["reason"], "图库工具测试论证理由：指定品牌排斥竞争");
        let conf = exps[0]["confidence"].as_f64().unwrap_or_default();
        assert!(
            (conf - 0.9).abs() < 1e-3,
            "置信度应近似 0.9（Neo4j 存 f32、读回 f64 允许精度误差），实际 {conf}"
        );

        let laws = hit["laws"].as_array().expect("laws 应为数组");
        assert!(!laws.is_empty(), "关联法条应随工具返回");
        assert_eq!(laws[0]["article_no"], "第八十六条");
    }
}