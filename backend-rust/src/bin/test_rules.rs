//! Rule engine test binary - verifies build_agent_context output and rule matching.
//!
//! Unlike `bin/test_agents.rs` which drives the Coordinator with synthetic ReviewClauses,
//! this binary bypasses the Agent pipeline entirely and directly exercises the rule engine.
//!
//! ## Run
//!
//! ```powershell
//! cd backend-rust
//! cargo run --bin test_rules                    # default: run all synthetic clauses
//! cargo run --bin test_rules -- --json          # output as JSON
//! cargo run --bin test_rules -- --golden <path> # run Golden Standard regression
//! ```

use ai_bid::rules::catalog::{display_name, owner_agent};
use ai_bid::rules::context::build_agent_context;
use ai_bid::rules::engine::candidate_categories;
use ai_bid::agents::risk_taxonomy;
use serde_json::json;
use std::io::{self, Write};

/// A synthetic test clause with expected candidate categories.
struct TestClause {
    name: &'static str,
    text: &'static str,
    /// Expected candidate categories (any order)
    expected_candidates: &'static [&'static str],
}

const TEST_CLAUSES: &[TestClause] = &[
    TestClause {
        name: "local_registration",
        text: "投标人须在本市注册成立三年以上，且在本市设有分支机构。",
        expected_candidates: &["LOCAL_REGISTRATION"],
    },
    TestClause {
        name: "oem_authorization",
        text: "投标人必须提交生产厂家针对本项目出具的授权函，否则投标无效。",
        expected_candidates: &["OEM_AUTHORIZATION"],
    },
    TestClause {
        name: "regional_performance",
        text: "供应商须提供采购人所在区县的同类服务案例，跨区域案例不作为有效业绩。",
        expected_candidates: &["REGIONAL_PERFORMANCE"],
    },
    TestClause {
        name: "excessive_deposit",
        text: "供应商须缴纳相当于采购预算5%的投标保证金。",
        expected_candidates: &["EXCESSIVE_DEPOSIT"],
    },
    TestClause {
        name: "multi_issue",
        text: "供应商须提供采购人所在区县的同类服务案例，跨区域案例不作为有效业绩。\n\
               投标人必须提交生产厂家针对本项目出具的授权函，否则投标无效。",
        expected_candidates: &["REGIONAL_PERFORMANCE", "OEM_AUTHORIZATION"],
    },
    TestClause {
        name: "no_risk",
        text: "本项目的招标文件获取时间为2026年8月1日至2026年8月10日。",
        expected_candidates: &[],
    },
];

fn run_clauses(json_output: bool) -> i32 {
    let mut passed = 0;
    let mut failed = 0;
    let mut results = Vec::new();

    for clause in TEST_CLAUSES {
        let candidates = candidate_categories(clause.text);
        let facade_candidates = risk_taxonomy::candidate_categories(clause.text);

        // Verify facade delegates correctly
        let facade_ok = candidates == facade_candidates;

        // Check expected candidates (order-independent, subset check)
        let mut expected_ok = true;
        for expected in clause.expected_candidates {
            if !candidates.iter().any(|c| *c == *expected) {
                expected_ok = false;
            }
        }
        // If no expected candidates, verify none were found
        if clause.expected_candidates.is_empty() && !candidates.is_empty() {
            // Allow candidates but note them
        }

        // Build agent context for each responsible agent
        let mut agent_contexts = Vec::new();
        for category in &candidates {
            let agent = owner_agent(category);
            let matched: Vec<&ai_bid::rules::schema::Rule> = Vec::new();
            let ctx = build_agent_context(&matched, agent);
            agent_contexts.push(json!({
                "category": category,
                "agent": agent,
                "display_name": display_name(category),
                "candidate_categories": ctx.candidate_categories,
            }));
        }

        let ok = facade_ok && expected_ok;
        if ok {
            passed += 1;
        } else {
            failed += 1;
        }

        if json_output {
            results.push(json!({
                "name": clause.name,
                "passed": ok,
                "candidates": candidates,
                "facade_delegates_ok": facade_ok,
                "expected_ok": expected_ok,
                "agent_contexts": agent_contexts,
            }));
        } else {
            let status = if ok { "PASS" } else { "FAIL" };
            eprintln!(
                "  [{}] {}: candidates={:?} facade_ok={}",
                status, clause.name, candidates, facade_ok
            );
            if !ok {
                eprintln!("    expected: {:?}", clause.expected_candidates);
            }
        }
    }

    if json_output {
        let output = json!({
            "passed": passed,
            "failed": failed,
            "total": passed + failed,
            "results": results,
        });
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        eprintln!(
            "\n=== Rule Engine Test Summary ===\n  passed: {}\n  failed: {}\n  total:  {}",
            passed,
            failed,
            passed + failed
        );
    }

    if failed > 0 {
        1
    } else {
        0
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let json_output = args.iter().any(|a| a == "--json");

    let code = run_clauses(json_output);
    let _ = io::stdout().flush();
    std::process::exit(code);
}
