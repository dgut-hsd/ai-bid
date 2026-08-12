//! Rule matching engine module - deterministic rule screening layer.
//!
//! Relationship with `agents/risk_taxonomy.rs`:
//! - This module is the main implementation (YAML-driven).
//! - `risk_taxonomy.rs` acts as a facade keeping 5 public signatures unchanged,
//!   delegating to this module.
//! - `react_loop.rs` / `coordinator.rs` need no changes.
//!
//! See `docs/rule-engine-test-plan.md` and the implementation plan.

pub mod catalog;
pub mod context;
pub mod engine;
pub mod matchers;
pub mod metrics;
pub mod schema;
pub mod validator;
