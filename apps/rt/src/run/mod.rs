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
pub mod agent_prompt_render;
pub mod amend_finalize;
mod analyze_validation;
mod artifact_update;
mod knowledge;
mod backfill_run_usage_cost;
mod backfill_run_usage_spec;
mod db_maintain;
mod doctor;
mod complete_spec;
mod context_slice;
mod dependency_precheck;
mod diff_context;
mod docs_stale_check;
mod emit_event;
pub mod emit_phase;
mod emit_pipeline;
pub mod env;
mod epic_fold;
pub mod event_projections;
pub use event_projections::{pipeline_state_for_spec, PipelineStateView};
pub use env::current_spec;
mod exec_rewave_check;
mod mark_checklist_item;
mod memory;
mod memory_cross_wave;
mod migrate_spec_headers;
mod memory_ingest;
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
mod scan_orchestrate;
mod scan_precompute;
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
mod sync_detect;
mod sync_registry;
mod transcript_watcher;
mod verify_pipeline;
mod wave_dependency;
mod wave_files;
mod wave_lib;
mod wave_scaffold;
mod wave_size_check;
mod wave_tree;
mod wikilink;

use clap::Subcommand;
use std::path::PathBuf;

/// The `run` subcommands — one variant per ported script.
#[derive(Debug, Subcommand)]
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
    },
    /// Persist agent memory, decisions/lessons, or knowledge entries.
    /// `cross-wave` is the read-side: emits markdown summarising prior waves.
    /// `list` emits all memory entries (knowledge_patterns + decisions + lessons).
    Memory {
        /// Subcommand: `agent`, `decision`, `knowledge`, `list`, or `cross-wave`.
        subcommand: String,
        /// Input JSON (Windows-friendly form; stdin is the POSIX fallback).
        #[arg(long)]
        json: Option<String>,
        /// `agent` / `cross-wave` — spec name (pipeline attribution for
        /// `agent`; parent spec for `cross-wave`).
        #[arg(long)]
        spec: Option<String>,
        /// `agent` / `cross-wave` — wave number (1-based).
        #[arg(long)]
        wave: Option<u32>,
        /// `agent` only — agent identifier/role (becomes `agent_type`).
        #[arg(long)]
        agent: Option<String>,
        /// `agent` only — one-line summary of what the agent produced.
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
    },
    /// One-shot ingest of legacy JSON files into the SQLite Wave 6a tables.
    ///
    /// Reads `.claude/knowledge.json`, `.claude/memory/decisions.json`, and
    /// `.claude/memory/lessons.json` (if present) and inserts their entries
    /// into `knowledge_patterns`, `memory_decisions`, `memory_lessons`.
    /// Prints a JSON summary. Fail-open per file.
    MemoryIngest {
        /// Remove the source JSON files after a successful ingest.
        #[arg(long)]
        delete: bool,
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
    PipelineSummary {
        /// Path to the spec directory (must contain `spec.md`).
        #[arg(long = "spec-dir")]
        spec_dir: Option<String>,
        /// Output format: `markdown` (default) or `json`.
        #[arg(long, default_value = "markdown")]
        format: String,
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
    /// Backfill `cost_usd_micros` on legacy `run_usage` rows.
    ///
    /// Default: only touch rows with NULL/0 cost (idempotent — applies the
    /// shared `compute_cost_micros` helper to historical NULLs). With
    /// `--force`, recomputes cost on every row carrying any non-zero token
    /// bucket, overwriting prior values. Use `--force` after the pricing
    /// formula changes (cache-aware buckets, new rate tiers).
    BackfillRunUsageCost {
        /// Recompute cost on ALL rows with non-zero tokens, overwriting any
        /// existing `cost_usd_micros`. Without this flag, only NULL/0 cost
        /// rows are touched.
        #[arg(long)]
        force: bool,
    },
    /// Backfill `spec` / `wave_id` / `agent_id` on legacy `run_usage` rows
    /// that came in without attribution, by joining against `run_attribution`.
    BackfillRunUsageSpec,
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
    /// and (optionally) residue. Prints a compact OK/WARN/FAIL report and
    /// exits 1 if any category is FAIL, 0 otherwise.
    Doctor {
        /// Also scan for dead file/script references (slower).
        #[arg(long)]
        residue: bool,
        /// Run a specific named check in isolation (e.g. `skill-discovery`).
        #[arg(long)]
        check: Option<String>,
        /// Output format: `text` (default) or `json`.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// SQLite harness database maintenance.
    ///
    /// Default (no flags): emit a JSON size/space report (read-only).
    /// `--vacuum`: WAL checkpoint + VACUUM; print before/after byte counts.
    /// `--prune-keep <N>`: delete all but the N most-recent events by id.
    DbMaintain {
        /// Run `PRAGMA wal_checkpoint(TRUNCATE)` then `VACUUM`.
        #[arg(long)]
        vacuum: bool,
        /// Keep only the N most-recent events; delete the rest.
        #[arg(long = "prune-keep")]
        prune_keep: Option<u32>,
    },
    /// Finalize open amendment windows for a session (appends `## Amendments` to spec.md,
    /// moves archived specs, updates the DB, and emits `pipeline.amend_close`).
    AmendFinalize {
        /// Session identifier whose open windows to finalize.
        #[arg(long = "session-id")]
        session_id: String,
    },
    /// Extract `[[wikilink]]` occurrences from every `.md` under `--spec-dir`,
    /// persist them into the `wikilinks` table, emit `{wikilinks,orphans}` JSON.
    WikilinkExtract {
        /// Spec directory to scan (recursive).
        #[arg(long = "spec-dir")]
        spec_dir: Option<String>,
    },
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
    /// progress, extracts a one-line resumo, and backfills SQLite events for
    /// specs that arrived via `git pull` without local pipeline events.
    /// Output is either a markdown table (default) or a JSON document.
    ActiveSpecs {
        /// Output format: `table` (default) or `json`.
        #[arg(long, default_value = "table")]
        format: String,
        /// Project root directory (default: current working directory).
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Skip the SQLite backfill step (useful in tests / read-only contexts).
        #[arg(long)]
        no_backfill: bool,
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
        /// Wave number (1-based). Omitted for non-wave specs.
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
        RunCmd::EmitPipeline { kind, spec, payload } => {
            emit_pipeline::run(emit_pipeline::EmitPipelineOpts { kind, spec, payload });
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
        } => context_slice::run(&context, spec.as_deref(), max_lines),
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
        ),
        RunCmd::MemoryIngest { delete } => memory_ingest::run(delete),
        RunCmd::PipelineStateIngest { delete } => {
            pipeline_state_ingest::run(pipeline_state_ingest::PipelineStateIngestOpts { delete });
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
        RunCmd::PipelineSummary { spec_dir, format } => {
            pipeline_summary::run(spec_dir.as_deref(), &format);
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
        RunCmd::BackfillRunUsageCost { force } => backfill_run_usage_cost::run(force),
        RunCmd::BackfillRunUsageSpec => backfill_run_usage_spec::run(),
        RunCmd::ScanOrchestrate { target, force } => {
            scan_orchestrate::run(force, target.as_deref());
        }
        RunCmd::ScanFinalize { skip_security } => scan_finalize::run(skip_security),
        RunCmd::OtelCollector => otel::collector::run(),
        RunCmd::TranscriptWatcher { once } => transcript_watcher::run(once),
        RunCmd::DiagnoseOtel {
            json,
            expect_rows_after,
        } => otel::diagnose::run(json, expect_rows_after.as_deref()),
        RunCmd::Doctor { residue, check, format } => doctor::run(doctor::DoctorOpts {
            residue,
            check,
            format,
        }),
        RunCmd::DbMaintain { vacuum, prune_keep } => {
            let mut args: Vec<String> = Vec::new();
            if vacuum {
                args.push("--vacuum".to_string());
            }
            if let Some(n) = prune_keep {
                args.push("--prune-keep".to_string());
                args.push(n.to_string());
            }
            db_maintain::run(&args);
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
        RunCmd::WikilinkExtract { spec_dir } => wikilink::run(spec_dir.as_deref()),
        RunCmd::WaveScaffold { spec_dir, plan } => {
            wave_scaffold::run(spec_dir.as_deref(), plan.as_deref());
        }
        RunCmd::ActiveSpecs { format, root, no_backfill } => {
            active_specs::run(active_specs::ActiveSpecsOpts {
                format,
                root,
                no_backfill,
            });
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
        } => agent_prompt_render::run(
            &spec,
            wave,
            &role,
            &subproject,
            agent_prompt_render::RenderMode::parse(&mode),
            retry_context_file.as_deref(),
        ),
    }
}
