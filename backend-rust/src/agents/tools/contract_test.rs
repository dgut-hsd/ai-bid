//! Tool Contract Test — 验证 Registry、Agent 授权、Tool 定义之间的一致性。
//!
//! 工具按调用方式分为两类：
//! - AgentCallable: LLM 通过 function calling 选择，需要 Agent 授权 + Prompt 指引
//! - PipelineInternal: Coordinator/其他 Rust 代码直接调用，不暴露给 LLM
//!
//! 所有 Group A 工具经代码调用路径分析，确认为 AgentCallable。

#[cfg(test)]
mod contract_tests {
    use crate::agents::registry::AgentRegistry;
    use crate::agents::tools::AgentTool;
    use crate::agents::tools::ToolRegistry;

    /// Group A 工具名称列表（14 个），全部为 AgentCallable
    const GROUP_A_TOOLS: &[&str] = &[
        "compare_versions", "detect_boilerplate",
        "verify_procurement_method", "verify_bid_deposit",
        "verify_announcement_period", "verify_bid_preparation_period",
        "calculate_timeline",
        "validate_scoring_formula", "validate_weight_distribution",
        "detect_subjective_scoring", "check_scoring_completeness",
        "verify_consortium_rules", "check_imported_products",
        "output_verification_batch",
    ];

    /// V3 工具（ProcedureAgent）
    const V3_TOOLS: &[&str] = &[
        "verify_procurement_method", "verify_bid_deposit",
        "verify_announcement_period", "verify_bid_preparation_period",
        "calculate_timeline",
    ];

    /// V4 工具（ScoringAgent）
    const V4_TOOLS: &[&str] = &[
        "validate_scoring_formula", "validate_weight_distribution",
        "detect_subjective_scoring", "check_scoring_completeness",
        "verify_consortium_rules",
    ];

    fn build_group_a_registry() -> ToolRegistry {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(crate::agents::tools::compare_versions::CompareVersionsTool::new(
            std::sync::Arc::new(std::collections::HashMap::new()),
            std::sync::Arc::new(Vec::new()),
        )));
        reg.register(Box::new(crate::agents::tools::detect_boilerplate::DetectBoilerplateTool::new(
            std::sync::Arc::new(std::collections::HashMap::new()),
            std::sync::Arc::new(Vec::new()),
        )));
        reg.register(Box::new(crate::agents::tools::verify_procurement_method::VerifyProcurementMethodTool));
        reg.register(Box::new(crate::agents::tools::verify_bid_deposit::VerifyBidDepositTool));
        reg.register(Box::new(crate::agents::tools::verify_announcement_period::VerifyAnnouncementPeriodTool));
        reg.register(Box::new(crate::agents::tools::verify_bid_preparation_period::VerifyBidPreparationPeriodTool));
        reg.register(Box::new(crate::agents::tools::calculate_timeline::CalculateTimelineTool));
        reg.register(Box::new(crate::agents::tools::validate_scoring_formula::ValidateScoringFormulaTool));
        reg.register(Box::new(crate::agents::tools::validate_weight_distribution::ValidateWeightDistributionTool));
        reg.register(Box::new(crate::agents::tools::detect_subjective_scoring::DetectSubjectiveScoringTool));
        reg.register(Box::new(crate::agents::tools::check_scoring_completeness::CheckScoringCompletenessTool));
        reg.register(Box::new(crate::agents::tools::verify_consortium_rules::VerifyConsortiumRulesTool));
        reg.register(Box::new(crate::agents::tools::check_imported_products::CheckImportedProductsTool));
        reg.register(Box::new(crate::agents::tools::output_verification_batch::OutputVerificationBatchTool));
        reg
    }

    // ─── Registry 一致性 ────────────────────────────────────────

    #[test]
    fn test_all_group_a_tools_registered() {
        let reg = build_group_a_registry();
        for name in GROUP_A_TOOLS {
            assert!(reg.contains(name), "Group A tool '{}' 未在 ToolRegistry 中注册", name);
        }
    }

    #[test]
    fn test_all_group_a_tools_have_valid_definitions() {
        let reg = build_group_a_registry();
        for name in GROUP_A_TOOLS {
            let defs = reg.definitions_filtered(&[name.to_string()]);
            assert_eq!(defs.len(), 1, "Group A tool '{}' definition 过滤失败", name);
            let def = &defs[0];
            assert!(def.get("type").is_some(), "工具 '{}' 缺少 type", name);
            let func = def.get("function").expect("缺少 function 字段");
            assert!(func.get("name").is_some(), "工具 '{}' 缺少 function.name", name);
            assert!(func.get("description").is_some(), "工具 '{}' 缺少 function.description", name);
            assert!(func.get("parameters").is_some(), "工具 '{}' 缺少 function.parameters", name);
        }
    }

    /// AgentCallable 工具必须：(1) 注册到 ToolRegistry (2) 分配给至少一个 Agent
    #[test]
    fn test_agent_callable_tools_have_agent_auth() {
        let agent_registry = AgentRegistry::builtin();
        let mut authorized_tools = std::collections::HashSet::new();
        for agent_id in agent_registry.all_ids() {
            if let Some(def) = agent_registry.get(agent_id) {
                for tool_name in def.tool_names {
                    authorized_tools.insert(tool_name.to_string());
                }
            }
        }
        for name in GROUP_A_TOOLS {
            assert!(
                authorized_tools.contains(*name),
                "AgentCallable 工具 '{}' 未被任何 Agent 授权。如果是 PipelineInternal 工具，\
                请将其从 GROUP_A_TOOLS 中移除；如果是 AgentCallable 工具，请分配到合适的 Agent。",
                name
            );
        }
    }

    /// Agent 授权的工具名必须在 ToolRegistry 中存在（防止拼写错误）
    #[test]
    fn test_agent_authorized_tools_exist_in_registry() {
        let reg = build_group_a_registry();
        let agent_registry = AgentRegistry::builtin();
        for agent_id in agent_registry.all_ids() {
            if let Some(def) = agent_registry.get(agent_id) {
                for tool_name in def.tool_names {
                    if GROUP_A_TOOLS.iter().any(|t| *t == *tool_name) {
                        assert!(
                            reg.contains(tool_name),
                            "Agent '{}' 授权了 Group A 工具 '{}'，但 ToolRegistry 中不存在（拼写错误？）",
                            def.display_name, tool_name
                        );
                    }
                }
            }
        }
    }

    // ─── verify_bid_preparation_period 事件 Contract（4B-3A）─────

    #[test]
    #[ignore = "4B 工具契约迁移未完成（另立项）：definition 未迁到新 schema"]
    fn test_bid_prep_definition_has_event_context_fields() {
        let tool = crate::agents::tools::verify_bid_preparation_period::VerifyBidPreparationPeriodTool;
        let def = tool.definition();
        let props = &def["function"]["parameters"]["properties"];

        // 新 Contract 字段必须存在
        assert!(props.get("document_issued_date_str").is_some(), "缺少 document_issued_date_str");
        assert!(props.get("first_response_deadline_date_str").is_some(), "缺少 first_response_deadline_date_str");
        assert!(props.get("procurement_object").is_some(), "缺少 procurement_object");
        assert!(props.get("is_government_procurement").is_some(), "缺少 is_government_procurement");

        // legacy announcement 字段描述必须明确不得用于准备期起算
        let ann = props["announcement_date_str"]["description"].as_str().unwrap();
        assert!(ann.contains("不得作为采购文件发出日的替代值"), "announcement_date_str 描述必须禁止替代文件发出日: {}", ann);

        // bid_deadline 字段描述必须限制为招标场景
        let bid = props["bid_deadline_date_str"]["description"].as_str().unwrap();
        assert!(bid.contains("仅适用于公开招标/邀请招标"), "bid_deadline_date_str 描述必须限定招标场景: {}", bid);

        // 工具总描述必须为"采购文件发出 → 投标/首次响应截止"，不得再写"公告发布至投标截止"
        let desc = def["function"]["description"].as_str().unwrap();
        assert!(desc.contains("采购文件发出"), "工具描述必须基于文件发出日: {}", desc);
        assert!(!desc.contains("公告发布到投标截止"), "工具描述不得继续使用公告发布口径");
    }

    // ─── verify_announcement_period PeriodType Contract（4B-4B）─

    #[test]
    #[ignore = "4B 工具契约迁移未完成（另立项）：definition 未迁到新 schema"]
    fn test_announcement_definition_has_period_type_fields() {
        let tool = crate::agents::tools::verify_announcement_period::VerifyAnnouncementPeriodTool;
        let def = tool.definition();
        let props = &def["function"]["parameters"]["properties"];

        // 新 Contract 字段必须存在
        for field in [
            "period_type",
            "notice_start_date_str",
            "notice_end_date_str",
            "document_availability_start_date_str",
            "document_availability_end_date_str",
            "procurement_object",
            "is_government_procurement",
            "supplier_selection_method",
            "invitation_method",
            "single_source_reason",
            "above_public_tender_threshold",
            "single_source_publicity_start_date_str",
            "single_source_publicity_end_date_str",
        ] {
            assert!(props.get(field).is_some(), "definition 缺少字段: {}", field);
        }
    }

    #[test]
    #[ignore = "4B 工具契约迁移未完成（另立项）：definition 未迁到新 schema"]
    fn test_announcement_period_type_enum_values() {
        let tool = crate::agents::tools::verify_announcement_period::VerifyAnnouncementPeriodTool;
        let def = tool.definition();
        let props = &def["function"]["parameters"]["properties"];

        let pt_enum = props["period_type"]["enum"].as_array().expect("period_type 必须为 enum");
        for v in ["notice_publication", "document_availability", "single_source_pre_acquisition_publicity"] {
            assert!(pt_enum.iter().any(|e| e.as_str() == Some(v)), "period_type enum 缺少: {}", v);
        }

        let ssm_enum = props["supplier_selection_method"]["enum"].as_array().expect("supplier_selection_method 必须为 enum");
        for v in ["prequalification_notice", "supplier_pool", "written_recommendation"] {
            assert!(ssm_enum.iter().any(|e| e.as_str() == Some(v)), "supplier_selection_method enum 缺少: {}", v);
        }

        let inv_enum = props["invitation_method"]["enum"].as_array().expect("invitation_method 必须为 enum");
        for v in ["public_notice", "supplier_pool", "written_recommendation"] {
            assert!(inv_enum.iter().any(|e| e.as_str() == Some(v)), "invitation_method enum 缺少: {}", v);
        }
    }

    #[test]
    #[ignore = "4B 工具契约迁移未完成（另立项）：definition 未迁到新 schema"]
    fn test_single_source_reason_enum_precision() {
        // 4B-4E Final Seal：single_source_reason.enum 必须精确为三类受控值
        let tool = crate::agents::tools::verify_announcement_period::VerifyAnnouncementPeriodTool;
        let def = tool.definition();
        let props = &def["function"]["parameters"]["properties"];
        let e = props["single_source_reason"]["enum"].as_array().expect("single_source_reason 必须为 enum");
        let values: Vec<&str> = e.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(
            values,
            vec!["only_supplier", "emergency", "continuity_additional_purchase"],
            "single_source_reason enum 必须精确包含三类值"
        );
    }

    #[test]
    #[ignore = "4B 工具契约迁移未完成（另立项）：definition 未迁到新 schema"]
    fn test_calculate_timeline_pure_calculation_schema() {
        // 4B-5B+C：schema 必须含 dates/calculations/from/to/day_count_type，
        // 不得暴露旧 constraints.min_days/max_days/legal_basis
        let tool = crate::agents::tools::calculate_timeline::CalculateTimelineTool;
        let def = tool.definition();
        let props = &def["function"]["parameters"]["properties"];
        assert!(props.get("dates").is_some(), "schema 必须含 dates");
        assert!(props.get("calculations").is_some(), "schema 必须含 calculations");
        let calc_items = &props["calculations"]["items"]["properties"];
        assert!(calc_items.get("from").is_some(), "calculations[].from 必须存在");
        assert!(calc_items.get("to").is_some(), "calculations[].to 必须存在");
        assert!(calc_items.get("day_count_type").is_some(), "calculations[].day_count_type 必须存在");
        assert!(props.get("constraints").is_none(), "schema 不得含旧 constraints");
        assert!(calc_items.get("min_days").is_none() && calc_items.get("max_days").is_none()
            && calc_items.get("legal_basis").is_none(), "schema 不得含法规字段");
        // day_count_type canonical enum
        let e = props["calculations"]["items"]["properties"]["day_count_type"]["enum"].as_array().unwrap();
        let values: Vec<&str> = e.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(values, vec!["calendar_days", "working_days"], "day_count_type canonical 值");
    }

    #[test]
    #[ignore = "4B 工具契约迁移未完成（另立项）：definition 未迁到新 schema"]
    fn test_announcement_legacy_field_descriptions() {
        let tool = crate::agents::tools::verify_announcement_period::VerifyAnnouncementPeriodTool;
        let def = tool.definition();
        let props = &def["function"]["parameters"]["properties"];

        // legacy announcement 描述必须禁止替代文件提供起点
        let ann = props["announcement_date_str"]["description"].as_str().unwrap();
        assert!(ann.contains("legacy"), "announcement_date_str 描述必须标注 legacy: {}", ann);
        assert!(ann.contains("不得作为采购文件发出/提供起点"), "announcement_date_str 描述必须禁止替代文件提供起点: {}", ann);

        // bid_deadline 描述必须禁止用于公告/文件提供期限
        let bid = props["bid_deadline_date_str"]["description"].as_str().unwrap();
        assert!(bid.contains("不得用于公告期或文件提供期计算"), "bid_deadline_date_str 描述必须禁止用于公告/文件提供期限: {}", bid);

        // schema required 只有 procurement_method
        let required = def["function"]["parameters"]["required"].as_array().expect("required 必须为数组");
        assert_eq!(required.len(), 1, "schema required 应只有 procurement_method");
        assert_eq!(required[0].as_str(), Some("procurement_method"));
    }

    // ─── Agent → Tool 分配验证 ──────────────────────────────────

    #[test]
    fn test_procedure_agent_has_v3_tools() {
        let agent_registry = AgentRegistry::builtin();
        let def = agent_registry.get(crate::agents::types::AgentId::Procedure)
            .expect("ProcedureAgent 未注册");
        for tool_name in V3_TOOLS {
            assert!(def.tool_names.iter().any(|t| *t == *tool_name),
                "ProcedureAgent 缺少 V3 工具 '{}'", tool_name);
        }
    }

    #[test]
    fn test_scoring_agent_has_v4_tools() {
        let agent_registry = AgentRegistry::builtin();
        let def = agent_registry.get(crate::agents::types::AgentId::Scoring)
            .expect("ScoringAgent 未注册");
        for tool_name in V4_TOOLS {
            assert!(def.tool_names.iter().any(|t| *t == *tool_name),
                "ScoringAgent 缺少 V4 工具 '{}'", tool_name);
        }
    }

    /// 验证 output_verification_batch 仅被 LegalVerifyAgent 使用
    /// （作为终端工具，类似 output_finding，不应被普通 Agent 使用）
    #[test]
    fn test_output_verification_batch_only_for_legal_verify() {
        let agent_registry = AgentRegistry::builtin();
        for agent_id in agent_registry.all_ids() {
            let is_legal_verify = matches!(agent_id, crate::agents::types::AgentId::LegalVerify);
            if let Some(def) = agent_registry.get(agent_id) {
                if !is_legal_verify {
                    assert!(
                        !def.tool_names.iter().any(|t| *t == "output_verification_batch"),
                        "Agent '{}' 不应持有 output_verification_batch（仅 LegalVerifyAgent 使用）",
                        def.display_name
                    );
                }
            }
        }
    }
}
