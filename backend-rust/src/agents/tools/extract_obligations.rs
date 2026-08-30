//! `extract_obligations` 工具 — 投标人义务聚合。
//!
//! 从文档中提取所有投标人必须满足的硬性条件，按类别结构化输出。
//! 很多"隐性歧视"通过分散义务实现——单条看着合理，聚合后才发现只有特定供应商能满足。
//!
//! ## 核心逻辑
//!
//! 1. 按 `scope` 确定提取范围（全文档 / 局部 / Agent 范围内）
//! 2. 按 `obligation_types` 分类扫描每个 chunk
//! 3. 识别 ★ 标记条款（关键否决项）
//! 4. 提取失败的后果说明
//! 5. 聚合后输出，便于发现"三合一排斥"模式
//!
//! ## 典型发现
//!
//! ① 资格条件里的资质等级 → 人员要求里的证书 → 设备清单里的型号 → 三合一排斥
//! ② 发现某供应商完美匹配所有义务项 → 存在"萝卜标"嫌疑

use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::chunk::Chunk;

use super::AgentTool;

/// `extract_obligations` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct ExtractObligationsArgs {
    /// 提取范围
    #[serde(default = "default_scope")]
    pub scope: String,
    /// 要提取的义务类型（空 = 全部）
    #[serde(default)]
    pub obligation_types: Vec<String>,
    /// scope=part 时指定要扫描的 chunk_id 列表
    #[serde(default)]
    pub clause_ids: Vec<String>,
    /// scope=agent_scope 时指定 Agent ID，按 Agent 职责范围过滤义务类型
    #[serde(default)]
    pub agent_id: Option<String>,
}

fn default_scope() -> String {
    "full_document".to_string()
}

/// 义务提取结果。
#[derive(Debug, serde::Serialize)]
struct ObligationsResult {
    /// 提取的义务列表
    obligations: Vec<Obligation>,
    /// 按类型分组的统计
    by_type: HashMap<String, usize>,
    /// ★ 标记义务数
    star_marked_count: usize,
    /// 组合风险信号
    risk_signals: Vec<String>,
    /// 覆盖率（已扫描 chunk 数 / 总 chunk 数）
    coverage_ratio: f64,
    /// 总摘要
    summary: String,
}

/// 单条义务。
#[derive(Debug, serde::Serialize)]
struct Obligation {
    /// 义务类型
    obligation_type: String,
    /// 来源 chunk_id
    chunk_id: String,
    /// 章节路径
    section_path: Vec<String>,
    /// 义务文本（关键句）
    text: String,
    /// 是否标 ★（关键否决项）
    is_star_marked: bool,
    /// 不满足的后果
    consequence_of_failure: Option<String>,
}

/// `extract_obligations` 工具实现。
pub struct ExtractObligationsTool {
    /// Chunk ID → Chunk 映射表
    pub chunks: Arc<HashMap<String, Chunk>>,
    /// 有序 chunk_id 列表
    pub chunk_order: Arc<Vec<String>>,
}

impl ExtractObligationsTool {
    pub fn new(chunks: Arc<HashMap<String, Chunk>>, chunk_order: Arc<Vec<String>>) -> Self {
        Self {
            chunks,
            chunk_order,
        }
    }

    /// 义务类型关键词映射。
    fn obligation_keywords(ob_type: &str) -> Vec<&str> {
        match ob_type {
            "资质" => vec![
                "资质",
                "证书",
                "许可证",
                "资格",
                "等级",
                "备案",
                "注册",
                "认定",
                "核准",
            ],
            "业绩" => vec![
                "业绩",
                "合同",
                "项目经验",
                "案例",
                "成功案例",
                "同类项目",
                "类似项目",
                "承担过",
            ],
            "人员" => vec![
                "人员",
                "项目经理",
                "技术负责人",
                "工程师",
                "注册",
                "持证",
                "职称",
                "学历",
                "专业",
                "从业",
            ],
            "设备" => vec![
                "设备",
                "仪器",
                "工具",
                "车辆",
                "机械",
                "生产",
                "检测",
                "实验室",
                "车间",
            ],
            "工期" => vec![
                "工期",
                "交付",
                "完成",
                "期限",
                "日历日",
                "工作日",
                "进度",
                "节点",
                "里程碑",
            ],
            "付款条件" => vec![
                "付款",
                "预付款",
                "进度款",
                "结算",
                "质保金",
                "支付",
                "合同金额",
            ],
            "售后" => vec![
                "售后",
                "保修",
                "维护",
                "技术支持",
                "培训",
                "响应",
                "到场",
                "7×24",
                "驻场",
            ],
            "保密" => vec![
                "保密",
                "信息安全",
                "数据",
                "隐私",
                "商业秘密",
                "敏感",
                "加密",
            ],
            "保险" => vec!["保险", "担保", "保函", "保证金", "履约", "投标"],
            _ => vec!["必须", "须", "应", "不得", "禁止", "需", "要求"],
        }
    }

    /// 所有义务类型。
    fn all_types() -> Vec<&'static str> {
        vec![
            "资质",
            "业绩",
            "人员",
            "设备",
            "工期",
            "付款条件",
            "售后",
            "保密",
            "保险",
            "其他",
        ]
    }

    /// Agent 职责范围 → 应关注的义务类型映射。
    /// 用于 scope=agent_scope 时自动过滤义务类型。
    fn agent_obligation_types(agent_id: &str) -> Vec<String> {
        match agent_id {
            "ProcedureAgent" => vec!["工期".into(), "付款条件".into(), "保险".into()],
            "DemandAgent" => vec!["资质".into(), "业绩".into(), "人员".into(), "设备".into()],
            "ContractAgent" => vec!["付款条件".into(), "保险".into(), "保密".into()],
            "ScoringAgent" => vec!["资质".into(), "业绩".into()],
            "SemanticRiskAgent" => vec!["资质".into(), "业绩".into(), "人员".into(), "设备".into(), "保险".into()],
            _ => Self::all_types().iter().map(|s| s.to_string()).collect(),
        }
    }

    /// 从文本中提取义务相关句子。
    fn extract_from_text(
        &self,
        text: &str,
        chunk_id: &str,
        section_path: &[String],
        types: &[String],
    ) -> Vec<Obligation> {
        let mut obligations = Vec::new();
        let target_types: Vec<String> = if types.is_empty() {
            Self::all_types().iter().map(|s| s.to_string()).collect()
        } else {
            types.to_vec()
        };

        for ob_type in &target_types {
            let keywords = Self::obligation_keywords(ob_type);
            for kw in &keywords {
                if text.contains(kw) {
                    // 提取包含关键词的句子（以句号/分号/换行为界）
                    let sentences: Vec<&str> = text.split(['。', '；', '\n', '!']).collect();

                    for sent in &sentences {
                        if sent.contains(kw) && sent.trim().len() > 6 {
                            let is_star = sent.contains('★')
                                || sent.contains('*')
                                || sent.contains("必须满足")
                                || sent.contains("实质性");

                            let consequence = if sent.contains("否则") {
                                let parts: Vec<&str> = sent.split("否则").collect();
                                if parts.len() > 1 {
                                    Some(parts[1].trim().to_string())
                                } else {
                                    None
                                }
                            } else if sent.contains("取消")
                                || sent.contains("废标")
                                || sent.contains("无效")
                            {
                                Some("可能导致投标无效或被取消资格".to_string())
                            } else {
                                None
                            };

                            // 避免重复
                            let trimmed = sent.trim().to_string();
                            let is_dup = obligations.iter().any(|o: &Obligation| {
                                o.text == trimmed && o.obligation_type == *ob_type
                            });
                            if !is_dup {
                                obligations.push(Obligation {
                                    obligation_type: ob_type.clone(),
                                    chunk_id: chunk_id.to_string(),
                                    section_path: section_path.to_vec(),
                                    text: trimmed,
                                    is_star_marked: is_star,
                                    consequence_of_failure: consequence,
                                });
                            }
                        }
                    }
                    break; // 只要匹配到一个关键词就够了，跳出 keywords 循环
                }
            }
        }

        // 兜底：检查强制性语言但未被上述类型覆盖的
        if types.is_empty() || types.contains(&"其他".to_string()) {
            let mandatory_markers = ["必须", "须 ", "不得", "禁止", "强制性"];
            for sent in text.split(['。', '；', '\n']) {
                if mandatory_markers.iter().any(|m| sent.contains(m)) && sent.trim().len() > 6 {
                    // 检查是否已被其他类型覆盖
                    let already_covered = obligations
                        .iter()
                        .any(|o: &Obligation| o.text.contains(sent.trim()));
                    if !already_covered {
                        obligations.push(Obligation {
                            obligation_type: "其他".to_string(),
                            chunk_id: chunk_id.to_string(),
                            section_path: section_path.to_vec(),
                            text: sent.trim().to_string(),
                            is_star_marked: sent.contains('★'),
                            consequence_of_failure: None,
                        });
                    }
                }
            }
        }

        obligations
    }

    /// 检测组合风险信号。
    fn detect_risk_signals(obligations: &[Obligation]) -> Vec<String> {
        let mut signals = Vec::new();

        // 按类型分组
        let mut by_type: HashMap<&str, Vec<&Obligation>> = HashMap::new();
        for ob in obligations {
            by_type.entry(&ob.obligation_type).or_default().push(ob);
        }

        // 三合一排斥：资质+人员+设备在同一 chunk 中出现
        let qual_chunks: Vec<&str> = by_type
            .get("资质")
            .map(|v| v.iter().map(|o| o.chunk_id.as_str()).collect())
            .unwrap_or_default();
        let personnel_chunks: Vec<&str> = by_type
            .get("人员")
            .map(|v| v.iter().map(|o| o.chunk_id.as_str()).collect())
            .unwrap_or_default();
        let equipment_chunks: Vec<&str> = by_type
            .get("设备")
            .map(|v| v.iter().map(|o| o.chunk_id.as_str()).collect())
            .unwrap_or_default();

        // 三合一排斥检测：资质+人员+设备三种类型均存在即触发
        // 不要求同一 chunk——三种要求分散在不同章节同样构成排他性组合
        let has_all_three = !qual_chunks.is_empty()
            && !personnel_chunks.is_empty()
            && !equipment_chunks.is_empty();
        if has_all_three {
            // 跨 chunk 检测: 三种类型分别分布在哪些 chunk 中
            let total_clauses = by_type.values().flatten().count();
            let three_type_clauses = qual_chunks.len() + personnel_chunks.len() + equipment_chunks.len();
            signals.push(format!(
                "⚠️ 三合一排斥风险：招标文件同时要求特定 资质（{}条）+ 人员（{}条）+ 设备（{}条），\
                 共 {} 项义务，合计涉及 {} 个条款。这三种要求组合可能形成排他性条件，\
                 建议评估是否有足够数量的潜在供应商满足全部要求。",
                qual_chunks.len(),
                personnel_chunks.len(),
                equipment_chunks.len(),
                three_type_clauses,
                total_clauses
            ));
        }

        // 品牌/型号指定 + 资质要求 = 萝卜标嫌疑
        let has_brand = obligations
            .iter()
            .any(|o| o.text.contains("品牌") || o.text.contains("型号") || o.text.contains("指定"));
        let has_qual = !qual_chunks.is_empty();
        if has_brand && has_qual {
            signals.push(
                "⚠️ 萝卜标嫌疑：同时存在品牌/型号指定和特定资质要求，可能为特定供应商量身定制"
                    .into(),
            );
        }

        // ★ 标记过于集中
        let star_count = obligations.iter().filter(|o| o.is_star_marked).count();
        if star_count > 5 {
            signals.push(format!(
                "⚠️ 共 {} 项 ★ 实质性要求，这可能不合理地限制竞争",
                star_count
            ));
        }

        signals
    }
}

#[async_trait::async_trait]
impl AgentTool for ExtractObligationsTool {
    fn name(&self) -> &str {
        "extract_obligations"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "extract_obligations",
                "description": "提取投标人全部硬性义务并按类别结构化，发现聚合排斥/萝卜标嫌疑。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "scope": {
                            "type": "string",
                            "enum": ["full_document", "part", "agent_scope"],
                            "description": "提取范围：full_document=全文档, part=指定条款范围, agent_scope=当前Agent负责的条款"
                        },
                        "obligation_types": {
                            "type": "array",
                            "items": {
                                "type": "string",
                                "enum": ["资质", "业绩", "人员", "设备", "工期", "付款条件", "售后", "保密", "保险", "其他"]
                            },
                            "description": "要提取的义务类型，不指定则全部提取"
                        },
                        "clause_ids": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "scope=part 时指定要扫描的 chunk_id 列表"
                        },
                        "agent_id": {
                            "type": "string",
                            "description": "scope=agent_scope 时指定 Agent ID，自动按职责过滤义务类型"
                        }
                    },
                    "required": []
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: ExtractObligationsArgs = serde_json::from_value(args)?;

        let mut all_obligations: Vec<Obligation> = Vec::new();

        // 确定要使用的 obligation_types（agent_scope 按 Agent 职责过滤）
        let effective_types: Vec<String> = if parsed.scope == "agent_scope"
            && let Some(ref agent_id) = parsed.agent_id
        {
            if parsed.obligation_types.is_empty() {
                Self::agent_obligation_types(agent_id)
            } else {
                parsed.obligation_types.clone()
            }
        } else {
            parsed.obligation_types.clone()
        };

        // 确定要扫描的 chunk 列表
        let chunk_ids_to_scan: Vec<String> = match parsed.scope.as_str() {
            "part" if !parsed.clause_ids.is_empty() => {
                // 只扫描指定的 clause_ids
                parsed.clause_ids.clone()
            }
            _ => {
                // full_document / agent_scope / part(无 clause_ids) → 全量
                self.chunk_order.iter().cloned().collect()
            }
        };

        let scanned_count = chunk_ids_to_scan.len();
        let total_chunks = self.chunk_order.len();
        let coverage_ratio = if total_chunks > 0 {
            scanned_count as f64 / total_chunks as f64
        } else {
            1.0
        };

        for chunk_id in &chunk_ids_to_scan {
            if let Some(chunk) = self.chunks.get(chunk_id) {
                let obs = self.extract_from_text(
                    &chunk.text,
                    &chunk.chunk_id,
                    &chunk.section_path,
                    &effective_types,
                );
                all_obligations.extend(obs);
            }
        }

        // 按类型分组统计
        let mut by_type: HashMap<String, usize> = HashMap::new();
        for ob in &all_obligations {
            *by_type.entry(ob.obligation_type.clone()).or_default() += 1;
        }

        // ★ 标记计数
        let star_count = all_obligations.iter().filter(|o| o.is_star_marked).count();

        // 风险信号
        let risk_signals = Self::detect_risk_signals(&all_obligations);

        // 摘要
        let summary = if all_obligations.is_empty() {
            "未提取到义务条款。".to_string()
        } else {
            let mut parts = vec![format!("共提取 {} 条投标人义务", all_obligations.len())];
            if star_count > 0 {
                parts.push(format!("其中 {} 条为 ★ 实质性要求", star_count));
            }
            if !risk_signals.is_empty() {
                parts.push(format!("发现 {} 个组合风险信号", risk_signals.len()));
            }
            parts.join("。")
        };

        let result = ObligationsResult {
            obligations: all_obligations,
            by_type,
            star_marked_count: star_count,
            risk_signals,
            coverage_ratio,
            summary,
        };

        Ok(serde_json::to_value(&result)?)
    }
}

// ─── 测试 ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chunk::{Chunk, ChunkType};

    fn make_chunk(id: &str, text: &str) -> Chunk {
        Chunk {
            chunk_id: id.to_string(),
            chunk_type: ChunkType::Leaf,
            section_path: vec!["第一章".to_string(), "投标人资格".to_string()],
            text: text.to_string(),
            page_start: 0,
            page_end: 1,
            source_block_ids: vec![],
            bbox_refs: vec![],
        }
    }

    fn make_tool() -> ExtractObligationsTool {
        let mut chunks = HashMap::new();
        let chunk_order = vec!["ch_001".to_string(), "ch_002".to_string()];

        chunks.insert(
            "ch_001".to_string(),
            make_chunk(
                "ch_001",
                "投标人须具备建筑工程施工总承包一级及以上资质。\
             项目经理须持有建筑工程专业一级注册建造师证书，\
             且具有5年以上同类项目管理经验。\
             ★ 核心设备须为自有，提供购置发票。",
            ),
        );
        chunks.insert(
            "ch_002".to_string(),
            make_chunk(
                "ch_002",
                "付款方式：合同签订后支付30%预付款，\
             验收合格后支付至合同金额的95%，\
             剩余5%作为质保金，质保期满后30日内无息退还。",
            ),
        );

        ExtractObligationsTool {
            chunks: Arc::new(chunks),
            chunk_order: Arc::new(chunk_order),
        }
    }

    #[test]
    fn test_extract_qualifications() {
        let tool = make_tool();
        let obs = tool.extract_from_text(
            "投标人须具备建筑工程施工总承包一级及以上资质。\
             项目经理须持有建筑工程专业一级注册建造师证书。",
            "ch_001",
            &["资格".to_string()],
            &["资质".to_string()],
        );
        assert!(!obs.is_empty());
        assert!(obs.iter().any(|o| o.text.contains("施工总承包")));
    }

    #[test]
    fn test_star_mark_detection() {
        let tool = make_tool();
        let obs = tool.extract_from_text(
            "★ 核心设备须为自有，提供购置发票。",
            "ch_001",
            &[],
            &["设备".to_string()],
        );
        assert!(!obs.is_empty());
        assert!(obs[0].is_star_marked);
    }

    #[test]
    fn test_risk_signals_triple_exclusion() {
        let obligations = vec![
            Obligation {
                obligation_type: "资质".to_string(),
                chunk_id: "ch_001".to_string(),
                section_path: vec![],
                text: "须具备一级资质".to_string(),
                is_star_marked: false,
                consequence_of_failure: None,
            },
            Obligation {
                obligation_type: "人员".to_string(),
                chunk_id: "ch_001".to_string(),
                section_path: vec![],
                text: "须持有一级建造师".to_string(),
                is_star_marked: false,
                consequence_of_failure: None,
            },
            Obligation {
                obligation_type: "设备".to_string(),
                chunk_id: "ch_001".to_string(),
                section_path: vec![],
                text: "须自有核心设备".to_string(),
                is_star_marked: false,
                consequence_of_failure: None,
            },
        ];
        let signals = ExtractObligationsTool::detect_risk_signals(&obligations);
        assert!(!signals.is_empty());
        assert!(signals.iter().any(|s| s.contains("三合一")));
    }

    #[test]
    fn test_no_false_positive_for_normal_clauses() {
        let tool = make_tool();
        let obs = tool.extract_from_text(
            "本项目位于东莞市松山湖园区，工期为合同签订后60个日历日。",
            "ch_003",
            &[],
            &["工期".to_string()],
        );
        assert!(!obs.is_empty());
    }
}
