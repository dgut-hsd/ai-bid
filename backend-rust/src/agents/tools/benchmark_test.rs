//! Group A Tool Micro Benchmark — 纯确定性工具性能测量 + UTF-8 fuzz 测试。
//!
//! 测量核心工具的 P50/P95/P99 延迟，输入规模从正常到极端。
//! 这是工具本身的 benchmark，不涉及 LLM / Coordinator。

#[cfg(test)]
mod micro_benchmark {
    use std::time::Instant;

    use crate::agents::tools::AgentTool;
    use crate::agents::tools::verify_bid_deposit::VerifyBidDepositTool;
    use crate::agents::tools::verify_announcement_period::VerifyAnnouncementPeriodTool;
    use crate::agents::tools::detect_subjective_scoring::DetectSubjectiveScoringTool;
    use crate::agents::tools::check_imported_products::CheckImportedProductsTool;
    use crate::agents::tools::validate_scoring_formula::ValidateScoringFormulaTool;
    use crate::agents::tools::validate_weight_distribution::ValidateWeightDistributionTool;
    use crate::agents::tools::verify_consortium_rules::VerifyConsortiumRulesTool;
    use crate::agents::tools::verify_procurement_method::VerifyProcurementMethodTool;
    use crate::agents::tools::check_scoring_completeness::CheckScoringCompletenessTool;

    fn percentile(mut samples: Vec<f64>, p: f64) -> f64 {
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let idx = ((samples.len() as f64) * p).ceil() as usize - 1;
        samples[idx.min(samples.len() - 1)]
    }

    fn measure<F: Fn()>(runs: usize, f: F) -> (f64, f64, f64, f64) {
        let mut samples = Vec::with_capacity(runs);
        for _ in 0..runs {
            let start = Instant::now();
            f();
            samples.push(start.elapsed().as_micros() as f64);
        }
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        (mean, percentile(samples.clone(), 0.50), percentile(samples.clone(), 0.95), percentile(samples.clone(), 0.99))
    }

    fn build_long_text(paragraphs: usize) -> String {
        let mut s = String::new();
        for i in 0..paragraphs {
            s.push_str(&format!(
                "第{}条 评审因素与评分标准。本项目采用综合评分法，价格分权重占比{}%，\
                技术分权重占比{}%，商务分权重占比{}%。评委根据投标文件响应情况酌情打分，\
                评审因素应当细化和量化。投标保证金不得超过合同金额的2%，履约保证金不得超过10%。\
                公告期限自发布之日起不少于20日。",
                i, 30 + i % 20, 40, 30 - i % 20
            ));
        }
        s
    }

    // ─── Micro Benchmark ────────────────────────────────────────

    #[test]
    #[ignore = "micro benchmark：硬性延迟断言依赖机器性能，CI 上 flaky，手动运行 cargo test -- --ignored"]
    fn bench_verify_bid_deposit() {
        let tool = VerifyBidDepositTool;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let args = serde_json::json!({
            "deposit_amount": 30.0,
            "budget_amount": 1500.0,
            "deposit_form": "保函",
            "deposit_type": "bid",
            "procurement_category": "货物"
        });
        let (mean, p50, p95, p99) = measure(1000, || {
            let _ = rt.block_on(tool.execute(args.clone()));
        });
        println!("verify_bid_deposit: mean={:.1}us p50={:.1}us p95={:.1}us p99={:.1}us", mean, p50, p95, p99);
        assert!(p99 < 5000.0, "verify_bid_deposit p99 应 < 5ms，实际 {:.1}us", p99);
    }

    #[test]
    #[ignore = "micro benchmark：硬性延迟断言依赖机器性能，CI 上 flaky，手动运行 cargo test -- --ignored"]
    fn bench_verify_announcement_period_smoke() {
        // debug timing smoke test（非可信生产 P99）：验证 verify_announcement_period 新 Contract。
        let tool = VerifyAnnouncementPeriodTool;
        let rt = tokio::runtime::Runtime::new().unwrap();
        // 真实 NoticePublication payload（4B-4C 新 Contract）
        let args = serde_json::json!({
            "procurement_method": "公开招标",
            "period_type": "notice_publication",
            "procurement_object": "goods",
            "is_government_procurement": true,
            "notice_start_date_str": "2025-03-03",
            "notice_end_date_str": "2025-03-10"
        });
        // Preflight：真实执行必须成功，禁止 let _ 吞错
        let preflight = rt.block_on(tool.execute(args.clone()));
        assert!(preflight.is_ok(), "preflight 必须成功（真实 Contract payload）: {:?}", preflight.err());
        let preflight_out = preflight.unwrap();
        assert_eq!(preflight_out["overall_status"], "compliant");
        assert_eq!(preflight_out["announcement_period"]["required_days"], 5);
        let (mean, p50, p95, p99) = measure(1000, || {
            let _ = rt.block_on(tool.execute(args.clone()));
        });
        println!("verify_announcement_period(smoke): mean={:.1}us p50={:.1}us p95={:.1}us p99={:.1}us", mean, p50, p95, p99);
        assert!(p99 < 5000.0);
    }

    #[test]
    #[ignore = "micro benchmark：硬性延迟断言依赖机器性能，CI 上 flaky，手动运行 cargo test -- --ignored"]
    fn bench_detect_subjective_scoring_normal() {
        let tool = DetectSubjectiveScoringTool;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let args = serde_json::json!({
            "scoring_text": "评委酌情打分，根据投标人综合表现给予相应分值。评分区间0-10分。"
        });
        let (mean, p50, p95, p99) = measure(1000, || {
            let _ = rt.block_on(tool.execute(args.clone()));
        });
        println!("detect_subjective_scoring(normal): mean={:.1}us p50={:.1}us p95={:.1}us p99={:.1}us", mean, p50, p95, p99);
        assert!(p99 < 5000.0);
    }

    #[test]
    #[ignore = "micro benchmark：硬性延迟断言依赖机器性能，CI 上 flaky，手动运行 cargo test -- --ignored"]
    fn bench_detect_subjective_scoring_long_text() {
        let tool = DetectSubjectiveScoringTool;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let long_text = build_long_text(200);
        assert!(long_text.len() > 30_000, "测试文本应 > 30KB，实际 {} bytes", long_text.len());
        let args = serde_json::json!({
            "scoring_text": long_text,
            "score_range_max": 20.0,
            "score_range_min": 0.0
        });
        let (mean, p50, p95, p99) = measure(100, || {
            let _ = rt.block_on(tool.execute(args.clone()));
        });
        println!("detect_subjective_scoring(40KB): mean={:.1}us p50={:.1}us p95={:.1}us p99={:.1}us", mean, p50, p95, p99);
        assert!(p99 < 50_000.0, "40KB 文本 p99 应 < 50ms，实际 {:.1}us", p99);
    }

    #[test]
    #[ignore = "micro benchmark：硬性延迟断言依赖机器性能，CI 上 flaky，手动运行 cargo test -- --ignored"]
    fn bench_check_imported_products_long_text() {
        let tool = CheckImportedProductsTool;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut long_text = build_long_text(100);
        long_text.push_str("本项目拟采购进口医疗设备，需经财政部门审批。");
        let args = serde_json::json!({
            "project_description": long_text,
            "procurement_category": "货物",
            "has_import_approval": false
        });
        let (mean, p50, p95, p99) = measure(100, || {
            let _ = rt.block_on(tool.execute(args.clone()));
        });
        println!("check_imported_products(20KB): mean={:.1}us p50={:.1}us p95={:.1}us p99={:.1}us", mean, p50, p95, p99);
        assert!(p99 < 50_000.0);
    }

    #[test]
    #[ignore = "micro benchmark：硬性延迟断言依赖机器性能，CI 上 flaky，手动运行 cargo test -- --ignored"]
    fn bench_validate_scoring_formula() {
        let tool = ValidateScoringFormulaTool;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let args = serde_json::json!({
            "price_weight": 30.0,
            "procurement_object": "goods",
            "procurement_method": "open_tender",
            "evaluation_method": "comprehensive_scoring",
            "price_evaluation_context": "normal",
            "scoring_formula_type": "最低价"
        });
        let (mean, p50, p95, p99) = measure(1000, || {
            let _ = rt.block_on(tool.execute(args.clone()));
        });
        println!("validate_scoring_formula: mean={:.1}us p50={:.1}us p95={:.1}us p99={:.1}us", mean, p50, p95, p99);
        assert!(p99 < 5000.0);
    }

    #[test]
    #[ignore = "micro benchmark：硬性延迟断言依赖机器性能，CI 上 flaky，手动运行 cargo test -- --ignored"]
    fn bench_validate_weight_distribution() {
        let tool = ValidateWeightDistributionTool;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut items = Vec::new();
        for i in 0..20 {
            items.push(serde_json::json!({
                "name": format!("评审因素{}", i),
                "weight": if i == 0 { 10.0 } else { 5.0 },
                "dimension": if i < 8 { "价格" } else if i < 14 { "技术" } else { "商务" }
            }));
        }
        let args = serde_json::json!({
            "procurement_category": "货物",
            "items": items
        });
        let (mean, p50, p95, p99) = measure(1000, || {
            let _ = rt.block_on(tool.execute(args.clone()));
        });
        println!("validate_weight_distribution(20 items): mean={:.1}us p50={:.1}us p95={:.1}us p99={:.1}us", mean, p50, p95, p99);
        assert!(p99 < 5000.0);
    }

    // ─── UTF-8 / 中文 Fuzz-style 测试 ────────────────────────────
    // 目标：这些工具绝不能因中文字符切片而 panic。

    fn run_tool(tool: &dyn AgentTool, args: serde_json::Value) -> Result<serde_json::Value, anyhow::Error> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(tool.execute(args))
    }

    /// 对每个样本运行工具，绝不能 panic
    fn assert_no_panic(tool_name: &str, samples: &[String], f: impl Fn(&str) -> Result<serde_json::Value, anyhow::Error>) {
        for (i, sample) in samples.iter().enumerate() {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = f(sample);
            }));
            assert!(
                result.is_ok(),
                "{} 对样本 #{} (len={}) panic 了！样本前缀: {:?}",
                tool_name, i, sample.len(),
                &sample.chars().take(30).collect::<String>()
            );
        }
    }

    fn build_fuzz_samples() -> Vec<String> {
        vec![
            String::new(),                                        // 空字符串
            "评委酌情打分。".to_string(),                          // 纯中文
            "score by judge discretion.".to_string(),             // 纯英文
            "评委酌情打分 emoji: 🎯🎉🚀 综合判断。".to_string(),    // 中英混合 + emoji
            "★标记条款：不接受联合体投标，资质必须叠加。".to_string(), // 特殊字符
            "保证金：100.5万元（¥1,005,000.00），保函形式提交。".to_string(),
            "日期：2025-06-01至2025/06/25，公告期限不少于20日。".to_string(),
            "设备参数：CPU≥3.0GHz，内存≥32GB DDR5，支持国产化替代。".to_string(),
            "a".repeat(10_000),                                    // 极长 ASCII
            "中".repeat(10_000),                                    // 极长中文
            "混合".repeat(5_000) + &"emoji🚀".repeat(100) + "尾部", // 极长混合
        ]
    }

    #[test]
    fn fuzz_detect_subjective_scoring_no_panic() {
        let tool = DetectSubjectiveScoringTool;
        let samples = build_fuzz_samples();
        assert_no_panic("detect_subjective_scoring", &samples, |s| {
            run_tool(&tool, serde_json::json!({
                "scoring_text": s,
                "score_range_max": 20.0,
                "score_range_min": 0.0
            }))
        });
    }

    #[test]
    fn fuzz_check_imported_products_no_panic() {
        let tool = CheckImportedProductsTool;
        let samples = build_fuzz_samples();
        assert_no_panic("check_imported_products", &samples, |s| {
            run_tool(&tool, serde_json::json!({
                "project_description": s,
                "procurement_category": "货物",
                "has_import_approval": false
            }))
        });
    }

    #[test]
    fn fuzz_verify_consortium_rules_no_panic() {
        let tool = VerifyConsortiumRulesTool;
        let samples = build_fuzz_samples();
        assert_no_panic("verify_consortium_rules", &samples, |s| {
            run_tool(&tool, serde_json::json!({ "clause_text": s }))
        });
    }

    #[test]
    fn fuzz_verify_procurement_method_no_panic() {
        let tool = VerifyProcurementMethodTool;
        let samples = build_fuzz_samples();
        assert_no_panic("verify_procurement_method", &samples, |s| {
            run_tool(&tool, serde_json::json!({
                "procurement_category": "货物",
                "budget_amount_wan": 300.0,
                "declared_method": s
            }))
        });
    }

    #[test]
    fn fuzz_check_scoring_completeness_no_panic() {
        let tool = CheckScoringCompletenessTool;
        let samples = build_fuzz_samples();
        assert_no_panic("check_scoring_completeness", &samples, |s| {
            run_tool(&tool, serde_json::json!({
                "procurement_category": "货物",
                "items": [
                    { "name": s, "score": 30.0, "dimension": "价格" },
                    { "name": "技术", "score": 50.0, "dimension": "技术" }
                ],
                "total_score": 100.0
            }))
        });
    }
}
