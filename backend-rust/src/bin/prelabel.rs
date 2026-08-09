//! Pre-labeling binary — extracts clauses from golden fixtures, runs them
//! through the YAML rule engine + legacy keyword engine, and outputs a
//! structured JSON report for Golden Standard construction.
//!
//! ## Run
//! ```powershell
//! cargo run --bin prelabel
//! ```

use ai_bid::agents::types::RiskFinding;
use ai_bid::rules::catalog::{display_name, owner_agent};
use ai_bid::rules::context::build_agent_context;
use ai_bid::rules::engine::{candidate_categories, normalize_finding};
use ai_bid::rules::metrics::DocumentMetrics;
use ai_bid::rules::validator::{evaluate_rulebook, load_rulebook};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;

// ── Fixture data model ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct GoldenDocument {
    document_id: String,
    source_path: String,
    sections: Vec<GoldenSection>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GoldenSection {
    level: u32,
    title: String,
    body_text: Option<String>,
    children: Option<Vec<GoldenSection>>,
    #[serde(default)]
    page_start: u32,
    #[serde(default)]
    page_end: u32,
}

// ── Output schema ────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct ClauseAnnotation {
    clause_index: usize,
    section_title: String,
    page_range: String,
    clause_text: String,
    text_length: usize,
    engine_candidates: Vec<String>,
    rulebook_hits: Vec<RuleHitSummary>,
    rulebook_categories: Vec<String>,
    is_critical: bool,
    critical_reason: String,
    critical_display_name: String,
    matched_rules_count: usize,
    agent_name: String,
    law_refs: Vec<String>,
    note: String,
}

#[derive(Debug, Serialize)]
struct RuleHitSummary {
    rule_id: String,
    category: String,
    severity: String,
    law_ref: String,
}

#[derive(Debug, Serialize)]
struct ReportSummary {
    total_clauses: usize,
    clauses_with_hits: usize,
    total_hits: usize,
    critical_clauses: usize,
    category_distribution: Vec<(String, usize)>,
    critical_rate: f64,
    fixture_path: String,
    rulebook_path: String,
}

// ── Recursive clause extraction ──────────────────────────────────

fn extract_clauses(sections: &[GoldenSection]) -> Vec<(String, String, String)> {
    let mut clauses = Vec::new();
    for sec in sections {
        // Only sections with meaningful body_text (>= 15 chars) are worth analyzing
        if let Some(ref text) = sec.body_text {
            let trimmed = text.trim();
            if trimmed.chars().count() >= 15 {
                clauses.push((
                    sec.title.clone(),
                    format!("p{}-p{}", sec.page_start, sec.page_end),
                    trimmed.to_string(),
                ));
            }
        }
        if let Some(ref children) = sec.children {
            clauses.extend(extract_clauses(children));
        }
    }
    clauses
}

// ── Main ────────────────────────────────────────────────────────

fn main() {
    dotenv::dotenv().ok();
    if let Some(parent) = std::env::current_dir().ok().and_then(|d| d.parent().map(|p| p.join(".env"))) {
        if parent.exists() {
            dotenv::from_path(&parent).ok();
        }
    }

    // 1. Load golden fixture
    let fixture_path = "tests/fixtures/golden_sections.json";
    let data = std::fs::read_to_string(fixture_path)
        .expect("Failed to read golden_sections.json");
    let doc: GoldenDocument = serde_json::from_str(&data)
        .expect("Failed to parse golden_sections.json");

    // 2. Load YAML rulebook
    let rulebook_path = "src/rules/data/conditions.yaml";
    let (rulebook, warnings) = load_rulebook(Path::new(rulebook_path))
        .expect("Failed to load conditions.yaml");
    if !warnings.is_empty() {
        eprintln!("??  Rulebook warnings:");
        for w in &warnings {
            eprintln!("  {w}");
        }
    }
    eprintln!("Loaded {} rules from {rulebook_path}", rulebook.rules.len());

    // 3. Extract clauses
    let clauses = extract_clauses(&doc.sections);
    eprintln!("Extracted {} clauses from fixture", clauses.len());

    // 4. Annotate each clause
    let mut annotations: Vec<ClauseAnnotation> = Vec::new();
    let mut total_hits = 0usize;
    let mut clauses_with_hits = 0usize;
    let mut critical_clauses = 0usize;
    let mut category_counts: Vec<(String, usize)> = Vec::new();

    for (idx, (title, page_range, text)) in clauses.iter().enumerate() {
        // 4a. Legacy keyword candidates (engine.rs)
        let engine_candidates: Vec<String> = candidate_categories(text)
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        // 4b. YAML rulebook evaluation
        let metrics = DocumentMetrics::extract_from_clause_text(text);
        let raw_hits = evaluate_rulebook(&rulebook, text, &metrics);

        // Convert raw hits to summary + dedup categories
        let mut hit_summaries = Vec::new();
        let mut seen_categories = Vec::new();
        for (rule_id, category) in &raw_hits {
            if let Some(rule) = rulebook.rules.iter().find(|r| r.id == *rule_id) {
                hit_summaries.push(RuleHitSummary {
                    rule_id: rule_id.clone(),
                    category: category.clone(),
                    severity: rule.severity.clone(),
                    law_ref: rule.law_ref.clone(),
                });
                if !seen_categories.contains(category) {
                    seen_categories.push(category.clone());
                }
            }
        }

        // 4c. Normalize finding using the FIRST matched category
        let mut finding = RiskFinding {
            risk_id: format!("RC_{idx:04}"),
            clause_ids: vec![format!("clause_{idx}")],
            block_ids: vec![],
            agent: "RuleEngineAgent".into(),
            no_risk: raw_hits.is_empty(),
            severity: ai_bid::agents::types::RiskSeverity::Medium,
            is_critical: false,
            critical_reason: String::new(),
            risk_type: String::new(),
            category_code: seen_categories.first().cloned().unwrap_or_default(),
            source_quote: text.clone(),
            legal_basis: vec![],
            case_refs: vec![],
            reason: String::new(),
            suggestion: String::new(),
            confidence: 0.8,
            initial_tier: ai_bid::agents::types::RiskTier::Medium,
            final_tier: ai_bid::agents::types::RiskTier::Medium,
            tier_escalated: false,
            truncated: false,
            suggested_agent: None,
            citations: vec![],
            finding_role: ai_bid::agents::types::FindingRole::Verified,
            knowledge_source: String::new(),
            verification_required: vec![],
            hypothesized_by: vec![],
            verified_by: vec![],
            page_number: None,
            section_path: None,
            context: None,
        };

        normalize_finding(&mut finding);

        // 4d. Build agent context
        let matched_rules_refs: Vec<&ai_bid::rules::schema::Rule> = rulebook
            .rules
            .iter()
            .filter(|r| raw_hits.iter().any(|(id, _)| id == &r.id))
            .collect();
        let agent_name = if seen_categories.is_empty() {
            "None"
        } else {
            owner_agent(&seen_categories[0])
        };
        let _ctx = build_agent_context(&matched_rules_refs, agent_name);

        // 4e. Collect law refs
        let law_refs: Vec<String> = matched_rules_refs
            .iter()
            .map(|r| r.law_ref.clone())
            .filter(|s| !s.is_empty())
            .collect();

        let note = if raw_hits.is_empty() {
            "无规则命中".into()
        } else if finding.is_critical {
            format!("??  Critical: {}", finding.critical_reason)
        } else {
            format!("命中 {} 条规则，非 Critical", raw_hits.len())
        };

        if !raw_hits.is_empty() {
            clauses_with_hits += 1;
            total_hits += raw_hits.len();
        }
        if finding.is_critical {
            critical_clauses += 1;
        }
        for cat in &seen_categories {
            if let Some(entry) = category_counts.iter_mut().find(|(c, _)| c == cat) {
                entry.1 += 1;
            } else {
                category_counts.push((cat.clone(), 1));
            }
        }

        annotations.push(ClauseAnnotation {
            clause_index: idx,
            section_title: title.clone(),
            page_range: page_range.clone(),
            clause_text: text.clone(),
            text_length: text.chars().count(),
            engine_candidates,
            rulebook_hits: hit_summaries,
            rulebook_categories: seen_categories,
            is_critical: finding.is_critical,
            critical_reason: finding.critical_reason.clone(),
            critical_display_name: display_name(&finding.category_code)
                .unwrap_or("")
                .to_string(),
            matched_rules_count: raw_hits.len(),
            agent_name: agent_name.into(),
            law_refs,
            note,
        });
    }

    // 5. Build summary
    category_counts.sort_by(|a, b| b.1.cmp(&a.1));
    let critical_rate = if !annotations.is_empty() {
        critical_clauses as f64 / annotations.len() as f64
    } else {
        0.0
    };

    let summary = ReportSummary {
        total_clauses: annotations.len(),
        clauses_with_hits,
        total_hits,
        critical_clauses,
        category_distribution: category_counts,
        critical_rate,
        fixture_path: fixture_path.into(),
        rulebook_path: rulebook_path.into(),
    };

    // 6. Output report
    let report = json!({
        "summary": {
            "total_clauses": summary.total_clauses,
            "clauses_with_hits": summary.clauses_with_hits,
            "total_hits": summary.total_hits,
            "critical_clauses": summary.critical_clauses,
            "critical_rate": format!("{:.1}%", summary.critical_rate * 100.0),
            "category_distribution": summary.category_distribution.iter().map(|(k, v)| {
                json!({"category": k, "count": v})
            }).collect::<Vec<_>>(),
            "fixture": summary.fixture_path,
            "rulebook": summary.rulebook_path,
            "document_source": doc.source_path,
            "document_id": doc.document_id,
        },
        "clauses": annotations,
    });

    // Print summary to stderr
    eprintln!("\n╔══════════════════════════════════════════════╗");
    eprintln!("║  Pre-labeling Report — Golden Fixture      ║");
    eprintln!("╚══════════════════════════════════════════════╝");
    eprintln!("  Document: {}", doc.source_path);
    eprintln!("  Fixture:  {fixture_path}");
    eprintln!("  Rulebook: {rulebook_path}");
    eprintln!("  ─────────────────────────────────────────");
    eprintln!("  Total clauses:        {}", summary.total_clauses);
    eprintln!("  Clauses with hits:    {}", summary.clauses_with_hits);
    eprintln!("  Total rule hits:      {}", summary.total_hits);
    eprintln!("  Critical clauses:     {}", summary.critical_clauses);
    eprintln!("  Critical rate:        {:.1}%", summary.critical_rate * 100.0);
    eprintln!("  ─────────────────────────────────────────");
    eprintln!("  Category distribution:");
    for (cat, count) in &summary.category_distribution {
        let display = display_name(cat).unwrap_or("—");
        eprintln!("    {:<25} {:>3}  ({})", cat, count, display);
    }
    eprintln!("  ─────────────────────────────────────────");
    eprintln!("  Top clauses:");
    for ann in annotations.iter().filter(|a| a.is_critical).take(5) {
        let title_short: String = ann.section_title.chars().take(28).collect();
        eprintln!(
            "    [CRITICAL] #{} {:<30} rules={} reason={}",
            ann.clause_index,
            title_short,
            ann.matched_rules_count,
            ann.critical_reason
        );
    }
    for ann in annotations.iter().filter(|a| !a.is_critical && a.matched_rules_count > 0).take(5) {
        let title_short: String = ann.section_title.chars().take(28).collect();
        eprintln!(
            "    [HIT]      #{} {:<30} rules={} categories={:?}",
            ann.clause_index,
            title_short,
            ann.matched_rules_count,
            ann.rulebook_categories
        );
    }

    // Print JSON to stdout for piping
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
