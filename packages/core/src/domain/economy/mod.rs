//! Economy domain — single source of truth for every cost/savings signal.
//!
//! W7A of [[2026-05-26-no-sqlite-git-source-of-truth]] migrated the four
//! record types (spans, savings, context-cost frames, API-cost frames) off
//! SQLite and onto the per-spec NDJSON event channel. The split is now:
//!
//! - [`model`] — pure `serde` records and aggregate types.
//! - [`scope`] — [`EconomyScope`] enum + newtype ids.
//! - [`writer`] — pure payload builders (`*_event` functions returning
//!   `(event_name, Value)`) consumed by the rt-side `event_route::emit`.
//! - [`reader`] — NDJSON-backed query functions.
//! - [`estimator`] — `tiktoken-rs` wrapper + pricing lookup table.
//! - [`multi_project`] — fan-out over project roots for
//!   [`EconomyScope::AllProjects`].
//! - [`sources`] — ingestion adapters for external cost streams.

pub mod estimator;
pub mod model;
pub mod multi_project;
pub mod reader;
pub mod scope;
pub mod sources;
pub mod writer;

// Re-exports — consumers `use mustard_core::domain::economy::{…}` without remembering
// which submodule owns each name.
pub use estimator::{
    estimate_input_tokens, estimate_output_tokens, model_pricing_usd_micros_per_million,
};
pub use model::{
    AgentCost, ApiCostFrame, ContextCostFrame, ContextRoutingMetrics, EconomySummary,
    SavingsBreakdown, SavingsBySource, SavingsRecord, SavingsSource, SessionCost, SpanRecord,
    SpecCost, WaveCost,
};
pub use multi_project::MultiProjectReader;
pub use reader::{
    context_routing_quality, economy_summary, per_agent_costs, per_spec_costs, per_wave_costs,
    savings_breakdown,
};
pub use scope::{AgentId, EconomyScope, ProjectPath, SpecId, WaveId};
pub use writer::{
    context_frame_event, injection_savings_tokens, run_event, savings_event,
};
