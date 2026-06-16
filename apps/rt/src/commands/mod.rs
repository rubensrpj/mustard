//! The `run` face of `mustard-rt` — the b4 script port.
//!
//! `mustard-rt on` / `mustard-rt check` are the enforcement faces: they read
//! the harness JSON from stdin and run hook modules. The `run` face is
//! different — it ports the utility *scripts* that used to live under
//! `templates/scripts/` as standalone `bun` programs. A `run` subcommand takes
//! its inputs as `clap` arguments (a directory, flags), never from stdin, and
//! prints its result to stdout exactly as the JS script did.
//!
//! Each ported script is its own submodule. (The early `sync-detect` /
//! `sync-registry` scanner ports were since removed — subproject discovery now
//! comes from grain's `grain.model.json` via the scan tool.)

pub mod migrate;
pub mod i18n;
pub mod agent;
pub mod checklist;
pub mod doctor;
pub mod review;
pub mod knowledge;
pub mod economy;
pub mod pipeline;
pub mod event;
pub mod wave;
pub mod spec;
pub mod maint;
pub mod scan;
pub mod scan_claude;
pub mod scan_guards;
pub mod feature;
pub mod digest_precision;
pub mod glossary_coverage;
pub mod lexicon_suggest;
pub mod lexicon_enrich;
// W3 of `2026-05-26-claude-paths-single-source` — three typed doctor checks
// (claude-paths, workspace-leaks, i1) that emit native JSON shapes. They are
// dispatched by `doctor.rs` but live in dedicated modules so the legacy
// `CheckResult` envelope stays out of their way.
pub use event::event_projections::{pipeline_state_from_events, PipelineStateView};
// Spec A v4 / W4 — behavior-regression gate connecting W1 (vocabulary),
// W1.5 (AST agnostic) and W2 (snapshot) primitives.
// Spec A v4 / W5 — span-level verdict ledger (`_review-spans.md`).
mod statusline;
// W4: lang-aware spec slug helper. Thin facade over `mustard_core::slugify`.
// W6: subcommand entry point (`i18n translate-heading`, `spec-lang resolve`).
// Spec A v4 / W6 — token-budget primitive used by `resume_bootstrap`.

use clap::Subcommand;
use std::path::PathBuf;

/// The `run` subcommands — one variant per ported script.
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum — clap-Subcommand; boxing breaks derive
pub enum RunCmd {
    /// Mine the workspace into `grain.model.json` via the bundled `scan` tool —
    /// THE scan (replaced the old in-tree miner + per-project skill/agent
    /// generation; the model is the single durable artifact).
    Scan {
        /// The workspace root to scan. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Output path. Defaults to `<root>/.claude/grain.model.json`.
        #[arg(long)]
        out: Option<PathBuf>,
        /// (Re)generate a lean CLAUDE.md for every subproject found in the
        /// grain model. Only the machine-owned scan-map block is regenerated;
        /// curated sections (Guards, Architecture, …) are preserved verbatim.
        /// Without this flag the command only warns about CLAUDE.md files that
        /// exceed the size threshold.
        #[arg(long)]
        full: bool,
    },
    /// Research a feature request against the repo via the `scan` digest (no
    /// source reading) and emit the structured insumos for decomposition +
    /// `scan spec`. The grounding step of the elicitation loop.
    Feature {
        /// The free-text feature/bugfix request to research.
        #[arg(long)]
        intent: String,
        /// Workspace root. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Deterministic check of how well a `CONTEXT.md` domain glossary covers the
    /// repo-vocabulary terms a feature intent touches (the digest's matched
    /// terms). Emits byte-stable JSON `{verdict, present, termsTotal,
    /// termsCovered, coveragePct, uncovered}` for the `/feature` ANALYZE nudge —
    /// it never grills inline and never blocks. Reuses the exact term matcher
    /// `context-slice` uses. Fail-open: a missing model / unreadable glossary
    /// degrades to `verdict: "na"` (no nudge), exit 0.
    #[command(name = "glossary-coverage")]
    GlossaryCoverage {
        /// The free-text feature request whose domain terms are scored.
        #[arg(long)]
        intent: String,
        /// A `CONTEXT.md` / `CONTEXT-MAP.md` glossary path. Repeatable.
        #[arg(long)]
        context: Vec<String>,
        /// Workspace root (holds `.claude/grain.model.json`). Defaults to `.`.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Fold the active session's `feature.query` (the digest's SUGGESTED
    /// anchors) × `feature.outcome` (the OBSERVED Read/Edit/Write of those
    /// anchors, emitted by the `feature_outcome_observer`) into a byte-stable
    /// digest-precision report — the deterministic CRITERION OF STOP for the
    /// locator redesign. Emits `{queries, recall_x1000, precision_x1000,
    /// anchorsSuggested, anchorsRead, readsTotal, perTerm:[{term, reads,
    /// queries, precision_x1000}]}`: recall = anchors read / anchors suggested,
    /// precision = reads-that-were-anchors / reads-in-window, perTerm = how many
    /// reads each query term led to. Fixed-point per-mille (no float), all lists
    /// sorted — no timestamps/paths leak. Reads events only (never the repo);
    /// fail-open, always exits 0.
    #[command(name = "digest-precision")]
    DigestPrecision {
        /// Workspace root. Defaults to the current directory (resolved to the
        /// workspace anchor like every run-face emitter).
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Correlate consecutive `feature.query` events of the WHOLE workspace
    /// telemetry (every session + spec scope, windowed to the most recent
    /// rounds; correlation grouped per emitting origin so contexts never
    /// cross-pair) — a `none`-tier term in one query followed by a NEW
    /// exact/fold/stem term in the next is a confirmed vocabulary bridge —
    /// into project-lexicon candidates `{missed, bridged, files}`, deduped
    /// (folded keys) against the lexicon in force (seed + project overlay).
    /// Without flags it only LISTS (byte-stable JSON; never writes).
    /// `--accept <missed>=<bridged>` records ONE entry in the project overlay
    /// `<root>/.claude/lexicons/<pair>.toml` (created from the template shape
    /// when absent; `[terms]` kept alphabetical, comments preserved) — never
    /// the embedded seed. Pair resolved like the digest: root `specLang` + `en`.
    #[command(name = "lexicon-suggest")]
    LexiconSuggest {
        /// Accept one candidate as `<missed>=<bridged>` and write it to the
        /// project lexicon overlay. Omit to list candidates (read-only).
        #[arg(long)]
        accept: Option<String>,
        /// Workspace root. Defaults to the current directory (resolved to the
        /// workspace anchor like every run-face emitter).
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// PROACTIVE sibling of `lexicon-suggest`: populate the project lexicon
    /// overlay with code→user-word bridges BEFORE the first query misses.
    ///
    /// The rt stays 100% deterministic — the AI never runs here; the
    /// orchestrator (the harness model, outside this binary) proposes the
    /// bridges between the two pure-data modes:
    ///
    /// `--check` (read-only): emit byte-stable JSON `{pair, language,
    /// unbridged}` — the top mined CODE terms (digest term index, discriminative
    /// rank) that are NOT a value of any lexicon entry (seed + project overlay),
    /// i.e. nothing maps a user word onto them. Empty list = no-op. Nothing is
    /// written.
    ///
    /// `--apply <proposals.json>` (gated, writes): read the orchestrator's
    /// `[{userWord, codeTerms}]` proposals and, for each code term, validate it
    /// EXISTS as a mined term in the model (deterministic anti-hallucination
    /// gate). Valid targets are written to `<root>/.claude/lexicons/<pair>.toml`
    /// via the shared `lexicon-suggest` writer (atomic, alphabetical, comments
    /// preserved) — never the embedded seed. Rejected targets
    /// (`target_not_in_model`) are reported, never written. Pair resolved like
    /// the digest: root `specLang`/`lang` primary subtag + `en`.
    LexiconEnrich {
        /// Read-only mode (the default): list the unbridged mined vocabulary.
        #[arg(long)]
        check: bool,
        /// Apply the bridges in this proposals JSON file (gated write). Takes
        /// precedence over `--check`.
        #[arg(long)]
        apply: Option<PathBuf>,
        /// Workspace root. Defaults to the current directory (resolved to the
        /// workspace anchor like every run-face emitter).
        #[arg(long, default_value = ".")]
        root: PathBuf,
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
    /// Finalize a pipeline spec — single-stage close straight to `completed`.
    CompleteSpec {
        /// Spec name (required unless `--archive-stale`/`--archive-followups`).
        spec: Option<String>,
        /// Idempotent alias of the single complete: re-emit `completed` + meta
        /// sync and drop any legacy state file. No filesystem move.
        #[arg(long)]
        archive: bool,
        /// No-op (retained for compatibility): the single-stage close no longer
        /// produces `closed-followup` specs, so there is nothing to sweep.
        #[arg(long = "archive-stale")]
        archive_stale: bool,
        /// No-op (retained for compatibility): see `--archive-stale`.
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
        /// W8.T8.8 — slice the given CLAUDE.md against the same relevance
        /// terms. Optional; the CONTEXT.md path(s) remain primary.
        #[arg(long = "context-claude-md")]
        context_claude_md: Option<String>,
    },
    /// Recall the knowledge records most relevant to a query from the unified
    /// store — the measurable CLI face of `knowledge::recall::recall_scored`
    /// (BM25 + relevance threshold, the same function the agent-prompt render
    /// calls in-process). Invoked as `mustard-rt run knowledge recall ...`
    /// (argv pre-routing in `main.rs` collapses the two tokens).
    ///
    /// Output is one byte-stable pair of lines per hit, best-first:
    /// `[{kind}] ({scope}) score={n} — {label}` + an indented ~120-char snippet
    /// of the content. An empty recall prints `(no matches)`. Determinism +
    /// fail-open mirror the underlying recall (never panics).
    #[command(name = "knowledge-recall")]
    KnowledgeRecall {
        /// The relevance query (role + task text, free-form). Required.
        #[arg(long)]
        query: String,
        /// Scope filter: `global` | `spec:NAME` | `wave:NAME:N`. Omit for all
        /// scopes (the default — the whole store is eligible).
        #[arg(long)]
        scope: Option<String>,
        /// `.claude/` directory to read the store from. Defaults to the
        /// workspace-resolved `.claude/` of the current directory.
        #[arg(long)]
        root: Option<PathBuf>,
        /// Result cap (default 5).
        #[arg(long, default_value_t = 5)]
        max: usize,
    },
    /// Garbage-collect non-substantive records from the unified knowledge store
    /// — the physical-delete leg of the quality gate (write rejects, read hides,
    /// `prune` removes). Invoked as `mustard-rt run knowledge prune ...` (argv
    /// pre-routing in `main.rs` collapses the two tokens).
    ///
    /// Scans ONLY the four content-addressed store dirs under `--root`
    /// (`memory/agent`, `memory/decisions`, `memory/lessons`, `knowledge`) —
    /// never `spec/{spec}/memory` (the name-addressed per-spec store). A file is
    /// a removal candidate iff it parses but is not
    /// `Knowledge::is_substantive` (the SAME criterion the write/read gates use).
    /// Dry-run by default (lists `would remove: <rel> (<reason>)`); `--apply`
    /// deletes and prints `removed: <rel> (<reason>)`. A substantive record is
    /// never deleted. Byte-stable output, fail-open, never panics.
    #[command(name = "knowledge-prune")]
    KnowledgePrune {
        /// `.claude/` directory whose store to sweep. Taken verbatim, like
        /// `knowledge recall --root`.
        #[arg(long)]
        root: PathBuf,
        /// Delete the candidates. Without it, the command only lists them
        /// (dry-run — nothing is mutated).
        #[arg(long)]
        apply: bool,
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
    /// Analyze file dependencies across waves (topological import DAG).
    ///
    /// Input via `--plan <file>` (preferred — survives the `rtk` wrapper) or
    /// stdin (legacy). Both transports accept BOTH shapes: the derivation form
    /// `{files, projectRoot}` and the rich plan JSON (`{waves: [{files}]}`,
    /// per-wave censuses unioned) that `plan-materialize --plan` consumes.
    WaveDependency {
        /// Path to a JSON file: `{files, projectRoot}` or a `--plan`-style
        /// `{waves: [...]}` document. Omit to read the same JSON from stdin.
        #[arg(long)]
        plan: Option<String>,
    },
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
    /// Suggest wave decomposition by file/entity count.
    ///
    /// With `--from-spec <path>`, computes `fileCount` / `layerCount` /
    /// `newEntityCount` deterministically in Rust from the spec's `## Files`
    /// section + a diff against the repo model's entity names (no LLM). Without
    /// it, reads a pre-computed signals JSON from stdin (legacy / override).
    ScopeDecompose {
        /// Compute the signals deterministically from this spec file instead of
        /// reading them from stdin.
        #[arg(long = "from-spec")]
        from_spec: Option<String>,
    },
    /// Classify a spec's scope (light / extended-light / full) deterministically.
    ///
    /// Reuses the same structural signals as `scope-decompose --from-spec`
    /// (fileCount / layerCount / newEntityCount), plus `--slice-match-count`
    /// from the `feature` digest's `sliceMatchCount`, and encodes the `/feature`
    /// SKILL's prose thresholds in code. Fail-open: an unreadable spec yields
    /// `{"scope":"full",...}` (the conservative default).
    ScopeClassify {
        /// Compute the signals deterministically from this spec file.
        #[arg(long = "from-spec")]
        from_spec: String,
        /// Count of matched recurring slices from the `feature` digest's
        /// `sliceMatchCount` — vocabulary-overlap precedent: >=2 counts toward
        /// full only alongside layer spread (layerCount >= 2); alone it is
        /// precedent evidence for the extended-light band. Defaults to 0.
        #[arg(long = "slice-match-count", default_value_t = 0)]
        slice_match_count: i64,
    },
    /// Fused pre-PLAN decision: `scope-classify` + `scope-decompose` from ONE
    /// signal computation (one spec read, one `scan facts` spawn, one turn).
    /// Returns `{scope, decompose, reason, waves, signals, filesSectionEmpty?}`
    /// — the union the `/feature` PLAN step needs to route, pick 1-vs-N, and
    /// seed `spec-draft --waves`. Replaces calling the two commands in sequence.
    PlanPrepare {
        /// Compute the signals deterministically from this spec file.
        #[arg(long = "from-spec")]
        from_spec: String,
        /// `sliceMatchCount` from the `feature` digest (same meaning as
        /// `scope-classify`). Defaults to 0.
        #[arg(long = "slice-match-count", default_value_t = 0)]
        slice_match_count: i64,
    },
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
    /// text and dispatches to `review::gate_regression_check::run`. Moments 2 and 3
    /// require external `diff` + snapshots that the bare CLI does not
    /// collect today — those moments are exercised via the
    /// `pre_edit_intent_gate` hook and the W5 span-level integration.
    /// Exit code mirrors the verdict: Green/Amber ⇒ 0, Red ⇒ 2.
    #[command(name = "gate-regression-check")]
    GateRegressionCheck {
        /// Spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: String,
        /// Moment to evaluate: 1 (pre-edit), 2 (during diff), 3 (after child return).
        #[arg(long, default_value_t = 1)]
        moment: u8,
        /// W5#3 — wave directory (e.g. `.claude/spec/<spec>/wave-5-rt`) used
        /// only with `--moment 3`. When set, the subcommand inspects that
        /// wave's `_review-spans.md` ledger via
        /// `review::review_spans::check_consolidation` and exits non-zero (2) when any
        /// row registered a red verdict. Lets close-gate scripts invoke the
        /// span-level decision without going through the `SubagentStop` hook.
        #[arg(long = "wave-dir")]
        wave_dir: Option<String>,
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
    /// Scan a project tree for committed secrets + misconfigurations.
    SecurityScan {
        /// Directory to scan. Defaults to the current directory.
        dir: Option<String>,
        /// Emit the machine-readable JSON report.
        #[arg(long)]
        json: bool,
    },
    /// Advisory gate: scan the git diff (working tree + staged, `git diff
    /// HEAD`) for stack-registry literals added to the agnostic surfaces
    /// (`apps/scan/src` / `packages/core/src` `.rs` files). Always exits 0;
    /// the verdict is the `ok` field of the JSON report.
    #[command(name = "hardcode-gate")]
    HardcodeGate,
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
    /// Run the local OTLP/JSON receiver for Claude Code native telemetry.
    ///
    /// Binds a loopback HTTP server on `MUSTARD_OTEL_PORT` (default 4318).
    /// Metrics/logs project into `claude_code_otel` (mustard.db); traces land
    /// span-level token usage as `run_usage` rows in telemetry.db via the
    /// telemetry writer (rows stamped with attribution at write time). Runs
    /// until a shutdown signal — the harness spawns it as a long-lived child
    /// via [`crate::hooks::session::session_start_inject`].
    OtelCollector,
    /// Stop the local OTEL collector for this project.
    ///
    /// Resolves the OTLP port from `MUSTARD_OTEL_PORT` (default 4318), kills
    /// whatever process is listening on it, and deletes the stale
    /// `.otel-collector.pid` file under `<project>/.claude/.harness/`. Killing
    /// by port (not by the drift-prone PID file) is the reliable teardown. Used
    /// by `install.ps1` before a reinstall so the previous daemon releases its
    /// exclusive lock on `mustard-rt.exe`. Fully fail-open; never exits non-zero.
    OtelStop,
    /// Watch `~/.claude/projects/**/*.jsonl` and re-ingest each session
    /// transcript into telemetry.db's `run_usage` table on every change.
    ///
    /// Opt-in daemon (Wave 3 — economia-moat-unification) spawned by
    /// [`crate::hooks::session::session_start_inject`] when `MUSTARD_TRANSCRIPT_WATCH=1`.
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
    /// Fold the active session's events into an `analyze.digest.summary`
    /// adherence report: did the scan digest answer (`analyze.digest.used`)
    /// and how many Read/Grep/Glob heartbeats targeted source files directly
    /// (before the first digest use / in total). Emits the event spec-scoped
    /// and prints the same JSON. Fire-and-forget telemetry: fail-open, no
    /// events means zero counts, always exits 0.
    #[command(name = "digest-adherence-finalize")]
    DigestAdherenceFinalize {
        /// Spec slug the summary event attributes to.
        #[arg(long)]
        spec: String,
    },
    // The folder name is spelled `wave-<n>-<role>` (angle brackets) throughout
    // this doc comment: a literal brace-n sequence is a clap help-template
    // token (forced line break) and would mangle the rendered --help.
    /// Materialise the canonical SDD wave layout from a declarative JSON plan.
    ///
    /// Renders `wave-plan.md` + each `wave-<n>-<role>/spec.md` (+ `meta.json`
    /// sidecars). Idempotent: existing files are never overwritten.
    ///
    /// Every entry in `waves` REQUIRES two fields (no serde default — omitting
    /// either is a parse error: stdout gains `error` + `hint`, exit 2):
    ///   - `n: u32`       — 1-based wave number, drives the folder name
    ///                      `wave-<n>-<role>`.
    ///   - `role: String` — role label (`general`, `backend`, …), the other
    ///                      half of the folder name.
    ///
    /// Minimal valid plan JSON:
    ///   {
    ///     "waves": [
    ///       { "n": 1, "role": "general", "summary": "…", "depends_on": [] },
    ///       { "n": 2, "role": "general", "summary": "…",
    ///         "depends_on": ["wave-1-general"] }
    ///     ],
    ///     "total_waves": 2,
    ///     "lang": "pt-BR"
    ///   }
    ///
    /// Only the per-wave BODY fields are optional (`#[serde(default)]` — a
    /// summary-only plan still deserialises):
    ///   - `tasks: [String]`      → `## Tasks`/`## Tarefas` (`- [ ] {task}`) in
    ///                              the wave spec, read back by
    ///                              `agent-prompt-render` as `{task_steps}`.
    ///   - `files: [String]`      → `## Files`/`## Arquivos` (`` - `{path}` ``),
    ///                              read back as `{reference_files}`.
    ///   - `acceptance: [String]` → NOT in the wave spec; the union across waves
    ///                              is carried into `wave-plan.md` under
    ///                              `## Acceptance Criteria`/`## Critérios de
    ///                              Aceitação`, where the QA gate reads it.
    /// The Plan agent authors these arrays; the body is never hand-written after
    /// the scaffold. Headings render in the effective language
    /// (`mustard.json#specLang` root-wins, plan `lang` as fallback). A wave with
    /// no `tasks` emits a stderr WARN (visible signal), not a bare heading.
    ///
    /// `plan-from-spec` is the canonical producer of the plan skeleton (it
    /// emits every required + body field); inside the pipeline prefer
    /// `plan-materialize`, which composes this scaffold with validation and
    /// the PLAN-phase events.
    #[command(verbatim_doc_comment)]
    WaveScaffold {
        /// Target spec directory.
        #[arg(long = "spec-dir")]
        spec_dir: Option<String>,
        /// Path to the plan JSON file.
        #[arg(long)]
        plan: Option<String>,
    },
    /// Deterministically merge a wave-plan's decomposition back down — the
    /// "reject decomposition" branch of `approve-only-flow.md`.
    ///
    /// `--mode full`: collapse N waves into a single `wave-1-{role}/spec.md`
    /// (parent root spec stays the orchestration doc), delete `wave-2..N`,
    /// patch `wave-plan.md` + parent `meta.json` to `totalWaves:1` /
    /// `isWavePlan:true` (NEVER zero waves for Full — the invariant).
    /// `--mode light`: merge every wave's sections into the root `spec.md`,
    /// delete all wave dirs + `wave-plan.md`, patch root `meta.json` to
    /// `isWavePlan:false`. Both set `scopeOverride:"user-rejected-waves"`.
    /// Atomic + idempotent + fail-open: a missing `wave-plan.md` →
    /// `{"ok":false,"reason":"no-wave-plan"}` (exit 0). Merged spec is written
    /// BEFORE any dir is deleted. Reuses `is_heading` / `write_atomic` /
    /// the wave-scaffold renderers.
    #[command(name = "wave-collapse")]
    WaveCollapse {
        /// Spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: String,
        /// Collapse mode: `full` (→ single wave-1) or `light` (→ single root spec).
        #[arg(long)]
        mode: String,
    },
    /// W10.T10.4 — Emit a deterministic wave-plan JSON consumable by
    /// `wave-scaffold`. Replaces the orchestrator-hand-rolled `plan.json` step.
    ///
    /// Emits the per-wave body fields (`tasks` / `files` / `acceptance`) always,
    /// even empty, so the JSON is a self-documenting skeleton: the deterministic
    /// role/dependency scaffold the Plan agent then folds the real body lines
    /// into before handing the plan to `wave-scaffold` (which materialises them
    /// — see [`WaveScaffold`]).
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
    /// Audit source files for pt-BR prose in EN-only files (diacritic-seed
    /// heuristic). Warn-only by default; `--strict` exits `1` on any hit.
    LanguageAudit {
        /// Output format: `text` (default) or `json`.
        #[arg(long, default_value = "text")]
        format: String,
        /// Exit `1` when any hit is found. Default is warn-only (exit `0`).
        #[arg(long)]
        strict: bool,
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
    /// last build result, repo-model summary (grain.model.json).
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
    /// Wave-routing face of the orchestrator. Reads the spec's `wave-plan.md`,
    /// builds the wave dependency DAG, and emits a deterministic JSON array
    /// ordered by dependency level — one item per agent, each carrying
    /// `{wave, role, subproject, depends_on, level, prompt_cmd, subagent_type}`.
    /// `prompt_cmd`
    /// is a ready `agent-prompt-render` invocation: the orchestrator runs it
    /// and relays the stdout to `Task`. Determines the dispatch order in Rust
    /// so the LLM stops interpreting the wave-plan by hand. Fail-open: a
    /// non-wave / unparseable spec degrades to `[]`; exit 0 always.
    #[command(name = "dispatch-plan")]
    DispatchPlan {
        /// Spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: String,
        /// Restrict the emitted array to a single wave (still carrying its real
        /// `depends_on` / `level`). Omit to emit the whole plan.
        #[arg(long)]
        wave: Option<u32>,
    },
    /// Render the agent dispatch prompt server-side from the embedded
    /// template. Substitutes every `{placeholder}` it can resolve; warns on
    /// stderr for any left unfilled. Stdout = raw prompt string ready for
    /// the Task tool (no JSON framing).
    AgentPromptRender {
        /// Spec slug under `.claude/spec/`. Optional: spec-less callers (the
        /// `/scan` Guards enrich step, `/task` with no scope) omit it — the
        /// renderer then derives the locale from `mustard.json#specLang` and
        /// fail-opens every spec-keyed lookup to an empty value.
        #[arg(long)]
        spec: Option<String>,
        /// Wave number (0-based, matching `wave-N-*` directories). Omitted for non-wave specs.
        #[arg(long)]
        wave: Option<u32>,
        /// Agent role token (e.g. `ui`, `backend`).
        #[arg(long)]
        role: String,
        /// Subproject path relative to the project root (e.g. `apps/dashboard`).
        /// Defaults to `.` (the project root) so a dispatch that is not scoped
        /// to a subproject never costs the orchestrator a usage-error
        /// round-trip — the render then reads the root `CLAUDE.md` Guards.
        #[arg(long, default_value = ".")]
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
        /// Ad-hoc task text for spec-less dispatch (the `/scan` Guards enrich
        /// step, `/task` with no scope). Fills the `## TASK` block when there is
        /// no spec `## Tasks` to read, so the prompt stays self-contained and the
        /// orchestrator never hand-appends the task after the render.
        #[arg(long = "task-text")]
        task_text: Option<String>,
        /// Emit mode: `inline` (default) prints the full rendered prompt;
        /// `ref` writes it to a deterministic `.dispatch/` file and prints a
        /// 2-line stub instead — pass the stub verbatim as the Task prompt
        /// and the PreToolUse hook expands it, so the full text never
        /// transits the orchestrator's context.
        #[arg(long, default_value = "inline")]
        emit: String,
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
        #[arg(long = "age-days", default_value_t = maint::worktree_gc::DEFAULT_AGE_DAYS)]
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
    /// Draft a new spec layout (`spec.md` + `meta.json`) conforming to
    /// `mustard_core::domain::spec::contract`. Replaces the literal ~80-line
    /// template block inside the `/mustard:feature` SKILL.md.
    ///
    /// `spec-draft` materialises ONLY the top-level `spec.md` + `meta.json`
    /// (recording `scope`/`totalWaves`/`isWavePlan`); full-scope wave dirs are
    /// materialised by `wave-scaffold`. `--lang` accepts BCP-47 only (`pt-BR` /
    /// `en-US`); short codes are rejected. `--signals` is a free-form
    /// comma-separated list embedded in `spec.md` as a comment.
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
        /// Waves recorded in `meta.json#totalWaves` under Full scope (default 1).
        /// The wave dirs themselves are materialised by `wave-scaffold`.
        #[arg(long, default_value_t = 1)]
        waves: u32,
        /// Overwrite an existing output directory.
        #[arg(long)]
        force: bool,
        /// Comma-separated repo-vocabulary terms for the internal Context
        /// enrichment query — pass the terms that produced a strong digest
        /// report during ANALYZE. Omitted: the raw intent is tokenised (a
        /// translated intent then repeats the weak query and the enrichment
        /// withholds itself).
        #[arg(long = "query-terms")]
        query_terms: Option<String>,
    },
    /// Compile the deterministic spec draft for one entity via `grain spec` and
    /// print the resulting Markdown verbatim to stdout. Thin passthrough to
    /// `mustard_core::domain::scan::Scan::spec`. Invoke as
    /// `mustard-rt run scan spec --entity <Name>`.
    #[command(name = "scan-spec")]
    ScanSpec {
        /// Entity/unit to create (substitutes `<Name>` in the grain recipe).
        #[arg(long)]
        entity: String,
        /// Existing sibling to mirror; omit for auto-pick.
        #[arg(long)]
        like: Option<String>,
        /// Extra operations beyond the base vertical (comma-separated, e.g. `approve,cancel`).
        #[arg(long, value_delimiter = ',')]
        ops: Vec<String>,
        /// Cross-cutting invariants the unit must obey (repeatable).
        #[arg(long)]
        invariant: Vec<String>,
        /// Workspace root (must contain `.claude/grain.model.json`).
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Enumerate every subproject `CLAUDE.md` whose `## Guards` block is still
    /// `pending` (the Wave-2 enrich hand-off seeded by `scan --full`). Emits a
    /// JSON array `[{path, subproject, kind, frameworks}]` parsed from each
    /// block's facts comment. Excludes the workspace-root unit. Fail-open: any
    /// IO error degrades to `[]` and exit 0.
    #[command(name = "scan-guards-list")]
    ScanGuardsList {
        /// Workspace root to walk. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Splice the enrich agent's authored guards into a subproject
    /// `CLAUDE.md`'s pending `## Guards` block: non-destructive (only the span
    /// between the markers changes), line-capped, and idempotent (the marker
    /// flips to its non-pending form so a re-run of `scan-guards-list` skips
    /// it). Refuses the workspace-root `CLAUDE.md`.
    #[command(name = "scan-guards-apply")]
    ScanGuardsApply {
        /// Path to the subproject `CLAUDE.md` to enrich.
        #[arg(long)]
        path: PathBuf,
        /// Workspace root the scan ran from. Used to classify whether `path` is
        /// the root unit (refused) or a nested subproject (spliced), via the
        /// same `subproject_of` rule `scan-guards-list` uses. Defaults to `.`.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Authored guard text, or `-` to read it from stdin. `allow_hyphen_values`
        /// so a body starting with a `-` bullet is not mistaken for a flag.
        #[arg(long, default_value = "-", allow_hyphen_values = true)]
        guards: String,
    },
    /// Validate a spec directory against the Wave 1 layout contract. Reads
    /// `meta.json` + `spec.md` and runs `mustard_core::domain::spec::contract::validate`.
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
        #[arg(long = "age-days", default_value_t = spec::spec_clear::DEFAULT_AGE_DAYS)]
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
    /// Emit the deterministic spec-approval event sequence (replaces the
    /// hand-assembled `emit-pipeline` steps in `approve-only-flow.md`).
    ///
    /// Emits, in order: `pipeline.stage {stage:"Plan"}` → `pipeline.status
    /// {from:"draft",to:"approved"}`, and — only with `--resume` — a trailing
    /// `pipeline.stage {stage:"Execute"}` (the `r`-suffix inline-resume case).
    /// With `--wave-plan`, the stage payloads carry `wave:1` so the wave-1
    /// `meta.json` sidecar is patched for dispatch. Reuses the canonical
    /// `emit-pipeline` internals (no subprocess). Prints a JSON report; exit 0.
    #[command(name = "approve-spec")]
    ApproveSpec {
        /// Spec slug under `.claude/spec/` to approve.
        #[arg(long)]
        spec: String,
        /// The spec is a wave plan — patch the wave-1 `meta.json` for dispatch.
        #[arg(long = "wave-plan")]
        wave_plan: bool,
        /// Inline-resume: also emit `pipeline.stage Execute` (the `r`-suffix
        /// branch). Without it, the flow stops at `approved` for a fresh session.
        #[arg(long)]
        resume: bool,
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
    /// F4-c item 4 — Propose (do NOT create) tactical fixes from structured
    /// `tactical_fix_candidates[]` in a spec's `review.result` / `qa.result`
    /// events. Emits one `tactical_fix.proposed` event per new candidate;
    /// never scaffolds a sub-spec (decision 6 — "não auto-aprovar").
    #[command(name = "tactical-fix-detect")]
    TacticalFixDetect {
        /// Spec whose review/qa events are scanned for candidates.
        #[arg(long)]
        spec: Option<String>,
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
    /// divergent files. Generated artefacts (`grain.model.json`, caches)
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
    ///
    /// The cache key (`rootCauseHash`) is computed **deterministically in Rust**
    /// from the affected files + the error message (`--files` + `--error`); the
    /// `/bugfix` ANALYZE step no longer has to hand a hash to the binary. An
    /// explicit `--hash` still works (override / legacy-key compat) and takes
    /// priority when supplied.
    #[command(name = "bugfix-cache")]
    BugfixCache {
        /// Cache signature hash — explicit override. When omitted, the hash is
        /// computed deterministically from `--error` + `--files`.
        #[arg(long)]
        hash: Option<String>,
        /// Error message / failure signature — drives the deterministic hash
        /// when `--hash` is not supplied.
        #[arg(long)]
        error: Option<String>,
        /// Write mode — record a new entry with the supplied summary.
        #[arg(long)]
        summary: Option<String>,
        /// Files affected — comma-separated list (write mode AND hash input).
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
    /// W5.T5.16 — Consolidate per-phase prelude (diff-context snapshot).
    #[command(name = "pipeline-prelude")]
    PipelinePrelude {
        /// Spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: String,
        /// Phase: `ANALYZE` / `PLAN` / `EXECUTE`.
        #[arg(long)]
        phase: String,
    },
    /// Composite PLAN materialisation: wave-scaffold + analyze-validation +
    /// `pipeline.scope` (full) + `pipeline.phase` PLAN, all in-process.
    /// Pressupposes `spec.md`/`meta.json` already drafted by `spec-draft`.
    /// Output: `{"events":[...],"scaffold":{created_files,skipped},`
    /// `"validation":{ok,issues}}` — byte-stable, ordered.
    #[command(name = "plan-materialize")]
    PlanMaterialize {
        /// Target spec directory.
        #[arg(long = "spec-dir")]
        spec_dir: String,
        /// Path to the plan JSON file.
        #[arg(long)]
        plan: String,
    },
    /// Composite dispatch face: dispatch-plan + inline agent-prompt-render for
    /// the NEXT pending wave level. Emits
    /// `[{wave, role, subproject, subagent_type, prompt}]` with the prompt
    /// text ready for `Task`. Pending = first dependency level with a wave not
    /// yet carrying `pipeline.wave.complete`; everything done → `[]`.
    #[command(name = "wave-advance")]
    WaveAdvance {
        /// Spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: String,
    },
    /// Composite CLOSE face: list `review.result` verdicts (advisory), run QA
    /// in-process, and — only with QA `overall=pass` — finalize via
    /// complete-spec + render pipeline-summary. QA fail/skip →
    /// `completed:false` with the reproved ACs, spec NOT closed (the
    /// `pipeline.complete` QA gate stays the authority — no bypass).
    /// Output: `{"completed":bool,"qa":{overall,criteria},"reviews":[...],`
    /// `"summary":...}`.
    #[command(name = "close-pipeline")]
    ClosePipeline {
        /// Spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: String,
    },
}

/// Dispatch a `run` subcommand.
///
/// Unlike the enforcement dispatcher this never touches stdin and never
/// produces an [`Outcome`](mustard_core::domain::model::contract::Outcome) — a `run`
/// script writes its own output and the process exits cleanly afterwards.
pub fn dispatch(cmd: RunCmd) {
    match cmd {
        RunCmd::Scan { root, out, full } => scan::run(&root, out.as_deref(), full),
        RunCmd::Feature { intent, root } => feature::run(&intent, &root),
        RunCmd::GlossaryCoverage {
            intent,
            context,
            root,
        } => glossary_coverage::run(&intent, &context, &root),
        RunCmd::DigestPrecision { root } => digest_precision::run(&root),
        RunCmd::LexiconSuggest { accept, root } => lexicon_suggest::run(accept.as_deref(), &root),
        RunCmd::LexiconEnrich { check, apply, root } => lexicon_enrich::run(check, apply.as_deref(), &root),
        RunCmd::DiffContext {
            parent,
            subproject,
            phase,
        } => pipeline::diff_context::run(parent.as_deref(), subproject.as_deref(), phase.as_deref()),
        RunCmd::EmitEvent {
            event,
            payload,
            spec,
            wave,
        } => event::emit_event::run(event.as_deref(), &payload, spec.as_deref(), wave),
        RunCmd::EmitPhase { spec, to, from } => {
            event::emit_phase::run(&spec, &to, from.as_deref());
        }
        RunCmd::EmitPipeline { kind, spec, payload, allow_no_qa } => {
            event::emit_pipeline::run(event::emit_pipeline::EmitPipelineOpts {
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
            migrate::migrate_spec_headers::run(migrate::migrate_spec_headers::MigrateOpts {
                apply,
                root,
                log,
                filter,
            });
        }
        RunCmd::MigrateToMeta { root, force, strip_headers } => {
            migrate::migrate_to_meta::run(migrate::migrate_to_meta::MigrateToMetaOpts {
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
        } => spec::complete_spec::run(spec.as_deref(), archive, archive_stale, archive_followups),
        RunCmd::ContextSlice {
            context,
            spec,
            context_claude_md,
        } => economy::context_slice::run(
            &context,
            spec.as_deref(),
            context_claude_md.as_deref(),
        ),
        RunCmd::KnowledgeRecall {
            query,
            scope,
            root,
            max,
        } => {
            // Parse the optional scope string; a malformed value is a usage
            // error (exit 2) rather than a silent widen to all-scopes.
            let parsed_scope = match scope.as_deref().map(knowledge::recall_cli::parse_scope) {
                None => None,
                Some(Ok(s)) => Some(s),
                Some(Err(msg)) => {
                    eprintln!("error: {msg}");
                    std::process::exit(2);
                }
            };
            knowledge::recall_cli::run(knowledge::recall_cli::RecallOpts {
                query,
                scope: parsed_scope,
                root,
                max,
            });
        }
        RunCmd::KnowledgePrune { root, apply } => {
            knowledge::prune::run(knowledge::prune::PruneOpts { root, apply });
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
        } => knowledge::memory::dispatch(
            &subcommand,
            json.as_deref(),
            spec.as_deref(),
            wave,
            agent.as_deref(),
            summary.as_deref(),
            files.as_deref(),
            grouped,
            &format,
            knowledge::memory::DispatchExtras {
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
            knowledge::memory_ingest::run_with(knowledge::memory_ingest::MemoryIngestOpts { delete, agent_memory });
        }
        RunCmd::PipelineStateIngest { delete: _ } => {
            pipeline::pipeline_state_ingest::run(pipeline::pipeline_state_ingest::PipelineStateIngestOpts);
        }
        RunCmd::EpicFold { detect, epic } => wave::epic_fold::run(detect, epic.as_deref()),
        RunCmd::SpecExtract {
            spec,
            wave,
            ac,
            measure,
        } => spec::spec_extract::run(&spec, wave, ac, measure),
        RunCmd::SpecLink {
            parent,
            child,
            reason,
        } => spec::spec_link::run(parent.as_deref(), child.as_deref(), reason.as_deref()),
        RunCmd::SpecChildren { parent } => spec::spec_children::run(parent.as_deref()),
        RunCmd::SpecChildrenTree { spec } => spec::spec_children_tree::run(spec.as_deref()),
        RunCmd::AnalyzeValidation { spec } => review::analyze_validation::run(spec.as_deref()),
        RunCmd::MarkChecklistItem {
            spec,
            item,
            line,
            cwd,
        } => checklist::mark_checklist_item::run(spec.as_deref(), item.as_deref(), line, cwd.as_deref()),
        RunCmd::WaveTree { spec_dir, format } => wave::wave_tree::run(&spec_dir, &format),
        RunCmd::WaveDependency { plan } => wave::wave_dependency::run(plan.as_deref()),
        RunCmd::WaveFiles { spec, wave } => wave::wave_files::run(spec.as_deref(), wave),
        RunCmd::ScopeDecompose { from_spec } => spec::scope_decompose::run(from_spec.as_deref()),
        RunCmd::ScopeClassify {
            from_spec,
            slice_match_count,
        } => spec::scope_decompose::run_classify(&from_spec, slice_match_count),
        RunCmd::PlanPrepare {
            from_spec,
            slice_match_count,
        } => spec::scope_decompose::run_prepare(&from_spec, slice_match_count),
        RunCmd::ExecRewaveCheck { spec } => wave::exec_rewave_check::run(spec.as_deref()),
        RunCmd::DependencyPrecheck { spec, subproject } => {
            review::dependency_precheck::run(spec.as_deref(), subproject.as_deref());
        }
        RunCmd::WaveSizeCheck { spec_dir } => wave::wave_size_check::run(spec_dir.as_deref()),
        RunCmd::GateRegressionCheck {
            spec,
            moment,
            wave_dir,
        } => {
            use crate::commands::review::gate_regression_check::{GateInput, Moment};
            // W5#3: Moment-3 + --wave-dir path consults the on-disk
            // `_review-spans.md` ledger via `review::review_spans::check_consolidation`.
            // Exits 0 when consolidation is allowed (no red rows) and 2 when
            // blocked. This is the close-gate path; ledger lives on disk so
            // we don't need diff + snapshots in argv.
            if moment == 3 {
                if let Some(wd) = wave_dir {
                    use crate::commands::review::review_spans::{check_consolidation, ConsolidationCheck};
                    let path = std::path::PathBuf::from(wd);
                    match check_consolidation(&path) {
                        ConsolidationCheck::Allowed => std::process::exit(0),
                        ConsolidationCheck::Blocked { .. } => std::process::exit(2),
                    }
                }
            }
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
            match review::gate_regression_check::run(input, moment_enum) {
                Ok(_) => std::process::exit(0),
                Err(_) => std::process::exit(2),
            }
        }
        RunCmd::QaRun { spec, format } => review::qa_run::run(&spec, &format),
        RunCmd::QaRunAll => review::qa_run_all::run(),
        RunCmd::RebuildSpecs => spec::rebuild_specs::run(),
        RunCmd::Metrics {
            subcommand,
            args,
            format,
        } => economy::metrics::run(subcommand.as_deref(), &args, &format),
        RunCmd::MetricsWaveStatus { spec } => {
            let mut argv: Vec<String> = Vec::new();
            if let Some(s) = spec {
                argv.push("--spec".to_string());
                argv.push(s);
            }
            economy::metrics_wave_status::run(&argv);
        }
        RunCmd::EventProjections {
            view,
            spec,
            wave,
            format,
        } => event::event_projections::run(view.as_deref(), spec.as_deref(), wave, &format),
        RunCmd::VerifyPipeline { format } => pipeline::verify_pipeline::run(&format),
        RunCmd::PipelineSummary { spec_dir, format, self_test } => {
            pipeline::pipeline_summary::run(spec_dir.as_deref(), &format, self_test);
        }
        RunCmd::ReviewResult {
            spec,
            verdict,
            critical,
            subproject,
        } => review::review_result::run(spec.as_deref(), verdict.as_deref(), critical, subproject.as_deref()),
        RunCmd::Statusline { preview } => statusline::run(preview),
        RunCmd::SecurityScan { dir, json } => review::security_scan::run(dir.as_deref(), json),
        RunCmd::HardcodeGate => review::hardcode_gate::run(),
        RunCmd::VerifyEmit {
            event,
            since,
            payload_key,
            payload_value,
            spec,
            quiet,
        } => event::verify_emit::run(
            event.as_deref(),
            since.as_deref(),
            payload_key.as_deref(),
            payload_value.as_deref(),
            spec.as_deref(),
            quiet,
        ),
        RunCmd::RtkGain => economy::rtk_gain::run(),
        RunCmd::OtelCollector => economy::otel::collector::run(),
        RunCmd::OtelStop => economy::otel::stop::run(),
        RunCmd::TranscriptWatcher { once } => economy::transcript_watcher::run(once),
        RunCmd::DiagnoseOtel {
            json,
            expect_rows_after,
        } => economy::otel::diagnose::run(json, expect_rows_after.as_deref()),
        RunCmd::Doctor { residue, check, format, json } => {
            // `--json` is a shorthand for `--format json` (W10.T10.6).
            let effective_format = if json { "json".to_string() } else { format };
            doctor::doctor::run(doctor::doctor::DoctorOpts {
                residue,
                check,
                format: effective_format,
            });
        }
        RunCmd::DocsStaleCheck { from, strict, include_nested } => {
            doctor::docs_stale_check::run(from.as_deref(), strict, include_nested);
        }
        RunCmd::LanguageAudit { format, strict } => {
            doctor::language_audit::run(doctor::language_audit::LanguageAuditOpts { format, strict });
        }
        RunCmd::ArtifactUpdate {
            check,
            apply,
            manifest,
        } => maint::artifact_update::run(check, apply, manifest.as_deref()),
        RunCmd::AmendFinalize { session_id } => agent::amend_finalize::run_cli(&session_id),
        RunCmd::DigestAdherenceFinalize { spec } => agent::digest_adherence_finalize::run(&spec),
        RunCmd::WaveScaffold { spec_dir, plan } => {
            wave::wave_scaffold::run(spec_dir.as_deref(), plan.as_deref());
        }
        RunCmd::WaveCollapse { spec, mode } => {
            wave::wave_collapse::run(wave::wave_collapse::WaveCollapseOpts { spec, mode });
        }
        RunCmd::PlanFromSpec { waves, roles, lang, summary } => {
            spec::plan_from_spec::run(spec::plan_from_spec::PlanFromSpecOpts {
                waves,
                roles,
                lang,
                summary,
            });
        }
        RunCmd::ActiveSpecs { format, root } => {
            spec::active_specs::run(spec::active_specs::ActiveSpecsOpts { format, root });
        }
        RunCmd::Status { harness, format, root } => {
            pipeline::status::run(pipeline::status::StatusOpts { harness, format, root });
        }
        RunCmd::ReviewPrefetch { pr_ref, format, root } => {
            let pr_ref = pr_ref.unwrap_or_default();
            if pr_ref.is_empty() {
                println!("{}",
                    serde_json::to_string_pretty(&serde_json::json!({"error":"pr-ref-required"}))
                        .unwrap_or_default()
                );
            } else {
                review::review_prefetch::run(review::review_prefetch::ReviewPrefetchOpts { pr_ref, format, root });
            }
        }
        RunCmd::ResumeBootstrap { spec, json } => pipeline::resume_bootstrap::run(&spec, json),
        RunCmd::DispatchPlan { spec, wave } => pipeline::dispatch_plan::run(&spec, wave),
        RunCmd::AgentPromptRender {
            spec,
            wave,
            role,
            subproject,
            mode,
            retry_context_file,
            task_filter,
            task_text,
            emit,
        } => agent::agent_prompt_render::run(
            spec.as_deref(),
            wave,
            &role,
            &subproject,
            agent::agent_prompt_render::RenderMode::parse(&mode),
            retry_context_file.as_deref(),
            task_filter.as_deref(),
            task_text.as_deref(),
            agent::agent_prompt_render::EmitMode::parse(&emit),
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
            maint::worktree_gc::run(maint::worktree_gc::WorktreeGcOpts {
                repo,
                age_days,
                apply,
            });
        }
        RunCmd::Unhook { repo, scope, confirm } => {
            maint::unhook::run(maint::unhook::UnhookOpts { repo, scope, confirm });
        }
        RunCmd::Rehook { repo, scope, confirm } => {
            maint::rehook::run(maint::rehook::RehookOpts { repo, scope, confirm });
        }
        RunCmd::SpecDraft {
            intent,
            scope,
            lang,
            signals,
            output,
            waves,
            force,
            query_terms,
        } => {
            spec::spec_draft::run(spec::spec_draft::SpecDraftOpts {
                intent,
                scope,
                lang,
                signals,
                output,
                waves,
                force,
                query_terms,
            });
        }
        RunCmd::ScanSpec { entity, like, ops, invariant, root } => {
            spec::scan_spec::run(spec::scan_spec::ScanSpecOpts {
                entity,
                like,
                ops,
                invariants: invariant,
                root,
            });
        }
        RunCmd::ScanGuardsList { root } => scan_guards::list::run(&root),
        RunCmd::ScanGuardsApply { path, root, guards } => {
            scan_guards::apply::run(&path, &root, &guards)
        }
        RunCmd::SpecValidate { spec, json } => {
            let _ = json; // currently always emits JSON
            spec::spec_validate::run(std::path::Path::new(&spec), true);
        }
        RunCmd::SpecMemory {
            subcommand,
            spec,
            name,
            kind,
            origin_wave,
            description,
        } => {
            spec::spec_memory::dispatch(
                subcommand.as_deref(),
                spec::spec_memory::SpecMemoryCreateOpts {
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
            spec::spec_clear::run(spec::spec_clear::SpecClearOpts {
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
            maint::claude_dir_prune::run(maint::claude_dir_prune::ClaudeDirPruneOpts {
                repo,
                apply,
                json,
            });
        }
        // --- W5 deep-refactor: T5.1–T5.16 -------------------------------------
        RunCmd::CloseOrchestrate { spec, skip_docs } => {
            pipeline::close_orchestrate::run(pipeline::close_orchestrate::CloseOrchestrateOpts { spec, skip_docs });
        }
        RunCmd::ReviewDispatch { pr, spec, subproject } => {
            review::review_dispatch::run(review::review_dispatch::ReviewDispatchOpts { pr, spec, subproject });
        }
        RunCmd::ApproveSpec { spec, wave_plan, resume } => {
            spec::approve_spec::run(spec::approve_spec::ApproveSpecOpts {
                spec,
                wave_plan,
                resume,
            });
        }
        RunCmd::TacticalFixCreate { parent, description, scope } => {
            spec::tactical_fix_create::run(spec::tactical_fix_create::TacticalFixOpts {
                parent,
                description,
                scope,
            });
        }
        RunCmd::TacticalFixDetect { spec } => {
            spec::tactical_fix_detect::run(spec.as_deref());
        }
        RunCmd::PrdBuild { intent, format } => {
            spec::prd_build::run(spec::prd_build::PrdBuildOpts { intent, format });
        }
        RunCmd::AdaptCursor { repo, dry_run } => {
            maint::adapt_cursor::run(maint::adapt_cursor::AdaptCursorOpts { repo, dry_run });
        }
        RunCmd::RefreshClaude { target, dry_run, templates_dir } => {
            maint::refresh_claude::run(maint::refresh_claude::RefreshClaudeOpts {
                target,
                dry_run,
                templates_dir,
            });
        }
        RunCmd::MaintDeps { dry_run } => {
            maint::maint_deps::run(maint::maint_deps::MaintDepsOpts { dry_run });
        }
        RunCmd::MaintValidate { dry_run } => {
            maint::maint_validate::run(maint::maint_validate::MaintValidateOpts { dry_run });
        }
        RunCmd::TaskChecklist { domain } => {
            checklist::task_checklist::run(checklist::task_checklist::TaskChecklistOpts { domain });
        }
        RunCmd::BugfixCache { hash, error, summary, files } => {
            review::bugfix_cache::run(review::bugfix_cache::BugfixCacheOpts { hash, error, summary, files });
        }
        RunCmd::ContextBudget { role, spec, wave } => {
            economy::context_budget::run(economy::context_budget::ContextBudgetOpts { role, spec, wave });
        }
        RunCmd::BackupSpecs {
            target,
            filter,
            dry_run,
            no_manifest,
        } => {
            spec::backup_specs::run(spec::backup_specs::BackupSpecsOpts {
                target,
                filter,
                dry_run,
                no_manifest,
            });
        }
        RunCmd::I18n { subcommand, from, to_lang } => {
            match subcommand.as_str() {
                "translate-heading" => i18n::i18n_translate::run(i18n::i18n_translate::TranslateHeadingOpts {
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
                "resolve" => spec::spec_lang_resolve::run(spec::spec_lang_resolve::SpecLangResolveOpts {
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
            "capture-baseline" => economy::economy_capture_baseline::run(
                economy::economy_capture_baseline::CaptureBaselineOpts {
                    operation: operation.unwrap_or_default(),
                    wave: wave.unwrap_or(0),
                    from_history,
                    spec: spec.clone(),
                },
            ),
            "reconcile" => economy::economy_reconcile::run(economy::economy_reconcile::ReconcileOpts {
                wave: wave.unwrap_or(0),
                spec: spec.clone(),
            }),
            "report" => economy::economy_report::run(economy::economy_report::ReportOpts {
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
            pipeline::pipeline_prelude::run(pipeline::pipeline_prelude::PreludeOpts { spec, phase });
        }
        RunCmd::PlanMaterialize { spec_dir, plan } => {
            pipeline::plan_materialize::run(pipeline::plan_materialize::PlanMaterializeOpts {
                spec_dir,
                plan,
            });
        }
        RunCmd::WaveAdvance { spec } => pipeline::wave_advance::run(&spec),
        RunCmd::ClosePipeline { spec } => pipeline::close_pipeline::run(&spec),
        RunCmd::SpecStatusBackfill { source, dry_run, spec } => {
            spec::spec_status_backfill::run_cli(spec::spec_status_backfill::BackfillOpts {
                source,
                dry_run,
                spec,
                cwd: None,
            });
        }

    }
}
