//! AgentRegistry — 集中管理 8 个 Agent 定义（Registry + Builder 模式）。
//!
//! 设计文档 §7.4 / temp.md Phase 2 定义。
//!
//! ## 设计模式
//!
//! - **Registry**: 集中管理 8 个 Agent 的 `AgentDefinition`，按 `AgentId` 枚举查找
//! - **Builder**: `instantiate()` 分离 Agent 配置与 `ReActLoop` 构造
//!
//! ## 工厂注入
//!
//! `instantiate()` 接收 `llm_factory` 和 `tools_factory` 两个工厂函数，
//! 每个 Agent 获得独立的 LLM 客户端和工具集，避免 `clone_box` 传染到
//! `LlmClient` / `AgentTool` / `ToolRegistry`。

use crate::agents::bus::AgentBus;
use crate::agents::prompts;
use crate::agents::react_loop::{LlmClient, ReActLoop};
use crate::agents::scout::SCOUT_SYSTEM_PROMPT;
use crate::agents::session_graph::SessionGraph;
use crate::agents::tools::ToolRegistry;
use crate::agents::trace::TraceLog;
use crate::agents::types::*;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Agent 注册表 — 集中管理 8 个 Agent 的静态定义。
pub struct AgentRegistry {
    definitions: HashMap<AgentId, AgentDefinition>,
}

impl AgentRegistry {
    /// 创建包含全部 8 个 Agent 内置定义的注册表。
    pub fn builtin() -> Self {
        let mut definitions = HashMap::new();

        definitions.insert(
            AgentId::FactCheck,
            AgentDefinition {
                id: AgentId::FactCheck,
                display_name: "事实核查Agent",
                system_prompt: prompts::FACT_CHECK_SYSTEM_PROMPT,
                default_max_turns: 10,
                complexity: AgentComplexity::Medium,
                section_keywords: &[
                    "时限", "金额", "预算", "截止", "格式", "装订", "密封", "盖章",
                ],
                tool_names: &[
                    "web_search",
                    "search_document",
                    "read_section",
                    "output_finding",
                    // 交叉引用完整性检查 + 模板比对
                    "check_cross_reference",
                    "compare_with_template",
                    // 数值/计算校验
                    "validate_calculation",
                ],
            },
        );

        definitions.insert(
            AgentId::Procedure,
            AgentDefinition {
                id: AgentId::Procedure,
                display_name: "采购程序审查Agent",
                system_prompt: prompts::PROCEDURE_SYSTEM_PROMPT,
                default_max_turns: 12,
                complexity: AgentComplexity::Medium,
                section_keywords: &[
                    "采购方式",
                    "公告",
                    "保证金",
                    "评审",
                    "废标",
                    "流标",
                    "开标",
                    "评标",
                ],
                tool_names: &[
                    "web_search",
                    "search_document",
                    "read_section",
                    "output_finding",
                    // V3 采购程序合规审查工具
                    "verify_procurement_method",
                    "verify_bid_deposit",
                    "verify_announcement_period",
                    "verify_bid_preparation_period",
                    // 零依赖计算工具
                    "calculate_timeline",
                ],
            },
        );

        definitions.insert(
            AgentId::RuleEngine,
            AgentDefinition {
                id: AgentId::RuleEngine,
                display_name: "硬性规则引擎Agent",
                system_prompt: prompts::RULE_ENGINE_SYSTEM_PROMPT,
                default_max_turns: 14,
                complexity: AgentComplexity::Low,
                section_keywords: &["必须", "不得", "禁止", "应", "不应", "资格条件", "实质性"],
                tool_names: &[
                    "web_search",
                    "search_document",
                    "read_section",
                    "output_finding",
                    // 义务提取与排斥检测
                    "extract_obligations",
                    "check_cross_reference",
                ],
            },
        );

        definitions.insert(
            AgentId::SemanticRisk,
            AgentDefinition {
                id: AgentId::SemanticRisk,
                display_name: "隐性风险识别Agent",
                system_prompt: prompts::SEMANTIC_RISK_SYSTEM_PROMPT,
                default_max_turns: 14,
                complexity: AgentComplexity::High,
                section_keywords: &[
                    "品牌", "型号", "原厂", "专利", "独家", "指定", "本地", "区域",
                ],
                tool_names: &[
                    "web_search",
                    "search_document",
                    "read_section",
                    "output_finding",
                    // 义务提取识别排他性组合
                    "extract_obligations",
                    "check_cross_reference",
                ],
            },
        );

        definitions.insert(
            AgentId::Scout,
            AgentDefinition {
                id: AgentId::Scout,
                display_name: "初筛Agent",
                system_prompt: SCOUT_SYSTEM_PROMPT,
                default_max_turns: 3,
                complexity: AgentComplexity::Low,
                section_keywords: &[], // 串行扫描全部 clauses，不参与关键词路由
                tool_names: &["read_section", "output_finding"], // ★ 无 web_search
            },
        );

        definitions.insert(
            AgentId::Scoring,
            AgentDefinition {
                id: AgentId::Scoring,
                display_name: "评分合规审查Agent",
                system_prompt: prompts::SCORING_SYSTEM_PROMPT,
                default_max_turns: 10,
                complexity: AgentComplexity::Medium,
                section_keywords: &[
                    "评分",
                    "评审因素",
                    "分值",
                    "价格分",
                    "技术分",
                    "商务分",
                    "权重",
                ],
                tool_names: &[
                    "web_search",
                    "search_document",
                    "read_section",
                    "output_finding",
                    // V4 评审标准审查工具
                    "validate_scoring_formula",
                    "validate_weight_distribution",
                    "detect_subjective_scoring",
                    "check_scoring_completeness",
                    "verify_consortium_rules",
                ],
            },
        );

        definitions.insert(
            AgentId::Demand,
            AgentDefinition {
                id: AgentId::Demand,
                display_name: "技术需求审查Agent",
                system_prompt: prompts::DEMAND_SYSTEM_PROMPT,
                default_max_turns: 12,
                complexity: AgentComplexity::Medium,
                section_keywords: &[
                    "技术", "参数", "规格", "性能", "功能", "配置", "认证", "国产",
                ],
                tool_names: &[
                    "web_search",
                    "search_document",
                    "read_section",
                    "output_finding",
                    // V4 进口产品 + 联合体审查
                    "check_imported_products",
                    // 技术参数矛盾检测
                    "search_contradiction",
                ],
            },
        );

        definitions.insert(
            AgentId::Contract,
            AgentDefinition {
                id: AgentId::Contract,
                display_name: "合同条款审查Agent",
                system_prompt: prompts::CONTRACT_SYSTEM_PROMPT,
                default_max_turns: 12,
                complexity: AgentComplexity::Medium,
                section_keywords: &["合同", "付款", "验收", "质保", "违约", "售后", "保修"],
                tool_names: &[
                    "web_search",
                    "search_document",
                    "read_section",
                    "output_finding",
                    "search_contradiction",
                ],
            },
        );

        definitions.insert(
            AgentId::LegalVerify,
            AgentDefinition {
                id: AgentId::LegalVerify,
                display_name: "法条验证Agent",
                system_prompt: prompts::LEGAL_VERIFY_SYSTEM_PROMPT,
                default_max_turns: 8,
                complexity: AgentComplexity::Low,
                section_keywords: &[], // Coordinator 按需调用，不参与路由
                tool_names: &["web_search", "search_document", "output_finding"],
            },
        );

        definitions.insert(
            AgentId::Debate,
            AgentDefinition {
                id: AgentId::Debate,
                display_name: "正反辩论Agent",
                system_prompt: prompts::DEBATE_SYSTEM_PROMPT,
                default_max_turns: 8,
                complexity: AgentComplexity::High,
                section_keywords: &[], // Coordinator 按需调用
                tool_names: &[
                    "web_search",
                    "search_document",
                    "read_section",
                    "output_finding",
                ],
            },
        );

        definitions.insert(
            AgentId::BlindSpot,
            AgentDefinition {
                id: AgentId::BlindSpot,
                display_name: "盲点复查Agent",
                system_prompt: prompts::BLIND_SPOT_SYSTEM_PROMPT,
                default_max_turns: 10,
                complexity: AgentComplexity::High,
                section_keywords: &[], // BlindSpot 审查所有条款，不按关键词路由
                tool_names: &[
                    "web_search",
                    "search_document",
                    "read_section",
                    "output_finding",
                ],
            },
        );

        // 打印全部 Agent 的工具职责分配（排查调用链路）
        eprintln!("[AgentRegistry] ── Agent 工具职责分配总览 ──");
        for (id, def) in definitions.iter() {
            eprintln!(
                "[AgentRegistry]   {} ({}) → {} 个工具: {:?}",
                def.display_name,
                id,
                def.tool_names.len(),
                def.tool_names
            );
        }
        eprintln!("[AgentRegistry] ──────────────────────────────");

        Self { definitions }
    }

    /// 按 AgentId 查找定义。
    pub fn get(&self, id: AgentId) -> Option<&AgentDefinition> {
        let def = self.definitions.get(&id);
        if let Some(d) = def {
            eprintln!(
                "[AgentRegistry] get: {} ({}) → 工具 {} 个: {:?}",
                d.display_name,
                id,
                d.tool_names.len(),
                d.tool_names
            );
        } else {
            eprintln!("[AgentRegistry] get: {} → 未注册!", id);
        }
        def
    }

    /// 按引用查找 Agent 定义（避免移动 ownership）。
    pub fn get_ref(&self, id: &AgentId) -> Option<&AgentDefinition> {
        self.definitions.get(id)
    }

    /// 获取所有注册的 Agent 定义。
    pub fn all(&self) -> Vec<&AgentDefinition> {
        self.definitions.values().collect()
    }

    /// 获取所有注册的 AgentId。
    pub fn all_ids(&self) -> Vec<AgentId> {
        self.definitions.keys().cloned().collect()
    }

    /// 注册一个动态 Agent。
    pub fn register_dynamic(&mut self, def: &DynamicAgentDefinition) {
        let agent_id = AgentId::Dynamic(def.id.clone());
        // 动态 Agent 的 system_prompt 存储在 DynamicAgentDefinition 中，
        // AgentDefinition 的 system_prompt 字段留空（由 ReActLoop 构造时按分支处理）
        let definition = AgentDefinition {
            id: agent_id.clone(),
            display_name: "",  // 运行时从 DynamicAgentDefinition 取
            system_prompt: "", // 运行时从 DynamicAgentDefinition 取
            default_max_turns: def.default_max_turns,
            complexity: def.complexity,
            section_keywords: &[], // 动态 Agent 的关键词从 DynamicAgentDefinition 取
            tool_names: &[
                "web_search",
                "search_document",
                "read_section",
                "output_finding",
            ],
        };
        self.definitions.insert(agent_id, definition);
    }

    /// 检查是否已注册指定 ID 的动态 Agent。
    pub fn has_dynamic(&self, id: &str) -> bool {
        self.definitions
            .contains_key(&AgentId::Dynamic(id.to_string()))
    }

    /// Builder: AgentDefinition + 工厂注入 → ReActLoop。
    ///
    /// 每个 Agent 获得：
    /// - 独立的 LLM 客户端（通过 `llm_factory` 创建）
    /// - 独立的工具集（通过 `tools_factory` 创建）
    /// - 共享的 SessionGraph 引用
    /// - 共享的 AgentBus (Sender) + 专属 Receiver
    /// - 共享的 TraceLog 引用
    ///
    /// ## 为什么用工厂函数而非 clone？
    ///
    /// `LlmClient` trait 和 `AgentTool` trait 的实现类型不保证 Clone。
    /// 工厂函数在 Coordinator 中捕获共享资源（Arc 引用），
    /// 每次调用创建新的包装实例而非 clone trait object。
    pub fn instantiate(
        &self,
        id: AgentId,
        llm: Box<dyn LlmClient>,
        tools: ToolRegistry,
        bus: Option<Arc<AgentBus>>,
        graph: Option<Arc<SessionGraph>>,
        trace: Arc<Mutex<TraceLog>>,
    ) -> Result<ReActLoop> {
        let def = self
            .definitions
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("Agent 定义未注册: {}", id))?;

        let config = def.to_agent_config();
        let mut agent = ReActLoop::new(config, llm, tools);
        agent.trace = trace;

        if let Some(b) = bus {
            agent = agent.with_bus(b);
        }
        if let Some(g) = graph {
            agent = agent.with_graph(g);
        }

        Ok(agent)
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_has_all_agents() {
        let registry = AgentRegistry::builtin();
        let all_reviewers = AgentId::all_reviewers();
        for id in &all_reviewers {
            assert!(registry.get(id.clone()).is_some(), "缺少 Agent: {}", id);
        }
        // BlindSpot / LegalVerify / Debate 也应该存在
        assert!(registry.get(AgentId::BlindSpot).is_some());
        assert!(registry.get(AgentId::LegalVerify).is_some());
        assert!(registry.get(AgentId::Debate).is_some());
    }

    #[test]
    fn test_agent_definition_to_config() {
        let registry = AgentRegistry::builtin();
        let def = registry.get(AgentId::FactCheck).unwrap();
        let config = def.to_agent_config();
        assert_eq!(config.name, "FactCheckAgent");
        assert_eq!(config.default_max_turns, 10);
        assert!(!config.tool_names.is_empty());
    }

    #[test]
    fn test_agent_id_display() {
        assert_eq!(AgentId::FactCheck.to_string(), "FactCheckAgent");
        assert_eq!(AgentId::BlindSpot.to_string(), "BlindSpotAgent");
    }

    #[test]
    fn test_agent_id_from_str() {
        assert_eq!(AgentId::parse("factcheck"), Some(AgentId::FactCheck));
        assert_eq!(AgentId::parse("FactCheckAgent"), Some(AgentId::FactCheck));
        assert_eq!(AgentId::parse("unknown"), None);
    }

    // ── 动态 Agent 注册 ──────────────────────────────────────

    fn make_test_dynamic_def(id: &str) -> DynamicAgentDefinition {
        DynamicAgentDefinition {
            id: id.to_string(),
            display_name: "测试动态Agent".to_string(),
            system_prompt: "你是一个测试Agent...".to_string(),
            default_max_turns: 8,
            complexity: AgentComplexity::Medium,
            section_keywords: vec!["测试".to_string(), "验证".to_string()],
            tool_names: vec!["web_search".to_string(), "output_finding".to_string()],
            created_at: "2026-01-01T00:00:00Z".to_string(),
            created_by: "BlindSpotAgent".to_string(),
            reason: "测试".to_string(),
            active: false,
        }
    }

    #[test]
    fn test_register_dynamic() {
        let mut registry = AgentRegistry::builtin();
        let def = make_test_dynamic_def("TestDynamic");
        registry.register_dynamic(&def);

        // has_dynamic 应返回 true
        assert!(registry.has_dynamic("TestDynamic"));

        // get 应能找到
        let agent_id = AgentId::Dynamic("TestDynamic".into());
        assert!(registry.get(agent_id).is_some());
    }

    #[test]
    fn test_has_dynamic_returns_false_for_unknown() {
        let registry = AgentRegistry::builtin();
        assert!(!registry.has_dynamic("NonExistent"));
    }

    #[test]
    fn test_register_dynamic_overwrites() {
        let mut registry = AgentRegistry::builtin();

        let def1 = make_test_dynamic_def("TestDynamic");
        registry.register_dynamic(&def1);

        // 第二次注册同名不 panic
        let def2 = make_test_dynamic_def("TestDynamic");
        registry.register_dynamic(&def2);

        assert!(registry.has_dynamic("TestDynamic"));
    }

    #[test]
    fn test_all_ids_includes_dynamic_after_register() {
        let mut registry = AgentRegistry::builtin();
        let count_before = registry.all_ids().len();

        let def = make_test_dynamic_def("TestDynamic");
        registry.register_dynamic(&def);

        let ids = registry.all_ids();
        assert!(ids.contains(&AgentId::Dynamic("TestDynamic".into())));
        // 动态 Agent 替换了之前的注册（如果同名），数量应 >= 之前
        assert!(ids.len() >= count_before);
    }

    // ── 特殊 Agent 配置验证 ──────────────────────────────────

    #[test]
    fn test_legal_verify_agent_config() {
        let registry = AgentRegistry::builtin();
        let def = registry.get(AgentId::LegalVerify).unwrap();
        let config = def.to_agent_config();
        assert_eq!(config.name, "LegalVerifyAgent");
        assert_eq!(config.default_max_turns, 8);
        // LegalVerify 不参与路由，section_keywords 为空
        assert!(def.section_keywords.is_empty());
    }

    #[test]
    fn test_debate_agent_config() {
        let registry = AgentRegistry::builtin();
        let def = registry.get(AgentId::Debate).unwrap();
        assert_eq!(def.complexity, AgentComplexity::High);
        assert!(def.section_keywords.is_empty());
        assert_eq!(def.default_max_turns, 8);
    }

    #[test]
    fn test_blind_spot_agent_config() {
        let registry = AgentRegistry::builtin();
        let def = registry.get(AgentId::BlindSpot).unwrap();
        assert!(def.section_keywords.is_empty());
        assert_eq!(def.default_max_turns, 10);
        assert_eq!(def.complexity, AgentComplexity::High);
    }

    #[test]
    fn test_total_agent_count() {
        let registry = AgentRegistry::builtin();
        let ids = registry.all_ids();
        // 7 reviewers + Scout + BlindSpot + LegalVerify + Debate = 11
        assert_eq!(ids.len(), 11, "内置 Agent 总数应为 11");
    }
}