//! The `run` subcommands for the harness event stream (`event/`).
//!
//! FOUR registrations per command. Two live in this file: the variant in
//! [`EventCmd`] AND its arm in [`dispatch`] below; forgetting the arm still
//! compiles, but the command vanishes from the CLI. The other two live in
//! the tests: the name in `tests/run_command_surface.rs`, and a caller (or a
//! justified `RUNTIME_WHITELIST` line) in `tests/template_parity.rs`.
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
    #[command(display_order = 71)]
    BaseCandidates {
        /// Skip the `git fetch` and list what the clone already knows. The
        /// default refreshes: the whole point is a menu that is true TODAY.
        #[arg(long)]
        no_fetch: bool,
    },
    /// Emit an arbitrary named harness event with a key/value payload.
    #[command(display_order = 5)]
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
    #[command(display_order = 6)]
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
    /// exit 2 unless every acceptance criterion in the spec's `spec.ndjson` has
    /// a passing last run (`qa-run` records each run there), or `--allow-no-qa`
    /// is passed (escape hatch for trusted callers or an explicit user
    /// override).
    #[command(display_order = 7)]
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
        /// `pipeline.complete` is refused (exit 2) unless every criterion in the
        /// spec's `spec.ndjson` has a passing last run.
        #[arg(long = "allow-no-qa")]
        allow_no_qa: bool,
        /// Free-form natural-language request. On `--kind pipeline.kind` it
        /// MINTS the unit's canonical name: one slug for the `{kind}/{slug}`
        /// branch, the events and the spec directory (hand it to `spec-draft
        /// --slug`). It supersedes a disagreeing `--spec`, and the report says
        /// so via `renamedFrom`.
        #[arg(long)]
        intent: Option<String>,
        /// The name the OPERATOR chose for this unit. On `--kind pipeline.kind`
        /// it OUTRANKS the name derived from `--intent`: the derivation is a
        /// suggestion, and the person who read it and corrected it on purpose
        /// decides. Canonicalised by that same derivation (spaces, accents and
        /// slashes collapse into the one slug spelling), so the unit still
        /// carries a single name; the report echoes `nameFrom`. Distinct from
        /// `--spec`, which stays a caller's guess and still loses.
        #[arg(long = "unit-name")]
        unit_name: Option<String>,
        /// What the unit IS — an open label (`feature`, `fix`, `hotfix`,
        /// `chore`, …). On `--kind pipeline.kind` it names the auto-branch
        /// (`{kind}/{slug}`). Omitted → NEVER a silent default: it is derived
        /// from the routing `kind` in `--payload` (`bugfix`/`tactical-fix` →
        /// `fix`, `feature`/`task` → `feature`) only where the base is the
        /// ordinary work base — the one place a hotfix is illegal by
        /// definition — and the report echoes `type` + `typeFrom`; anywhere
        /// else, or with no routing kind, the call is refused asking for this
        /// flag. Fix-vs-hotfix is never inferred from the request: a fix that
        /// waits for the next release and one that goes to production are the
        /// same change.
        #[arg(long = "type")]
        work_kind: Option<String>,
        /// Base branch the work branch is cut from — the OPERATOR's own
        /// answer, taken against the branches this repository REALLY has. When
        /// set, it MUST name one of them (a name no branch carries → error, and
        /// the error LISTS what is there); it is never refused for missing from
        /// `git.flow`, which refuses nothing. When omitted, the project's
        /// primary base. The `--type` does not decide it, in either direction.
        #[arg(long)]
        base: Option<String>,
        /// The pending item (`P-{n}`) this unit delivers. On `--kind
        /// pipeline.kind` it must name an OPEN item of the pending ledger (any
        /// other id → error, exit 1, before any emit); the id is recorded in the
        /// unit's event, and merging the unit's pull request closes that item.
        /// Ignored for every other kind.
        #[arg(long, value_name = "ID")]
        pending: Option<String>,
    },
    /// Query the harness event log by view.
    #[command(display_order = 27)]
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
    #[command(display_order = 68)]
    Notebook {
        /// The item to record — one note, stored as one line. Omitted: the
        /// notebook is read, not written.
        #[arg(long)]
        add: Option<String>,
        /// The work branch whose notebook this is, e.g. `dev_my-unit`.
        /// Omitted: the branch the checkout is standing on.
        #[arg(long)]
        unit: Option<String>,
        /// The unit's SLUG, naming the notebook directly — the same name every
        /// other spec command takes. Wins over `--unit` and over the current
        /// branch, so a call from the wrong directory cannot pick another unit.
        #[arg(long)]
        spec: Option<String>,
        /// This item EXPLAINS THE SYMPTOM the operator reported — it is not an
        /// adjacent finding.
        ///
        /// The difference is not a matter of degree. An adjacent finding is the
        /// next cycle's prompt; a finding that explains the reported symptom IS
        /// the answer to the request in flight, and recording it quietly means
        /// deciding, on the operator's behalf, to keep working on something
        /// else. That decision is theirs.
        ///
        /// Marked here, the close gate refuses to close the unit until the item
        /// is settled — the same way it already refuses an unmarked checklist
        /// item.
        #[arg(long = "explains-symptom")]
        explains_symptom: bool,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// The PENDING ledger: work agreed in the conversation that has not closed
    /// yet. It lives OUTSIDE every unit, in `.claude/pending/ledger.json` of the
    /// main checkout, so it is recorded with no unit open, survives a branch
    /// switch and outlives the unit that delivers it. Without a flag it LISTS
    /// `{ok, open, closed, count_line}`; `--add` records one item and prints
    /// its `P-{n}` id; `--close` settles one as delivered, always with a
    /// non-blank `--reason`. A removal (`--remove`, `--drop`) takes two calls:
    /// the first shows what would leave and prints a code, the second passes
    /// that code in `--confirm` after the user's yes. `--stale` shows the idle
    /// items once, and `--expire --keep` drops the ones the user did not keep.
    #[command(display_order = 75)]
    Pending {
        /// Record a new item (needs `--title` and `--detail`).
        #[arg(long, conflicts_with_all = ["close", "drop"])]
        add: bool,
        /// What was agreed, one line.
        #[arg(long)]
        title: Option<String>,
        /// Its scope or reason, one line.
        #[arg(long)]
        detail: Option<String>,
        /// Settle the item `P-{n}` as DELIVERED.
        #[arg(long, value_name = "ID", conflicts_with = "drop")]
        close: Option<String>,
        /// Drop the item `P-{n}` ON PURPOSE: the same removal as `--remove
        /// --id P-{n}`, shown first and confirmed with `--confirm`.
        #[arg(long, value_name = "ID", group = "removal")]
        drop: Option<String>,
        /// Why the item leaves the list. Required by `--close`, `--drop` and
        /// `--remove`; a blank one is refused and nothing is written.
        #[arg(long)]
        reason: Option<String>,
        /// Take items out, with ONE selector (`--id`, `--term` or `--before`)
        /// and a `--reason`. Without `--confirm` it only shows what would
        /// leave and prints the code to confirm with.
        #[arg(long, group = "removal")]
        remove: bool,
        /// Removal selector: the items `P-{n}`, comma-separated.
        #[arg(long, value_name = "IDS", requires = "remove")]
        id: Option<String>,
        /// Removal selector: words searched in the title and the detail.
        #[arg(long, requires = "remove")]
        term: Option<String>,
        /// Removal selector: the items recorded before this day, `YYYY-MM-DD`.
        #[arg(long, value_name = "DAY", requires = "remove")]
        before: Option<String>,
        /// The code a removal preview printed: removes exactly that set, and
        /// nothing when the list changed since.
        #[arg(long, value_name = "CODE", requires = "removal")]
        confirm: Option<String>,
        /// Bring a dropped item `P-{n}` back to open.
        #[arg(long, value_name = "ID")]
        reopen: Option<String>,
        /// Show, once, the open items idle for 30 days or more, with the one
        /// question to ask the user.
        #[arg(long)]
        stale: bool,
        /// Drop as expired the idle items the last `--stale` showed, all but
        /// the `--keep` ones.
        #[arg(long)]
        expire: bool,
        /// The idle items that stay, comma-separated.
        #[arg(long, value_name = "IDS", requires = "expire")]
        keep: Option<String>,
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
        EventCmd::EmitPipeline {
            kind,
            spec,
            payload,
            allow_no_qa,
            intent,
            unit_name,
            work_kind,
            base,
            pending,
        } => {
            // Pela linha de comando, os tipos que criam ou avançam uma spec
            // recusam; as portas de dentro chamam o `run` direto.
            event::emit_pipeline::refuse_spec_door(&kind);
            event::emit_pipeline::run(event::emit_pipeline::EmitPipelineOpts {
                kind,
                spec,
                payload,
                allow_no_qa,
                intent,
                unit_name,
                base,
                work_kind,
                pending,
            });
        }
        EventCmd::EventProjections {
            view,
            spec,
            wave,
            format,
        } => event::event_projections::run(view.as_deref(), spec.as_deref(), wave, &format),
        EventCmd::Notebook { add, unit, spec, explains_symptom, root } => {
            event::notebook::run(
                &root,
                unit.as_deref(),
                spec.as_deref(),
                add.as_deref(),
                explains_symptom,
            );
        }
        EventCmd::Pending {
            add,
            title,
            detail,
            close,
            drop,
            reason,
            remove,
            id,
            term,
            before,
            confirm,
            reopen,
            stale,
            expire,
            keep,
            root,
        } => {
            event::pending::run(&event::pending::PendingOpts {
                root,
                add,
                title,
                detail,
                close,
                drop,
                reason,
                now: None,
                remove,
                id,
                term,
                before,
                confirm,
                reopen,
                stale,
                expire,
                keep,
            });
        }
    }
}
