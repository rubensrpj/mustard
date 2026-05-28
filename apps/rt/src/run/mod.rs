//! The `run` face of `mustard-rt` — the b4 script port.
//!
//! `mustard-rt on` / `mustard-rt check` are the enforcement faces: they read
//! the harness JSON from stdin and run hook modules. The `run` face is
//! different — it ports the utility *scripts* that used to live under
//! `templates/scripts/` as standalone `bun` programs. A `run` subcommand takes
//! its inputs as `clap` arguments (a directory, flags), never from stdin, and
//! prints its result to stdout exactly as the JS script did.
//!
//! Each ported script is its own submodule. Wave 1 ports `sync-detect`
//! (subproject discovery + SHA-256 change detection) and the scanner subsystem
//! it shares with the still-JS `sync-registry.js`.

pub mod scan;
pub mod active_specs;
pub mod adapt_cursor;
pub mod agent_prompt_render;
pub mod amend_finalize;
mod analyze_validation;
pub mod backup_specs;
pub mod blob_spill;
pub mod bugfix_cache;
pub mod claude_dir_prune;
pub mod refresh_claude;
pub mod close_orchestrate;
pub mod context_budget;
pub mod economy_capture_baseline;
pub mod economy_reconcile;
pub mod economy_report;
pub mod event_route;
pub mod event_writer_ndjson;
pub mod i18n_translate;
pub mod maint_deps;
pub mod maint_validate;
pub mod pipeline_prelude;
pub mod prd_build;
pub mod review_dispatch;
pub mod skill_cache;
pub mod skill_fetch;
pub mod spec_lang_resolve;
pub mod spec_clear;
pub mod tactical_fix_create;
pub mod task_checklist;
mod artifact_update;
mod knowledge;
mod doctor;
// W3 of `2026-05-26-claude-paths-single-source` — three typed doctor checks
// (claude-paths, workspace-leaks, i1) that emit native JSON shapes. They are
// dispatched by `doctor.rs` but live in dedicated modules so the legacy
// `CheckResult` envelope stays out of their way.
pub mod doctor_claude_paths;
pub mod doctor_workspace_leaks;
pub mod doctor_i1;
pub mod plan_from_spec;
mod complete_spec;
mod context_slice;
mod dependency_precheck;
mod diff_context;
mod docs_stale_check;
mod emit_event;
mod graph_dead;
mod graph_index;
pub mod emit_phase;
mod emit_pipeline;
pub mod env;
mod epic_fold;
pub mod event_projections;
pub use event_projections::{pipeline_state_from_events, PipelineStateView};
// Spec A v4 / W4 — behavior-regression gate connecting W1 (vocabulary),
// W1.5 (AST agnostic) and W2 (snapshot) primitives.
pub mod gate_regression_check;
pub use env::current_spec;
mod exec_rewave_check;
mod mark_checklist_item;
pub(crate) mod memory;
mod memory_cross_wave;
mod migrate_spec_headers;
mod migrate_to_meta;
mod memory_ingest;
mod rehook;
mod metrics;
mod metrics_wave_status;
pub(crate) mod otel;
mod pipeline_state_ingest;
mod pipeline_summary;
mod qa_run;
mod qa_run_all;
mod rebuild_specs;
mod recipe_match;
pub mod resume_bootstrap;
mod review_prefetch;
mod review_result;
mod rtk_gain;
mod status;
mod scan_finalize;
mod scan_md_validate;
mod scan_orchestrate;
mod scan_precompute;
mod scan_recipes_validate;
mod scan_structural;
mod scope_decompose;
mod security_scan;
pub mod skill_discovery_lint;
mod skills;
mod statusline;
mod verify_emit;
mod spec_children;
mod spec_children_tree;
mod spec_extract;
mod spec_link;
mod spec_sections;
// W4: lang-aware spec slug helper. Thin facade over `mustard_core::slugify`.
// W6: subcommand entry point (`i18n translate-heading`, `spec-lang resolve`).
pub mod spec_slug;
mod spec_draft;
pub mod spec_scaffold;
pub mod spec_status_backfill;
mod spec_memory;
mod spec_validate;
pub(crate) mod skill_resolve;
mod sync_detect;
mod sync_registry;
pub mod unhook;
mod transcript_watcher;
mod verify_pipeline;
pub mod wave_context;
mod wave_dependency;
mod wave_files;
mod wave_lib;
mod wave_scaffold;
mod wave_size_check;
pub mod wave_summary;
mod wave_tree;
pub mod worktree_gc;

use clap::Subcommand;
use std::path::PathBuf;

/// The `run` subcommands — one variant per ported script.
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum — clap-Subcommand; boxing breaks derive
pub enum RunCmd {
    /// Discover subprojects, detect roles, and emit the `sync-detect` JSON.
    SyncDetect {
        /// The monorepo root to scan. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Scan entities, clusters and conventions; write `entity-registry.json` v4.0.
    SyncRegistry {
        /// The monorepo root to scan. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Regenerate even when the registry is already populated.
        #[arg(long)]
        force: bool,
    },
    /// Emit a compact git diff summary for agent context.
    DiffContext {
        /// Branch to compare against (auto-detects `main`/`master`).
        #[arg(long)]
        parent: Option<String>,
        /// Scope the diff to a path.
        #[arg(long)]
        subproject: Option<String>,
        /// Pipeline phase — `analyze` is a silent no-op.
        #[arg(long)]
        phase: Option<String>,
    },
    /// Emit an arbitrary named harness event with a key/value payload.
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
    },
    /// Rewrite legacy spec headers (`### Status:` + `### Phase:`) into the
    /// canonical `### Stage:` / `### Outcome:` / `### Flags:` triple
    /// (spec-lifecycle-unification Wave 7). Dry-run by default; `--apply`
    /// (mutually exclusive with `--dry-run`) writes atomically per file. The
    /// audit log is written in both modes.
    MigrateSpecHeaders {
        /// Preview only — write the audit log, touch no spec files (default).
        #[arg(long, default_value_t = true, conflicts_with = "apply")]
        dry_run: bool,
        /// Apply the rewrite (atomic per file). Required to mutate spec files.
        #[arg(long)]
        apply: bool,
        /// Root directory to scan recursively. Defaults to `.claude/spec`.
        #[arg(long, default_value = ".claude/spec")]
        root: PathBuf,
        /// Audit-log path. Defaults to
        /// `.claude/.harness/migration-{date}.log.json`.
        #[arg(long)]
        log: Option<PathBuf>,
        /// Case-insensitive substring filter on the file path (subset).
        #[arg(long)]
        filter: Option<String>,
    },
    /// Extract lifecycle headers from every `.md` under `<root>` into a
    /// sidecar `meta.json` (Wave 3 of mustard-unification). Atomic per file,
    /// idempotent (skips when sidecar already present unless `--force`).
    ///
    /// The headers stay in the `.md` for this step — the second-pass clean-up
    /// removes them once every consumer reads from `meta.json`.
    MigrateToMeta {
        /// Root directory to walk recursively. Defaults to `.claude/spec`.
        #[arg(long)]
        root: Option<PathBuf>,
        /// Force-rewrite existing `meta.json` sidecars.
        #[arg(long)]
        force: bool,
        /// After writing each `meta.json`, also strip the legacy `### Stage:` /
        /// `### Outcome:` / `### Phase:` / `### Scope:` / `### Lang:` /
        /// `### Checkpoint:` / `### Parent:` / `### Flags:` /
        /// `### Total waves:` / `### Status:` headers from the `.md`. Idempotent.
        #[arg(long = "strip-headers")]
        strip_headers: bool,
    },
    /// Finalize a pipeline spec (followup mark, archive, or stale sweep).
    CompleteSpec {
        /// Spec name (required unless `--archive-stale`/`--archive-followups`).
        spec: Option<String>,
        /// Finalize archival: move the spec to `completed/` and drop state.
        #[arg(long)]
        archive: bool,
        /// Archive every `closed-followup` state older than 24 h.
        #[arg(long = "archive-stale")]
        archive_stale: bool,
        /// Archive every `closed-followup` state regardless of age.
        #[arg(long = "archive-followups")]
        archive_followups: bool,
    },
    /// Cut the relevant term blocks from one or more `CONTEXT.md` glossaries.
    ///
    /// W8.T8.8 also accepts `--context-claude-md <path>`: a CLAUDE.md file
    /// whose `## Heading` / `### Heading` sections are kept when their body
    /// contains any spec-derived relevance term. The CLAUDE.md slice is
    /// emitted after the CONTEXT.md slice (separated by a blank line).
    ContextSlice {
        /// A `CONTEXT.md` / `CONTEXT-MAP.md` path. Repeatable.
        #[arg(long)]
        context: Vec<String>,
        /// The spec file to match relevance against.
        #[arg(long)]
        spec: Option<String>,
        /// Override the line cap (`MUSTARD_GLOSSARY_MAX_LINES`).
        #[arg(long = "max-lines")]
        max_lines: Option<usize>,
        /// W8.T8.8 — slice the given CLAUDE.md against the same relevance
        /// terms. Optional; the CONTEXT.md path(s) remain primary.
        #[arg(long = "context-claude-md")]
        context_claude_md: Option<String>,
    },
    /// Resolve a scope into its minimum concept-node closure.
    ///
    /// Walks the Wave-3 graph (`.claude/graph/`) from the seeds derived from
    /// `{entities, operation, layer, role, seeds}`, dedup'ing by id, sorted
    /// by distance, truncated by the role's prompt budget, and dereferenced
    /// to in-line content. Emits byte-stable JSON. Fail-open: a missing
    /// graph or an empty scope degrades to an empty closure.
    ContextResolve {
        /// Scope JSON literal (e.g. `{"entities":["user"],"role":"explore"}`).
        #[arg(long)]
        scope: Option<String>,
        /// Read the scope JSON from a file instead of `--scope`.
        #[arg(long = "scope-file")]
        scope_file: Option<PathBuf>,
    },
    /// Persist agent memory, decisions/lessons, or knowledge entries.
    /// `cross-wave` is the read-side: emits markdown summarising prior waves.
    /// `list` emits all memory entries (knowledge_patterns + decisions + lessons).
    ///
    /// W7 (deep-refactor) adds three subcommands sharing this clap variant:
    /// `write` (`agent_memory` insert + `--verify` round-trip), `search`
    /// (FTS5 + scope filter on `agent_memory`), and `feedback`
    /// (`memory_feedback` append for `deprecate|bump|supersede|use`).
    /// `cross-wave` gains `--cluster <C>` to scope to a single role across
    /// prior waves.
    Memory {
        /// Subcommand: `agent`, `decision`, `knowledge`, `list`, `cross-wave`,
        /// `write`, `search`, or `feedback`.
        subcommand: String,
        /// Input JSON (Windows-friendly form; stdin is the POSIX fallback).
        #[arg(long)]
        json: Option<String>,
        /// `agent` / `cross-wave` / `write` / `search` — spec name.
        #[arg(long)]
        spec: Option<String>,
        /// `agent` / `cross-wave` / `write` — wave number (1-based).
        #[arg(long)]
        wave: Option<u32>,
        /// `agent` only — agent identifier/role (becomes `agent_type`).
        #[arg(long)]
        agent: Option<String>,
        /// `agent` / `write` — one-line summary of what the agent produced.
        #[arg(long)]
        summary: Option<String>,
        /// `agent` only — comma-separated list of files affected
        /// (recorded under `details.files`).
        #[arg(long)]
        files: Option<String>,
        /// `list` only — group entries by type (pattern / decision / convention).
        #[arg(long)]
        grouped: bool,
        /// `list` only — output format: `json` (default) or `table`.
        #[arg(long, default_value = "json")]
        format: String,
        /// `cross-wave` / `search` — scope to a single cluster (role suffix
        /// of `wave-N-<role>` for cross-wave; `agent_memory.role` for search).
        #[arg(long)]
        cluster: Option<String>,
        /// `search` only — FTS5 query string.
        #[arg(long)]
        query: Option<String>,
        /// `feedback` only — target memory file path
        /// (e.g. `.claude/memory/decisions/2026-05-26-foo.md`).
        ///
        /// wave-18-rt-followups (W4#3): was `--id <i64>` while memory lived
        /// in SQLite (`agent_memory.id`). After the W4B migration the memory
        /// store is a flat `MarkdownStore`, so the addressable unit is the
        /// file path; the integer id is meaningless. The dispatcher now
        /// forwards this into `FeedbackOpts.path`.
        #[arg(long)]
        path: Option<PathBuf>,
        /// `feedback` only — one of `deprecate|bump|supersede|use`.
        #[arg(long)]
        kind: Option<String>,
        /// `write` only — role label (e.g. `rt`, `dashboard`).
        #[arg(long)]
        role: Option<String>,
        /// `write` only — body text (free-form, may contain JSON).
        #[arg(long)]
        details: Option<String>,
        /// `write` only — initial confidence (0.0–1.0, default 0.5).
        #[arg(long)]
        confidence: Option<f64>,
        /// `write` only — round-trip the row after insert to confirm the
        /// schema + FTS5 mirror are healthy.
        #[arg(long)]
        verify: bool,
        /// `search` only — include rows whose effective confidence (after
        /// lazy decay) sits below the default 0.3 threshold.
        #[arg(long = "include-low")]
        include_low: bool,
        /// `search` only — result cap (default 20).
        #[arg(long)]
        limit: Option<usize>,
        /// `feedback` only — attribution token for the agent supplying the signal.
        #[arg(long = "by-role")]
        by_role: Option<String>,
        /// `feedback` only — free-form note recorded alongside the signal.
        #[arg(long)]
        note: Option<String>,
    },
    /// One-shot ingest of legacy JSON files into the SQLite Wave 6a tables.
    ///
    /// Default: reads `.claude/knowledge.json`, `.claude/memory/decisions.json`,
    /// and `.claude/memory/lessons.json` (if present) and inserts their
    /// entries into `knowledge_patterns`, `memory_decisions`, `memory_lessons`.
    ///
    /// `--agent-memory` (W7 deep-refactor): walks `.claude/.agent-memory/`
    /// (legacy rolling-cap-20 JSON sink) and forwards each entry into
    /// `agent_memory`, then removes the directory on success. Fail-open per
    /// entry.
    ///
    /// Prints a JSON summary. Fail-open per file.
    MemoryIngest {
        /// Remove the source JSON files after a successful ingest.
        #[arg(long)]
        delete: bool,
        /// Migrate `.claude/.agent-memory/` to the `agent_memory` SQLite
        /// table and remove the directory.
        #[arg(long = "agent-memory")]
        agent_memory: bool,
    },
    /// One-shot ingest of `.pipeline-states/*.json` files into the SQLite event stream.
    ///
    /// Globs `.claude/.pipeline-states/*.json` (excluding `*.metrics.json`), lenient-parses
    /// each file, and emits retroactive `pipeline.*` events into the harness event store.
    /// Preserves original `updatedAt` timestamps for correct event ordering.
    /// Fail-open per file — errors are collected into the output JSON, not propagated.
    PipelineStateIngest {
        /// Remove each successfully-ingested JSON file after ingest.
        #[arg(long)]
        delete: bool,
    },
    /// Detect or fold a completed epic.
    EpicFold {
        /// List epics whose children are all in `CLOSE`.
        #[arg(long)]
        detect: bool,
        /// Fold the named epic.
        #[arg(long)]
        epic: Option<String>,
    },
    /// Cut a single wave slice (or AC block) from a `spec.md`.
    SpecExtract {
        /// Path to the spec file.
        #[arg(long)]
        spec: String,
        /// Wave number to extract.
        #[arg(long)]
        wave: Option<u32>,
        /// Extract the `## Acceptance Criteria` section instead.
        #[arg(long)]
        ac: bool,
        /// Emit a JSON omission-measurement instead of the slice text.
        #[arg(long)]
        measure: bool,
    },
    /// Link a child spec to a parent (epic) spec.
    SpecLink {
        /// Parent (epic) spec name.
        #[arg(long)]
        parent: Option<String>,
        /// Child spec name.
        #[arg(long)]
        child: Option<String>,
        /// Why the split happened (recorded in the `spec.link` event).
        #[arg(long)]
        reason: Option<String>,
    },
    /// UNION of sub-specs linked to `--parent` via `spec.link` events AND via
    /// filesystem `### Parent:` headers. Used by the dashboard "Sub-specs"
    /// tab so sub-specs created on a teammate's machine (header present but
    /// no `spec.link` event in this developer's SQLite) still surface.
    /// Emits JSON `Vec<ChildEntry>` with a `source: event|header|both` tag
    /// per row. Fail-open: any error degrades to `[]`.
    SpecChildren {
        /// Parent (epic) spec slug whose children to enumerate.
        #[arg(long)]
        parent: Option<String>,
    },
    /// Project a parent spec's waves + acceptance criteria + sub-specs into a
    /// single JSON document. Consumed by the dashboard's `spec_children_tree`
    /// Tauri command (Wave 3 of `spec-lifecycle-unification`). Fail-open: a
    /// missing spec or store degrades to empty arrays.
    SpecChildrenTree {
        /// Parent spec slug under `.claude/spec/` (flat layout).
        #[arg(long)]
        spec: Option<String>,
    },
    /// Validate a spec's structure (WARN-level — never blocks).
    AnalyzeValidation {
        /// Path to the spec file.
        #[arg(long)]
        spec: Option<String>,
    },
    /// Mark a `## Checklist` item done in a spec.
    MarkChecklistItem {
        /// Spec name or absolute `spec.md` path.
        #[arg(long)]
        spec: Option<String>,
        /// Substring of the checklist item text to match.
        #[arg(long)]
        item: Option<String>,
        /// 1-based line number of the checkbox (alternative to `--item`).
        #[arg(long)]
        line: Option<usize>,
        /// Project root override.
        #[arg(long)]
        cwd: Option<String>,
    },
    /// Render a spec's wave structure as an ASCII or JSON tree.
    WaveTree {
        /// Path to the spec directory.
        #[arg(long = "spec-dir")]
        spec_dir: String,
        /// Output format: `ascii` (default) or `json`.
        #[arg(long, default_value = "ascii")]
        format: String,
    },
    /// Analyze file dependencies across waves (reads JSON from stdin).
    WaveDependency,
    /// Return the declared-files count and full markdown body of a wave's
    /// sub-spec (`.claude/spec/{spec}/wave-{wave}-*/spec.md`). Used by the
    /// dashboard "Ondas" tab to show the canon `## Arquivos` count and pop
    /// open a drawer with the wave markdown. Fail-open: missing files →
    /// `{"count":0,"markdown":"","path":null}`.
    WaveFiles {
        /// Parent spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: Option<String>,
        /// Wave number (1-based).
        #[arg(long)]
        wave: Option<u32>,
    },
    /// Suggest wave decomposition by file/entity count (reads JSON from stdin).
    ScopeDecompose,
    /// Check whether a spec should be decomposed at EXECUTE entry.
    ExecRewaveCheck {
        /// Path to the spec file.
        #[arg(long)]
        spec: Option<String>,
    },
    /// Pre-dispatch factual gate: greps the spec's subproject for every JSX
    /// symbol and named import it references, and reports those whose
    /// `export` is missing. Self-created paths (declared in `## Files`) are
    /// excluded. Output is single-line JSON; exit code is always 0
    /// (fail-open) — the orchestrator decides whether to block dispatch.
    DependencyPrecheck {
        /// Path to the spec file or its containing directory (resolves
        /// `<dir>/spec.md`).
        #[arg(long)]
        spec: Option<String>,
        /// Override the auto-detected subproject scan root
        /// (`apps/<name>` / `packages/<name>` common ancestor of `## Files`).
        #[arg(long)]
        subproject: Option<String>,
    },
    /// Audit per-wave file/layer counts inside a wave-plan.
    WaveSizeCheck {
        /// Path to the spec directory.
        #[arg(long = "spec-dir")]
        spec_dir: Option<String>,
    },
    /// Spec A v4 / W4 — run the behavior-regression gate at the requested moment.
    ///
    /// Reads the spec's `plan.txt` (or `spec.md` body) as the Moment-1 plan
    /// text and dispatches to `gate_regression_check::run`. Moments 2 and 3
    /// require external `diff` + snapshots that the bare CLI does not
    /// collect today — those moments are exercised via the
    /// `pre_edit_intent_check` hook and the W5 span-level integration.
    /// Exit code mirrors the verdict: Green/Amber ⇒ 0, Red ⇒ 2.
    #[command(name = "gate-regression-check")]
    GateRegressionCheck {
        /// Spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: String,
        /// Moment to evaluate: 1 (pre-edit), 2 (during diff), 3 (after child return).
        #[arg(long, default_value_t = 1)]
        moment: u8,
    },
    /// Match an entity + operation to a code recipe skeleton.
    RecipeMatch {
        /// Entity name.
        #[arg(long)]
        entity: Option<String>,
        /// Operation type.
        #[arg(long)]
        operation: Option<String>,
        /// Subproject path used for placeholder resolution.
        #[arg(long)]
        subproject: Option<String>,
    },
    /// Execute a spec's Acceptance Criteria; emit a `qa.result` event.
    QaRun {
        /// Spec name (resolved under `.claude/specs` or `.claude/spec` — flat layout).
        #[arg(long)]
        spec: String,
        /// Output format: `json` (default) or `html` (extra artifact).
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// Run QA for every active spec and aggregate the results.
    ///
    /// Iterates active specs via `SqliteSpecReader`, calls `qa-run` for each,
    /// and emits a JSON batch report `{ ran, failed, skipped, errors }`.
    /// Fail-open per spec — individual failures land in `errors[]`.
    QaRunAll,
    /// Rematerialise the denormalised `specs` + `metrics_projection` tables
    /// from the event stream. Closes the gap the eliminate-bun migration
    /// opened: pre-2026-05-20 nothing populated those tables since the JS
    /// harness writer was removed, which is why every dashboard spec card
    /// fell back to `"unknown"`.
    RebuildSpecs,
    /// Render pipeline + hook telemetry (`collect` / `report` subcommand).
    Metrics {
        /// Subcommand: `collect` or `report`.
        subcommand: Option<String>,
        /// Subcommand flags (`--hooks-only`, `--since`, `--event`).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
        /// Output format: `json` (default) or `html`.
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// Per-wave status + telemetry roll-up for a parent (epic) spec.
    ///
    /// Promoted to a top-level `RunCmd` variant so clap renders `--spec` in
    /// `--help` natively (wave-network spec AC-6). Aliased to `metrics-wave-status`;
    /// invoked from CLI as `mustard-rt run metrics wave-status --spec <parent>`
    /// via argv pre-routing in `main.rs`.
    #[command(name = "metrics-wave-status")]
    MetricsWaveStatus {
        /// Parent (epic) spec name under `.claude/spec/` (flat layout).
        #[arg(long)]
        spec: Option<String>,
    },
    /// Query the harness event log by view.
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
    /// Run build/test verification for the active pipeline's subprojects.
    VerifyPipeline {
        /// Output format: `json` (default) or `html`.
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// Render a CLOSE-phase Done/Left/Next-Steps summary for a spec.
    ///
    /// With `--self-test`: instantiate a minimal [`mustard_core::SpecSummaryDoc`],
    /// serialise it to pretty JSON, print to stdout, and exit 0. Used by
    /// `cargo run -p mustard-rt -- run pipeline-summary --self-test` in AC-1A-1.
    PipelineSummary {
        /// Path to the spec directory (must contain `spec.md`).
        #[arg(long = "spec-dir")]
        spec_dir: Option<String>,
        /// Output format: `markdown` (default) or `json`.
        #[arg(long, default_value = "markdown")]
        format: String,
        /// Self-test mode: serialise a minimal SpecSummaryDoc and exit 0.
        #[arg(long = "self-test")]
        self_test: bool,
    },
    /// Record a REVIEW-phase verdict (emits a `review.result` event + metric).
    ReviewResult {
        /// Spec name.
        #[arg(long)]
        spec: Option<String>,
        /// Verdict: `approved` or `rejected`.
        #[arg(long)]
        verdict: Option<String>,
        /// Count of critical findings.
        #[arg(long, default_value_t = 0)]
        critical: i64,
        /// Subproject the review targeted.
        #[arg(long)]
        subproject: Option<String>,
    },
    /// Render the Claude Code status bar (reads the payload JSON from stdin),
    /// or `--preview` every shipped theme on its own labelled line.
    Statusline {
        /// Skip stdin; render every theme with a synthetic payload.
        #[arg(long)]
        preview: bool,
    },
    /// Skill-family CLI: `validate`, `graph`, or `orphans`.
    Skills {
        /// Subcommand: `validate`, `graph`, or `orphans`.
        subcommand: Option<String>,
        /// Subcommand flags (`--json`, `--factual`, `--days`, …).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Scan a project tree for committed secrets + misconfigurations.
    SecurityScan {
        /// Directory to scan. Defaults to the current directory.
        dir: Option<String>,
        /// Emit the machine-readable JSON report.
        #[arg(long)]
        json: bool,
    },
    /// Confirm a named harness event landed within a recent window.
    VerifyEmit {
        /// Event name to match (required).
        #[arg(long)]
        event: Option<String>,
        /// Look-back window, e.g. `30s`, `1m`, `500ms` (alias `--within`).
        #[arg(long, alias = "within")]
        since: Option<String>,
        /// Also require `payload[key]` to exist.
        #[arg(long = "payload-key")]
        payload_key: Option<String>,
        /// With `--payload-key`, require equality.
        #[arg(long = "payload-value")]
        payload_value: Option<String>,
        /// Also filter by the `spec` field.
        #[arg(long)]
        spec: Option<String>,
        /// Suppress stdout on success.
        #[arg(long)]
        quiet: bool,
    },
    /// Normalise `rtk gain` analytics into the Mustard JSON shape.
    RtkGain,
    /// Pre-dispatch orchestration for `/scan` — emits the dispatch plan JSON.
    ScanOrchestrate {
        /// Single subproject to scan (optional positional).
        target: Option<String>,
        /// Full re-scan: ignore the change-detection cache.
        #[arg(long)]
        force: bool,
    },
    /// Post-dispatch finalization for `/scan` — registry + skills + security.
    ScanFinalize {
        /// Skip the security scan step.
        #[arg(long = "skip-security")]
        skip_security: bool,
    },
    /// Agnostic Rust-only structural scan of one subproject (or every detected
    /// subproject when `--subproject` is omitted). Parses manifests, counts
    /// source extensions, and runs the agnostic cluster discovery; writes a
    /// `stack.md` ≤60 lines under `<sub>/.claude/commands/` and prints a JSON
    /// digest to stdout. Fail-open per parser.
    ScanStructural {
        /// Subproject path relative to the repo root (e.g. `apps/cli`).
        /// Defaults to scanning the repo root + every `apps/*` + `packages/*`.
        #[arg(long)]
        subproject: Option<String>,
    },
    /// Validate `.md` artifacts generated by `/scan` against the W3 contract:
    /// size caps per kind, `<!-- mustard:generated -->` fence presence under
    /// `commands/` and `skills/`, wirelink resolution against
    /// `.claude/graph/index.md`, `Ref:` path existence, and cross-file
    /// paragraph duplication. Fail-open unless `--strict` is set.
    ScanMdValidate {
        /// Limit the scan to one subproject (path relative to repo root).
        #[arg(long)]
        from: Option<String>,
        /// Exit `1` when any hit is found. Default is warn-only (exit `0`).
        #[arg(long)]
        strict: bool,
    },
    /// Validate `.claude/recipes/<sub>/*.json` shape: required keys, real
    /// `files[].path` existence inside the recipe's subproject, and absence of
    /// literal `{Entity}` / `{ClusterLabel}` placeholders. Fail-open unless
    /// `--strict` is set.
    ScanRecipesValidate {
        /// Exit `1` when any hit is found. Default is warn-only (exit `0`).
        #[arg(long)]
        strict: bool,
    },
    /// Run the local OTLP/JSON receiver for Claude Code native telemetry.
    ///
    /// Binds a loopback HTTP server on `MUSTARD_OTEL_PORT` (default 4318).
    /// Metrics/logs project into `claude_code_otel` (mustard.db); traces land
    /// span-level token usage as `run_usage` rows in telemetry.db via the
    /// telemetry writer (rows stamped with attribution at write time). Runs
    /// until a shutdown signal — the harness spawns it as a long-lived child
    /// via [`crate::hooks::session_start`].
    OtelCollector,
    /// Watch `~/.claude/projects/**/*.jsonl` and re-ingest each session
    /// transcript into telemetry.db's `run_usage` table on every change.
    ///
    /// Opt-in daemon (Wave 3 — economia-moat-unification) spawned by
    /// [`crate::hooks::session_start`] when `MUSTARD_TRANSCRIPT_WATCH=1`.
    /// Runs until process termination. With `--once`, performs a single
    /// backfill sweep of the current cwd's transcript directory and exits.
    TranscriptWatcher {
        /// Backfill mode: ingest every transcript currently in
        /// `~/.claude/projects/<encoded(cwd)>/` once, then exit. Default `false`
        /// (long-lived daemon).
        #[arg(long)]
        once: bool,
    },
    /// End-to-end health check of the Mustard ↔ Claude Code OTEL pipeline.
    DiagnoseOtel {
        /// Emit the machine-readable JSON report.
        #[arg(long)]
        json: bool,
        /// Wait `Xs`/`Xms`, then assert the row count grew (exit 1 on fail).
        #[arg(long = "expect-rows-after")]
        expect_rows_after: Option<String>,
    },
    /// Read-only installation health diagnostic: wiring, drift, state health,
    /// wave-integrity, claude-paths, workspace-leaks, i1, and (optionally)
    /// residue. Prints a compact OK/WARN/FAIL report and exits 1 if any
    /// category is FAIL, 0 otherwise.
    ///
    /// Pass `--json` as a shortcut for `--format json` (W10.T10.6).
    Doctor {
        /// Also scan for dead file/script references (slower).
        #[arg(long)]
        residue: bool,
        /// Run a specific named check in isolation: `skill-discovery`,
        /// `wave-integrity`, `claude-paths` (W3.T3.4), `workspace-leaks`
        /// (W3.T3.8), or `i1` (W3.T3.9).
        #[arg(long)]
        check: Option<String>,
        /// Output format: `text` (default) or `json`.
        #[arg(long, default_value = "text")]
        format: String,
        /// Shorthand for `--format json` (W10.T10.6).
        #[arg(long)]
        json: bool,
    },
    /// Finalize open amendment windows for a session (appends `## Amendments` to spec.md,
    /// moves archived specs, updates the DB, and emits `pipeline.amend_close`).
    AmendFinalize {
        /// Session identifier whose open windows to finalize.
        #[arg(long = "session-id")]
        session_id: String,
    },
    /// Build the concept-node graph index from `.claude/graph/`.
    ///
    /// Walks every markdown file under `.claude/graph/`, parses its frontmatter
    /// `id` + inline `[[id]]` edges, constructs the `id → path` lookup table +
    /// adjacency map, validates (orphan / cycle → warning), writes the
    /// `index.md` MOC, and (best-effort) injects `aliases:[id]` into matching
    /// `.claude/skills/*/SKILL.md` files. Emits byte-stable pretty JSON.
    /// Fail-open: a missing graph directory degrades to an empty index.
    GraphIndex,
    /// List concept-nodes with zero spec backlinks (deletion candidates).
    ///
    /// Walks `<project>/.claude/spec/**/spec.md`, parses each auto-managed
    /// `## Backlinks` block, and returns the set of ids in `.claude/graph/`
    /// that no spec links to. Emits byte-stable pretty JSON
    /// (`{ "dead": [...], "count": <usize> }`). Fail-open: a missing graph
    /// or spec tree degrades to `{ "dead": [], "count": 0 }`.
    GraphDead,
    /// Materialise the canonical SDD wave layout (wave-plan + wave-N/spec.md
    /// + review/spec.md + qa/spec.md) from a declarative JSON plan. Idempotent.
    WaveScaffold {
        /// Target spec directory.
        #[arg(long = "spec-dir")]
        spec_dir: Option<String>,
        /// Path to the plan JSON file.
        #[arg(long)]
        plan: Option<String>,
    },
    /// W10.T10.4 — Emit a deterministic wave-plan JSON consumable by
    /// `wave-scaffold`. Replaces the orchestrator-hand-rolled `plan.json` step.
    #[command(name = "plan-from-spec")]
    PlanFromSpec {
        /// Total wave count (>= 1).
        #[arg(long, default_value_t = 1)]
        waves: u32,
        /// Comma-separated role list (replicates the last role when waves > len).
        #[arg(long, default_value = "mixed")]
        roles: String,
        /// BCP-47 narrative locale (`pt-BR` / `en-US`).
        #[arg(long, default_value = "pt-BR")]
        lang: String,
        /// Optional summary applied to every wave.
        #[arg(long)]
        summary: Option<String>,
    },
    /// Scan markdown docs for obsolete terms declared in `.claude/.docs-audit.json`.
    ///
    /// Emits a JSON report of stale-doc hits. With `--strict` (or env
    /// `MUSTARD_DOCS_AUDIT_MODE=strict` set by the caller), exits `1` when any
    /// hit is found — the close gate uses this to block CLOSE on narrative
    /// drift after an architectural spec lands.
    DocsStaleCheck {
        /// Limit the audit to a single spec (`from_spec` field). Defaults to
        /// running every audit declared in the registry.
        #[arg(long)]
        from: Option<String>,
        /// Exit `1` when any hit is found. Default is warn-only (exit `0`).
        #[arg(long)]
        strict: bool,
        /// Also recurse into nested `apps/*/.claude/**` installed-payload copies.
        /// Default `false` — the audit scans only source-of-truth docs (the
        /// repo-root `.claude/` tree and each subproject's root `CLAUDE.md`).
        /// Equivalent to `MUSTARD_DOCS_AUDIT_INCLUDE_NESTED=1`.
        #[arg(long)]
        include_nested: bool,
    },
    /// Check (or apply) freshness of managed artifacts against their upstreams.
    ///
    /// Maintainer-side: reads `apps/cli/templates/.artifacts.json` and probes
    /// each external upstream. Fail-open — network errors degrade an artifact
    /// to `unknown` and never fail the command.
    ArtifactUpdate {
        /// Probe upstreams and emit the JSON freshness report (the default).
        #[arg(long)]
        check: bool,
        /// Pull updates into vendored trees / bump pinned versions.
        #[arg(long)]
        apply: bool,
        /// Manifest path (default `apps/cli/templates/.artifacts.json`).
        #[arg(long)]
        manifest: Option<String>,
    },
    /// Discover active specs from the filesystem (Outcome=Active, Stage=Plan|Execute).
    ///
    /// Replaces the LLM-side glob/grep loop in `/mustard:spec`: reads
    /// `.claude/spec/*/spec.md` directly, filters headers, counts wave
    /// progress, extracts a one-line resumo.
    /// Output is either a markdown table (default) or a JSON document.
    ActiveSpecs {
        /// Output format: `table` (default) or `json`.
        #[arg(long, default_value = "table")]
        format: String,
        /// Project root directory (default: current working directory).
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Project + harness status snapshot.
    ///
    /// Default mode: git branch, modified files, active vs orphaned pipelines,
    /// last build result, entity-registry summary.
    ///
    /// `--harness` mode: reads `.claude/settings.json`, groups hooks by lifecycle
    /// event, resolves enforcement mode from env vars, and renders a 4-column
    /// table (Hook | Matcher | Enforces | Mode).
    Status {
        /// Include hooks table (harness view).
        #[arg(long)]
        harness: bool,
        /// Output format: `table` (default) or `json`.
        #[arg(long, default_value = "table")]
        format: String,
        /// Project root directory (default: current working directory).
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// List installed skills with name, source, and description.
    ///
    /// Globs `<root>/.claude/skills/*/SKILL.md`, parses YAML frontmatter, and
    /// renders a table or JSON array. Source defaults to `manual` when the
    /// frontmatter `source:` field is absent.
    SkillsList {
        /// Output format: `table` (default) or `json`.
        #[arg(long, default_value = "table")]
        format: String,
        /// Project root directory (default: current working directory).
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Browse entity registry and knowledge base.
    ///
    /// Subcommand `glossary`: reads `<root>/.claude/entity-registry.json`,
    /// iterates entities (skipping `_`-prefixed metadata keys), and renders
    /// name + description + first ref. Optional `--filter TERM` narrows by
    /// case-insensitive substring match on name or description.
    Knowledge {
        /// Subcommand: `glossary`.
        subcommand: Option<String>,
        /// Case-insensitive substring filter on name or description (`glossary`).
        #[arg(long)]
        filter: Option<String>,
        /// Output format: `table` (default) or `json`.
        #[arg(long, default_value = "table")]
        format: String,
        /// Project root directory (default: current working directory).
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Prefetch a GitHub Pull Request into a structured JSON document.
    ///
    /// Shell-outs to `gh pr view --json ...` and re-emits a clean structure
    /// ready for the LLM to consume. `--format table` prints a compact
    /// executive summary (title, author, scope, comments, review states).
    /// Fail-open: if `gh` is not in the PATH, emits `{"error":"gh-not-found"}`.
    ReviewPrefetch {
        /// PR reference: a number (`123`) or GitHub URL.
        pr_ref: Option<String>,
        /// Output format: `json` (default) or `table`.
        #[arg(long, default_value = "json")]
        format: String,
        /// Project root override (optional).
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Single-shot resume decision for `/mustard:spec`: mode, stage, operational
    /// spec path, wave progress, dispatch failure, refresh flags, wave model,
    /// resumo, agent roles. Emits `pipeline.resume_mode` before returning
    /// (idempotent — debounced 10 s). Fail-open: every IO error degrades a
    /// field to `null`/`false`; exit 0 always.
    ResumeBootstrap {
        /// Spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: String,
        /// Emit pretty JSON instead of the compact text table.
        #[arg(long)]
        json: bool,
    },
    /// Render the agent dispatch prompt server-side from the embedded
    /// template. Substitutes every `{placeholder}` it can resolve; warns on
    /// stderr for any left unfilled. Stdout = raw prompt string ready for
    /// the Task tool (no JSON framing).
    AgentPromptRender {
        /// Spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: String,
        /// Wave number (0-based, matching `wave-N-*` directories). Omitted for non-wave specs.
        #[arg(long)]
        wave: Option<u32>,
        /// Agent role token (e.g. `ui`, `backend`).
        #[arg(long)]
        role: String,
        /// Subproject path relative to the project root (e.g. `apps/dashboard`).
        #[arg(long)]
        subproject: PathBuf,
        /// Render mode: `first` (default), `granular`, `fix-loop`.
        #[arg(long, default_value = "first")]
        mode: String,
        /// File containing the `{retry_context}` text for granular/fix-loop.
        #[arg(long = "retry-context-file")]
        retry_context_file: Option<PathBuf>,
        /// Keep only task lines whose content matches this pattern (e.g.
        /// `"T0\\.(1|5)"`). Supports literal chars, `\\.` escape, and
        /// `(a|b)` alternation. Omit to include all tasks.
        #[arg(long = "task-filter")]
        task_filter: Option<String>,
        /// W8.T8.9 — soft token budget. When set, the renderer trims the
        /// bulky placeholders (`task_steps`, `context_md`, `prior_wave_diff`,
        /// `cross_wave_memory`, `recommended_skills`) to keep the prompt at
        /// or below this many estimated model tokens. The estimator uses the
        /// 4-chars-per-token heuristic; trimming is head-preserving.
        #[arg(long = "budget-tokens")]
        budget_tokens: Option<usize>,
    },
    /// Garbage-collect orphan Claude agent worktrees under
    /// `<repo>/.claude/worktrees/agent-*`.
    ///
    /// Enumerates the directory, computes each entry's age (via
    /// `<repo>/.git/worktrees/<name>/HEAD` mtime, falling back to the dir's
    /// own mtime), and reports/removes entries older than `--age-days N`
    /// (default 7). Dry-run by default; `--apply` is required to mutate the
    /// filesystem. Emits `worktree.gc.run` and
    /// `pipeline.economy.operation.invoked` to the harness event store.
    WorktreeGc {
        /// Repo root override. Defaults to the current working directory.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Age threshold in whole days. Worktrees older than this are
        /// eligible for removal.
        #[arg(long = "age-days", default_value_t = worktree_gc::DEFAULT_AGE_DAYS)]
        age_days: u32,
        /// Preview only — no filesystem mutation (the default).
        #[arg(long, default_value_t = true, conflicts_with = "apply")]
        dry_run: bool,
        /// Apply the removal. Required to mutate the filesystem.
        #[arg(long)]
        apply: bool,
    },
    /// Kill-switch: rename `.claude/settings.json` → `.disabled-<ts>` and wipe
    /// volatile harness state (`.agent-state/`, `.cluster-cache.json`,
    /// `.worktrees/`). Restore with [`Self::Rehook`].
    ///
    /// `--scope this` (default) acts on the current repo's `.claude/` only.
    /// `--scope monorepo` also sweeps every `apps/*/.claude/` +
    /// `packages/*/.claude/`. `--scope all` adds the user-global
    /// `~/.claude/settings.json`, gated by `--confirm` (otherwise reported as
    /// `state: "skipped"`). Emits a pretty JSON report.
    Unhook {
        /// Repo root override. Defaults to the current working directory.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Scope: `this` (default), `monorepo`, or `all`.
        #[arg(long, default_value = "this")]
        scope: String,
        /// Required for `--scope all` to also touch the user-global
        /// `~/.claude/settings.json`.
        #[arg(long)]
        confirm: bool,
    },
    /// Reverse [`Self::Unhook`]: in each `.claude/` in scope, rename the
    /// newest `settings.json.disabled*` snapshot back to `settings.json`.
    /// Volatile state directories that `unhook` wiped are left alone — the
    /// runtime regenerates them on the next run. Emits a pretty JSON report.
    Rehook {
        #[arg(long)]
        repo: Option<PathBuf>,
        #[arg(long, default_value = "this")]
        scope: String,
        #[arg(long)]
        confirm: bool,
    },
    /// Draft a new spec layout (`spec.md` + `meta.json` + optional wave plan)
    /// conforming to `mustard_core::spec::contract`. Replaces the literal
    /// ~80-line template block inside the `/mustard:feature` SKILL.md.
    ///
    /// `--scope full` materialises `wave-plan.md` + `wave-N-{role}/spec.md`
    /// directories. `--lang` accepts BCP-47 only (`pt-BR` / `en-US`); short
    /// codes are rejected. `--signals` is a free-form comma-separated list
    /// embedded in `spec.md` as a comment for downstream tooling.
    SpecDraft {
        /// Free-text intent (becomes the spec title + slug seed).
        #[arg(long)]
        intent: String,
        /// `light` (single-shot) or `full` (wave plan).
        #[arg(long, default_value = "full")]
        scope: String,
        /// BCP-47 narrative locale (`pt-BR` / `en-US`).
        #[arg(long, default_value = "pt-BR")]
        lang: String,
        /// Optional comma-separated signal list (`layers,files,registry`).
        #[arg(long)]
        signals: Option<String>,
        /// Output directory (default `.claude/spec/{slug}/`).
        #[arg(long)]
        output: Option<PathBuf>,
        /// Number of waves under Full scope (default 1).
        #[arg(long, default_value_t = 1)]
        waves: u32,
        /// Role applied to each scaffolded wave (default `mixed`).
        #[arg(long, default_value = "mixed")]
        role: String,
        /// Overwrite an existing output directory.
        #[arg(long)]
        force: bool,
    },
    /// Validate a spec directory against the Wave 1 layout contract. Reads
    /// `meta.json` + `spec.md` and runs `mustard_core::spec::contract::validate`.
    /// Exit code 0 ⇒ ok, 2 ⇒ violations, 1 ⇒ IO failure.
    SpecValidate {
        /// Path to a spec directory or `spec.md` file. A bare slug resolves
        /// to `.claude/spec/{slug}/`.
        #[arg(long)]
        spec: String,
        /// Emit pretty JSON (default — kept for symmetry with siblings).
        #[arg(long)]
        json: bool,
    },
    /// Score every discoverable SKILL.md against a free-text intent +
    /// subproject + phase. Pure Rust — no LLM. Emits the top-K skills with
    /// a numeric score and reason list. Consumed in-process by
    /// `agent-prompt-render` to fill `{recommended_skills}`.
    SkillResolve {
        /// Free-text intent (verb + nouns).
        #[arg(long)]
        intent: String,
        /// Optional subproject path (e.g. `apps/dashboard`).
        #[arg(long)]
        subproject: Option<String>,
        /// Pipeline phase: `ANALYZE` / `EXECUTE` / `REVIEW` / `PLAN` / `QA`.
        #[arg(long)]
        phase: Option<String>,
        /// Top-K cap (default 5).
        #[arg(long = "top-k", default_value_t = 5)]
        top_k: usize,
        /// Emit JSON instead of the table form.
        #[arg(long)]
        json: bool,
    },
    /// Manage per-spec memory entries (`memory/<name>.md`). Currently
    /// supports `create`. Generated files carry standardised frontmatter,
    /// automatic wirelinks to the spec + wave of origin, and the canonical
    /// sections `## Origem` / `## Aplica-se a` / `## Status` / `## Relacionado`.
    SpecMemory {
        /// Subcommand verb (`create`).
        subcommand: Option<String>,
        /// Spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: String,
        /// Memory entry name (kebab-case).
        #[arg(long)]
        name: String,
        /// Entry kind: `principle` / `process` / `reference`.
        #[arg(long, default_value = "principle")]
        kind: String,
        /// Origin wave label (e.g. `wave-1-mixed`).
        #[arg(long = "origin-wave")]
        origin_wave: Option<String>,
        /// Optional one-line description.
        #[arg(long)]
        description: Option<String>,
    },
    /// Sweep terminal, idle spec directories under `.claude/spec/` (W5.T5.5).
    ///
    /// Default is **dry-run**: enumerates every spec whose `meta.json` reports
    /// `stage=close` + `outcome=completed` and whose most-recent NDJSON event
    /// is older than `--age-days N` (default 15). Pass `--apply` to
    /// `fs::remove_dir_all` each candidate. Emits a JSON report and a
    /// `spec.clear.run` harness event.
    SpecClear {
        /// Repo root override. Defaults to the current working directory.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Age threshold in whole days. Specs whose newest event is older than
        /// this become candidates.
        #[arg(long = "age-days", default_value_t = spec_clear::DEFAULT_AGE_DAYS)]
        age_days: u32,
        /// Preview only — emit the report, mutate nothing (the default).
        #[arg(long, default_value_t = true, conflicts_with = "apply")]
        dry_run: bool,
        /// Apply the removals. Required to mutate the filesystem.
        #[arg(long)]
        apply: bool,
        /// Sweep every terminal spec regardless of age.
        #[arg(long)]
        all: bool,
        /// Restrict the sweep to one spec slug.
        #[arg(long)]
        name: Option<String>,
    },
    /// Audit (and optionally remove) drift in a project's `.claude/` directory.
    ///
    /// Enumerates every direct child of `.claude/`, classifies each against a
    /// declared consumer list (KEEP / STALE / ORPHAN / LEGACY / CACHE), and
    /// either reports candidates (default `--dry-run`) or removes the ORPHAN
    /// / LEGACY ones (`--apply`). Emits byte-stable pretty JSON; fail-open at
    /// every step — exit code is always 0.
    #[command(name = "claude-dir-prune")]
    ClaudeDirPrune {
        /// Repo root override. Defaults to the current working directory.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Preview only — emit the report, mutate nothing (the default).
        #[arg(long, default_value_t = true, conflicts_with = "apply")]
        dry_run: bool,
        /// Apply the removals. Required to mutate the filesystem.
        #[arg(long)]
        apply: bool,
        /// Reserved for parity with sibling subcommands — JSON is the only
        /// format today, but the flag exists so callers can pass it.
        #[arg(long)]
        json: bool,
    },
    /// W5.T5.1 — Drive the CLOSE-phase gates (verify → qa → docs-stale → summary).
    #[command(name = "close-orchestrate")]
    CloseOrchestrate {
        /// Spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: String,
        /// Skip the docs-stale-check gate.
        #[arg(long = "skip-docs")]
        skip_docs: bool,
    },
    /// W5.T5.2 — Orchestrate the REVIEW phase steps (prefetch + diff + DORA emits).
    #[command(name = "review-dispatch")]
    ReviewDispatch {
        /// PR number.
        #[arg(long)]
        pr: u64,
        /// Spec slug for event attribution.
        #[arg(long)]
        spec: Option<String>,
        /// Subproject to scope the diff to.
        #[arg(long)]
        subproject: Option<String>,
    },
    /// W5.T5.3 — Create a sub-spec linked to a parent spec for a tactical fix.
    #[command(name = "tactical-fix-create")]
    TacticalFixCreate {
        /// Parent spec slug (already created in `.claude/spec/`).
        #[arg(long)]
        parent: String,
        /// Free-text description of the fix (becomes the title + slug seed).
        #[arg(long)]
        description: String,
        /// Scope flag: `touch` / `light` (default) / `full`.
        #[arg(long, default_value = "light")]
        scope: String,
    },
    /// W5.T5.4 — Build a PRD JSON document from a free-text intent.
    #[command(name = "prd-build")]
    PrdBuild {
        /// Free-text intent (verb + nouns).
        #[arg(long)]
        intent: String,
        /// Output format: `json` (default) is the only supported value.
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// W5.T5.5a — Fetch and install a skill from a local path or GitHub spec.
    #[command(name = "skill-fetch")]
    SkillFetch {
        /// Source spec: `path:./local`, `github:owner/repo/path`, or a slug.
        #[arg(long)]
        name: String,
        /// Skip writes (preview only).
        #[arg(long)]
        dry_run: bool,
    },
    /// W5.T5.5b — Inspect the skill install cache for one entry.
    #[command(name = "skill-cache")]
    SkillCache {
        /// Skill slug to check.
        #[arg(long = "check")]
        check: String,
    },
    /// W5.T5.6 — Generate `.cursorrules` from the repo's `CLAUDE.md` tree.
    #[command(name = "adapt-cursor")]
    AdaptCursor {
        /// Repo root override.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Preview only — no filesystem mutation.
        #[arg(long)]
        dry_run: bool,
    },
    /// Refresh stale `.claude/` installs after edits in `apps/cli/templates/`.
    ///
    /// Walks `apps/cli/templates/{refs,commands/mustard,skills}/**`, SHA-256
    /// compares each source against the consumer `.claude/<sub>/`, and copies
    /// divergent files. Generated artefacts (`entity-registry.json`, caches)
    /// and volatile state dirs are excluded. Emits `{copied, skipped,
    /// conflicts, errors}` JSON. Fail-open; exit code is always 0.
    #[command(name = "refresh-claude")]
    RefreshClaude {
        /// Target consumer directory (the project whose `.claude/` to refresh).
        /// Defaults to the current working directory.
        #[arg(long)]
        target: Option<PathBuf>,
        /// Preview only — compare and report, but do NOT write any files.
        #[arg(long)]
        dry_run: bool,
        /// Override the templates source directory (defaults to auto-discovery).
        #[arg(long = "templates-dir")]
        templates_dir: Option<PathBuf>,
    },
    /// W5.T5.7a — Install dependencies in every detected subproject.
    #[command(name = "maint-deps")]
    MaintDeps {
        /// Preview only — print the resolved install commands without running.
        #[arg(long)]
        dry_run: bool,
    },
    /// W5.T5.7b — Run build/type-check validation in every detected subproject.
    #[command(name = "maint-validate")]
    MaintValidate {
        /// Preview only — print the resolved validate commands without running.
        #[arg(long)]
        dry_run: bool,
    },
    /// W5.T5.8 — Return the canonical audit checklist for a domain.
    #[command(name = "task-checklist")]
    TaskChecklist {
        /// Domain token (e.g. `copy`, `design`, `a11y`, `i18n`, `consistency`,
        /// `api-contract`).
        #[arg(long)]
        domain: String,
    },
    /// W5.T5.9 — Read or write the bugfix root-cause cache for retry reuse.
    #[command(name = "bugfix-cache")]
    BugfixCache {
        /// Cache signature hash.
        #[arg(long)]
        hash: String,
        /// Write mode — record a new entry with the supplied summary.
        #[arg(long)]
        summary: Option<String>,
        /// Files affected — comma-separated list (write mode only).
        #[arg(long)]
        files: Option<String>,
    },
    /// W5.T5.10 — Compute the recommended prompt budget for a role + wave.
    #[command(name = "context-budget")]
    ContextBudget {
        /// Agent role token.
        #[arg(long)]
        role: String,
        /// Spec slug (optional — only echoed in the report).
        #[arg(long)]
        spec: Option<String>,
        /// Wave number (optional).
        #[arg(long)]
        wave: Option<u32>,
    },
    /// W5.T5.11 — Idempotent cross-platform copy of `.claude/spec/` into a backup tree.
    #[command(name = "backup-specs")]
    BackupSpecs {
        /// Destination directory.
        #[arg(long)]
        target: Option<PathBuf>,
        /// Filter: `all` (default) or `active`.
        #[arg(long, default_value = "all")]
        filter: String,
        /// Preview only.
        #[arg(long)]
        dry_run: bool,
        /// Suppress the `MANIFEST.json` (default: emit a SHA-256 manifest at the backup root).
        #[arg(long)]
        no_manifest: bool,
    },
    /// W5.T5.13 — Translate a markdown heading line into a target locale.
    #[command(name = "i18n")]
    I18n {
        /// Subcommand: `translate-heading` is the only verb today.
        subcommand: String,
        /// Raw heading line, e.g. `## Tarefas`.
        #[arg(long)]
        from: Option<String>,
        /// Target BCP-47 locale (`pt-BR` / `en-US`).
        #[arg(long = "to-lang")]
        to_lang: Option<String>,
    },
    /// W5.T5.14 — Resolve the narrative locale for a spec.
    #[command(name = "spec-lang")]
    SpecLang {
        /// Subcommand: `resolve` is the only verb today.
        subcommand: String,
        /// Spec slug or directory path.
        #[arg(long)]
        spec: Option<String>,
    },
    /// W5.T5.15 — Auditable economy operations: capture-baseline / reconcile / report.
    #[command(name = "economy")]
    Economy {
        /// Subcommand: `capture-baseline` / `reconcile` / `report`.
        subcommand: String,
        /// Operation name (capture-baseline).
        #[arg(long)]
        operation: Option<String>,
        /// Wave number (capture-baseline, reconcile).
        #[arg(long)]
        wave: Option<u32>,
        /// Use historical telemetry as the baseline source (capture-baseline).
        #[arg(long = "from-history")]
        from_history: bool,
        /// Output format: `json` (default) or `table` (report only).
        #[arg(long, default_value = "json")]
        format: String,
        /// Spec name (per-spec baseline file; W2 path catalog).
        #[arg(long)]
        spec: Option<String>,
    },
    /// W4 spec-status-consistency — one-shot alignment of spec.md ↔ meta.json
    /// headers across all specs. Default source is `spec` (spec.md is
    /// authoritative; meta.json is rewritten to match). With `--source meta`
    /// the direction reverses. `--dry-run` previews without writing.
    #[command(name = "spec-status-backfill")]
    SpecStatusBackfill {
        /// Authoritative source: `spec` (default) or `meta`.
        #[arg(long, default_value = "spec")]
        source: String,
        /// Preview changes without writing any files.
        #[arg(long)]
        dry_run: bool,
        /// Restrict the run to a single spec slug.
        #[arg(long)]
        spec: Option<String>,
    },
    /// W5.T5.16 — Consolidate per-phase prelude (sync-detect + diff-context).
    #[command(name = "pipeline-prelude")]
    PipelinePrelude {
        /// Spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: String,
        /// Phase: `ANALYZE` / `PLAN` / `EXECUTE`.
        #[arg(long)]
        phase: String,
    },
}

/// Dispatch a `run` subcommand.
///
/// Unlike the enforcement dispatcher this never touches stdin and never
/// produces an [`Outcome`](mustard_core::model::contract::Outcome) — a `run`
/// script writes its own output and the process exits cleanly afterwards.
pub fn dispatch(cmd: RunCmd) {
    match cmd {
        RunCmd::SyncDetect { root } => sync_detect::run(&root),
        RunCmd::SyncRegistry { root, force } => sync_registry::run(&root, force),
        RunCmd::DiffContext {
            parent,
            subproject,
            phase,
        } => diff_context::run(parent.as_deref(), subproject.as_deref(), phase.as_deref()),
        RunCmd::EmitEvent {
            event,
            payload,
            spec,
            wave,
        } => emit_event::run(event.as_deref(), &payload, spec.as_deref(), wave),
        RunCmd::EmitPhase { spec, to, from } => {
            emit_phase::run(&spec, &to, from.as_deref());
        }
        RunCmd::EmitPipeline { kind, spec, payload, allow_no_qa } => {
            emit_pipeline::run(emit_pipeline::EmitPipelineOpts {
                kind,
                spec,
                payload,
                allow_no_qa,
            });
        }
        RunCmd::MigrateSpecHeaders {
            dry_run,
            apply,
            root,
            log,
            filter,
        } => {
            // `--apply` overrides the default `dry_run: true`; the clap
            // `conflicts_with` prevents both being passed explicitly.
            let _ = dry_run;
            migrate_spec_headers::run(migrate_spec_headers::MigrateOpts {
                apply,
                root,
                log,
                filter,
            });
        }
        RunCmd::MigrateToMeta { root, force, strip_headers } => {
            migrate_to_meta::run(migrate_to_meta::MigrateToMetaOpts {
                root,
                force,
                strip_headers,
            });
        }
        RunCmd::CompleteSpec {
            spec,
            archive,
            archive_stale,
            archive_followups,
        } => complete_spec::run(spec.as_deref(), archive, archive_stale, archive_followups),
        RunCmd::ContextSlice {
            context,
            spec,
            max_lines,
            context_claude_md,
        } => context_slice::run(
            &context,
            spec.as_deref(),
            max_lines,
            context_claude_md.as_deref(),
        ),
        RunCmd::ContextResolve { scope, scope_file } => {
            scan::resolve::run(scope.as_deref(), scope_file.as_deref());
        }
        RunCmd::Memory {
            subcommand,
            json,
            spec,
            wave,
            agent,
            summary,
            files,
            grouped,
            format,
            cluster,
            query,
            path,
            kind,
            role,
            details,
            confidence,
            verify,
            include_low,
            limit,
            by_role,
            note,
        } => memory::dispatch(
            &subcommand,
            json.as_deref(),
            spec.as_deref(),
            wave,
            agent.as_deref(),
            summary.as_deref(),
            files.as_deref(),
            grouped,
            &format,
            memory::DispatchExtras {
                cluster,
                query,
                kind,
                role,
                details,
                confidence,
                verify,
                include_low,
                limit,
                by_role,
                note,
                feedback_path: path,
            },
        ),
        RunCmd::MemoryIngest { delete, agent_memory } => {
            memory_ingest::run_with(memory_ingest::MemoryIngestOpts { delete, agent_memory });
        }
        RunCmd::PipelineStateIngest { delete: _ } => {
            pipeline_state_ingest::run(pipeline_state_ingest::PipelineStateIngestOpts);
        }
        RunCmd::EpicFold { detect, epic } => epic_fold::run(detect, epic.as_deref()),
        RunCmd::SpecExtract {
            spec,
            wave,
            ac,
            measure,
        } => spec_extract::run(&spec, wave, ac, measure),
        RunCmd::SpecLink {
            parent,
            child,
            reason,
        } => spec_link::run(parent.as_deref(), child.as_deref(), reason.as_deref()),
        RunCmd::SpecChildren { parent } => spec_children::run(parent.as_deref()),
        RunCmd::SpecChildrenTree { spec } => spec_children_tree::run(spec.as_deref()),
        RunCmd::AnalyzeValidation { spec } => analyze_validation::run(spec.as_deref()),
        RunCmd::MarkChecklistItem {
            spec,
            item,
            line,
            cwd,
        } => mark_checklist_item::run(spec.as_deref(), item.as_deref(), line, cwd.as_deref()),
        RunCmd::WaveTree { spec_dir, format } => wave_tree::run(&spec_dir, &format),
        RunCmd::WaveDependency => wave_dependency::run(),
        RunCmd::WaveFiles { spec, wave } => wave_files::run(spec.as_deref(), wave),
        RunCmd::ScopeDecompose => scope_decompose::run(),
        RunCmd::ExecRewaveCheck { spec } => exec_rewave_check::run(spec.as_deref()),
        RunCmd::DependencyPrecheck { spec, subproject } => {
            dependency_precheck::run(spec.as_deref(), subproject.as_deref());
        }
        RunCmd::WaveSizeCheck { spec_dir } => wave_size_check::run(spec_dir.as_deref()),
        RunCmd::GateRegressionCheck { spec, moment } => {
            use crate::run::gate_regression_check::{self, GateInput, Moment};
            let spec_path = std::path::PathBuf::from(".claude/spec").join(&spec).join("spec.md");
            let plan_text = std::fs::read_to_string(&spec_path).unwrap_or_default();
            let moment_enum = match moment {
                1 => Moment::One,
                2 => Moment::Two,
                3 => Moment::Three,
                _ => Moment::One,
            };
            let input = GateInput {
                spec_path,
                plan_text,
                diff: Vec::new(),
                declared_fns: Vec::new(),
                before_snapshot: None,
                after_snapshot: None,
            };
            match gate_regression_check::run(input, moment_enum) {
                Ok(_) => std::process::exit(0),
                Err(_) => std::process::exit(2),
            }
        }
        RunCmd::RecipeMatch {
            entity,
            operation,
            subproject,
        } => recipe_match::run(entity.as_deref(), operation.as_deref(), subproject.as_deref()),
        RunCmd::QaRun { spec, format } => qa_run::run(&spec, &format),
        RunCmd::QaRunAll => qa_run_all::run(),
        RunCmd::RebuildSpecs => rebuild_specs::run(),
        RunCmd::Metrics {
            subcommand,
            args,
            format,
        } => metrics::run(subcommand.as_deref(), &args, &format),
        RunCmd::MetricsWaveStatus { spec } => {
            let mut argv: Vec<String> = Vec::new();
            if let Some(s) = spec {
                argv.push("--spec".to_string());
                argv.push(s);
            }
            metrics_wave_status::run(&argv);
        }
        RunCmd::EventProjections {
            view,
            spec,
            wave,
            format,
        } => event_projections::run(view.as_deref(), spec.as_deref(), wave, &format),
        RunCmd::VerifyPipeline { format } => verify_pipeline::run(&format),
        RunCmd::PipelineSummary { spec_dir, format, self_test } => {
            pipeline_summary::run(spec_dir.as_deref(), &format, self_test);
        }
        RunCmd::ReviewResult {
            spec,
            verdict,
            critical,
            subproject,
        } => review_result::run(spec.as_deref(), verdict.as_deref(), critical, subproject.as_deref()),
        RunCmd::Statusline { preview } => statusline::run(preview),
        RunCmd::Skills { subcommand, args } => skills::run(subcommand.as_deref(), &args),
        RunCmd::SecurityScan { dir, json } => security_scan::run(dir.as_deref(), json),
        RunCmd::VerifyEmit {
            event,
            since,
            payload_key,
            payload_value,
            spec,
            quiet,
        } => verify_emit::run(
            event.as_deref(),
            since.as_deref(),
            payload_key.as_deref(),
            payload_value.as_deref(),
            spec.as_deref(),
            quiet,
        ),
        RunCmd::RtkGain => rtk_gain::run(),
        RunCmd::ScanOrchestrate { target, force } => {
            scan_orchestrate::run(force, target.as_deref());
        }
        RunCmd::ScanFinalize { skip_security } => scan_finalize::run(skip_security),
        RunCmd::ScanStructural { subproject } => scan_structural::run(subproject.as_deref()),
        RunCmd::ScanMdValidate { from, strict } => {
            scan_md_validate::run(from.as_deref(), strict);
        }
        RunCmd::ScanRecipesValidate { strict } => scan_recipes_validate::run(strict),
        RunCmd::OtelCollector => otel::collector::run(),
        RunCmd::TranscriptWatcher { once } => transcript_watcher::run(once),
        RunCmd::DiagnoseOtel {
            json,
            expect_rows_after,
        } => otel::diagnose::run(json, expect_rows_after.as_deref()),
        RunCmd::Doctor { residue, check, format, json } => {
            // `--json` is a shorthand for `--format json` (W10.T10.6).
            let effective_format = if json { "json".to_string() } else { format };
            doctor::run(doctor::DoctorOpts {
                residue,
                check,
                format: effective_format,
            });
        }
        RunCmd::DocsStaleCheck { from, strict, include_nested } => {
            docs_stale_check::run(from.as_deref(), strict, include_nested);
        }
        RunCmd::ArtifactUpdate {
            check,
            apply,
            manifest,
        } => artifact_update::run(check, apply, manifest.as_deref()),
        RunCmd::AmendFinalize { session_id } => amend_finalize::run_cli(&session_id),
        RunCmd::GraphIndex => graph_index::run(),
        RunCmd::GraphDead => graph_dead::run(),
        RunCmd::WaveScaffold { spec_dir, plan } => {
            wave_scaffold::run(spec_dir.as_deref(), plan.as_deref());
        }
        RunCmd::PlanFromSpec { waves, roles, lang, summary } => {
            plan_from_spec::run(plan_from_spec::PlanFromSpecOpts {
                waves,
                roles,
                lang,
                summary,
            });
        }
        RunCmd::ActiveSpecs { format, root } => {
            active_specs::run(active_specs::ActiveSpecsOpts { format, root });
        }
        RunCmd::Status { harness, format, root } => {
            status::run(status::StatusOpts { harness, format, root });
        }
        RunCmd::SkillsList { format, root } => {
            // Delegate to the existing skills::run with the "list" subcommand,
            // passing --format and --root via the args slice.
            let args: Vec<String> = vec![
                "--format".to_string(),
                format,
                "--root".to_string(),
                root.display().to_string(),
            ];
            skills::run(Some("list"), &args);
        }
        RunCmd::Knowledge { subcommand, filter, format, root } => {
            match subcommand.as_deref() {
                Some("glossary") | None => {
                    knowledge::run(knowledge::GlossaryOpts { filter, format, root });
                }
                Some(other) => {
                    eprintln!("knowledge: unknown subcommand '{other}'. Try: glossary");
                    std::process::exit(1);
                }
            }
        }
        RunCmd::ReviewPrefetch { pr_ref, format, root } => {
            let pr_ref = pr_ref.unwrap_or_default();
            if pr_ref.is_empty() {
                println!("{}",
                    serde_json::to_string_pretty(&serde_json::json!({"error":"pr-ref-required"}))
                        .unwrap_or_default()
                );
            } else {
                review_prefetch::run(review_prefetch::ReviewPrefetchOpts { pr_ref, format, root });
            }
        }
        RunCmd::ResumeBootstrap { spec, json } => resume_bootstrap::run(&spec, json),
        RunCmd::AgentPromptRender {
            spec,
            wave,
            role,
            subproject,
            mode,
            retry_context_file,
            task_filter,
            budget_tokens,
        } => agent_prompt_render::run(
            &spec,
            wave,
            &role,
            &subproject,
            agent_prompt_render::RenderMode::parse(&mode),
            retry_context_file.as_deref(),
            task_filter.as_deref(),
            budget_tokens,
        ),
        RunCmd::WorktreeGc {
            repo,
            age_days,
            dry_run,
            apply,
        } => {
            // `dry_run` defaults to `true`; clap's `conflicts_with` blocks
            // passing both. `--apply` is the authoritative mutator flag.
            let _ = dry_run;
            worktree_gc::run(worktree_gc::WorktreeGcOpts {
                repo,
                age_days,
                apply,
            });
        }
        RunCmd::Unhook { repo, scope, confirm } => {
            unhook::run(unhook::UnhookOpts { repo, scope, confirm });
        }
        RunCmd::Rehook { repo, scope, confirm } => {
            rehook::run(rehook::RehookOpts { repo, scope, confirm });
        }
        RunCmd::SpecDraft {
            intent,
            scope,
            lang,
            signals,
            output,
            waves,
            role,
            force,
        } => {
            spec_draft::run(spec_draft::SpecDraftOpts {
                intent,
                scope,
                lang,
                signals,
                output,
                waves,
                role,
                force,
            });
        }
        RunCmd::SpecValidate { spec, json } => {
            let _ = json; // currently always emits JSON
            spec_validate::run(std::path::Path::new(&spec), true);
        }
        RunCmd::SkillResolve {
            intent,
            subproject,
            phase,
            top_k,
            json,
        } => {
            skill_resolve::run(skill_resolve::SkillResolveOpts {
                intent,
                subproject,
                phase,
                json,
                top_k,
            });
        }
        RunCmd::SpecMemory {
            subcommand,
            spec,
            name,
            kind,
            origin_wave,
            description,
        } => {
            spec_memory::dispatch(
                subcommand.as_deref(),
                spec_memory::SpecMemoryCreateOpts {
                    spec,
                    name,
                    kind,
                    origin_wave,
                    description,
                },
            );
        }
        RunCmd::SpecClear {
            repo,
            age_days,
            dry_run,
            apply,
            all,
            name,
        } => {
            // `dry_run` defaults to `true`; clap's `conflicts_with` ensures the
            // two flags don't co-exist. `--apply` is the authoritative mutator.
            let _ = dry_run;
            spec_clear::run(spec_clear::SpecClearOpts {
                repo,
                age_days,
                apply,
                all,
                name,
            });
        }
        RunCmd::ClaudeDirPrune {
            repo,
            dry_run,
            apply,
            json,
        } => {
            // `dry_run` defaults to `true`; clap's `conflicts_with` blocks
            // both flags from coexisting. `--apply` is the authoritative
            // mutator flag.
            let _ = dry_run;
            claude_dir_prune::run(claude_dir_prune::ClaudeDirPruneOpts {
                repo,
                apply,
                json,
            });
        }
        // --- W5 deep-refactor: T5.1–T5.16 -------------------------------------
        RunCmd::CloseOrchestrate { spec, skip_docs } => {
            close_orchestrate::run(close_orchestrate::CloseOrchestrateOpts { spec, skip_docs });
        }
        RunCmd::ReviewDispatch { pr, spec, subproject } => {
            review_dispatch::run(review_dispatch::ReviewDispatchOpts { pr, spec, subproject });
        }
        RunCmd::TacticalFixCreate { parent, description, scope } => {
            tactical_fix_create::run(tactical_fix_create::TacticalFixOpts {
                parent,
                description,
                scope,
            });
        }
        RunCmd::PrdBuild { intent, format } => {
            prd_build::run(prd_build::PrdBuildOpts { intent, format });
        }
        RunCmd::SkillFetch { name, dry_run } => {
            skill_fetch::run(skill_fetch::SkillFetchOpts { name, dry_run });
        }
        RunCmd::SkillCache { check } => {
            skill_cache::run(skill_cache::SkillCacheOpts { check });
        }
        RunCmd::AdaptCursor { repo, dry_run } => {
            adapt_cursor::run(adapt_cursor::AdaptCursorOpts { repo, dry_run });
        }
        RunCmd::RefreshClaude { target, dry_run, templates_dir } => {
            refresh_claude::run(refresh_claude::RefreshClaudeOpts {
                target,
                dry_run,
                templates_dir,
            });
        }
        RunCmd::MaintDeps { dry_run } => {
            maint_deps::run(maint_deps::MaintDepsOpts { dry_run });
        }
        RunCmd::MaintValidate { dry_run } => {
            maint_validate::run(maint_validate::MaintValidateOpts { dry_run });
        }
        RunCmd::TaskChecklist { domain } => {
            task_checklist::run(task_checklist::TaskChecklistOpts { domain });
        }
        RunCmd::BugfixCache { hash, summary, files } => {
            bugfix_cache::run(bugfix_cache::BugfixCacheOpts { hash, summary, files });
        }
        RunCmd::ContextBudget { role, spec, wave } => {
            context_budget::run(context_budget::ContextBudgetOpts { role, spec, wave });
        }
        RunCmd::BackupSpecs {
            target,
            filter,
            dry_run,
            no_manifest,
        } => {
            backup_specs::run(backup_specs::BackupSpecsOpts {
                target,
                filter,
                dry_run,
                no_manifest,
            });
        }
        RunCmd::I18n { subcommand, from, to_lang } => {
            match subcommand.as_str() {
                "translate-heading" => i18n_translate::run(i18n_translate::TranslateHeadingOpts {
                    from: from.unwrap_or_default(),
                    to_lang: to_lang.unwrap_or_default(),
                }),
                other => {
                    eprintln!("i18n: unknown subcommand {other:?}. Try: translate-heading");
                    std::process::exit(1);
                }
            }
        }
        RunCmd::SpecLang { subcommand, spec } => {
            match subcommand.as_str() {
                "resolve" => spec_lang_resolve::run(spec_lang_resolve::SpecLangResolveOpts {
                    spec: spec.unwrap_or_default(),
                }),
                other => {
                    eprintln!("spec-lang: unknown subcommand {other:?}. Try: resolve");
                    std::process::exit(1);
                }
            }
        }
        RunCmd::Economy {
            subcommand,
            operation,
            wave,
            from_history,
            format,
            spec,
        } => match subcommand.as_str() {
            "capture-baseline" => economy_capture_baseline::run(
                economy_capture_baseline::CaptureBaselineOpts {
                    operation: operation.unwrap_or_default(),
                    wave: wave.unwrap_or(0),
                    from_history,
                    spec: spec.clone(),
                },
            ),
            "reconcile" => economy_reconcile::run(economy_reconcile::ReconcileOpts {
                wave: wave.unwrap_or(0),
                spec: spec.clone(),
            }),
            "report" => economy_report::run(economy_report::ReportOpts {
                format,
                spec: spec.clone(),
            }),
            other => {
                eprintln!(
                    "economy: unknown subcommand {other:?}. Try: capture-baseline / reconcile / report"
                );
                std::process::exit(1);
            }
        },
        RunCmd::PipelinePrelude { spec, phase } => {
            pipeline_prelude::run(pipeline_prelude::PreludeOpts { spec, phase });
        }
        RunCmd::SpecStatusBackfill { source, dry_run, spec } => {
            spec_status_backfill::run_cli(spec_status_backfill::BackfillOpts {
                source,
                dry_run,
                spec,
                cwd: None,
            });
        }

    }
}
