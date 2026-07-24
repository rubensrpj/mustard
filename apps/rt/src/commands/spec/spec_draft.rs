//! `mustard-rt run spec-draft` — generate a spec.md + meta.json (+ wave-plan)
//! conforming to [`mustard_core::domain::spec::contract`].
//!
//! Replaces the ~80 lines of literal-template boilerplate that lived inline in
//! `plugin/commands/feature.md` (W6 will remove the
//! literal block from that SKILL.md once this subcommand is in place).
//!
//! ## CLI shape
//!
//! ```text
//! mustard-rt run spec-draft \
//!     --intent "<free-text intent>" \
//!     --scope  light|full \
//!     --lang   pt-BR|en-US \
//!     [--signals layers,files,...] \
//!     [--output PATH]
//! ```
//!
//! ## Output
//!
//! When `--output PATH` is omitted, the new spec lands under
//! `.claude/spec/{slug}/` (`slug` derived from `--intent`).
//!
//! The spec dir is materialised as:
//!
//! ```text
//! {output}/
//!   spec.md              # PRD + (when scope=full) plan
//!   meta.json            # canonical lifecycle metadata (scope/totalWaves/isWavePlan)
//!   memory/_index.md     # T1.9 — stub memory index
//! ```
//!
//! Full-scope wave decomposition (`wave-plan.md` + `wave-N-{role}/spec.md` +
//! review/qa scaffolds) is materialised separately by `wave-scaffold` from a
//! plan JSON — `spec-draft` only writes the top-level spec.md + meta.json.
//!
//! Idempotent: if `output` already exists, the writer refuses to overwrite
//! unless `--force` is passed. Fail-open per file write (a single failure is
//! reported but does not abort the rest of the layout).

use crate::shared::context::project_dir;
use crate::commands::spec::spec_scaffold;
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs as mfs;
use mustard_core::domain::meta::Meta;
use mustard_core::domain::scan::DigestQuery;
use mustard_core::domain::spec::contract::{
    AcceptanceCriterion, ChecklistItem, SectionBody, SpecInput, PLAN_SECTIONS, PRD_SECTIONS,
};
use mustard_core::{
    domain::model::view::Phase,
    platform::i18n::{translate, Locale, Tone},
    Outcome, Scan, Scope, Stage,
};
use serde_json::json;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::str::FromStr;

/// Human-readable instruction inserted into the drafter prompt for `tone`.
/// Mirrors the Tone semantics in `mustard_core::platform::i18n::apply_tone`.
#[must_use]
pub fn tone_prompt_instruction(tone: Tone) -> &'static str {
    match tone {
        Tone::Didactic => {
            "Write this spec narrative in didactic tone — expand abbreviations on first use \
             (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon."
        }
        Tone::Technical => {
            "Write this spec narrative in technical tone — direct, jargon and abbreviations \
             welcome, no parenthetical glossing."
        }
        Tone::Concise => {
            "Write this spec narrative in concise tone — minimal prose, drop parentheticals \
             and filler, collapse whitespace."
        }
    }
}

/// Options for `mustard-rt run spec-draft`.
pub struct SpecDraftOpts {
    /// Free-text intent (becomes the spec title + slug seed).
    pub intent: String,
    /// `light` or `full`.
    pub scope: String,
    /// `pt-BR` / `en-US` (BCP-47 only — short forms rejected).
    pub lang: String,
    /// Optional comma-separated signals (e.g. `layers,files,registry`).
    pub signals: Option<String>,
    /// Optional output directory. Defaults to `.claude/spec/{slug}/`.
    pub output: Option<PathBuf>,
    /// Waves recorded in `meta.json#totalWaves` under Full scope (default 1).
    /// The wave dirs themselves are materialised by `wave-scaffold`.
    pub waves: u32,
    /// Overwrite existing output directory.
    pub force: bool,
    /// Optional comma-separated repo-vocabulary terms for the internal digest
    /// query (the terms that produced a strong report during ANALYZE). When
    /// absent, the raw intent is tokenised — which on a translated intent
    /// (e.g. PT over an EN repo) predictably repeats the weak query.
    pub query_terms: Option<String>,
    /// Honour `--scope full` even when the deterministic routing gate would
    /// auto-rebaixar it. The override is still RECORDED (a
    /// `pipeline.scope.override` event) so it stays auditable — see
    /// [`apply_scope_gate`].
    pub force_scope: bool,
}

/// Directory entries the harness writes into a spec directory BEFORE the spec
/// itself is drafted: the per-spec NDJSON event log and the dispatch sidecar.
/// Opening a work unit emits the first event, which creates `<spec>/.events/`;
/// the draft then arrives to find "its own" directory already there.
const HARNESS_STATE_ENTRIES: &[&str] = &[".events", ".dispatch"];

/// `true` when `dir` exists but holds NOTHING except the harness state listed in
/// [`HARNESS_STATE_ENTRIES`] — i.e. no spec has been drafted into it yet.
///
/// Creating the work unit and drafting its spec are two steps of one sequence,
/// and the first used to block the second: the event log landed in the spec
/// directory, `output.exists()` fired, and the draft refused with "pass
/// `--force` to overwrite" — an overwrite flag demanded for a directory holding
/// nothing to overwrite. Anything else present (a `spec.md`, a `meta.json`, a
/// wave dir, a stray file) is a REAL draft the guard must still protect.
fn holds_only_harness_state(dir: &std::path::Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        // Unreadable: treat as occupied — refusing is the safe direction when
        // we cannot prove the directory is empty of drafted work.
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !HARNESS_STATE_ENTRIES.contains(&name.as_str()) {
            return false;
        }
    }
    true
}

/// Scan existing spec directories under `spec_parent` for a NEAR-duplicate of
/// `slug` — a sibling whose hyphen-token set overlaps `slug`'s by a high ratio
/// (Jaccard >= 0.6 with >= 2 shared tokens). Catches a re-draft of the same
/// intent that slugged slightly differently before it silently creates a second
/// directory. Returns the first near-duplicate name. Fail-open: an unreadable
/// directory or a too-short slug yields `None`.
fn find_near_duplicate(spec_parent: &std::path::Path, slug: &str) -> Option<String> {
    use std::collections::BTreeSet;
    let cand: BTreeSet<&str> = slug.split('-').filter(|t| !t.is_empty()).collect();
    if cand.len() < 2 {
        return None;
    }
    for entry in std::fs::read_dir(spec_parent).ok()?.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == slug {
            continue; // exact match is handled by the output.exists() check.
        }
        let other: BTreeSet<&str> = name.split('-').filter(|t| !t.is_empty()).collect();
        let shared = cand.intersection(&other).count();
        let union = cand.union(&other).count();
        // Jaccard >= 0.6, i.e. shared/union >= 3/5, computed in integers.
        if shared >= 2 && shared * 5 >= union * 3 {
            return Some(name);
        }
    }
    None
}

/// Entry point.
pub fn run(opts: SpecDraftOpts) {
    let Some(scope) = Scope::parse(&opts.scope) else {
        emit_error("invalid --scope (expected `light` or `full`)", &opts.scope);
        return;
    };
    let Ok(lang_locale) = Locale::from_str(&opts.lang) else {
        emit_error("invalid --lang (expected BCP-47 `pt-BR` or `en-US`)", &opts.lang);
        return;
    };
    let slug = slug_from_intent(&opts.intent, lang_locale);
    if slug.is_empty() {
        emit_error("intent did not yield a slug", &opts.intent);
        return;
    }

    let auto_output = opts.output.is_none();
    let output = opts.output.unwrap_or_else(|| {
        let project = PathBuf::from(project_dir());
        ClaudePaths::for_project(&project)
            .and_then(|p| p.for_spec(&slug))
            .map(|sp| sp.dir().to_path_buf())
            .unwrap_or_else(|_| {
                ClaudePaths::compose_unchecked(&project)
                    .spec_dir()
                    .join(&slug)
            })
    });

    // A directory holding only the harness's own event log is not a drafted
    // spec — see [`holds_only_harness_state`]. Everything else still refuses.
    if output.exists() && !opts.force && !holds_only_harness_state(&output) {
        emit_error("output exists; pass --force to overwrite", &output.display().to_string());
        return;
    }
    // Near-duplicate guard (auto-slug only): a re-draft of the same intent can
    // slug slightly differently and silently create a SECOND spec directory
    // beside the first. Block on a high hyphen-token overlap with an existing
    // sibling; --force or an explicit --output overrides. Same language is
    // implicit — token overlap is near-zero across languages.
    if auto_output && !opts.force {
        if let Some(parent) = output.parent() {
            if let Some(dup) = find_near_duplicate(parent, &slug) {
                emit_error(
                    "a near-duplicate spec already exists; pass --force or --output to override",
                    &dup,
                );
                return;
            }
        }
    }
    if let Err(e) = mfs::create_dir_all(&output) {
        emit_error("could not create output directory", &e.to_string());
        return;
    }

    // ---- Resolve the project build command (AC default) from mustard.json. ----
    // No hardcoded `rtk cargo build`: the AC runs the project's own build, or a
    // neutral placeholder the user fills in when no `buildCommand` is set.
    let project_root = PathBuf::from(project_dir());
    let build_command =
        mustard_core::ProjectConfig::load(&project_root).build_command_or_fallback();

    // ---- Query the scan digest (the same insumos `feature::run` emits).
    // Deterministic, token-free, fail-open: a missing model or empty match
    // yields nothing. A low-confidence answer (`weak`/`none`) yields nothing
    // either (no labelled noise). `--query-terms` lets the orchestrator pass the
    // repo-vocabulary terms that produced a strong report (a PT intent
    // re-tokenised raw repeats the weak query). The anchors are REPORTED on
    // stdout, never written into the spec: they are read candidates for the
    // orchestrator, and the PRD layer is prose-only (see
    // [`render_scan_anchors`]). The digest does NOT seed the `## Checklist`
    // either — an anchor is evidence, never an implementation target; the real
    // file census is authored in ANALYZE/PLAN (`## Files`). ----
    let digest = scan_digest(&opts.intent, opts.query_terms.as_deref());
    let scan_anchors = digest
        .as_ref()
        .and_then(|q| render_scan_anchors(q, lang_locale));

    // ---- Build the canonical input + validate before writing. ----
    let input = build_input(
        &slug,
        &opts.intent,
        scope,
        &opts.lang,
        opts.waves,
        lang_locale,
        &build_command,
    );
    if let Err(violations) = mustard_core::domain::spec::contract::validate(&input) {
        let detail = violations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        emit_error("draft failed contract validation", &detail);
        return;
    }

    // ---- Resolve tone from mustard.json (wired into the drafter prompt). ----
    let tone = mustard_core::ProjectConfig::load(&project_root).i18n().tone;

    // ---- Materialise files. ----
    let mut written: Vec<String> = Vec::new();
    if let Err(e) = spec_scaffold::write_spec_md(&output, &input, &opts.signals, lang_locale, tone) {
        emit_error("write spec.md", &e);
        return;
    }
    written.push(output.join("spec.md").display().to_string());

    let meta = build_meta_from_input(&input);
    if let Err(e) = spec_scaffold::write_meta_json(&output, &meta) {
        emit_error("write meta.json", &e);
        return;
    }
    written.push(output.join("meta.json").display().to_string());

    // ---- Deterministic ROUTING GATE — the most expensive routing error is the
    // orchestrator asking for `--scope full` when the deterministic signals
    // (single-layer, few files) do not justify it. The machine enforces the
    // economy already written in the SKILL instead of the orchestrator having
    // to "remember" it: re-classify the spec.md we just wrote and auto-rebaixar
    // a non-justified full (rewriting meta.json — the source-of-truth the
    // scope_guard / close-gate read). `--force-scope` honours the request but
    // records the override. Fail-open: an unreadable spec leaves `full`
    // untouched. ----
    let scope_downgraded =
        apply_scope_gate(&project_root, &output, &slug, scope, opts.force_scope, &meta, digest.as_ref());

    // Record the `ANALYZE` phase now that the slug exists (see
    // [`backfill_analyze_phase`]).
    backfill_analyze_phase(&project_root, &slug);

    // D6: the `memory/_index.md` is NOT born at draft time. A fresh spec used to
    // ship an empty stub (and, before the i18n keys existed, a `<missing-key>`
    // line). The index is now born on the FIRST knowledge capture, so an unused
    // spec carries no orphan index file.

    // Full-scope wave decomposition is owned by `wave-scaffold` (plan-driven:
    // per-wave roles/summaries/deps + review/qa scaffolds). `spec-draft` only
    // materialises the top-level spec.md + meta.json — `meta.json` already
    // records `scope=full` + `totalWaves` + `isWavePlan`, so consumers know a
    // wave plan is expected before `wave-scaffold` fills it in.

    // The effective scope is the downgraded one when the gate acted, so the
    // report's `scope` matches the meta.json the gate rewrote (no contradiction
    // between stdout and the persisted source-of-truth).
    let effective_scope = scope_downgraded
        .as_ref()
        .and_then(|d| d.get("to").and_then(serde_json::Value::as_str))
        .unwrap_or_else(|| scope_str(scope));
    let mut report = json!({
        "ok": true,
        "spec": slug,
        "scope": effective_scope,
        "lang": opts.lang,
        "tone": tone.as_str(),
        "tone_instruction": tone_prompt_instruction(tone),
        "output": output.display().to_string(),
        "files": written,
    });
    if let (Some(obj), Some(downgrade)) = (report.as_object_mut(), scope_downgraded) {
        obj.insert("scopeDowngraded".to_string(), downgrade);
    }
    // The scan anchors ride the REPORT, not the artifact — the orchestrator
    // reads them to decide what to open, and `## Context` stays prose.
    if let (Some(obj), Some(anchors)) = (report.as_object_mut(), scan_anchors) {
        obj.insert("scanAnchors".to_string(), json!(anchors));
    }
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
}

// ---------------------------------------------------------------------------
// Routing gate (deterministic scope enforcement)
// ---------------------------------------------------------------------------

/// Deterministic routing gate. A `--scope full` that the structural signals do
/// not justify (single-layer, few files, no net-new entity) is the single most
/// expensive routing error — the full pipeline's ceremony is re-paid as harness
/// context on every turn. This re-classifies the spec.md just written (via
/// [`scope_decompose::classify_from_spec`] — the SAME deterministic thresholds
/// `scope-classify` uses, never reimplemented) and:
///
/// - **AUTO-REBAIXA** when `requested == Full`, the classifier returns
///   `light`/`extended-light`, the census is trustworthy (not an empty/
///   placeholder `## Files` section), and `--force-scope` was NOT passed.
///   The downgrade rewrites `meta.json` (the source-of-truth `scope_guard` /
///   close-gate read) to the classified scope, emits a
///   `pipeline.scope.downgrade` event, and returns the
///   `{from,to,reason,signals}` object the caller folds into stdout's
///   `scopeDowngraded`.
/// - **OVERRIDE** (no meta change) when `requested == Full`, the classifier
///   disagrees, but `--force-scope` was passed: the full is honoured, yet a
///   `pipeline.scope.override` event records the divergence so the override is
///   auditable, never silent. Returns `None` (no `scopeDowngraded`).
/// - **NO-OP** otherwise: a light/extended-light request (the gate only acts on
///   an unjustified full), a classifier that agrees the scope is `full`, or a
///   non-confident classification (`filesSectionEmpty` — a freshly-drafted spec
///   whose census is still a placeholder; downgrading off `fileCount=0` would
///   wrongly rebaixar every Full before its census lands). Returns `None`.
///
/// `slice_match_count` is threaded from the digest the run already computed
/// (`scan_digest` → `q.slices.len()`, mirroring `feature::run`'s
/// `sliceMatchCount`) so the classifier sees the same vocabulary-overlap signal
/// the `/feature` PLAN step does. Fail-open: an unreadable spec.md classifies to
/// the conservative `full` (`classify_from_spec`'s own fallback), which never
/// triggers a downgrade — the requested full stands.
fn apply_scope_gate(
    project_root: &std::path::Path,
    output: &std::path::Path,
    slug: &str,
    requested: Scope,
    force_scope: bool,
    meta: &Meta,
    digest: Option<&DigestQuery>,
) -> Option<serde_json::Value> {
    use crate::commands::spec::scope_decompose::classify_from_spec;

    // The gate only ever acts on a `full` request — a light/extended-light
    // request is already the economical path, nothing to rebaixar.
    if !matches!(requested, Scope::Full) {
        return None;
    }

    // Same vocabulary-overlap signal the digest feeds `/feature`'s scope-classify
    // (`sliceMatchCount`). Absent digest ⇒ 0 (the conservative read for the
    // slice conditions, matching `classify`'s default).
    let slice_match_count = digest.map_or(0, |q| q.slices.len() as i64);

    let verdict = classify_from_spec(&output.join("spec.md"), slice_match_count);
    let classified = verdict.get("scope").and_then(serde_json::Value::as_str).unwrap_or("full");
    let signals = verdict.get("signals").cloned().unwrap_or_else(|| json!({}));

    // A non-confident verdict (`scope: "abstain"` — the `## Files` census is
    // still a placeholder, so `fileCount` parsed to 0) is NOT grounds to
    // rebaixar: the same spec can flip to full once its census lands. Only a
    // trustworthy classification gates. (`classify_from_spec` flags this.)
    let confident = !verdict
        .get("filesSectionEmpty")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    // The classifier agrees `full` is justified (3+ layers / net-new / wide) —
    // or the spec was unreadable and fell open to the conservative `full`.
    // Nothing to do; the request stands.
    if classified == "full" {
        return None;
    }

    // requested == full, classifier disagrees (light / extended-light).
    if force_scope {
        // Override: honour the requested full, but RECORD the divergence so it
        // is auditable. No meta change — the request is intentional.
        emit_scope_event(
            project_root,
            slug,
            "pipeline.scope.override",
            json!({
                "requested": scope_str(requested),
                "classified": classified,
                "signals": signals,
            }),
        );
        return None;
    }

    // A non-confident verdict cannot justify a downgrade — leave the full alone.
    if !confident {
        return None;
    }

    // AUTO-REBAIXA: rewrite meta.json to the classified scope (the
    // source-of-truth `scope_guard` / close-gate read). The downgraded scope is
    // light/extended-light, neither of which carries waves — clear the wave-plan
    // fields so the persisted meta is internally consistent (a Light/ext-light
    // spec is never a wave plan). The spec.md narrative is left as-is (cosmetic:
    // meta decides; a stale "full" plan section is harmless next to a light meta).
    let downgraded_meta = Meta {
        scope: Some(classified.to_string()),
        is_wave_plan: None,
        total_waves: None,
        ..meta.clone()
    };
    if let Err(e) = spec_scaffold::write_meta_json(output, &downgraded_meta) {
        // Fail-open: if we cannot rewrite the meta we must NOT claim a downgrade
        // (the source-of-truth would still say full). Leave the request intact.
        let _ = e;
        return None;
    }

    let downgrade = json!({
        "from": scope_str(requested),
        "to": classified,
        "reason": "deterministic-routing-gate",
        "signals": signals,
    });
    emit_scope_event(
        project_root,
        slug,
        "pipeline.scope.downgrade",
        json!({
            "requested": scope_str(requested),
            "classified": classified,
            "signals": signals,
        }),
    );
    Some(downgrade)
}

/// Emit one `pipeline.scope.*` routing event through the shared economy/route
/// channel (the same envelope builder the other `pipeline.*` emitters use),
/// attributing it to this spec slug. Fail-open: telemetry never blocks the draft.
fn emit_scope_event(
    project_root: &std::path::Path,
    slug: &str,
    event_name: &str,
    payload: serde_json::Value,
) {
    use mustard_core::domain::model::event::ActorKind;
    crate::shared::events::economy::emit(
        &project_root.to_string_lossy(),
        ActorKind::Cli,
        "spec-draft",
        event_name,
        Some(slug),
        payload,
    );
}

// ---------------------------------------------------------------------------
// Building / writing
// ---------------------------------------------------------------------------

/// Build a default [`SpecInput`] for the given intent. The stub sections each
/// carry a single placeholder line — they are valid against the contract but
/// the user is expected to flesh them out. Section *bodies* are localised via
/// `lang_locale` (the body is spec-facing narrative); the canonical section
/// *keys* in [`PRD_SECTIONS`] / [`PLAN_SECTIONS`] stay in their EN, language-
/// agnostic spelling and are translated to display headings only at render.
fn build_input(
    slug: &str,
    intent: &str,
    scope: Scope,
    lang: &str,
    waves: u32,
    lang_locale: Locale,
    build_command: &str,
) -> SpecInput {
    SpecInput {
        slug: slug.to_string(),
        title: intent.to_string(),
        stage: Some(Stage::Plan),
        outcome: Some(Outcome::Active),
        phase: Some(Phase::Plan),
        scope: Some(scope),
        lang: Some(lang.to_string()),
        // Invariant (2026-06-02-full-sempre-uma-wave): a Full spec floors at ≥1
        // wave. The floor is named by [`scope_decompose::wave_floor_for_full`]
        // (single source of the "Full ⇒ ≥1 wave" rule); a caller asking for >1
        // wave signals a multi-wave decomposition and raises N above the floor.
        // Light carries no waves at all.
        total_waves: if matches!(scope, Scope::Full) {
            let floor = crate::commands::spec::scope_decompose::wave_floor_for_full(waves > 1);
            Some(waves.max(floor))
        } else {
            None
        },
        prd_sections: PRD_SECTIONS
            .iter()
            .map(|n| SectionBody {
                name: (*n).to_string(),
                body: prd_section_default(n, intent, lang_locale),
            })
            .collect(),
        plan_sections: if matches!(scope, Scope::Full) {
            PLAN_SECTIONS
                .iter()
                .map(|n| SectionBody {
                    name: (*n).to_string(),
                    body: plan_section_default(n, lang_locale),
                })
                .collect()
        } else {
            Vec::new()
        },
        acceptance_criteria: seed_acceptance_criteria(lang_locale, build_command),
        checklist: build_checklist(lang_locale),
    }
}

/// Seed the EARS-shaped skeleton `## Acceptance Criteria` for a fresh draft.
///
/// A draft is born spec-DRIVEN, not rubber-stamped: two behaviour ACs in
/// `when X, then Y` form (the join reused from
/// [`mustard_core::domain::capability::scenario_statement`]) whose `<…>` markers
/// DEMAND the orchestrator fill in the concrete trigger, outcome, and verifying
/// command — plus ONE trailing build-green SAFETY criterion (the compile floor,
/// the single tautology `analyze-validation`'s weak-AC linter tolerates). The
/// old lone "Pipeline build green" AC passed whether or not the feature existed;
/// it survives only as the LAST safety net here, never as the only criterion.
fn seed_acceptance_criteria(lang: Locale, build_command: &str) -> Vec<AcceptanceCriterion> {
    use mustard_core::domain::capability::scenario_statement;
    let skeleton_command = translate("ac.skeleton.command", lang).to_string();
    vec![
        AcceptanceCriterion {
            id: "AC-1".to_string(),
            statement: scenario_statement(
                translate("ac.skeleton.when_primary", lang),
                translate("ac.skeleton.then_primary", lang),
            ),
            command: skeleton_command.clone(),
        },
        AcceptanceCriterion {
            id: "AC-2".to_string(),
            statement: scenario_statement(
                translate("ac.skeleton.when_secondary", lang),
                translate("ac.skeleton.then_secondary", lang),
            ),
            command: skeleton_command,
        },
        AcceptanceCriterion {
            id: "AC-3".to_string(),
            statement: translate("ac.safety.build_green", lang).to_string(),
            command: build_command.to_string(),
        },
    ]
}

/// Build the trackable `## Checklist` for a fresh draft: a single hand-trackable
/// task item (`T1`, mirroring the `tasks` plan placeholder).
///
/// The draft deliberately does NOT seed one item per scan anchor. A digest
/// anchor is a READ candidate (evidence to read before deciding), never an
/// implementation target — seeding write-tracking from it baked lexical noise
/// into the artifact as "implement the change in → <file>" items. Field case
/// (sialia, client-tabs): a `strong`-by-coverage answer (every query term found
/// a rung) whose anchors were stem-matched neighbours (`receivable`→`receiver`,
/// `create`→`creates`: Safe2Pay DTOs, seeders, tests) — none of them the files
/// actually touched, all of which the orchestrator then deleted by hand. A
/// strong MATCH report measures term COVERAGE, not anchor PRECISION, so it is
/// not a licence to treat anchors as a verdict.
///
/// The real file census is authored in ANALYZE/PLAN (`## Files`), and the
/// `checklist-auto-mark` hook tracks whatever ` → <path>` items land there —
/// keyed off the files DECIDED, not the files the digest guessed. The single
/// fallback item keeps the contract's `ChecklistEmpty` rule and the close-gate
/// checklist gate satisfied. Anchors still ride the RUN as READ evidence — on
/// the stdout report, not in the artifact ([`render_scan_anchors`]).
fn build_checklist(lang: Locale) -> Vec<ChecklistItem> {
    vec![ChecklistItem {
        label: translate("checklist.first_task", lang).to_string(),
        path: None,
        done: false,
    }]
}

/// Default body for a PRD section. `name` is a canonical contract key — a
/// language-agnostic EN identifier from [`PRD_SECTIONS`] (`"context"`,
/// `"users"`, …). The returned body is fully localised via the catalogue
/// (the body is part of the spec-facing narrative; only the keys are EN).
///
/// `"context"` is PROSE ONLY — the intent sentence plus the why-now prompt.
/// The scan anchors the drafter used to splice in here were a bullet list of
/// file paths, which the shipped spec law (`refs/feature/spec-language.md`)
/// forbids in the PRD layer: `## Context` briefs a human rediscovering the work,
/// so paths, identifiers and lists belong to `## Root cause` / `## Files`. They
/// now ride the command's stdout report instead (see [`render_scan_anchors`]).
fn prd_section_default(name: &str, intent: &str, lang: Locale) -> String {
    let fill_why_now = translate("placeholder.fill_why_now", lang);
    match name {
        "context" => format!("{intent}.\n\n{fill_why_now}"),
        "users" => translate("placeholder.fill_beneficiary", lang).to_string(),
        "metric" => translate("placeholder.fill_metric", lang).to_string(),
        "non-goals" => translate("placeholder.fill_excluded", lang).to_string(),
        // Contract ballast, NEVER rendered: `write_spec_md` skips this entry
        // (single-emitter rule — the AC list block owns the heading) but
        // `check_sections` still requires the entry present with a non-empty
        // body. EN literal on purpose; the old localized `placeholder.see_below`
        // copy was retired with the duplicate heading it captioned.
        "acceptance-criteria" => "(rendered from the acceptance_criteria list)".to_string(),
        _ => translate("placeholder.fill", lang).to_string(),
    }
}

/// Default body for a Plan section. `name` is a canonical contract key — a
/// language-agnostic EN identifier from [`PLAN_SECTIONS`] (`"files"`,
/// `"tasks"`, `"boundaries"`).
fn plan_section_default(name: &str, lang: Locale) -> String {
    match name {
        "files" => translate("placeholder.fill_files", lang).to_string(),
        // D2: `## Tarefas` is the agent's roadmap, a plain list — NOT a tracked
        // checklist. Only `## Checklist` carries `[ ]` (with auto-mark on
        // `→ <path>`). A checkbox here was a false gate target nothing marks.
        "tasks" => "- T1 — ...".to_string(),
        "boundaries" => "IN: ...\nOUT: ...".to_string(),
        _ => translate("placeholder.fill", lang).to_string(),
    }
}

/// Max anchors / slices surfaced in the Context enrichment block. The digest
/// already returns ~12 anchors; cap so a wide query does not inflate the spec.
const SCAN_ANCHOR_CAP: usize = 12;
const SCAN_SLICE_CAP: usize = 6;
/// Max matched terms annotated per anchor (from the digest's `files_detail`
/// audit trail) — keeps the per-anchor note concise.
const ANCHOR_TERM_CAP: usize = 4;

/// Query the scan digest for the intent — the same deterministic insumos
/// `feature::run` emits, recomputed here. It costs no tokens (a local query
/// against `grain.model.json`, not an AI call). The answer feeds the reported
/// anchor briefing ([`render_scan_anchors`]). Returns `None` when the model
/// is absent or the query failed (fail-open: the report simply omits the
/// briefing).
fn scan_digest(intent: &str, query_terms: Option<&str>) -> Option<DigestQuery> {
    let model = PathBuf::from(project_dir())
        .join(".claude")
        .join("grain.model.json");
    // `--query-terms` (comma-separated) takes precedence over re-tokenising
    // the raw intent: the orchestrator passes the repo-vocabulary terms that
    // already produced a strong report, instead of this command silently
    // repeating the user's-vocabulary query (predictably weak on a PT intent
    // over an EN repo — the field case that seeded a scaffold with noise).
    let terms: Vec<String> = match query_terms {
        Some(csv) => csv
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect(),
        None => crate::commands::feature::domain_terms(intent),
    };
    if terms.is_empty() {
        return None;
    }
    Scan::locate().digest_query(&model, &terms).ok()
}

/// Whether the digest's honest match report flags the answer as low-confidence:
/// `weak` (under half the terms matched / derived tiers only) or `none`
/// (nothing matched — the anchors, if any, are structural noise). An empty
/// reason (payload from an older scan binary) keeps the legacy confident
/// behaviour, and `strong`/`generated_only` are trusted.
fn digest_low_confidence(q: &DigestQuery) -> bool {
    matches!(q.report.reason.as_str(), "weak" | "none")
}

/// The matched index terms that carried `file` into the anchor list, from the
/// digest's `files_detail` audit trail (empty when the payload predates the
/// field or the anchor is a touchpoint-tail path hit).
fn anchor_terms<'a>(q: &'a DigestQuery, file: &str) -> &'a [String] {
    q.files_detail
        .iter()
        .find(|d| d.file == file)
        .map_or(&[], |d| d.terms.as_slice())
}

/// Render the scan-anchor briefing from a digest answer — the read candidates
/// the orchestrator should open before deciding. Pure (no I/O) so it is
/// unit-testable. Returns `None` when there is nothing to show.
///
/// This markdown goes to the command's stdout report (`scanAnchors`), NEVER
/// into the spec: it is a bullet list of file paths, and the shipped spec law
/// (`refs/feature/spec-language.md`) keeps the PRD layer prose-only.
///
/// Confidence rule (tightened after the field case where a PT intent's
/// internal re-query came back `weak` and seeded the scaffold with 12
/// lexical-noise anchors the orchestrator then had to overwrite by hand): a
/// low-confidence answer (`weak`/`none` report) materialises NOTHING — noise
/// must never enter the artifact, labelled or not. This mirrors the
/// `planningWithheld` contract of the `feature` stdout payload. The caller
/// can re-enable the enrichment by passing `--query-terms` with the
/// repo-vocabulary terms that produced a strong report. On a confident
/// answer each anchor is annotated with the matched terms that carried it
/// (`files_detail`, capped at [`ANCHOR_TERM_CAP`]).
fn render_scan_anchors(q: &DigestQuery, lang: Locale) -> Option<String> {
    if digest_low_confidence(q) {
        return None;
    }
    let mut block = String::new();
    if !q.files.is_empty() {
        let _ = writeln!(block, "{}:", translate("context.scan_anchors", lang));
        for f in q.files.iter().take(SCAN_ANCHOR_CAP) {
            let terms = anchor_terms(q, f);
            if terms.is_empty() {
                let _ = writeln!(block, "- {f}");
            } else {
                let joined = terms
                    .iter()
                    .take(ANCHOR_TERM_CAP)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(block, "- {f} ({joined})");
            }
        }
    }
    if !q.slices.is_empty() {
        let joined = q
            .slices
            .iter()
            .take(SCAN_SLICE_CAP)
            .map(|s| format!("{} (×{})", s.label, s.recurrence))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = write!(block, "\n{}: {}", translate("context.scan_slices", lang), joined);
    }
    let trimmed = block.trim_end().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// Build a [`Meta`] from a [`SpecInput`]. Used by [`run`] before delegating
/// to [`spec_scaffold::write_meta_json`].
fn build_meta_from_input(input: &SpecInput) -> Meta {
    Meta {
        stage: input.stage.map(|s| format!("{s:?}")),
        outcome: input.outcome.map(|o| format!("{o:?}")),
        phase: input.phase.map(|p| format!("{p:?}").to_uppercase()),
        scope: input.scope.map(scope_str).map(str::to_string),
        lang: input.lang.clone(),
        checkpoint: None,
        parent: None,
        is_wave_plan: input.total_waves.map(|n| n > 0),
        total_waves: input.total_waves,
        // A freshly drafted spec carries no qualifier flag (Plan/Active).
        flags: mustard_core::MetaFlags::default(),
        // The trackable checklist lives in the spec markdown at draft time and
        // in each WAVE's sidecar after the scaffold — never in the root meta
        // (explicit OUT of the checklist-progresso spec).
        checklist: Vec::new(),
        raw: serde_json::Value::Null,
    }
}

// D6: the `memory/_index.md` is no longer materialised at draft time (the old
// `write_memory_stub` shipped an empty stub on every spec). The index is now
// created/updated on the first knowledge capture.

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Max number of words kept in a generated slug. A paragraph-length intent is
/// cut here — on a word boundary, never mid-word (the old 60-char `.take`
/// decapitated the final word, e.g. `…contas-a-r`).
const SLUG_MAX_TOKENS: usize = 5;

/// Derive a kebab-case slug from a free-text intent by delegating to the
/// canonical [`mustard_core::slugify`] — per-locale accent fold + stopword
/// drop — instead of a hand-rolled char map that kept stopwords (`em`, `a`,
/// `de`) and mangled accents (`visão` → `vis-o`). Capped to the first
/// [`SLUG_MAX_TOKENS`] words so the cut always lands on a boundary.
fn slug_from_intent(intent: &str, lang: Locale) -> String {
    mustard_core::slugify(intent, lang)
        .split('-')
        .take(SLUG_MAX_TOKENS)
        .collect::<Vec<_>>()
        .join("-")
}

/// Canonical lowercase string for the scope (matches `Scope` `serde` rename).
fn scope_str(scope: Scope) -> &'static str {
    match scope {
        Scope::Full => "full",
        Scope::Light => "light",
        Scope::Touch => "touch",
    }
}

fn emit_error(reason: &str, detail: &str) {
    let body = json!({
        "ok": false,
        "error": reason,
        "detail": detail,
    });
    println!("{}", serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".into()));
}

/// Backfill the `ANALYZE` phase marker for a freshly-born spec slug.
///
/// ANALYZE runs in the parent context *before* any spec dir exists, so the
/// orchestrator can never attribute a phase event to it: there is no slug yet,
/// and every emitter (`emit-phase`, `emit-pipeline`) requires `--spec`. The old
/// SKILL instruction to "Emit `pipeline.stage: Analyze`" at the top of ANALYZE
/// was therefore unsatisfiable and failed silently. `spec-draft` is the first
/// moment the slug exists, so we record ANALYZE here — via the same
/// [`emit_phase`](crate::commands::event::emit_phase) primitive `plan-materialize`
/// uses for PLAN, so the phase track reads `ANALYZE → PLAN`.
///
/// It writes a bare `pipeline.phase` event (no `meta.json` patch), leaving the
/// sidecar `spec-draft` just wrote (`stage: Plan`) untouched. Guarded on a
/// *fresh* slug only — when the spec already carries a phase (a `--force`
/// re-draft of an already-advanced spec) it emits nothing, so the track is never
/// regressed back to ANALYZE. Fail-open: telemetry never blocks the draft.
fn backfill_analyze_phase(cwd: &std::path::Path, slug: &str) {
    use crate::commands::event::emit_phase;
    if emit_phase::last_phase_for_spec(cwd, slug).is_none() {
        let _ = emit_phase::run_at(cwd, slug, "ANALYZE", None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn slug_basic_kebab() {
        assert_eq!(slug_from_intent("Add user CRUD", Locale::EnUs), "add-user-crud");
        assert_eq!(
            slug_from_intent("  ---  Fix login   bug  ", Locale::EnUs),
            "fix-login-bug"
        );
    }

    #[test]
    fn slug_drops_stopwords_no_midword_cut() {
        // Field report (sialia): the hand-rolled slug kept "em/a/de" and cut
        // "receber" → "r". Delegating to slugify drops stopwords per-locale and
        // the token cap lands on a word boundary.
        let s = slug_from_intent(
            "Espelhar em contas a pagar a visão de listagem de contas a receber",
            Locale::PtBr,
        );
        assert_eq!(s, "espelhar-contas-pagar-visao-listagem");
        assert!(!s.ends_with('-'));
    }

    #[test]
    fn near_duplicate_flags_high_overlap_only() {
        let dir = tempdir().unwrap();
        let parent = dir.path();
        std::fs::create_dir_all(parent.join("refatoracao-global-tratamento-erro")).unwrap();
        std::fs::create_dir_all(parent.join("unrelated-login-flow")).unwrap();

        // High token overlap with the existing PT spec → flagged.
        assert_eq!(
            find_near_duplicate(parent, "refatoracao-global-tratamento-erro-handler").as_deref(),
            Some("refatoracao-global-tratamento-erro"),
        );
        // A genuinely different spec is not blocked.
        assert!(find_near_duplicate(parent, "add-dark-mode-toggle").is_none());
        // Cross-language: an EN slug shares too few tokens with the PT dir → None.
        assert!(find_near_duplicate(parent, "error-handling-global-refactor").is_none());
    }

    /// Count `pipeline.phase` events with a given `to` value under `cwd`'s spec.
    fn phase_to_count(cwd: &std::path::Path, slug: &str, to: &str) -> usize {
        let events_dir = cwd.join(".claude").join("spec").join(slug).join(".events");
        mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir)
            .iter()
            .filter(|e| {
                e.event == "pipeline.phase"
                    && e.payload.get("to").and_then(serde_json::Value::as_str) == Some(to)
            })
            .count()
    }

    /// A fresh slug gets exactly one `ANALYZE` phase marker, and a repeat call
    /// (e.g. a `--force` re-draft while still at ANALYZE) adds nothing — the
    /// guard sees the track tip is already ANALYZE. This is the missing sibling
    /// of `plan-materialize`'s PLAN emit: the phase track must read ANALYZE → PLAN.
    #[test]
    fn backfill_analyze_records_one_marker_idempotently() {
        use crate::commands::event::emit_phase::last_phase_for_spec;
        let dir = tempdir().unwrap();
        backfill_analyze_phase(dir.path(), "demo-spec");
        backfill_analyze_phase(dir.path(), "demo-spec");
        assert_eq!(phase_to_count(dir.path(), "demo-spec", "ANALYZE"), 1);
        assert_eq!(
            last_phase_for_spec(dir.path(), "demo-spec").as_deref(),
            Some("ANALYZE"),
        );
    }

    /// A spec that already advanced past ANALYZE (e.g. `plan-materialize` ran the
    /// PLAN emit) must NOT be regressed: a re-draft's backfill emits nothing, so
    /// the track tip stays PLAN and no late ANALYZE marker appears.
    #[test]
    fn backfill_analyze_never_regresses_an_advanced_spec() {
        use crate::commands::event::emit_phase::{last_phase_for_spec, run_at};
        let dir = tempdir().unwrap();
        let _ = run_at(dir.path(), "demo-spec", "PLAN", None);
        backfill_analyze_phase(dir.path(), "demo-spec");
        assert_eq!(
            last_phase_for_spec(dir.path(), "demo-spec").as_deref(),
            Some("PLAN"),
            "backfill must not regress an advanced spec to ANALYZE",
        );
        assert_eq!(phase_to_count(dir.path(), "demo-spec", "ANALYZE"), 0);
    }

    #[test]
    fn slug_caps_on_word_boundary() {
        // 10 content words → first 5 kept, cut on a boundary (no partial word).
        let s = slug_from_intent(
            "alpha beta gamma delta epsilon zeta eta theta iota kappa",
            Locale::EnUs,
        );
        assert_eq!(s, "alpha-beta-gamma-delta-epsilon");
    }

    /// Build a [`DigestQuery`] from the literal JSON the scan binary emits —
    /// the same boundary `scan_digest` crosses in production.
    fn digest(json: &str) -> DigestQuery {
        serde_json::from_str(json).expect("digest payload json")
    }

    #[test]
    fn render_scan_anchors_lists_anchors_and_slices() {
        let q = digest(
            r#"{"query":["list"],"slices":[{"label":"List","recurrence":3}],
                "files":["src/list.rs","src/view.rs"],"miss":false,
                "report":{"matched":1,"total":1,"reason":"strong","terms":[]}}"#,
        );
        let s = render_scan_anchors(&q, Locale::PtBr).unwrap();
        assert!(s.contains("Âncoras (do scan):"));
        assert!(s.contains("- src/list.rs"));
        assert!(s.contains("Fatias recorrentes"));
        assert!(s.contains("List (×3)"));
    }

    #[test]
    fn render_scan_anchors_none_when_empty() {
        let q = digest(r#"{"miss":true}"#);
        assert!(render_scan_anchors(&q, Locale::EnUs).is_none());
    }

    #[test]
    fn render_scan_anchors_caps_anchors_and_uses_en_heading() {
        let files: Vec<String> = (0..20).map(|i| format!("f{i}.rs")).collect();
        let q = digest(&format!(
            r#"{{"files":{},"miss":false}}"#,
            serde_json::to_string(&files).unwrap()
        ));
        let s = render_scan_anchors(&q, Locale::EnUs).unwrap();
        assert!(s.contains("Anchors (from scan):"));
        assert_eq!(s.matches("- f").count(), SCAN_ANCHOR_CAP);
    }

    /// Roundtrip (tightened after the field case where labelled weak anchors
    /// still had to be overwritten by hand) — a `weak` digest answer
    /// materialises NO Context block at all (noise never enters the artifact,
    /// labelled or not). `none` behaves identically.
    #[test]
    fn roundtrip_weak_digest_reports_no_anchors() {
        let weak = digest(
            r#"{"query":["payables"],
                "files":["src/financial/accounts.rs","src/financial/codes.rs"],
                "files_detail":[{"file":"src/financial/accounts.rs","score_x1024":512,"terms":["financial"]}],
                "slices":[{"label":"crud","recurrence":3,"entities":["X"]}],
                "miss":false,
                "report":{"matched":1,"total":3,"reason":"weak","terms":[]}}"#,
        );
        assert!(digest_low_confidence(&weak));
        // Context: NOTHING — anchors AND slices from a weak answer are noise
        // the orchestrator would have to overwrite by hand (field case).
        assert!(
            render_scan_anchors(&weak, Locale::PtBr).is_none(),
            "weak answer must materialise no Context block"
        );
        assert!(render_scan_anchors(&weak, Locale::EnUs).is_none());

        let none = digest(r#"{"files":["src/x.rs"],"miss":true,"report":{"matched":0,"total":2,"reason":"none","terms":[]}}"#);
        assert!(digest_low_confidence(&none));
        assert!(render_scan_anchors(&none, Locale::PtBr).is_none());
    }

    /// Roundtrip (robustez-ancoras fase 2) — a `strong` answer keeps the plain
    /// anchor label and annotates each anchor with the matched terms from
    /// `files_detail` (lote 1's audit trail). The checklist is no longer seeded
    /// from anchors — see [`build_checklist`].
    #[test]
    fn roundtrip_strong_digest_annotates_anchor_terms() {
        let strong = digest(
            r#"{"query":["payable","nature"],
                "files":["src/payables/page.rs","src/payables/list.rs","src/tail.rs"],
                "files_detail":[
                    {"file":"src/payables/page.rs","score_x1024":4096,"terms":["payable","nature","account","code","extra"]},
                    {"file":"src/payables/list.rs","score_x1024":2048,"terms":["payable"]},
                    {"file":"src/tail.rs","score_x1024":0,"terms":[]}],
                "miss":false,
                "report":{"matched":2,"total":2,"reason":"strong","terms":[]}}"#,
        );
        assert!(!digest_low_confidence(&strong));
        let s = render_scan_anchors(&strong, Locale::EnUs).unwrap();
        assert!(s.contains("Anchors (from scan):"), "plain label on strong:\n{s}");
        assert!(!s.contains("LOW CONFIDENCE"), "no weak label on strong:\n{s}");
        // Term annotation, capped at ANCHOR_TERM_CAP (5th term dropped).
        assert!(
            s.contains("- src/payables/page.rs (payable, nature, account, code)"),
            "terms annotated + capped:\n{s}"
        );
        assert!(!s.contains("extra"), "cap at {ANCHOR_TERM_CAP}:\n{s}");
        assert!(s.contains("- src/payables/list.rs (payable)"), "single term:\n{s}");
        // A touchpoint-tail anchor (no terms) renders bare — no `()` noise.
        assert!(s.contains("- src/tail.rs\n") || s.ends_with("- src/tail.rs"), "bare tail anchor:\n{s}");
        assert!(!s.contains("src/tail.rs ("), "no empty annotation:\n{s}");
        // An old-binary payload (empty reason) is treated as confident, so the
        // Context block still renders (legacy compat).
        let old = digest(r#"{"files":["src/a.rs"],"miss":false}"#);
        assert!(!digest_low_confidence(&old));
        assert!(render_scan_anchors(&old, Locale::EnUs).is_some());
    }

    #[test]
    fn build_input_validates() {
        let input = build_input("demo", "Demo", Scope::Full, "pt-BR", 2, Locale::PtBr, "rtk cargo build");
        assert!(mustard_core::domain::spec::contract::validate(&input).is_ok());
    }

    /// Invariant lock (2026-06-02-full-sempre-uma-wave): a Full draft NEVER
    /// yields `total_waves == 0`, and the meta it produces NEVER has
    /// `isWavePlan == Some(false)`. Probed at the most adversarial input —
    /// `waves: 0` from the caller — which `total_waves: Some(waves.max(1))`
    /// (~L246) must floor to 1. Light is unaffected: it carries no waves at all
    /// (`total_waves == None`, `isWavePlan == None`).
    #[test]
    fn full_draft_never_zero_waves_or_non_wave_plan() {
        for waves in [0u32, 1, 2, 7] {
            let input = build_input(
                "demo", "Demo", Scope::Full, "pt-BR", waves, Locale::PtBr, "rtk cargo build",
            );
            // total_waves is floored to ≥ 1 for Full.
            assert_eq!(
                input.total_waves,
                Some(waves.max(1)),
                "Full draft floors total_waves to ≥ 1 (caller waves={waves})"
            );
            assert!(input.total_waves.unwrap_or(0) >= 1, "Full total_waves ≥ 1");
            // The contract agrees the floored input is valid (FullScopeNoWaves
            // would fire on total_waves==0).
            assert!(mustard_core::domain::spec::contract::validate(&input).is_ok());
            // The derived meta marks it as a wave plan — never Some(false).
            let meta = build_meta_from_input(&input);
            assert_eq!(meta.total_waves, Some(waves.max(1)));
            assert_eq!(
                meta.is_wave_plan,
                Some(true),
                "Full meta isWavePlan must be Some(true), never Some(false)"
            );
            assert_ne!(meta.is_wave_plan, Some(false));
        }
        // Light: no waves, no wave-plan flag (invariant is Full-only).
        let light = build_input(
            "demo", "Demo", Scope::Light, "en-US", 0, Locale::EnUs, "rtk cargo build",
        );
        assert_eq!(light.total_waves, None, "Light carries no waves");
        let light_meta = build_meta_from_input(&light);
        assert_eq!(light_meta.is_wave_plan, None);
        assert_eq!(light_meta.total_waves, None);
    }

    #[test]
    fn build_input_validates_in_en_us() {
        // Section *keys* are canonical EN identifiers; bodies are localised.
        let input = build_input("demo", "Demo", Scope::Full, "en-US", 2, Locale::EnUs, "rtk cargo build");
        assert!(mustard_core::domain::spec::contract::validate(&input).is_ok());
        // Body strings should be EN, not PT.
        let users = input
            .prd_sections
            .iter()
            .find(|s| s.name == "users")
            .unwrap();
        assert!(users.body.contains("fill in"), "EN body got: {}", users.body);
    }

    #[test]
    fn build_input_ac_seed_is_ears_with_trailing_build_safety() {
        // The build command flows into the trailing build-green SAFETY AC (the
        // LAST criterion), not `rtk cargo build` as the only AC; the leading ACs
        // are EARS behaviour skeletons, never a lone build tautology.
        let input = build_input("demo", "Demo", Scope::Light, "en-US", 0, Locale::EnUs, "pnpm build");
        let acs = &input.acceptance_criteria;
        assert!(acs.len() >= 2, "seed carries behaviour ACs + a safety AC, got {}", acs.len());
        assert_eq!(acs.last().unwrap().command, "pnpm build", "build command is the trailing safety AC");
        // AC-1/AC-2 are EARS skeletons: `when <…>, then <…>` markers that DEMAND
        // filling, verified by a placeholder command (never a build tautology).
        assert!(acs[0].statement.contains("when <"), "AC-1 is an EARS skeleton: {}", acs[0].statement);
        assert!(acs[0].statement.contains("then <"), "AC-1 carries a then-clause: {}", acs[0].statement);
        assert_ne!(acs[0].command, "pnpm build", "skeleton AC command is not the build");
        assert!(acs[0].command.contains('<'), "skeleton AC command is a fill-me placeholder: {}", acs[0].command);
        // Neutral fallback flows through verbatim when no buildCommand is set.
        let input2 = build_input(
            "demo",
            "Demo",
            Scope::Light,
            "en-US",
            0,
            Locale::EnUs,
            mustard_core::BUILD_COMMAND_FALLBACK,
        );
        assert_eq!(
            input2.acceptance_criteria.last().unwrap().command,
            mustard_core::BUILD_COMMAND_FALLBACK
        );
    }

    #[test]
    fn build_checklist_is_a_single_trackable_task() {
        // The draft never seeds checklist items from scan anchors (an anchor is
        // a READ candidate, not an implementation target) — it always drafts the
        // single hand-trackable fallback so the close-gate has something to track.
        let items = build_checklist(Locale::EnUs);
        assert_eq!(items.len(), 1);
        assert!(items[0].path.is_none(), "no auto-mark path on the fallback item");
        assert!(!items[0].label.is_empty());
        // Localised label resolves for the other locale too.
        assert_eq!(build_checklist(Locale::PtBr).len(), 1);
    }

    /// D1/D2: a Light spec OWNS its execution → it keeps a parseable
    /// `## Checklist` so the close-gate has something to enforce. (A Full draft
    /// is always a wave-plan parent — `total_waves` is forced to ≥ 1 — so its
    /// checklist lives in the waves; that suppression is covered below.)
    #[test]
    fn drafted_light_spec_has_parseable_checklist() {
        use mustard_core::domain::spec::contract::CHECKLIST_HEADING;
        let dir = tempdir().unwrap();
        let out = dir.path().join("specs").join("light");
        run(SpecDraftOpts {
            intent: "Demo intent".into(),
            scope: "light".into(),
            lang: "pt-BR".into(),
            signals: None,
            output: Some(out.clone()),
            waves: 0,
            force: false,
            query_terms: None,
            force_scope: false,
        });
        let body = std::fs::read_to_string(out.join("spec.md")).unwrap();
        let heading = format!("## {CHECKLIST_HEADING}");
        assert!(body.contains(&heading), "light spec.md missing `{heading}`:\n{body}");
        let after = body.split_once(&heading).expect("checklist heading split").1;
        let section = after.split("\n## ").next().unwrap_or(after);
        assert!(
            section.lines().any(|l| l.trim_start().starts_with("- [ ] ")),
            "light: no parseable `- [ ]` item in Checklist:\n{section}"
        );
    }

    /// D2: the `## Tarefas` placeholder is a PLAIN list — no `- [ ]` checkbox.
    /// Only `## Checklist` carries the tracked box. Asserted at the placeholder
    /// source so it holds regardless of which scope renders the section.
    #[test]
    fn tasks_placeholder_is_plain_list_no_checkbox() {
        let tasks = plan_section_default("tasks", Locale::PtBr);
        assert!(tasks.starts_with("- T1"), "Tarefas is a plain list item: {tasks:?}");
        assert!(!tasks.contains("[ ]"), "Tarefas must carry no checkbox: {tasks:?}");
    }

    /// D1: a wave-plan parent (every Full draft — `total_waves` forced ≥ 1)
    /// emits NEITHER `## Tarefas` nor `## Checklist` — both belong to the waves.
    #[test]
    fn wave_plan_parent_suppresses_tasks_and_checklist() {
        use mustard_core::domain::spec::contract::CHECKLIST_HEADING;
        let dir = tempdir().unwrap();
        let out = dir.path().join("specs").join("epic");
        run(SpecDraftOpts {
            intent: "Demo intent".into(),
            scope: "full".into(),
            lang: "pt-BR".into(),
            signals: None,
            output: Some(out.clone()),
            waves: 3,
            force: false,
            query_terms: None,
            force_scope: false,
        });
        let body = std::fs::read_to_string(out.join("spec.md")).unwrap();
        let checklist_heading = format!("## {CHECKLIST_HEADING}");
        assert!(
            !body.contains(&checklist_heading),
            "wave-plan parent must NOT emit `{checklist_heading}`:\n{body}"
        );
        // The Tarefas heading (PT-BR) must also be absent on the parent.
        assert!(
            !body.contains("## Tarefas"),
            "wave-plan parent must NOT emit `## Tarefas`:\n{body}"
        );
        // It still carries its other plan sections (Arquivos / Limites) — only
        // the actionable Tarefas/Checklist are suppressed.
        assert!(body.contains("## Arquivos"), "parent keeps Arquivos:\n{body}");
        assert!(body.contains("## Limites"), "parent keeps Limites:\n{body}");
    }

    #[test]
    fn section_heading_for_localises() {
        use crate::commands::spec::spec_scaffold::section_heading_for;
        // The canonical key is EN; the display heading is per-locale.
        assert_eq!(section_heading_for("context", Locale::EnUs), "Context");
        assert_eq!(section_heading_for("context", Locale::PtBr), "Contexto");
        // Unknown section name passes through unchanged.
        assert_eq!(section_heading_for("extra", Locale::EnUs), "extra");
    }

    /// Roundtrip AC-1 (TF 2026-06-10-ac-heading-unico): a VIRGIN draft — every
    /// scope × locale — carries exactly ONE AC heading in `spec.md` and passes
    /// its own `analyze-validation` with `ok: true` (zero issues). This is the
    /// regression the duplicated heading broke: `section_block` captured the
    /// placeholder section, `parse_ac_items` came back empty, and every fresh
    /// draft was born flagged `unparseable-ac`.
    #[test]
    fn roundtrip_virgin_draft_single_ac_heading_and_validation_ok() {
        use crate::commands::spec::spec_sections::is_heading;
        for (scope, lang, waves) in [
            ("light", "pt-BR", 0),
            ("light", "en-US", 0),
            ("full", "pt-BR", 2),
            ("full", "en-US", 2),
        ] {
            let dir = tempdir().unwrap();
            let out = dir.path().join("specs").join("rt");
            run(SpecDraftOpts {
                intent: "Demo roundtrip intent".into(),
                scope: scope.into(),
                lang: lang.into(),
                signals: None,
                output: Some(out.clone()),
                waves,
                force: false,
                query_terms: None,
                force_scope: false,
            });
            let spec_md = out.join("spec.md");
            let body = std::fs::read_to_string(&spec_md)
                .unwrap_or_else(|e| panic!("{scope}/{lang}: draft not written: {e}"));
            let ac_headings = body
                .lines()
                .filter(|l| is_heading(l, "acceptance-criteria"))
                .count();
            assert_eq!(
                ac_headings, 1,
                "{scope}/{lang}: exactly ONE AC heading expected:\n{body}"
            );
            let root = std::path::PathBuf::from(crate::shared::context::project_dir());
            let issues =
                crate::commands::review::analyze_validation::validate(&root, &spec_md, &body);
            assert!(
                issues.is_empty(),
                "{scope}/{lang}: virgin draft must validate ok:true — {issues:?}\n{body}"
            );
        }
    }

    #[test]
    fn writes_full_layout_end_to_end() {
        let dir = tempdir().unwrap();
        let opts = SpecDraftOpts {
            intent: "Demo intent".into(),
            scope: "full".into(),
            lang: "pt-BR".into(),
            signals: None,
            output: Some(dir.path().join("specs").join("demo")),
            waves: 2,
            force: false,
            query_terms: None,
            force_scope: false,
        };
        run(opts);
        let root = dir.path().join("specs").join("demo");
        assert!(root.join("spec.md").exists());
        assert!(root.join("meta.json").exists());
        // D6: a fresh draft no longer ships a `memory/_index.md` stub.
        assert!(!root.join("memory").join("_index.md").exists());
        // Wave dirs are NOT created by spec-draft — that is wave-scaffold's job.
        assert!(!root.join("wave-plan.md").exists());
        assert!(!root.join("wave-1-mixed").exists());
    }

    #[test]
    fn rejects_light_scope_short_lang() {
        let dir = tempdir().unwrap();
        let opts = SpecDraftOpts {
            intent: "Demo".into(),
            scope: "light".into(),
            lang: "pt".into(),
            signals: None,
            output: Some(dir.path().join("out")),
            waves: 0,
            force: false,
            query_terms: None,
            force_scope: false,
        };
        run(opts);
        // Output dir should not have been populated.
        assert!(!dir.path().join("out").join("spec.md").exists());
    }

    // --- Deterministic routing gate (apply_scope_gate) --------------------

    /// Plant a workspace anchor (`mustard.json` + `.claude/`) so
    /// `workspace_root` accepts the project root and a `## Files` census parses
    /// against a real (if model-less) project — mirrors scope_decompose's
    /// `plant_project`.
    fn plant_project(root: &std::path::Path) {
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
    }

    /// Write a synthetic `spec.md` + a Full `meta.json` under
    /// `{root}/.claude/spec/{slug}/` and return that spec dir. The census in
    /// `spec_body` drives `classify_from_spec`.
    fn seed_full_spec(root: &std::path::Path, slug: &str, spec_body: &str) -> std::path::PathBuf {
        let spec_dir = root.join(".claude").join("spec").join(slug);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), spec_body).unwrap();
        let full_input = build_input(
            slug, "Demo", Scope::Full, "en-US", 1, Locale::EnUs, "build",
        );
        let meta = build_meta_from_input(&full_input);
        spec_scaffold::write_meta_json(&spec_dir, &meta).unwrap();
        spec_dir
    }

    /// Count `pipeline.scope.*` events of `name` under the spec's `.events`.
    fn scope_event_count(spec_dir: &std::path::Path, name: &str) -> usize {
        let events_dir = spec_dir.join(".events");
        mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir)
            .iter()
            .filter(|e| e.event == name)
            .count()
    }

    /// The scope token persisted in `meta.json`.
    fn meta_scope(spec_dir: &std::path::Path) -> Option<String> {
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(spec_dir.join("meta.json")).unwrap())
                .unwrap();
        v.get("scope").and_then(|s| s.as_str()).map(str::to_string)
    }

    /// A genuinely multi-layer Full (3 distinct role buckets) is JUSTIFIED —
    /// the gate keeps `full`, rewrites no meta, emits no downgrade.
    #[test]
    fn scope_gate_keeps_justified_full() {
        let dir = tempdir().unwrap();
        plant_project(dir.path());
        let spec = "# S\n\n## Files\n\
            - backend/api/handler.rs\n\
            - core/schema/model.rs\n\
            - app/ui/view.tsx\n";
        let spec_dir = seed_full_spec(dir.path(), "justified", spec);
        let meta = mustard_core::read_meta(&spec_dir.join("meta.json")).unwrap();

        let out = apply_scope_gate(
            dir.path(), &spec_dir, "justified", Scope::Full, false, &meta, None,
        );
        assert!(out.is_none(), "justified full must not downgrade: {out:?}");
        assert_eq!(meta_scope(&spec_dir).as_deref(), Some("full"), "meta untouched");
        assert_eq!(scope_event_count(&spec_dir, "pipeline.scope.downgrade"), 0);
    }

    /// A net-new entity (Create-marked bullet corroborated by a prose token) is
    /// also a JUSTIFIED full even at a single layer — gate keeps full.
    #[test]
    fn scope_gate_keeps_full_on_net_new_entity() {
        let dir = tempdir().unwrap();
        plant_project(dir.path());
        let spec = "# S\nAdd the Invoice entity.\n\n## Files\n\
            - src/models/invoice.ts (create)\n";
        let spec_dir = seed_full_spec(dir.path(), "newent", spec);
        let meta = mustard_core::read_meta(&spec_dir.join("meta.json")).unwrap();

        let out = apply_scope_gate(
            dir.path(), &spec_dir, "newent", Scope::Full, false, &meta, None,
        );
        assert!(out.is_none(), "net-new entity ⇒ justified full: {out:?}");
        assert_eq!(meta_scope(&spec_dir).as_deref(), Some("full"));
    }

    /// A NON-justified Full (1 layer, ≤5 files, no net-new) is AUTO-REBAIXADO to
    /// light: returns `scopeDowngraded`, rewrites `meta.json#scope=light` (and
    /// clears the wave-plan fields), and emits a `pipeline.scope.downgrade`.
    #[test]
    fn scope_gate_downgrades_unjustified_full() {
        let dir = tempdir().unwrap();
        plant_project(dir.path());
        // Two files, ONE generic role bucket (`lib`) ⇒ layerCount 1, no net-new.
        let spec = "# S\n\n## Files\n- src/util/a.ts\n- src/util/b.ts\n";
        let spec_dir = seed_full_spec(dir.path(), "unjustified", spec);
        let meta = mustard_core::read_meta(&spec_dir.join("meta.json")).unwrap();

        let out = apply_scope_gate(
            dir.path(), &spec_dir, "unjustified", Scope::Full, false, &meta, None,
        );
        let downgrade = out.expect("unjustified full must downgrade");
        assert_eq!(downgrade["from"], json!("full"));
        assert_eq!(downgrade["to"], json!("light"));
        assert!(downgrade.get("reason").and_then(|r| r.as_str()).is_some());
        // meta.json is the source-of-truth the gate rewrites.
        assert_eq!(meta_scope(&spec_dir).as_deref(), Some("light"), "meta rewritten to light");
        // Light is never a wave plan — the wave-plan fields are cleared.
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(spec_dir.join("meta.json")).unwrap())
                .unwrap();
        assert!(v.get("totalWaves").is_none() || v["totalWaves"].is_null(), "no totalWaves on light: {v}");
        assert!(v.get("isWavePlan").is_none() || v["isWavePlan"].is_null(), "no isWavePlan on light: {v}");
        // The downgrade event is recorded.
        assert_eq!(scope_event_count(&spec_dir, "pipeline.scope.downgrade"), 1);
        assert_eq!(scope_event_count(&spec_dir, "pipeline.scope.override"), 0);
    }

    /// `--force-scope` over a non-justified Full HONOURS the full but records a
    /// `pipeline.scope.override` event — the override is auditable, not silent.
    /// No `scopeDowngraded`; meta.json stays `full`.
    #[test]
    fn scope_gate_force_scope_overrides_and_records() {
        let dir = tempdir().unwrap();
        plant_project(dir.path());
        let spec = "# S\n\n## Files\n- src/util/a.ts\n- src/util/b.ts\n";
        let spec_dir = seed_full_spec(dir.path(), "forced", spec);
        let meta = mustard_core::read_meta(&spec_dir.join("meta.json")).unwrap();

        let out = apply_scope_gate(
            dir.path(), &spec_dir, "forced", Scope::Full, /* force_scope */ true, &meta, None,
        );
        assert!(out.is_none(), "--force-scope ⇒ no downgrade: {out:?}");
        assert_eq!(meta_scope(&spec_dir).as_deref(), Some("full"), "meta stays full under override");
        assert_eq!(scope_event_count(&spec_dir, "pipeline.scope.override"), 1, "override recorded");
        assert_eq!(scope_event_count(&spec_dir, "pipeline.scope.downgrade"), 0);
    }

    /// A `light`/`extended-light` REQUEST is left untouched — the gate only acts
    /// on an unjustified full, never on an already-economical request.
    #[test]
    fn scope_gate_noop_on_light_request() {
        let dir = tempdir().unwrap();
        plant_project(dir.path());
        let spec = "# S\n\n## Files\n- src/util/a.ts\n";
        let spec_dir = seed_full_spec(dir.path(), "lightreq", spec);
        let meta = mustard_core::read_meta(&spec_dir.join("meta.json")).unwrap();

        let out = apply_scope_gate(
            dir.path(), &spec_dir, "lightreq", Scope::Light, false, &meta, None,
        );
        assert!(out.is_none(), "light request ⇒ no-op: {out:?}");
        assert_eq!(scope_event_count(&spec_dir, "pipeline.scope.downgrade"), 0);
        assert_eq!(scope_event_count(&spec_dir, "pipeline.scope.override"), 0);
    }

    /// FAIL-OPEN: a non-confident classification (the `## Files` census is a
    /// placeholder ⇒ `fileCount=0` ⇒ `filesSectionEmpty`) must NOT downgrade —
    /// a freshly-drafted Full whose census has not landed keeps its full.
    #[test]
    fn scope_gate_does_not_downgrade_non_confident_empty_census() {
        let dir = tempdir().unwrap();
        plant_project(dir.path());
        // `## Files` present but only a placeholder line ⇒ zero parsed paths.
        let spec = "# S\n\n## Files\n_(a preencher após o censo)_\n";
        let spec_dir = seed_full_spec(dir.path(), "premature", spec);
        let meta = mustard_core::read_meta(&spec_dir.join("meta.json")).unwrap();

        let out = apply_scope_gate(
            dir.path(), &spec_dir, "premature", Scope::Full, false, &meta, None,
        );
        assert!(out.is_none(), "non-confident verdict must not downgrade: {out:?}");
        assert_eq!(meta_scope(&spec_dir).as_deref(), Some("full"), "full preserved on placeholder census");
        assert_eq!(scope_event_count(&spec_dir, "pipeline.scope.downgrade"), 0);
    }

    /// FAIL-OPEN: an unreadable spec.md classifies to the conservative `full`
    /// (classify_from_spec's fallback), which never triggers a downgrade.
    #[test]
    fn scope_gate_fail_open_on_unreadable_spec() {
        let dir = tempdir().unwrap();
        plant_project(dir.path());
        // Spec dir + meta but NO spec.md → classify_from_spec falls open to full.
        let spec_dir = dir.path().join(".claude").join("spec").join("ghost");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let full_input = build_input(
            "ghost", "Demo", Scope::Full, "en-US", 1, Locale::EnUs, "build",
        );
        let meta = build_meta_from_input(&full_input);
        spec_scaffold::write_meta_json(&spec_dir, &meta).unwrap();

        let out = apply_scope_gate(
            dir.path(), &spec_dir, "ghost", Scope::Full, false, &meta, None,
        );
        assert!(out.is_none(), "unreadable spec ⇒ conservative full, no downgrade: {out:?}");
        assert_eq!(meta_scope(&spec_dir).as_deref(), Some("full"));
    }
}
