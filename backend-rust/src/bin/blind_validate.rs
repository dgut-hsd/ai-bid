//! Blind-v2 ground truth validation ? runs the YAML rule engine + normalize_finding
//! directly against the 30 frozen annotations' source_quote texts.
//!
//! This is a lightweight proxy for the full pipeline: it tests whether the rule
//! engine + critical_evidence logic would catch each injected risk, without
//! needing the server / PDF parsing / LLM agents.
//!
//! ## Run
//! ```powershell
//! cargo run --bin blind_validate
//! ```

use ai_bid::agents::types::{RiskFinding, RiskSeverity, RiskTier, FindingRole};
use ai_bid::rules::catalog::display_name;
use ai_bid::rules::engine::{candidate_categories, normalize_finding};
use ai_bid::rules::metrics::DocumentMetrics;
use ai_bid::rules::validator::{evaluate_rulebook, load_rulebook};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Annotation {
    document_id: String,
    finding_id: String,
    category_code: String,
    risk_type: String,
    severity: String,
    is_critical: bool,
    source_quote: String,
}

/// Map blind-v2 category codes (e.g. "C01_LOCAL_REGISTRATION") to canonical
/// codes (e.g. "LOCAL_REGISTRATION").
fn canonical_from_blind(code: &str) -> &str {
    // Format: "C01_LOCAL_REGISTRATION" ? strip prefix before first '_'
    if let Some(idx) = code.find('_') {
        &code[idx + 1..]
    } else {
        code
    }
}

fn main() {
    dotenv::dotenv().ok();
    if let Some(parent) = std::env::current_dir()
        .ok()
        .and_then(|d| d.parent().map(|p| p.join(".env")))
    {
        if parent.exists() {
            dotenv::from_path(&parent).ok();
        }
    }

    // 1. Load rulebook
    let rulebook_path = "src/rules/data/conditions.yaml";
    let (book, warnings) = load_rulebook(Path::new(rulebook_path))
        .expect("Failed to load conditions.yaml");
    if !warnings.is_empty() {
        eprintln!("??  Rulebook warnings: {}", warnings.len());
    }
    eprintln!("Loaded {} rules", book.rules.len());

    // 2. Load annotations
    let ann_path = "../benchmark/blind-v2/data/annotations.jsonl";
    let raw = std::fs::read_to_string(ann_path)
        .expect("Failed to read annotations.jsonl");
    let annotations: Vec<Annotation> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("Failed to parse annotation"))
        .collect();
    eprintln!("Loaded {} ground truth annotations", annotations.len());

    // 3. Evaluate each annotation
    let mut tp = 0;      // category correctly detected
    let mut fp = 0;      // category detected but not in ground truth (per annotation)
    let mut fn_ = 0;     // category missed
    let mut critical_tp = 0;   // Critical correctly marked
    let mut critical_fn = 0;   // Critical missed
    let mut critical_fp = 0;   // Non-critical marked as Critical

    let mut per_category: Vec<(String, usize, usize, usize)> = Vec::new();

    for ann in &annotations {
        let text = &ann.source_quote;
        let expected_cat = canonical_from_blind(&ann.category_code);

        // Rule engine evaluation
        let metrics = DocumentMetrics::extract_from_clause_text(text);
        let hits = evaluate_rulebook(&book, text, &metrics);
        let detected_cats: Vec<String> = hits
            .iter()
            .map(|(_, c)| c.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        // Legacy keyword candidates
        let legacy_candidates: Vec<String> = candidate_categories(text)
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        // Normalize finding (simulates what the agent pipeline does)
        let mut finding = RiskFinding {
            risk_id: ann.finding_id.clone(),
            clause_ids: vec![],
            block_ids: vec![],
            agent: "RuleEngineAgent".into(),
            no_risk: hits.is_empty() && legacy_candidates.is_empty(),
            severity: match ann.severity.as_str() {
                "high" => RiskSeverity::High,
                "medium" => RiskSeverity::Medium,
                _ => RiskSeverity::Medium,
            },
            is_critical: false,
            critical_reason: String::new(),
            risk_type: String::new(),
            category_code: detected_cats.first().cloned().unwrap_or_default(),
            source_quote: text.clone(),
            legal_basis: vec![],
            case_refs: vec![],
            reason: String::new(),
            suggestion: String::new(),
            confidence: 0.8,
            initial_tier: RiskTier::Medium,
            final_tier: RiskTier::Medium,
            tier_escalated: false,
            truncated: false,
            suggested_agent: None,
            citations: vec![],
            finding_role: FindingRole::Verified,
            knowledge_source: String::new(),
            verification_required: vec![],
            hypothesized_by: vec![],
            verified_by: vec![],
            page_number: None,
            section_path: None,
            context: None,
        };
        normalize_finding(&mut finding);

        // Check category match (rule engine OR legacy)
        let all_detected: Vec<&String> = detected_cats
            .iter()
            .chain(legacy_candidates.iter())
            .collect();
        let cat_matched = all_detected.iter().any(|c| *c == expected_cat);

        // Check critical match
        let critical_matched = finding.is_critical;

        // Tally
        if cat_matched {
            tp += 1;
        } else {
            fn_ += 1;
        }
        // Count false positives as extra categories detected
        for c in &detected_cats {
            if c != expected_cat {
                fp += 1;
            }
        }

        if ann.is_critical {
            if critical_matched {
                critical_tp += 1;
            } else {
                critical_fn += 1;
            }
        } else if critical_matched {
            critical_fp += 1;
        }

        // Per-category tracking
        if let Some(entry) = per_category.iter_mut().find(|(c, _, _, _)| c == expected_cat) {
            entry.1 += 1; // total
            if cat_matched {
                entry.2 += 1; // hit
            } else {
                entry.3 += 1; // miss
            }
        } else {
            per_category.push((expected_cat.to_string(), 1, if cat_matched { 1 } else { 0 }, if cat_matched { 0 } else { 1 }));
        }

        // Per-annotation detail
        let cat_status = if cat_matched { "?" } else { "?" };
        let crit_status = if ann.is_critical {
            if critical_matched { "?C" } else { "?C" }
        } else if critical_matched { "?FP" } else { "  " };
        eprintln!(
            "  {} {} {:<25} [{:<8}] crit={} hits={:?} legacy={:?}",
            ann.finding_id,
            cat_status,
            expected_cat,
            ann.severity,
            crit_status,
            detected_cats,
            legacy_candidates
        );
    }

    // 4. Summary
    let total = annotations.len();
    let recall = if tp + fn_ > 0 { tp as f64 / (tp + fn_) as f64 } else { 1.0 };
    let precision = if tp + fp > 0 { tp as f64 / (tp + fp) as f64 } else { 1.0 };
    let f1 = if recall + precision > 0.0 { 2.0 * recall * precision / (recall + precision) } else { 0.0 };
    let critical_recall = if critical_tp + critical_fn > 0 {
        critical_tp as f64 / (critical_tp + critical_fn) as f64
    } else { 1.0 };
    let critical_precision = if critical_tp + critical_fp > 0 {
        critical_tp as f64 / (critical_tp + critical_fp) as f64
    } else { 1.0 };

    eprintln!("\n????????????????????????????????????????????????????????????");
    eprintln!("?  Blind-v2 Ground Truth ? Rule Engine Validation         ?");
    eprintln!("????????????????????????????????????????????????????????????");
    eprintln!("  Total annotations:     {total}");
    eprintln!("  ?????????????????????????????????????????????????????");
    eprintln!("  Category detection:");
    eprintln!("    TP={tp}  FP={fp}  FN={fn_}");
    eprintln!("    Recall={:.1}%  Precision={:.1}%  F1={:.1}%", recall * 100.0, precision * 100.0, f1 * 100.0);
    eprintln!("  ?????????????????????????????????????????????????????");
    eprintln!("  Critical marking:");
    eprintln!("    Critical TP={critical_tp}  Critical FN={critical_fn}  Critical FP={critical_fp}");
    eprintln!("    Critical Recall={:.1}%  Critical Precision={:.1}%",
        critical_recall * 100.0, critical_precision * 100.0);
    eprintln!("  ?????????????????????????????????????????????????????");
    eprintln!("  Per-category breakdown:");
    per_category.sort_by(|a, b| a.0.cmp(&b.0));
    for (cat, total_c, hit, miss) in &per_category {
        let display = display_name(cat).unwrap_or("?");
        eprintln!("    {:<25} {}/{} hit, {} miss  ({})",
            cat, hit, total_c, miss, display);
    }

    // Baseline comparison
    eprintln!("  ?????????????????????????????????????????????????????");
    eprintln!("  Baseline (blind-v2-final-20260727):");
    eprintln!("    Recall=70.0%  Precision=56.8%  F1=62.7%");
    eprintln!("    Critical Recall=30.0%");
    eprintln!("  ?????????????????????????????????????????????????????");
    eprintln!("  Target:");
    eprintln!("    Critical Recall ? 95%");

    // JSON output for piping
    let report = serde_json::json!({
        "total_annotations": total,
        "category_detection": { "tp": tp, "fp": fp, "fn": fn_, "recall": recall, "precision": precision, "f1": f1 },
        "critical_marking": { "tp": critical_tp, "fn": critical_fn, "fp": critical_fp, "recall": critical_recall, "precision": critical_precision },
        "baseline": { "recall": 0.70, "precision": 0.568, "f1": 0.627, "critical_recall": 0.30 },
        "per_category": per_category,
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
