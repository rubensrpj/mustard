//! The `run` subcommands for the harness event stream (`event/`).
//!
//! TWO registrations per command, both in this file: the variant in
//! [`EventCmd`] AND its arm in [`dispatch`] below. Forgetting the second
//! still compiles, but the command vanishes from the CLI.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run event <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::{event};

/// The `run` subcommands owned by the harness event stream (`event/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum EventCmd {
    /// List the branches a unit could be cut from, newest commit first.
    #[command(display_order = 93)]
    BaseCandidates {
        /// Skip the `git fetch` and list what the clone already knows. The
        /// default refreshes: the whole point is a menu that is true TODAY.
        #[arg(long)]
        no_fetch: bool,
    },
    /// Emit an arbitrary named harness event with a key/value payload.
    #[command(display_order = 8)]
    EmitEvent {
        /// Event name, e.g. `review.start`.
        #[arg(long)]
        event: Option<String>,
        /// Payload entry as `key=value` (repeatable). A value that parses as
        /// JSON is stored typed; otherwise it is kept as a string.
        #[arg(long = "payload")]
        payload: Vec<String>,
        /// Spec identifier (sets the event's top-level `spec` field).
        #[arg(long)]
        spec: Option<String>,
        /// Wave number (defaults to 0).
        #[arg(long, default_value_t = 0)]
        wave: u32,
    },
    /// Record a `pipeline.phase` transition event from a SKILL.
    #[command(display_order = 9)]
    EmitPhase {
        /// Spec identifier.
        #[arg(long)]
        spec: String,
        /// Phase being entered, e.g. `ANALYZE`.
        #[arg(long)]
        to: String,
        /// Prior phase (optional; defaults to the spec's last known phase).
        #[arg(long)]
        from: Option<String>,
    },
    /// Append a typed pipeline event (`pipeline.scope`, `pipeline.status`, etc.).
    ///
    /// On `--kind pipeline.complete` the REVIEW/QA gate refuses emission with
    /// exit 2 unless a `qa.result` event with `overall=pass` exists for the
    /// spec, or `--allow-no-qa` is passed (escape hatch for trusted callers
    /// like `qa-run` itself or an explicit user override).
    #[command(display_order = 10)]
    EmitPipeline {
        /// Pipeline event kind, e.g. `pipeline.scope`. Must be one of the 8 known kinds.
        #[arg(long)]
        kind: String,
        /// Spec the event is attributed to.
        #[arg(long)]
        spec: String,
        /// Optional JSON payload string.
        #[arg(long)]
        payload: Option<String>,
        /// Bypass the REVIEW/QA gate on `pipeline.complete`. Without this flag,
        /// `pipeline.complete` is refused (exit 2) unless a passing `qa.result`
        /// event exists for the spec.
        #[arg(long = "allow-no-qa")]
        allow_no_qa: bool,
        /// Free-form natural-language request. On `--kind pipeline.kind` it
        /// MINTS the unit's canonical name: one slug for the `{kind}/{slug}`
        /// branch, the events and the spec directory (hand it to `spec-draft
        /// --slug`). It supersedes a disagreeing `--spec`, and the report says
        /// so via `renamedFrom`.
        #[arg(long)]
        intent: Option<String>,
        /// What the unit IS: `feature`, `fix` or `hotfix`. On
        /// `--kind pipeline.kind` it names the auto-branch (`{kind}/{slug}`)
        /// and, through `git.flow`, the base the unit is cut from. Omitted →
        /// `feature`. Never inferred from the request: a fix that waits for the
        /// next release and one that goes to production are the same change.
        #[arg(long = "type")]
        work_kind: Option<String>,
        /// Integration base the work branch is cut from. When set, it MUST name
        /// one of the project's `git.flow` integration bases (unknown → error
        /// telling you to declare it), and a `hotfix` may not name the base
        /// ordinary work is cut from; when omitted, the base follows from
        /// `--type`. Agnostic — derived from `git.flow`, never hardcoded.
        #[arg(long)]
        base: Option<String>,
    },
    /// Query the harness event log by view.
    #[command(display_order = 32)]
    EventProjections {
        /// View name: `agent-visibility`, `pipeline-state`, `session-summary`,
        /// `epic-summary`, `cross-session-timeline`, `spec-tree`, `pr-metrics`,
        /// `active-pipelines` (no `--spec` required).
        #[arg(long)]
        view: Option<String>,
        /// Spec name (required by `pipeline-state` / `epic-summary`).
        #[arg(long)]
        spec: Option<String>,
        /// Wave filter for `agent-visibility`.
        #[arg(long)]
        wave: Option<u32>,
        /// Output format: `json` (default) or `html`.
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// The per-branch NOTEBOOK of a work unit: what surfaced during the work
    /// and does NOT belong to its spec. The porta rule is one line — what
    /// belongs to the spec AMENDS the spec, what does not comes here. Records
    /// live under `.claude/spec/<slug>/notebook.md`, beside the unit's own
    /// state, so they travel with the branch and disappear with it. Without
    /// `--add` it READS the notebook back; once the pull request opens, that
    /// reading is the next cycle's prompt.
    #[command(display_order = 90)]
    Notebook {
        /// The item to record — one note, stored as one line. Omitted: the
        /// notebook is read, not written.
        #[arg(long)]
        add: Option<String>,
        /// The work branch whose notebook this is, e.g. `dev_my-unit`.
        /// Omitted: the branch the checkout is standing on.
        #[arg(long)]
        unit: Option<String>,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}

/// Dispatch one `event`-family `run` subcommand.
pub fn dispatch(cmd: EventCmd) {
    match cmd {
        EventCmd::BaseCandidates { no_fetch } => event::base_candidates::run(no_fetch),
        EventCmd::EmitEvent {
            event,
            payload,
            spec,
            wave,
        } => event::emit_event::run(event.as_deref(), &payload, spec.as_deref(), wave),
        EventCmd::EmitPhase { spec, to, from } => {
            event::emit_phase::run(&spec, &to, from.as_deref());
        }
        EventCmd::EmitPipeline { kind, spec, payload, allow_no_qa, intent, work_kind, base } => {
            event::emit_pipeline::run(event::emit_pipeline::EmitPipelineOpts {
                kind,
                spec,
                payload,
                allow_no_qa,
                intent,
                base,
                work_kind,
            });
        }
        EventCmd::EventProjections {
            view,
            spec,
            wave,
            format,
        } => event::event_projections::run(view.as_deref(), spec.as_deref(), wave, &format),
        EventCmd::Notebook { add, unit, root } => {
            event::notebook::run(&root, unit.as_deref(), add.as_deref());
        }
    }
}
