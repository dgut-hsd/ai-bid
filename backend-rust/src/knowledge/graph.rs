//! 组长：Neo4j 访问层。
//!
//! 环境变量：
//!   - `NEO4J_URI`       默认 `bolt://localhost:7687`
//!   - `NEO4J_USER`      默认 `neo4j`
//!   - `NEO4J_PASSWORD`  无默认，必须通过环境变量或 `.env` 提供

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use neo4rs::{query, Graph};

use crate::knowledge::types::{
    CaseEntity, Decision, DimensionEntity, EntityDecision, LawArticleEntity, RiskEntity,
    RiskExperience, RuleEntity, SearchHit, PRESET_DIMENSIONS,
};

/// Neo4j 连接封装。
pub struct Neo4jClient {
    graph: Graph,
}

impl Neo4jClient {
    /// 连接 Neo4j，参数从环境变量读取。
    pub async fn connect() -> Result<Self> {
        let uri = std::env::var("NEO4J_URI").unwrap_or_else(|_| "bolt://localhost:7687".into());
        let user = std::env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".into());
        let password = std::env::var("NEO4J_PASSWORD").unwrap_or_default();
        let graph = Graph::new(uri.as_str(), user.as_str(), password.as_str())
            .await
            .with_context(|| {
                format!(
                    "无法连接 Neo4j: {}（请先启动容器，并检查 NEO4J_URI / NEO4J_USER / NEO4J_PASSWORD）",
                    uri
                )
            })?;
        Ok(Self { graph })
    }

    /// 库中已有的所有 law_id 集合（查重数据源）。
    pub async fn all_law_ids(&self) -> Result<HashSet<String>> {
        let mut result = self
            .graph
            .execute(query("MATCH (l:Law) RETURN l.law_id AS law_id"))
            .await?;
        let mut ids = HashSet::new();
        while let Some(row) = result.next().await? {
            let id: String = row.get("law_id")?;
            ids.insert(id);
        }
        Ok(ids)
    }

    /// 写入实体（全部 MERGE，幂等，可重复执行）。
    ///
    /// 图模型遵循《设计.md》§9.4.4：
    ///   (Risk)-[:cites]->(Law)，(Law)-[:has_article]->(Article)
    ///   (Risk)-[:cites]->(Case)，(Article)-[:cited_in]->(Case)
    ///   (Risk)-[:exemplifies]->(ProhibitionRule)，(ProhibitionRule)-[:based_on]->(Article)
    ///   (Article)-[:applies_to]->(Dimension)
    ///
    /// 注意：即便 `decision == Exists`（仅法条已存在），也始终确保 Risk 节点存在（upsert_risk 幂等），
    /// 并补全 Case/ProhibitionRule/Dimension 及关系——dedup 只依 law_id 判定，不能假设 Risk 已落库。
    /// upsert_risk 仅在节点创建时写入 snippet（ON CREATE），符合 P3"不覆盖历史摘录"。
    pub async fn write(&self, decisions: Vec<EntityDecision>) -> Result<()> {
        self.ensure_dimensions().await?;
        for d in decisions {
            self.upsert_risk(&d).await?;
            for law in &d.laws {
                self.upsert_law(&d, law).await?;
                self.upsert_article(&d, law).await?;
            }
            for c in &d.cases {
                self.upsert_case(&d, c).await?;
            }
            for rule in &d.rules {
                self.upsert_rule(&d, rule).await?;
            }
            self.link_dimension(&d).await?;
        }
        Ok(())
    }

    /// upsert Risk 节点，并为本次审核创建一个 RiskExperience 节点（MERGE 幂等）并挂
/// (Risk)-[:has_experience]->(RiskExperience)。Neo4j 属性值禁止 map 数组，故经验独立成节点。
    async fn upsert_risk(&self, d: &EntityDecision) -> Result<()> {
        let risk_cql = r"
MERGE (r:Risk {risk_id: $risk_id})
ON CREATE SET r.name = $risk_name, r.severity = $severity";
        self.graph
            .run(
                query(risk_cql)
                    .param("risk_id", d.risk.id.as_str())
                    .param("risk_name", d.risk.name.as_str())
                    .param("severity", d.risk.severity.as_str()),
            )
            .await
            .context("写入 Risk 节点失败")?;

        let e = &d.risk_experience;
        let exp_cql = r"
MERGE (r:Risk {risk_id: $risk_id})
MERGE (e:RiskExperience {experience_id: $experience_id})
ON CREATE SET e.candidate_id = $candidate_id, e.source_quote = $source_quote,
              e.reason = $reason, e.suggestion = $suggestion, e.confidence = $confidence
MERGE (r)-[:has_experience]->(e)";
        self.graph
            .run(
                query(exp_cql)
                    .param("risk_id", d.risk.id.as_str())
                    .param("experience_id", e.experience_id.as_str())
                    .param("candidate_id", e.candidate_id.as_str())
                    .param("source_quote", e.source_quote.as_str())
                    .param("reason", e.reason.as_str())
                    .param("suggestion", e.suggestion.as_str())
                    .param("confidence", e.confidence),
            )
            .await
            .context("写入 RiskExperience 节点失败")?;
        Ok(())
    }

    /// upsert Law 节点：确定性 ID + 元数据（short_name / effective_date / status / level / issuing_body / doc_number / year）。
    /// 关系 (Risk)-[:cites]->(Law)。节点与关系分开 MERGE，避免重复节点。
    async fn upsert_law(&self, d: &EntityDecision, law: &LawArticleEntity) -> Result<()> {
        let cql = r"
MATCH (r:Risk {risk_id: $risk_id})
MERGE (l:Law {law_id: $law_id})
ON CREATE SET l.name = $name, l.short_name = $short_name,
              l.effective_date = $effective_date, l.status = $status,
              l.level = $level, l.issuing_body = $issuing_body,
              l.doc_number = $doc_number, l.year = $year
ON MATCH  SET l.name = coalesce(l.name, $name),
              l.short_name = coalesce(l.short_name, $short_name),
              l.effective_date = coalesce(l.effective_date, $effective_date),
              l.status = coalesce(l.status, $status),
              l.level = coalesce(l.level, $level),
              l.issuing_body = coalesce(l.issuing_body, $issuing_body),
              l.doc_number = coalesce(l.doc_number, $doc_number),
              l.year = coalesce(l.year, $year)
MERGE (r)-[:cites]->(l)";
        let meta = law.meta.as_ref();
        self.graph
            .run(
                query(cql)
                    .param("risk_id", d.risk.id.as_str())
                    .param("law_id", law.law_id.as_str())
                    .param("name", law.law_name.as_str())
                    .param("short_name", law.short_name.as_str())
                    .param("effective_date", meta.map(|m| m.effective_date.as_str()).unwrap_or(""))
                    .param("status", meta.map(|m| m.status.as_str()).unwrap_or("effective"))
                    .param("level", meta.map(|m| m.level.as_str()).unwrap_or(""))
                    .param("issuing_body", meta.map(|m| m.issuing_body.as_str()).unwrap_or(""))
                    .param("doc_number", meta.map(|m| m.doc_number.as_str()).unwrap_or(""))
                    .param("year", meta.and_then(|m| m.year.as_deref()).unwrap_or("")),
            )
            .await
            .context("写入 Law 节点失败")?;
        Ok(())
    }

    /// upsert Article 节点：确定性 ID + 条款号；关系 (Law)-[:has_article]->(Article)。
    /// full_text / summary 只写条款自身内容（来自外部条款库），不承载单次审核的 reason/摘录。
    async fn upsert_article(&self, d: &EntityDecision, law: &LawArticleEntity) -> Result<()> {
        let Some(article_id) = &law.article_id else {
            return Ok(());
        };
        let cql = r"
MATCH (l:Law {law_id: $law_id})
MERGE (a:Article {article_id: $article_id})
ON CREATE SET a.law_id = $law_id, a.article_no = $article_no
ON MATCH  SET a.law_id = coalesce(a.law_id, $law_id),
              a.article_no = coalesce(a.article_no, $article_no)
MERGE (l)-[:has_article]->(a)";
        self.graph
            .run(
                query(cql)
                    .param("law_id", law.law_id.as_str())
                    .param("article_id", article_id.as_str())
                    .param("article_no", law.article_no.as_deref().unwrap_or("")),
            )
            .await
            .context("写入 Article 节点失败")?;

        // (Article)-[:applies_to]->(Dimension)
        if let Some(dim) = &d.dimension {
            let cql = r"
MATCH (a:Article {article_id: $article_id})
MERGE (dim:Dimension {dimension_id: $dim_id})
ON CREATE SET dim.name = $name, dim.description = $description
MERGE (a)-[:applies_to]->(dim)";
            self.graph
                .run(
                    query(cql)
                        .param("article_id", article_id.as_str())
                        .param("dim_id", dim.dimension_id.as_str())
                        .param("name", dim.name.as_str())
                        .param("description", dim.description.as_str()),
                )
                .await
                .context("写入 Article→Dimension 关系失败")?;
        }
        Ok(())
    }

    /// upsert Case 节点：确定性 ID + 属性；关系 (Risk)-[:cites]->(Case) 与
    /// (Article)-[:cited_in]->(Case)（当该 candidate 同时引用条款时）。
    async fn upsert_case(&self, d: &EntityDecision, c: &CaseEntity) -> Result<()> {
        let cql = r"
MATCH (r:Risk {risk_id: $risk_id})
MERGE (c:Case {case_id: $case_id})
ON CREATE SET c.title = $title, c.issuing_body = $issuing_body,
              c.year = $year, c.case_type = $case_type,
              c.summary = $summary, c.full_text = $full_text, c.keywords = $keywords
ON MATCH  SET c.title = coalesce(c.title, $title),
              c.issuing_body = coalesce(c.issuing_body, $issuing_body),
              c.year = coalesce(c.year, $year),
              c.case_type = coalesce(c.case_type, $case_type),
              c.summary = coalesce(c.summary, $summary),
              c.full_text = coalesce(c.full_text, $full_text),
              c.keywords = coalesce(c.keywords, $keywords)
MERGE (r)-[:cites]->(c)";
        self.graph
            .run(
                query(cql)
                    .param("risk_id", d.risk.id.as_str())
                    .param("case_id", c.case_id.as_str())
                    .param("title", c.title.as_str())
                    .param("issuing_body", c.issuing_body.as_str())
                    .param("year", c.year.unwrap_or(0))
                    .param("case_type", c.case_type.as_str())
                    .param("summary", c.summary.as_str())
                    .param("full_text", c.full_text.as_str())
                    .param("keywords", c.keywords.as_slice()),
            )
            .await
            .context("写入 Case 节点失败")?;

        // (Article)-[:cited_in]->(Case)：本候选引用的每一条款都可追溯引用此案例
        for law in &d.laws {
            if let Some(article_id) = &law.article_id {
                let cql = r"
MATCH (a:Article {article_id: $article_id})
MATCH (c:Case {case_id: $case_id})
MERGE (a)-[:cited_in]->(c)";
                self.graph
                    .run(
                        query(cql)
                            .param("article_id", article_id.as_str())
                            .param("case_id", c.case_id.as_str()),
                    )
                    .await
                    .context("写入 Article→Case cited_in 关系失败")?;
            }
        }
        Ok(())
    }

    /// upsert ProhibitionRule 节点：确定性 ID；关系 (Risk)-[:exemplifies]->(Rule) 与
    /// (Rule)-[:based_on]->(Article)。
    async fn upsert_rule(&self, d: &EntityDecision, rule: &RuleEntity) -> Result<()> {
        let cql = r"
MATCH (r:Risk {risk_id: $risk_id})
MERGE (p:ProhibitionRule {rule_id: $rule_id})
ON CREATE SET p.content = $content, p.source = $source,
              p.category = $category, p.severity = $severity
ON MATCH  SET p.content = coalesce(p.content, $content),
              p.source = coalesce(p.source, $source),
              p.category = coalesce(p.category, $category),
              p.severity = coalesce(p.severity, $severity)
MERGE (r)-[:exemplifies]->(p)";
        self.graph
            .run(
                query(cql)
                    .param("risk_id", d.risk.id.as_str())
                    .param("rule_id", rule.rule_id.as_str())
                    .param("content", rule.content.as_str())
                    .param("source", rule.source.as_str())
                    .param("category", rule.category.as_str())
                    .param("severity", rule.severity.as_str()),
            )
            .await
            .context("写入 ProhibitionRule 节点失败")?;

        // (ProhibitionRule)-[:based_on]->(Article)
        for law in &d.laws {
            if let Some(article_id) = &law.article_id {
                let cql = r"
MATCH (p:ProhibitionRule {rule_id: $rule_id})
MATCH (a:Article {article_id: $article_id})
MERGE (p)-[:based_on]->(a)";
                self.graph
                    .run(
                        query(cql)
                            .param("rule_id", rule.rule_id.as_str())
                            .param("article_id", article_id.as_str()),
                    )
                    .await
                    .context("写入 ProhibitionRule→Article based_on 关系失败")?;
            }
        }
        Ok(())
    }

    /// 预置审查维度节点（幂等）。
    async fn ensure_dimensions(&self) -> Result<()> {
        for (dim_id, name, desc) in PRESET_DIMENSIONS {
            let cql = "MERGE (dim:Dimension {dimension_id: $dim_id})
ON CREATE SET dim.name = $name, dim.description = $description";
            self.graph
                .run(
                    query(cql)
                        .param("dim_id", dim_id)
                        .param("name", name)
                        .param("description", desc),
                )
                .await
                .context("写入预置 Dimension 节点失败")?;
        }
        Ok(())
    }

    /// Exists 分支下确保维度关系被补全（Article 已存在时仅补 applies_to）。
    async fn link_dimension(&self, d: &EntityDecision) -> Result<()> {
        if let Some(dim) = &d.dimension {
            for law in &d.laws {
                if let Some(article_id) = &law.article_id {
                    let cql = r"
MATCH (a:Article {article_id: $article_id})
MERGE (dim:Dimension {dimension_id: $dim_id})
ON CREATE SET dim.name = $name, dim.description = $description
MERGE (a)-[:applies_to]->(dim)";
                    self.graph
                        .run(
                            query(cql)
                                .param("article_id", article_id.as_str())
                                .param("dim_id", dim.dimension_id.as_str())
                                .param("name", dim.name.as_str())
                                .param("description", dim.description.as_str()),
                        )
                        .await
                        .context("写入 Article→Dimension 关系失败")?;
                }
            }
        }
        Ok(())
    }

    /// 关键词查询风险及关联的法条/条款/案例/负面清单/维度（按风险名匹配）。
    /// 多查询、按行对齐填充，避免 DISTINCT collect 的字段错位问题。
    pub async fn search(&self, q: &str) -> Result<Vec<SearchHit>> {
        let mut hits: Vec<SearchHit> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();

        // 第一查询：Risk + 关联 Law/Article
        let cql = r"
MATCH (r:Risk)
WHERE r.name CONTAINS $q
OPTIONAL MATCH (r)-[:cites]->(l:Law)
OPTIONAL MATCH (l)-[:has_article]->(a:Article)
RETURN r.risk_id AS risk_id, r.name AS risk_name, r.severity AS severity,
       coalesce(l.law_id, '') AS law_id, coalesce(l.name, '') AS name,
       coalesce(l.short_name, '') AS short_name,
       coalesce(a.article_id, '') AS article_id, coalesce(a.article_no, '') AS article_no,
       coalesce(a.summary, '') AS article_summary";
        let mut result = self.graph.execute(query(cql).param("q", q)).await?;
        while let Some(row) = result.next().await? {
            let risk_id: String = row.get("risk_id")?;
            let risk_name: String = row.get("risk_name")?;
            let severity: String = row.get("severity")?;
            let law_id: String = row.get("law_id")?;
            let name: String = row.get("name")?;
            let short_name: String = row.get("short_name")?;
            let article_id: String = row.get("article_id")?;
            let article_no: String = row.get("article_no")?;
            let article_summary: String = row.get("article_summary")?;

            let idx = match index.get(&risk_id) {
                Some(&i) => i,
                None => {
                    hits.push(SearchHit {
                        risk: RiskEntity {
                            id: risk_id.clone(),
                            name: risk_name,
                            severity,
                        },
                        laws: Vec::new(),
                        snippet: String::new(),
                        cases: Vec::new(),
                        rules: Vec::new(),
                        experiences: Vec::new(),
                        dimension: None,
                    });
                    index.insert(risk_id.clone(), hits.len() - 1);
                    hits.len() - 1
                }
            };

            if law_id.is_empty() {
                continue;
            }
            let key = format!("{law_id}:{article_id}");
            if hits[idx]
                .laws
                .iter()
                .any(|l| format!("{}:{}", l.law_id, l.article_id.as_deref().unwrap_or("")) == key)
            {
                continue;
            }
            hits[idx].laws.push(LawArticleEntity {
                law_id,
                law_name: name,
                short_name,
                article_id: (!article_id.is_empty()).then_some(article_id),
                article_no: (!article_no.is_empty()).then_some(article_no),
                summary: article_summary,
                meta: None,
            });
        }

        // 第二查询：Risk + 关联 Case（每个命中风险可能引用多个案例）
        let case_cql = r"
MATCH (r:Risk)
WHERE r.name CONTAINS $q
OPTIONAL MATCH (r)-[:cites]->(c:Case)
RETURN r.risk_id AS risk_id,
       coalesce(c.case_id, '') AS case_id,
       coalesce(c.title, '') AS title,
       coalesce(c.issuing_body, '') AS issuing_body,
       coalesce(c.year, 0) AS year,
       coalesce(c.case_type, '') AS case_type,
       coalesce(c.summary, '') AS summary,
       coalesce(c.full_text, '') AS full_text,
       coalesce(c.keywords, []) AS keywords";
        let mut case_result = self
            .graph
            .execute(query(case_cql).param("q", q))
            .await?;
        while let Some(row) = case_result.next().await? {
            let risk_id: String = row.get("risk_id")?;
            let case_id: String = row.get("case_id")?;
            if case_id.is_empty() {
                continue;
            }
            if let Some(&i) = index.get(&risk_id) {
                let year: i64 = row.get("year")?;
                hits[i].cases.push(CaseEntity {
                    case_id,
                    title: row.get("title")?,
                    issuing_body: row.get("issuing_body")?,
                    year: (year != 0).then_some(year),
                    case_type: row.get("case_type")?,
                    summary: row.get("summary")?,
                    full_text: row.get("full_text")?,
                    keywords: row.get("keywords")?,
                });
            }
        }

        // 经验节点查询：Risk + 关联 RiskExperience（每次审核一条处置经验）
        let exp_cql = r"
MATCH (r:Risk)
WHERE r.name CONTAINS $q
OPTIONAL MATCH (r)-[:has_experience]->(e:RiskExperience)
RETURN r.risk_id AS risk_id,
       coalesce(e.experience_id, '') AS experience_id,
       coalesce(e.candidate_id, '') AS candidate_id,
       coalesce(e.source_quote, '') AS source_quote,
       coalesce(e.reason, '') AS reason,
       coalesce(e.suggestion, '') AS suggestion,
       coalesce(e.confidence, 0.0) AS confidence";
        let mut exp_result = self
            .graph
            .execute(query(exp_cql).param("q", q))
            .await?;
        while let Some(row) = exp_result.next().await? {
            let risk_id: String = row.get("risk_id")?;
            let experience_id: String = row.get("experience_id")?;
            if experience_id.is_empty() {
                continue;
            }
            if let Some(&i) = index.get(&risk_id) {
                let confidence: f32 = row.get("confidence")?;
                hits[i].experiences.push(RiskExperience {
                    experience_id,
                    candidate_id: row.get("candidate_id")?,
                    source_quote: row.get("source_quote")?,
                    reason: row.get("reason")?,
                    suggestion: row.get("suggestion")?,
                    confidence,
                });
            }
        }

        // 第三查询：Risk + 关联 ProhibitionRule + Dimension
        let rule_cql = r"
MATCH (r:Risk)
WHERE r.name CONTAINS $q
OPTIONAL MATCH (r)-[:exemplifies]->(p:ProhibitionRule)
OPTIONAL MATCH (r)-[:cites]->(:Law)-[:has_article]->(:Article)-[:applies_to]->(dim:Dimension)
RETURN r.risk_id AS risk_id,
       coalesce(p.rule_id, '') AS rule_id,
       coalesce(p.content, '') AS content,
       coalesce(p.source, '') AS source,
       coalesce(p.category, '') AS category,
       coalesce(p.severity, '') AS severity,
       coalesce(dim.dimension_id, '') AS dimension_id,
       coalesce(dim.name, '') AS dimension_name,
       coalesce(dim.description, '') AS dimension_description";
        let mut rule_result = self
            .graph
            .execute(query(rule_cql).param("q", q))
            .await?;
        while let Some(row) = rule_result.next().await? {
            let risk_id: String = row.get("risk_id")?;
            let Some(&i) = index.get(&risk_id) else {
                continue;
            };
            let rule_id: String = row.get("rule_id")?;
            if !rule_id.is_empty()
                && !hits[i]
                    .rules
                    .iter()
                    .any(|existing| existing.rule_id == rule_id)
            {
                hits[i].rules.push(RuleEntity {
                    rule_id,
                    content: row.get("content")?,
                    source: row.get("source")?,
                    category: row.get("category")?,
                    severity: row.get("severity")?,
                });
            }
            let dimension_id: String = row.get("dimension_id")?;
            if !dimension_id.is_empty() && hits[i].dimension.is_none() {
                hits[i].dimension = Some(DimensionEntity {
                    dimension_id,
                    name: row.get("dimension_name")?,
                    description: row.get("dimension_description")?,
                });
            }
        }

        Ok(hits)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_law_round_trip() {
        // 1. 初始化 Neo4j 客户端（未配置环境时跳过测试）
        let client = match Neo4jClient::connect().await {
            Ok(c) => c,
            Err(e) => {
                // 未配置 Neo4j 环境时跳过（此时静默返回，避免 CI 报错）；
                // 若期望验证 round-trip，请先配置 NEO4J_PASSWORD 等环境变量。
                eprintln!("[test_law_round_trip] 跳过：无法连接 Neo4j — {e}");
                return;
            }
        };

        // 2. 构造 Risk 实体 (id, name, severity)
        let risk = RiskEntity {
            id: "test_risk_roundtrip_001".to_string(),
            name: "测试风险项".to_string(),
            severity: "HIGH".to_string(),
        };

        // 3. 构造 Law 实体 (law_id, law_name, short_name, article_id, article_no, summary, meta)
        let law = LawArticleEntity {
            law_id: "law_test_001".to_string(),
            law_name: "中华人民共和国政府采购法实施条例".to_string(),
            short_name: "政府采购法实施条例".to_string(),
            article_id: Some("article_test_001".to_string()),
            article_no: Some("第20条".to_string()),
            summary: "测试条款摘要".to_string(),
            meta: None,
        };

        // 4. 构造 EntityDecision 实体 (使用正确的 Decision::New)
        let decision = EntityDecision {
            candidate_id: "candidate_test_001".to_string(),
            decision: Decision::New,
            risk: risk.clone(),
            laws: vec![law.clone()],
            cases: vec![],
            rules: vec![],
            dimension: None,
            risk_experience: RiskExperience {
                experience_id: "exp_test_001".to_string(),
                candidate_id: "candidate_test_001".to_string(),
                source_quote: "测试摘录片段".to_string(),
                reason: "测试论证理由".to_string(),
                suggestion: "测试处置建议".to_string(),
                confidence: 0.95,
            },
        };

        // 5. 执行写入（失败即 fail，不再静默跳过，保证 round-trip 的"通过"可信）
        client
            .upsert_risk(&decision)
            .await
            .unwrap_or_else(|e| panic!("写入 Risk 节点失败: {e}"));
        client
            .upsert_law(&decision, &law)
            .await
            .unwrap_or_else(|e| panic!("写入 Law 节点失败: {e}"));
        client
            .upsert_article(&decision, &law)
            .await
            .unwrap_or_else(|e| panic!("写入 Article 节点失败: {e}"));

        // 6. 执行真实检索 (Round-trip)
        let search_hits = client.search("测试风险项").await.expect("查询应正常返回");

        assert!(!search_hits.is_empty(), "必须能够查出刚写入的数据");

        let hit = search_hits
            .iter()
            .find(|h| h.risk.id == "test_risk_roundtrip_001")
            .expect("必须找到匹配的 Risk 记录");

        assert!(!hit.laws.is_empty(), "返回结果中的 laws 列表不能为空");
        let returned_law = &hit.laws[0];

        // 7. 验证处置经验节点（RiskExperience）随查询返回
        assert_eq!(hit.experiences.len(), 1, "必须有一条处置经验节点");
        assert_eq!(hit.experiences[0].suggestion, "测试处置建议");
        assert_eq!(hit.experiences[0].reason, "测试论证理由");
        assert_eq!(hit.experiences[0].source_quote, "测试摘录片段");

        // 8. 验证法律名称 (law_name) 和条款号 (article_no) 未丢失
        assert_eq!(
            returned_law.law_name, law.law_name,
            "法律名称应完全一致且不能丢失！"
        );
        assert_eq!(
            returned_law.article_no, law.article_no,
            "条款号应完全一致！"
        );
    }
}