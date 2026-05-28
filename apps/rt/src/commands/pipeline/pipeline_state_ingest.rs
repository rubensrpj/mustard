//! `mustard-rt run pipeline-state-ingest` — no-op after W2A migration.
//!
//! This subcommand previously ingested `.claude/.pipeline-states/*.json` files
//! into the SQLite event store. After the W2A no-sqlite migration, pipeline
//! state is sourced directly from NDJSON event files under
//! `.claude/spec/*/.events/`. The ingest step is no longer required.
//!
//! The command is kept as a no-op so existing automation that calls
//! `mustard-rt run pipeline-state-ingest` does not break with an unknown
//! subcommand error.

use serde_json::json;

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// Options for `mustard-rt run pipeline-state-ingest`. No-op after W2A —
/// retained as a unit struct so the CLI parser keeps a stable target.
pub struct PipelineStateIngestOpts;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// `mustard-rt run pipeline-state-ingest [--delete]` — no-op.
///
/// Returns the canonical empty-run JSON so callers that parse the output
/// continue to work without modification.
pub fn run(_opts: PipelineStateIngestOpts) {
    let out = json!({
        "ingested": 0,
        "deleted": 0,
        "errors": [],
    });
    println!("{out}");
}
