//! Unified risk taxonomy, evidence gating, and critical-issue policy - Facade.
//!
//! This file is a backward-compatible facade: the 5 public function signatures
//! are kept byte-identical, all internals delegate to `crate::rules::`.
//!
//! This allows `react_loop.rs` (calls `review_candidates_for_agent` / `display_name`)
//! and `coordinator.rs` (calls `normalize_finding` / `is_actionable` /
//! `canonical_category`) to use the new rule engine without any changes.
//!
//! Implementation lives in `rules/engine.rs` and `rules/catalog.rs`.

use crate::agents::types::RiskFinding;
use crate::rules;

/// Delegates to `rules::engine::canonical_category` - signature unchanged.
pub fn canonical_category(finding: &RiskFinding) -> String {
    rules::engine::canonical_category(finding)
}

/// Delegates to `rules::catalog::display_name` - signature unchanged.
pub fn display_name(code: &str) -> Option<&'static str> {
    rules::catalog::display_name(code)
}

/// Delegates to `rules::engine::candidate_categories` - signature unchanged.
pub fn candidate_categories(text: &str) -> Vec<&'static str> {
    rules::engine::candidate_categories(text)
}

/// Delegates to `rules::catalog::owner_agent` - signature unchanged.
pub fn owner_agent(code: &str) -> &'static str {
    rules::catalog::owner_agent(code)
}

/// Delegates to `rules::engine::review_candidates_for_agent` - signature unchanged.
pub fn review_candidates_for_agent(text: &str, agent: &str) -> Vec<&'static str> {
    rules::engine::review_candidates_for_agent(text, agent)
}

/// Delegates to `rules::engine::is_actionable` - signature unchanged.
pub fn is_actionable(finding: &RiskFinding) -> bool {
    rules::engine::is_actionable(finding)
}

/// Delegates to `rules::engine::normalize_finding` - signature unchanged.
pub fn normalize_finding(finding: &mut RiskFinding) {
    rules::engine::normalize_finding(finding);
}
