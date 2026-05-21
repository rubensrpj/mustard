//! Economy domain — single source of truth for every cost/savings signal.
//!
//! Wave 1 of the `economia-moat-unification` spec: this module consolidates
//! the four record types (spans, savings, context-cost frames, API-cost
//! frames) under one writer/reader pair, with [`EconomyScope`] as the
//! cross-cutting query selector and [`estimator`] providing a token-count
//! preview backed by `tiktoken-rs`.
//!
//! Layered the same way the rest of `mustard-core` is split:
//!
//! - [`model`] — pure `serde` records and aggregate types.
//! - [`scope`] — [`EconomyScope`] enum + newtype ids.
//! - [`writer`] — 4 `record_*` functions; the only writers in the system.
//! - [`reader`] — 6 query functions; the only readers in the system.
//! - [`estimator`] — `tiktoken-rs` wrapper + pricing lookup table.
//! - [`multi_project`] — fan-out over many project DBs for
//!   [`EconomyScope::AllProjects`].
//! - [`sources`] — placeholder for W3 ingestion adapters.

pub mod estimator;
pub mod model;
pub mod multi_project;
pub mod reader;
pub mod scope;
pub mod sources;
pub mod store;
pub mod writer;

// Re-exports — consumers `use mustard_core::economy::{…}` without remembering
// which submodule owns each name. Same shape as `store::*` and `model::*`.
pub use estimator::{
    estimate_input_tokens, estimate_output_tokens, model_pricing_usd_micros_per_million,
};
pub use model::{
    AgentCost, ApiCostFrame, ContextCostFrame, ContextRoutingMetrics, EconomySummary,
    SavingsBreakdown, SavingsBySource, SavingsRecord, SavingsSource, SpanRecord, SpecCost,
    WaveCost,
};
pub use multi_project::MultiProjectReader;
pub use reader::{
    context_routing_quality, economy_summary, per_agent_costs, per_spec_costs, per_wave_costs,
    savings_breakdown,
};
pub use scope::{AgentId, EconomyScope, ProjectPath, SpecId, WaveId};
pub use store::open_for;
pub use writer::{record_api_cost, record_context_cost, record_savings, record_span};
