//! The `run` subcommands for the spec lifecycle (`spec/`).
//!
//! TWO registrations per command, both in this file: the variant in
//! [`SpecCmd`] AND its arm in [`dispatch`] below. Forgetting the second
//! still compiles, but the command vanishes from the CLI.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run spec <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::{spec};

/// The `run` subcommands owned by the spec lifecycle (`spec/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum SpecCmd {
    /// Finalize a pipeline spec — single-stage close straight to `completed`.
    #[command(display_order = 13)]
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
    /// Cut a single wave slice (or AC block) from a `spec.md`.
    #[command(display_order = 18)]
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
    #[command(display_order = 19)]
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
    #[command(display_order = 20)]
    SpecChildren {
        /// Parent (epic) spec slug whose children to enumerate.
        #[arg(long)]
        parent: Option<String>,
    },
    /// Project a parent spec's waves + acceptance criteria + sub-specs into a
    /// single JSON document. Consumed by the dashboard's `spec_children_tree`
    /// Tauri command (Wave 3 of `spec-lifecycle-unification`). Fail-open: a
    /// missing spec or store degrades to empty arrays.
    #[command(display_order = 21)]
    SpecChildrenTree {
        /// Parent spec slug under `.claude/spec/` (flat layout).
        #[arg(long)]
        spec: Option<String>,
    },
    /// Suggest wave decomposition by file/entity count.
    ///
    /// With `--from-spec <path>`, computes `fileCount` / `layerCount` /
    /// `newEntityCount` deterministically in Rust from the spec's `## Files`
    /// section + a diff against the repo model's entity names (no LLM). Without
    /// it, reads a pre-computed signals JSON from stdin (legacy / override).
    #[command(display_order = 27)]
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
    #[command(display_order = 28)]
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
    #[command(display_order = 29)]
    PlanPrepare {
        /// Compute the signals deterministically from this spec file.
        #[arg(long = "from-spec")]
        from_spec: String,
        /// `sliceMatchCount` from the `feature` digest (same meaning as
        /// `scope-classify`). Defaults to 0.
        #[arg(long = "slice-match-count", default_value_t = 0)]
        slice_match_count: i64,
    },
    /// Rematerialise the denormalised `specs` + `metrics_projection` tables
    /// from the event stream. Closes the gap the eliminate-bun migration
    /// opened: pre-2026-05-20 nothing populated those tables since the JS
    /// harness writer was removed, which is why every dashboard spec card
    /// fell back to `"unknown"`.
    #[command(display_order = 36)]
    RebuildSpecs,
    /// W10.T10.4 — Emit a deterministic wave-plan JSON consumable by
    /// `wave-scaffold`. Replaces the orchestrator-hand-rolled `plan.json` step.
    ///
    /// Emits the per-wave body fields (`tasks` / `files` / `acceptance`) always,
    /// even empty, so the JSON is a self-documenting skeleton: the deterministic
    /// role/dependency scaffold the Plan agent then folds the real body lines
    /// into before handing the plan to `wave-scaffold` (which materialises them
    /// — see [`WaveScaffold`]).
    #[command(name = "plan-from-spec")]
    #[command(display_order = 57)]
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
    /// Discover active specs from the filesystem (Outcome=Active, Stage=Plan|Execute).
    ///
    /// Replaces the LLM-side glob/grep loop in `/mustard:spec`: reads
    /// `.claude/spec/*/spec.md` directly, filters headers, counts wave
    /// progress, extracts a one-line resumo.
    /// Output is either a markdown table (default) or a JSON document.
    #[command(display_order = 61)]
    ActiveSpecs {
        /// Output format: `table` (default) or `json`.
        #[arg(long, default_value = "table")]
        format: String,
        /// Project root directory (default: current working directory).
        #[arg(long, default_value = ".")]
        root: PathBuf,
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
    #[command(display_order = 70)]
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
        /// Honour the requested `--scope full` even when the deterministic
        /// routing gate would auto-rebaixar it to light/extended-light. The
        /// override is recorded (a `pipeline.scope.override` event) so it is
        /// auditable, never silent.
        #[arg(long = "force-scope")]
        force_scope: bool,
    },
    /// Compile the deterministic spec draft for one entity via `grain spec` and
    /// print the resulting Markdown verbatim to stdout. Thin passthrough to
    /// `mustard_core::domain::scan::Scan::spec`. Invoke as
    /// `mustard-rt run scan spec --entity <Name>`.
    #[command(name = "scan-spec")]
    #[command(display_order = 71)]
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
    /// Validate a spec directory against the Wave 1 layout contract. Reads
    /// `meta.json` + `spec.md` and runs `mustard_core::domain::spec::contract::validate`.
    /// Exit code 0 ⇒ ok, 2 ⇒ violations, 1 ⇒ IO failure.
    #[command(display_order = 76)]
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
    #[command(display_order = 77)]
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
    /// ADVISORY structural linter for a capability or spec document.
    ///
    /// `--capability {slug}` lints `.claude/capabilities/{slug}.md`; `--spec
    /// {slug}` lints `.claude/spec/{slug}/spec.md`. Pass exactly one (neither /
    /// both prints a usage-error JSON). Catches the brittle-markdown failures
    /// the project keeps hitting: an Acceptance-Criterion the qa-run parser
    /// cannot read, a blank `command:`, a `[[..]]` that resolves to nothing.
    ///
    /// Reuses `capability::parse`, the qa-run AC parser (`parse_ac_items` /
    /// `extract_ac_section`), and the single `[[ ]]` scanner + resolver — no
    /// parallel parser/scanner. Prints byte-stable JSON
    /// `{ ok, doc, issues: [{level, rule, message}] }` (issues sorted by rule
    /// then message; `ok` = no error-level issue) and ALWAYS exits 0. Not wired
    /// into any gate — the owner may wire it into `doctor` later.
    #[command(display_order = 79)]
    SpecLint {
        /// Lint `.claude/capabilities/{slug}.md`.
        #[arg(long)]
        capability: Option<String>,
        /// Lint `.claude/spec/{slug}/spec.md`.
        #[arg(long)]
        spec: Option<String>,
    },
    /// Sweep terminal, idle spec directories under `.claude/spec/` (W5.T5.5).
    ///
    /// Default is **dry-run**: enumerates every spec whose `meta.json` reports
    /// `stage=close` + `outcome=completed` and whose most-recent NDJSON event
    /// is older than `--age-days N` (default 15). Pass `--apply` to
    /// `fs::remove_dir_all` each candidate. Emits a JSON report and a
    /// `spec.clear.run` harness event.
    #[command(display_order = 80)]
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
    #[command(display_order = 84)]
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
    #[command(display_order = 85)]
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
    #[command(display_order = 86)]
    TacticalFixDetect {
        /// Spec whose review/qa events are scanned for candidates.
        #[arg(long)]
        spec: Option<String>,
    },
    /// W5.T5.11 — Idempotent cross-platform copy of `.claude/spec/` into a backup tree.
    #[command(name = "backup-specs")]
    #[command(display_order = 94)]
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
    /// W5.T5.14 — Resolve the narrative locale for a spec.
    #[command(name = "spec-lang")]
    #[command(display_order = 95)]
    SpecLang {
        /// Subcommand: `resolve` is the only verb today.
        subcommand: String,
        /// Spec slug or directory path.
        #[arg(long)]
        spec: Option<String>,
    },
    /// W4 spec-status-consistency — one-shot alignment of spec.md ↔ meta.json
    /// headers across all specs. Default source is `spec` (spec.md is
    /// authoritative; meta.json is rewritten to match). With `--source meta`
    /// the direction reverses. `--dry-run` previews without writing.
    #[command(name = "spec-status-backfill")]
    #[command(display_order = 97)]
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
}

/// Dispatch one `spec`-family `run` subcommand.
pub fn dispatch(cmd: SpecCmd) {
    match cmd {
        SpecCmd::CompleteSpec {
            spec,
            archive,
            archive_stale,
            archive_followups,
        } => spec::complete_spec::run(spec.as_deref(), archive, archive_stale, archive_followups),
        SpecCmd::SpecExtract {
            spec,
            wave,
            ac,
            measure,
        } => spec::spec_extract::run(&spec, wave, ac, measure),
        SpecCmd::SpecLink {
            parent,
            child,
            reason,
        } => spec::spec_link::run(parent.as_deref(), child.as_deref(), reason.as_deref()),
        SpecCmd::SpecChildren { parent } => spec::spec_children::run(parent.as_deref()),
        SpecCmd::SpecChildrenTree { spec } => spec::spec_children_tree::run(spec.as_deref()),
        SpecCmd::ScopeDecompose { from_spec } => spec::scope_decompose::run(from_spec.as_deref()),
        SpecCmd::ScopeClassify {
            from_spec,
            slice_match_count,
        } => spec::scope_decompose::run_classify(&from_spec, slice_match_count),
        SpecCmd::PlanPrepare {
            from_spec,
            slice_match_count,
        } => spec::scope_decompose::run_prepare(&from_spec, slice_match_count),
        SpecCmd::RebuildSpecs => spec::rebuild_specs::run(),
        SpecCmd::PlanFromSpec { waves, roles, lang, summary } => {
            spec::plan_from_spec::run(spec::plan_from_spec::PlanFromSpecOpts {
                waves,
                roles,
                lang,
                summary,
            });
        }
        SpecCmd::ActiveSpecs { format, root } => {
            spec::active_specs::run(spec::active_specs::ActiveSpecsOpts { format, root });
        }
        SpecCmd::SpecDraft {
            intent,
            scope,
            lang,
            signals,
            output,
            waves,
            force,
            query_terms,
            force_scope,
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
                force_scope,
            });
        }
        SpecCmd::ScanSpec { entity, like, ops, invariant, root } => {
            spec::scan_spec::run(spec::scan_spec::ScanSpecOpts {
                entity,
                like,
                ops,
                invariants: invariant,
                root,
            });
        }
        SpecCmd::SpecValidate { spec, json } => {
            let _ = json; // currently always emits JSON
            spec::spec_validate::run(std::path::Path::new(&spec), true);
        }
        SpecCmd::SpecMemory {
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
        SpecCmd::SpecLint { capability, spec } => {
            spec::spec_lint::run(capability.as_deref(), spec.as_deref());
        }
        SpecCmd::SpecClear {
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
        SpecCmd::ApproveSpec { spec, wave_plan, resume } => {
            spec::approve_spec::run(spec::approve_spec::ApproveSpecOpts {
                spec,
                wave_plan,
                resume,
            });
        }
        SpecCmd::TacticalFixCreate { parent, description, scope } => {
            spec::tactical_fix_create::run(spec::tactical_fix_create::TacticalFixOpts {
                parent,
                description,
                scope,
            });
        }
        SpecCmd::TacticalFixDetect { spec } => {
            spec::tactical_fix_detect::run(spec.as_deref());
        }
        SpecCmd::BackupSpecs {
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
        SpecCmd::SpecLang { subcommand, spec } => {
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
        SpecCmd::SpecStatusBackfill { source, dry_run, spec } => {
            spec::spec_status_backfill::run_cli(spec::spec_status_backfill::BackfillOpts {
                source,
                dry_run,
                spec,
                cwd: None,
            });
        }
    }
}
