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
//!     [--slug  <the name the base gate minted>] \
//!     [--signals layers,files,...] \
//!     [--output PATH] \
//!     [--plan PATH]
//! ```
//!
//! ## The unit is NAMED before the draft runs
//!
//! `--intent` is the spec TITLE. It is no longer where the unit's name comes
//! from on the pipeline path: the base gate mints that name
//! (`emit-pipeline --kind pipeline.kind`, which reports it as `spec`) and this
//! command CONSUMES it — `--slug` verbatim, or, when the flag is absent, the
//! slug half of the work branch the cut below just put the tree on. Deriving a
//! second name from `--intent` is the last resort, for a hand-run draft that no
//! work unit ever signalled. See [`resolve_slug`].
//!
//! ## The branch is cut FIRST
//!
//! The draft is the first thing a work unit produces, and the unit IS its
//! branch — so before a byte is written this command consumes the session's
//! pending work-branch marker and checks that branch out (see
//! [`cut_work_branch`]). Everything below then lands inside the unit: the
//! `spec.md`, the wave layout and the negative proof. Nothing is written when
//! the checkout fails on a protected integration base.
//!
//! ## Output
//!
//! When `--output PATH` is omitted, the new spec lands under
//! `.claude/spec/{slug}/` (`slug` per [`resolve_slug`]).
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
//! ## `--plan` — the fused first materialisation
//!
//! Without `--plan` the command writes only the top-level `spec.md` +
//! `meta.json`; the Full-scope wave decomposition (`wave-plan.md` +
//! `wave-N-{role}/spec.md` + sidecars) is materialised later, by
//! `plan-materialize`.
//!
//! With `--plan <FILE>` the two steps become one call. The command already
//! RECORDS the wave decision (`meta.json#totalWaves`), and requiring a second
//! invocation to act on what it just recorded is the ceremony this flag removes:
//! the plan's `acceptance` lines become the spec's acceptance criteria (see
//! [`adopt_plan_acceptance_criteria`]) and the SAME in-process composite
//! `plan-materialize` runs — wave-scaffold renderer, analyze-validation, the
//! dependency DAG, the negative proof, the `pipeline.scope` + PLAN emits — via
//! [`plan_materialize::materialize_fresh`]. A gate refusal exits 2 and takes the
//! layout back down with it, so a retry never meets a directory this run
//! half-built. `plan-materialize` is unchanged and remains the
//! RE-materialisation door for a layout reconciled onto an edited plan.
//!
//! Idempotent: if `output` already exists, the writer refuses to overwrite
//! unless `--force` is passed. Fail-open per file write (a single failure is
//! reported but does not abort the rest of the layout).

use crate::shared::context::{project_dir, session_id};
use crate::commands::pipeline::plan_materialize;
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
use serde::Deserialize;
use serde_json::json;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
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
    /// Free-text intent — the spec TITLE, and the LAST-RESORT slug seed (see
    /// [`resolve_slug`]).
    pub intent: String,
    /// The unit's canonical name, minted at the base gate and reported there as
    /// `spec`. Present ⇒ used VERBATIM: the draft consumes the name the unit
    /// already carries instead of deriving a second one. Absent ⇒ the work
    /// branch, then the intent — see [`resolve_slug`].
    pub slug: Option<String>,
    /// `light` or `full`.
    pub scope: String,
    /// `pt-BR` / `en-US` (BCP-47 only — short forms rejected).
    pub lang: String,
    /// Optional comma-separated signals (e.g. `layers,files,registry`).
    pub signals: Option<String>,
    /// Optional output directory. Defaults to `.claude/spec/{slug}/`.
    pub output: Option<PathBuf>,
    /// Optional path to the CONVERSATION MATERIAL file — the channel that
    /// carries what the discussion established into the drafted spec (see
    /// [`ConversationMaterial`]). A FILE, not a flag value: the payload holds
    /// newlines, quotes and non-ASCII, none of which survive a shell argument
    /// intact. Absent (or carrying nothing) ⇒ the draft is byte-identical to a
    /// draft without the channel.
    pub material: Option<PathBuf>,
    /// Waves recorded in `meta.json#totalWaves` under Full scope (default 1).
    /// The wave dirs themselves are materialised by `wave-scaffold`.
    pub waves: u32,
    /// Optional path to the plan JSON the Plan agent authored. Present ⇒ the
    /// draft is FUSED with the PLAN-phase materialisation: after `spec.md` +
    /// `meta.json` land, this same invocation runs the composite
    /// [`crate::commands::pipeline::plan_materialize::materialize_fresh`], so
    /// one call produces `wave-plan.md` and every wave directory with the
    /// negative proof taken in the same pass. Absent ⇒ the command behaves
    /// exactly as it always has.
    pub plan: Option<PathBuf>,
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
/// itself is drafted: the per-spec NDJSON event log, the dispatch sidecar and
/// the cut's own record of the base it cut from. Opening a work unit emits the
/// first event, which creates `<spec>/.events/`; the draft then arrives to find
/// "its own" directory already there.
///
/// [`CUT_BASE_FILE`] earns its place on the SAME terms as the other two, and it
/// is named here rather than spelled a second time: it is written by the CUT,
/// which runs before this command exists to be blocked by it, it holds one
/// machine token the harness wrote to itself (an integration base name), and
/// this command RETIRES it — [`spec_scaffold::write_meta_json`] folds it into
/// `meta.json#base` and removes it. Nobody authors it and it never reaches the
/// merge; the authored work of a unit is `spec.md`, its waves, its proof, its
/// change log and its review verdicts, and every one of those still refuses.
const HARNESS_STATE_ENTRIES: &[&str] =
    &[".events", ".dispatch", crate::shared::work_kind::CUT_BASE_FILE];

/// `true` when `dir` exists but holds NOTHING except the harness state listed in
/// [`HARNESS_STATE_ENTRIES`] — i.e. no spec has been drafted into it yet.
///
/// Creating the work unit and drafting its spec are two steps of one sequence,
/// and the first used to block the second: the event log landed in the spec
/// directory, `output.exists()` fired, and the draft refused with "pass
/// `--force` to overwrite" — an overwrite flag demanded for a directory holding
/// nothing to overwrite. Anything else present (a `spec.md`, a `meta.json`, a
/// wave dir, a stray file) is a REAL draft the guard must still protect.
///
/// That is why the cut records its base as [`HARNESS_STATE_ENTRIES`]' third
/// entry and NOT as a `meta.json`: a sidecar written by step one is read here as
/// step two's own output, and the unit came out cut and spec-less.
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

// ---------------------------------------------------------------------------
// The conversation channel
// ---------------------------------------------------------------------------

/// One term the conversation defined, and what it means HERE. A definition is
/// the cheapest thing to lose and the most expensive to re-derive: every
/// implementer that does not have it invents its own.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    /// The term as the conversation used it.
    term: String,
    /// What it means in THIS spec.
    meaning: String,
}

/// One decision and the REASON it was taken. The reason is not decoration: a
/// decision without it is exactly the thing a later reader cannot use — they
/// can see WHAT was chosen and have no way to tell whether the choice still
/// holds. That is why [`load_material`] refuses a reason-less decision instead
/// of carrying half of it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    /// What was decided.
    decision: String,
    /// Why it was decided that way.
    reason: String,
}

/// One verified statement plus the evidence that makes it CHECKABLE: the file
/// it was read at, and the line when the claim is line-precise. A statement
/// with no file is an opinion; the loader refuses it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Finding {
    /// What was verified.
    statement: String,
    /// The file the claim was checked against.
    file: String,
    /// The line, when the claim is line-precise (a file-level claim omits it).
    #[serde(default)]
    line: Option<u32>,
}

/// The structured material a conversation produced, carried into the draft by
/// `spec-draft --material <FILE>`.
///
/// Three kinds because they BEHAVE differently: a definition is a term and its
/// local meaning, a decision is a choice plus its reason, a finding is a claim
/// plus the file (and line) it was checked at. Each lands in a section of its
/// own — never in the prose-only opening section, which is precisely where the
/// material used to be crammed and then rejected.
///
/// `deny_unknown_fields` is deliberate: a mistyped key (`decision` for
/// `decisions`) would otherwise deserialise into an empty channel and drop the
/// material silently — the very defect this file is closing.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConversationMaterial {
    #[serde(default)]
    definitions: Vec<Definition>,
    #[serde(default)]
    decisions: Vec<Decision>,
    #[serde(default)]
    findings: Vec<Finding>,
}

impl ConversationMaterial {
    /// `true` when the channel carries nothing — the draft must then be
    /// byte-identical to a draft with no channel at all.
    fn is_empty(&self) -> bool {
        self.definitions.is_empty() && self.decisions.is_empty() && self.findings.is_empty()
    }
}

/// EN display headings for the three material sections. Language-agnostic on
/// purpose — the same reasoning as `CHECKLIST_HEADING`: these sections are read
/// by machinery (the per-wave cut, the QA extractor), so every consumer keys
/// off ONE literal. `spec_sections::variants` registers both the EN and PT
/// spellings so a hand-authored spec still resolves through the shared
/// resolver.
const DEFINITIONS_HEADING: &str = "Definitions";
const DECISIONS_HEADING: &str = "Decisions";
const EVIDENCE_HEADING: &str = "Evidence";

/// Read and check the conversation-material channel at `path`.
///
/// FAIL-CLOSED, unlike most of this command: the operator handed over material
/// explicitly, so a malformed file must stop the draft rather than degrade to
/// an empty channel. Degrading here would reproduce the defect — the material
/// vanishes and nobody is told.
///
/// # Errors
///
/// The file could not be read, is not the expected JSON shape, or carries an
/// entry missing the half that makes it usable (a definition with no meaning,
/// a decision with no reason, a finding with no file).
fn load_material(path: &Path) -> Result<ConversationMaterial, String> {
    let raw = mfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let material: ConversationMaterial =
        serde_json::from_str(&raw).map_err(|e| format!("{}: {e}", path.display()))?;
    for (i, d) in material.definitions.iter().enumerate() {
        if d.term.trim().is_empty() || d.meaning.trim().is_empty() {
            return Err(format!(
                "definitions[{i}]: a definition needs both the term and what it means here"
            ));
        }
    }
    for (i, d) in material.decisions.iter().enumerate() {
        if d.decision.trim().is_empty() || d.reason.trim().is_empty() {
            return Err(format!(
                "decisions[{i}]: a decision without its reason is unusable to a later reader"
            ));
        }
    }
    for (i, f) in material.findings.iter().enumerate() {
        if f.statement.trim().is_empty() || f.file.trim().is_empty() {
            return Err(format!(
                "findings[{i}]: a finding needs a statement and the file it was checked at"
            ));
        }
    }
    Ok(material)
}

/// Render the material as markdown sections — one `## ` heading per kind that
/// carries something, and NOTHING for a kind that does not. Pure (no I/O) so it
/// is unit-testable. Returns `None` for an empty channel, which is what makes
/// the feature invisible when unused.
///
/// Each item follows the project's established "bullet + attribute line" shape
/// (the same one `- **AC-N** — …` / `  Command: \`…\`` uses), so the per-wave
/// cut can lift a finding's file out of `Evidence:` without a bespoke grammar.
fn render_material_sections(material: &ConversationMaterial) -> Option<String> {
    if material.is_empty() {
        return None;
    }
    let mut block = String::new();
    if !material.definitions.is_empty() {
        let _ = write!(block, "\n## {DEFINITIONS_HEADING}\n\n");
        for d in &material.definitions {
            let _ = writeln!(block, "- **{}** — {}", d.term.trim(), d.meaning.trim());
        }
    }
    if !material.decisions.is_empty() {
        let _ = write!(block, "\n## {DECISIONS_HEADING}\n\n");
        for d in &material.decisions {
            let _ = writeln!(block, "- {}\n  Reason: {}", d.decision.trim(), d.reason.trim());
        }
    }
    if !material.findings.is_empty() {
        let _ = write!(block, "\n## {EVIDENCE_HEADING}\n\n");
        for f in &material.findings {
            let at = match f.line {
                Some(line) => format!("{}:{line}", f.file.trim()),
                None => f.file.trim().to_string(),
            };
            let _ = writeln!(block, "- {}\n  Evidence: `{at}`", f.statement.trim());
        }
    }
    Some(block)
}

/// Splice the material sections onto the `spec.md` [`spec_scaffold::write_spec_md`]
/// just wrote.
///
/// Appended by the DRAFTER rather than threaded through the scaffold on
/// purpose: the scaffold owns the canonical layout shared with
/// `tactical-fix-create`, and the channel is `spec-draft`'s own concern. It
/// also makes the "invisible when unused" rule structural instead of
/// conditional — an empty channel performs no second write at all, so the
/// bytes cannot drift.
///
/// # Errors
///
/// The freshly-written `spec.md` could not be read back or rewritten.
fn append_material_sections(output: &Path, material: &ConversationMaterial) -> Result<(), String> {
    let Some(block) = render_material_sections(material) else {
        return Ok(());
    };
    let path = output.join("spec.md");
    let mut body = mfs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(&block);
    mfs::write_atomic(&path, body.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
}

/// Entry point — resolves the project root from the process context and maps
/// the run's outcome to the process exit code.
///
/// Exit 2 belongs to ONE outcome: the fused `--plan` materialisation refused
/// (see [`run_at`]). Every other failure keeps printing `{"ok": false, …}` and
/// exiting 0, exactly as it always has.
pub fn run(opts: SpecDraftOpts) {
    let project = PathBuf::from(project_dir());
    let code = run_at(&project, opts);
    if code != 0 {
        std::process::exit(code);
    }
}

/// The command's whole body, against an EXPLICIT project root — so a test can
/// drive the fused `--plan` path end to end without the composite reaching out
/// of its tempdir into the real checkout the process happens to sit in.
///
/// Returns the exit code [`run`] applies: `2` when the fused materialisation
/// refused, `0` otherwise.
pub(crate) fn run_at(project_root: &Path, opts: SpecDraftOpts) -> i32 {
    let Some(scope) = Scope::parse(&opts.scope) else {
        emit_error("invalid --scope (expected `light` or `full`)", &opts.scope);
        return 0;
    };
    let Ok(lang_locale) = Locale::from_str(&opts.lang) else {
        emit_error("invalid --lang (expected BCP-47 `pt-BR` or `en-US`)", &opts.lang);
        return 0;
    };
    // The channel is read and checked BEFORE anything is written: a malformed
    // material file must not leave a half-drafted spec behind.
    let material = match opts.material.as_deref() {
        Some(path) => match load_material(path) {
            Ok(m) => m,
            Err(detail) => {
                emit_error("invalid --material", &detail);
                return 0;
            }
        },
        None => ConversationMaterial::default(),
    };

    // ---- The unit's branch is cut BEFORE a single byte is written. ----
    //
    // The spec, its wave layout and its negative proof are the first things the
    // work produces, and they belong to the unit — which IS the branch. They
    // used to land on the integration base the analysis was approved from (a
    // `.claude/spec/` carve-out in `work_branch_gate` existed precisely to let
    // them). Cutting here puts the whole layout inside the branch in this one
    // call, and keeps the proof's premise intact: the branch is cut off a fresh
    // base, before wave 1, so the code the criteria describe still does not
    // exist when `ac-negative-check` runs below.
    let work_branch = match cut_work_branch(project_root) {
        Ok(branch) => branch,
        Err(detail) => {
            emit_error("could not cut the work branch for this unit", &detail);
            return 0;
        }
    };

    // The unit's name — CONSUMED, not invented, whenever the unit already has
    // one. Resolved after the cut because the BRANCH is what remembers it: the
    // one just cut, or the one already under the tree when the auto-branch hook
    // cut it first (which is what happens on every shipped run).
    let slug = resolve_slug(
        project_root,
        opts.slug.as_deref(),
        work_branch.as_deref(),
        &opts.intent,
        lang_locale,
    );
    if slug.is_empty() {
        emit_error("intent did not yield a slug", &opts.intent);
        return 0;
    }

    let auto_output = opts.output.is_none();
    let output = opts.output.unwrap_or_else(|| {
        ClaudePaths::for_project(project_root)
            .and_then(|p| p.for_spec(&slug))
            .map(|sp| sp.dir().to_path_buf())
            .unwrap_or_else(|_| {
                ClaudePaths::compose_unchecked(project_root)
                    .spec_dir()
                    .join(&slug)
            })
    });

    // A directory holding only the harness's own event log is not a drafted
    // spec — see [`holds_only_harness_state`]. Everything else still refuses.
    if output.exists() && !opts.force && !holds_only_harness_state(&output) {
        emit_error("output exists; pass --force to overwrite", &output.display().to_string());
        return 0;
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
                return 0;
            }
        }
    }
    if let Err(e) = mfs::create_dir_all(&output) {
        emit_error("could not create output directory", &e.to_string());
        return 0;
    }

    // ---- Resolve the project build command (AC default) from mustard.json. ----
    // No hardcoded `rtk cargo build`: the AC runs the project's own build, or a
    // neutral placeholder the user fills in when no `buildCommand` is set.
    let build_command =
        mustard_core::ProjectConfig::load(project_root).build_command_or_fallback();

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
    let digest = scan_digest(project_root, &opts.intent, opts.query_terms.as_deref());
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
        return 0;
    }

    // ---- Resolve tone from mustard.json (wired into the drafter prompt). ----
    let tone = mustard_core::ProjectConfig::load(project_root).i18n().tone;

    // ---- Materialise files. ----
    let mut written: Vec<String> = Vec::new();
    if let Err(e) = spec_scaffold::write_spec_md(&output, &input, &opts.signals, lang_locale, tone) {
        emit_error("write spec.md", &e);
        return 0;
    }
    written.push(output.join("spec.md").display().to_string());

    // The plan's own acceptance criteria supersede the skeleton the draft seeds
    // — see [`adopt_plan_acceptance_criteria`]. A no-op without `--plan`.
    if let Some(plan) = opts.plan.as_deref() {
        if let Err(e) = adopt_plan_acceptance_criteria(&output, plan) {
            emit_error("adopt plan acceptance criteria", &e);
            return 0;
        }
    }

    // The conversation channel — each kind in a section of its own. A no-op
    // when nothing was carried.
    if let Err(e) = append_material_sections(&output, &material) {
        emit_error("write conversation material", &e);
        return 0;
    }

    let meta = build_meta_from_input(&input);
    if let Err(e) = spec_scaffold::write_meta_json(&output, &meta) {
        emit_error("write meta.json", &e);
        return 0;
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
        apply_scope_gate(project_root, &output, &slug, scope, opts.force_scope, &meta, digest.as_ref());

    // Record the `ANALYZE` phase now that the slug exists (see
    // [`backfill_analyze_phase`]).
    backfill_analyze_phase(project_root, &slug);

    // D6: the `memory/_index.md` is NOT born at draft time. A fresh spec used to
    // ship an empty stub (and, before the i18n keys existed, a `<missing-key>`
    // line). The index is now born on the FIRST knowledge capture, so an unused
    // spec carries no orphan index file.

    // ---- The FUSED materialisation (`--plan`). ----
    // Without `--plan` the command stops here: `meta.json` records
    // `scope=full` + `totalWaves` + `isWavePlan`, and the layout is the
    // re-materialisation door's job. WITH `--plan` the command acts on the wave
    // count it just recorded instead of asking for a second invocation to do
    // it: the same in-process composite `plan-materialize` performs (the
    // wave-scaffold renderer, analyze-validation, the dependency DAG, the
    // negative proof, the `pipeline.scope` + PLAN emits) runs here, and a
    // refusal takes the layout back down with it.
    let materialize = opts.plan.as_deref().map(|plan| {
        let plan_path = if plan.is_absolute() {
            plan.to_path_buf()
        } else {
            project_root.join(plan)
        };
        let report = plan_materialize::materialize_fresh(project_root, &output, &plan_path);
        let refused = plan_materialize::refused(&report);
        // Everything the composite left standing is part of what THIS call
        // produced, so it joins the same `files` list as spec.md / meta.json.
        // Nothing is listed after a refusal — the rollback removed it.
        if !refused {
            for rel in report["scaffold"]["created_files"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
            {
                written.push(output.join(rel).display().to_string());
            }
        }
        (report, refused)
    });

    // The effective scope is the downgraded one when the gate acted, so the
    // report's `scope` matches the meta.json the gate rewrote (no contradiction
    // between stdout and the persisted source-of-truth).
    let effective_scope = scope_downgraded
        .as_ref()
        .and_then(|d| d.get("to").and_then(serde_json::Value::as_str))
        .unwrap_or_else(|| scope_str(scope));
    let refused = materialize.as_ref().is_some_and(|(_, refused)| *refused);
    let mut report = json!({
        "ok": !refused,
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
    // The branch this layout was written INTO — present only when a work unit
    // was signalled, so a hand-run draft's report is byte-identical to before.
    if let (Some(obj), Some(branch)) = (report.as_object_mut(), work_branch) {
        obj.insert("workBranch".to_string(), json!(branch));
    }
    // What the channel actually carried — so a material file that was read but
    // yielded nothing is visible in the report instead of looking like success.
    if let (Some(obj), false) = (report.as_object_mut(), material.is_empty()) {
        obj.insert(
            "material".to_string(),
            json!({
                "definitions": material.definitions.len(),
                "decisions": material.decisions.len(),
                "findings": material.findings.len(),
            }),
        );
    }
    // The scan anchors ride the REPORT, not the artifact — the orchestrator
    // reads them to decide what to open, and `## Context` stays prose.
    if let (Some(obj), Some(anchors)) = (report.as_object_mut(), scan_anchors) {
        obj.insert("scanAnchors".to_string(), json!(anchors));
    }
    // The composite's own report travels whole, under its own key — the reader
    // that has to fix an unproven criterion needs the `proof` slot verbatim, and
    // summarising it here would be a second spelling of the same verdict.
    if let (Some(obj), Some((composite, _))) = (report.as_object_mut(), materialize) {
        obj.insert("materialize".to_string(), composite);
    }
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    i32::from(refused) * 2
}

/// Cut this session's pending work branch so everything the draft writes lands
/// INSIDE the unit — the same cut [`crate::hooks::write::work_branch_gate`]
/// performs on a file mutation, taken here because `spec-draft` writes through
/// the filesystem and no PreToolUse hook ever sees it.
///
/// `Ok(Some(branch))` — the unit's branch is the checkout (cut now, or already
/// there). `Ok(None)` — no work unit was signalled at all (a hand-run draft, a
/// test): nothing was promised, so nothing is enforced.
///
/// `Err(detail)` covers the two outcomes the draft must not survive:
///
/// 1. the checkout holds ANOTHER unit's branch with uncommitted work, so the
///    cut was REFUSED ([`crate::commands::event::work_branch::busy_checkout`]).
///    Proceeding would write this unit's spec, waves and proof onto the other
///    unit's branch — and drafting is the moment that arrangement is decided,
///    because this door opens before any `Write` reaches the hook gate. The
///    refusal is surfaced verbatim: it already names the branch, the paths and
///    the act that unblocks it.
/// 2. git refused the checkout *while the tree sits on a protected integration
///    base*, so writing here would put the unit's artefacts on the base — the
///    exact arrangement this reorder removes. A git refusal anywhere else
///    (another work branch, a linked worktree, a detached HEAD) warns on stderr
///    and proceeds: nothing is landing on a base, so there is nothing to refuse.
///    This mirrors the hook gate's own split, which denies only when staying
///    would leave the edit on a protected branch.
///
/// The same split covers [`CutOutcome::BaseUnknown`] — an emergency whose base
/// nothing recorded, in a project declaring several. No cut was attempted there
/// (guessing which base an emergency came from is the thing that refuses to
/// happen), so the tree is wherever it was, and the same question decides:
/// on a base, refuse; anywhere else, say so and draft.
fn cut_work_branch(project_root: &Path) -> Result<Option<String>, String> {
    use crate::commands::event::work_branch::{cut_pending_work_branch, is_protected, CutOutcome};

    match cut_pending_work_branch(project_root, &session_id()) {
        CutOutcome::NoPending => Ok(None),
        CutOutcome::AlreadyThere(branch) | CutOutcome::Cut(branch) => Ok(Some(branch)),
        CutOutcome::Refused(busy) => {
            let lang = mustard_core::ProjectConfig::load(project_root).i18n().lang;
            Err(busy.reason(lang))
        }
        CutOutcome::BaseUnknown { target, current, candidates } => {
            let config = mustard_core::ProjectConfig::load(project_root);
            let said = translate("workbranch.base.unknown", config.i18n().lang)
                .replace("{target}", &target)
                .replace("{candidates}", &candidates.join(", "));
            if current.as_deref().is_some_and(|b| is_protected(project_root, b, &config)) {
                // Same split as a failed checkout, for the same reason: the tree
                // sits on an integration base, so drafting here would leave the
                // spec, its waves and its proof on the base instead of in the
                // unit. The other positions are somebody's work branch — the
                // draft lands there and the operator is told on stderr.
                Err(said)
            } else {
                eprintln!("spec-draft: WARN: {said}");
                Ok(None)
            }
        }
        CutOutcome::Failed { target, current, error } => {
            let config = mustard_core::ProjectConfig::load(project_root);
            let at = current.as_deref().unwrap_or("?");
            if current.as_deref().is_some_and(|b| is_protected(project_root, b, &config)) {
                Err(format!(
                    "'{at}' is an integration base and checking out '{target}' failed ({error}); \
                     drafting here would leave the spec, its waves and its proof on the base \
                     instead of in the unit. Resolve the git state and draft again."
                ))
            } else {
                eprintln!(
                    "spec-draft: WARN: could not check out '{target}' ({error}); the draft is \
                     written on '{at}', which is not an integration base."
                );
                Ok(None)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The fused plan channel
// ---------------------------------------------------------------------------

/// Replace the drafted skeleton `## Acceptance Criteria` block with the criteria
/// the PLAN declares, when `--plan` names a plan that carries any.
///
/// A fresh draft seeds three EARS skeletons whose command is the literal
/// `<runnable command that verifies this criterion>` placeholder — the negative
/// proof recognises that marker and records the criterion as UNPROVEN without
/// running anything, by design. So on the fused path the plan's own `acceptance`
/// lines are the criteria: the skeleton exists precisely as the placeholder the
/// Plan agent fills, and the plan file is where it filled them. Without this the
/// fused call could only ever refuse itself.
///
/// The lines are carried VERBATIM, in wave order, de-duplicated, and bullet-
/// normalised — the same reduction [`crate::commands::wave::wave_scaffold`]
/// applies when it synthesizes the global block into `wave-plan.md`, so the
/// parent `spec.md` and `wave-plan.md` state the same criteria in the same
/// words. Any `Command:` / `Expect:` / `Control:` continuation the plan wrote
/// into the line survives untouched; nothing is re-derived.
///
/// Returns `Ok(true)` when the block was replaced. A plan that declares no
/// acceptance line, a plan that cannot be read, or a `spec.md` with no AC
/// heading all leave the draft exactly as it was (`Ok(false)`) — the composite
/// that runs next is the one that judges the plan, and pre-empting its refusal
/// with a different one here would move the verdict away from the gate.
///
/// # Errors
///
/// The freshly-written `spec.md` could not be read back or rewritten.
fn adopt_plan_acceptance_criteria(output: &Path, plan: &Path) -> Result<bool, String> {
    let Some(bullets) = plan_acceptance_bullets(plan) else {
        return Ok(false);
    };
    let path = output.join("spec.md");
    let body = mfs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let Some(replaced) = replace_acceptance_block(&body, &bullets) else {
        return Ok(false);
    };
    mfs::write_atomic(&path, replaced.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(true)
}

/// The union of every wave's `acceptance` lines from the plan JSON, in wave
/// order, de-duplicated and bullet-normalised. `None` when the file cannot be
/// read/parsed or declares no acceptance line at all.
fn plan_acceptance_bullets(plan: &Path) -> Option<Vec<String>> {
    let raw = mfs::read_to_string(plan).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let mut bullets: Vec<String> = Vec::new();
    for wave in doc.get("waves")?.as_array()? {
        for ac in wave
            .get("acceptance")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
        {
            let trimmed = ac.trim();
            if trimmed.is_empty() {
                continue;
            }
            let bullet = if trimmed.starts_with('-') {
                trimmed.to_string()
            } else {
                format!("- {trimmed}")
            };
            if !bullets.contains(&bullet) {
                bullets.push(bullet);
            }
        }
    }
    (!bullets.is_empty()).then_some(bullets)
}

/// Whether a line is a STRUCTURAL marker of the document rather than content of
/// whichever section it happens to sit in.
///
/// The two dividers belong to `spec.md` as a whole — consumers slice the file at
/// them — so a section rewrite must carry them across instead of treating them
/// as body it may drop. Compared trimmed, because the renderer is free to indent.
fn is_structural_marker(line: &str) -> bool {
    let t = line.trim();
    t == mustard_core::domain::spec::contract::PLAN_DIVIDER
        || t == mustard_core::domain::spec::contract::PRD_DIVIDER
}

/// Swap the body of `body`'s `## Acceptance Criteria` section for `bullets`,
/// keeping the heading line the renderer wrote (it is localised) and everything
/// before and after the section byte-identical. `None` when the document
/// carries no such heading.
fn replace_acceptance_block(body: &str, bullets: &[String]) -> Option<String> {
    use crate::commands::spec::spec_sections::is_heading;
    let lines: Vec<&str> = body.lines().collect();
    let start = lines
        .iter()
        .position(|l| is_heading(l, "acceptance-criteria"))?;
    // The section runs to the next `## ` heading of any kind, or to the end.
    let end = lines
        .iter()
        .skip(start + 1)
        .position(|l| l.starts_with("## "))
        .map_or(lines.len(), |offset| start + 1 + offset);
    // Everything between the two headings is the section BODY and is replaced —
    // except the structural markers, which belong to the document rather than to
    // any section. The PRD/PLAN divider sits exactly there on a Full draft, and
    // dropping it is not cosmetic: the dashboard slices the PRD at that comment
    // and renders an empty tab without it, permanently, because `spec.md` is
    // written by this command alone and no later pass restores it (found in
    // review, 2026-07-30 — the fused path made this the DEFAULT door).
    let carried: Vec<String> = lines[start + 1..end]
        .iter()
        .filter(|l| is_structural_marker(l))
        .map(|l| (*l).to_string())
        .collect();
    let mut out: Vec<String> = lines[..=start].iter().map(|l| (*l).to_string()).collect();
    out.push(String::new());
    out.extend(bullets.iter().cloned());
    out.push(String::new());
    for marker in carried {
        out.push(marker);
        out.push(String::new());
    }
    out.extend(lines[end..].iter().map(|l| (*l).to_string()));
    let mut joined = out.join("\n");
    if body.ends_with('\n') {
        joined.push('\n');
    }
    Some(joined)
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
        dropped: None,
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
fn scan_digest(project_root: &Path, intent: &str, query_terms: Option<&str>) -> Option<DigestQuery> {
    let model = project_root.join(".claude").join("grain.model.json");
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
        // The base the unit was cut from is not an input to the draft — only
        // the CUT knows it, and it wrote it down before this call. `None` here
        // is what `write_meta_json` reads as "carry over what is on disk".
        base: None,
        is_wave_plan: input.total_waves.map(|n| n > 0),
        total_waves: input.total_waves,
        // A freshly drafted spec carries no qualifier flag (Plan/Active).
        flags: mustard_core::MetaFlags::default(),
        // The trackable checklist lives in the spec markdown at draft time and
        // in each WAVE's sidecar after the scaffold — never in the root meta
        // (explicit OUT of the checklist-progresso spec).
        checklist: Vec::new(),
        // Findings are seeded by the collector from what the review and the
        // proof ledger actually recorded — never invented at draft time.
        findings: Vec::new(),
        raw: serde_json::Value::Null,
    }
}

// D6: the `memory/_index.md` is no longer materialised at draft time (the old
// `write_memory_stub` shipped an empty stub on every spec). The index is now
// created/updated on the first knowledge capture.

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The unit's name, asked of whatever actually KNOWS it, in that order:
///
/// 1. `--slug` — the name the base gate minted for this unit
///    ([`crate::commands::event::emit_pipeline::mint_unit_name_at`]). Verbatim:
///    the draft is consuming a decision, not making one.
/// 2. the unit's BRANCH — `{base}_{slug}`, so its slug half IS the gate's name
///    ([`work_branch::slug_of_work_branch`]). Asked of the branch this call just
///    cut when it cut one, and otherwise of the CHECKOUT: the session marker
///    that carried the name from the gate is consumed by whoever cuts first, and
///    on the shipped path that is the auto-branch hook on the flow's first
///    write — long before the draft runs. So [`cut_work_branch`] answers `None`
///    there, and reading only its answer would leave this leg unreachable on
///    exactly the path it exists for. The branch is what still remembers.
/// 3. `--intent`, through the SAME derivation the gate mints with
///    ([`spec_slug::canonical`]) — the hand-run draft that no work unit ever
///    signalled (the tree sits on an integration base, or on no branch at all),
///    byte-identical to the behaviour this command always had.
///
/// Steps 1 and 2 are what stop a unit carrying two names: before them the gate
/// was handed a slug the caller invented while the draft derived its own from
/// its own `--intent`, and nothing reconciled the two — so `resume-bootstrap`
/// rebuilt `{base}_{slug}` from the spec and never matched the checkout.
///
/// Step 2 is deliberately BLIND to how the checkout got there. A draft standing
/// on `{base}_{slug}` is inside a unit that is already named, and drafting a
/// second name from there is the defect, not the fallback: if the intent really
/// opens a NEW unit, the gate minted its name and the marker or the hook put its
/// branch under the tree first.
fn resolve_slug(
    project_root: &Path,
    explicit: Option<&str>,
    work_branch: Option<&str>,
    intent: &str,
    lang: Locale,
) -> String {
    use crate::commands::event::work_branch;
    use crate::commands::spec::spec_slug;

    if let Some(given) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return given.to_string();
    }
    let config = mustard_core::ProjectConfig::load(project_root);
    let unit_branch = match work_branch {
        Some(branch) => Some(branch.to_string()),
        None => config.vcs().and_then(|vcs| {
            work_branch::current_branch(&vcs, &project_root.to_string_lossy())
        }),
    };
    if let Some(from_branch) = unit_branch
        .as_deref()
        .and_then(|branch| work_branch::slug_of_work_branch(branch, &config))
    {
        return from_branch;
    }
    spec_slug::canonical(intent, lang)
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

    /// AC-2 — the draft CONSUMES the unit's name instead of minting a second
    /// one.
    ///
    /// The gate names the unit (`emit-pipeline --kind pipeline.kind` reports it
    /// as `spec`); handing that name here must land the spec directory under
    /// it, with `--intent` keeping only its OTHER job — the spec title. The
    /// fixture is only worth anything because the two differ: the slug the
    /// draft would have derived is asserted absent from disk.
    #[test]
    fn spec_draft_consumes_the_slug_it_is_given() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        plant_project(project);

        let given = "work-unit-has-one-name";
        let intent = "Something the drafter would have slugged completely differently";
        let derived = crate::commands::spec::spec_slug::canonical(intent, Locale::EnUs);
        assert_ne!(derived, given, "the fixture only proves something if the two differ");

        let code = run_at(
            project,
            SpecDraftOpts {
                intent: intent.to_string(),
                slug: Some(given.to_string()),
                scope: "light".into(),
                lang: "en-US".into(),
                signals: None,
                output: None,
                material: None,
                waves: 1,
                plan: None,
                force: false,
                query_terms: None,
                force_scope: false,
            },
        );
        assert_eq!(code, 0, "a light draft with an explicit name exits clean");

        let spec_root = project.join(".claude").join("spec");
        assert!(
            spec_root.join(given).join("spec.md").exists(),
            "the spec lands under the name the unit already carries",
        );
        assert!(
            !spec_root.join(&derived).exists(),
            "and NOT under a second name derived from the intent",
        );
        // `--intent` keeps its other job.
        let body = std::fs::read_to_string(spec_root.join(given).join("spec.md")).unwrap();
        assert!(body.contains(intent), "the intent is still the spec title:\n{body}");

        // Without the flag, the name still comes from the UNIT: the slug half
        // of the branch this draft cut. Only a draft that no unit signalled
        // (no branch at all) derives one from the intent.
        std::fs::write(
            project.join("mustard.json"),
            r#"{"lang":"en-US","git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        assert_eq!(
            resolve_slug(project, None, Some("dev_named-at-the-gate"), intent, Locale::EnUs),
            "named-at-the-gate",
        );
        assert_eq!(resolve_slug(project, None, None, intent, Locale::EnUs), derived);
    }

    /// AC-2/AC-3, the leg the flag does not cover — the name survives the hook
    /// cutting the branch FIRST.
    ///
    /// The shipped order is: the gate mints the name and drops the pending
    /// marker → the flow's first write trips the auto-branch hook, which cuts
    /// `{base}_{slug}` and CONSUMES the marker → `spec-draft` runs. By then
    /// `cut_pending_work_branch` answers `NoPending`, so a draft that only
    /// looked at what it cut itself had nothing to consume and derived a second
    /// name from `--intent` — on every full run, which is the one path this unit
    /// exists for. The fixture reproduces that order exactly: a real repo, a
    /// work branch already checked out, NO marker on disk.
    #[test]
    fn spec_draft_recovers_the_unit_name_from_the_branch_it_stands_on() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        plant_project(project);
        std::fs::write(
            project.join("mustard.json"),
            r#"{"lang":"en-US","git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();

        // A repository standing on the unit's branch, with nothing pending.
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .args(args)
                .current_dir(project)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            assert!(ok, "git {args:?} failed");
        };
        git(&["init", "-b", "dev"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "t"]);
        git(&["add", "-A"]);
        git(&["commit", "-m", "base", "--no-gpg-sign"]);
        git(&["checkout", "-b", "dev_named-at-the-gate"]);

        let intent = "Something the drafter would have slugged completely differently";
        let derived = crate::commands::spec::spec_slug::canonical(intent, Locale::EnUs);
        assert_ne!(derived, "named-at-the-gate", "the fixture only proves something if they differ");

        // No `--slug`, no marker: exactly what `spec-draft` meets on a full run.
        assert_eq!(
            resolve_slug(project, None, None, intent, Locale::EnUs),
            "named-at-the-gate",
            "the draft must consume the name its own branch carries",
        );

        let code = run_at(
            project,
            SpecDraftOpts {
                intent: intent.to_string(),
                slug: None,
                scope: "light".into(),
                lang: "en-US".into(),
                signals: None,
                output: None,
                material: None,
                waves: 1,
                plan: None,
                force: false,
                query_terms: None,
                force_scope: false,
            },
        );
        assert_eq!(code, 0, "a light draft inside the unit's branch exits clean");

        let spec_root = project.join(".claude").join("spec");
        assert!(
            spec_root.join("named-at-the-gate").join("spec.md").exists(),
            "the spec lands under the name the branch carries",
        );
        assert!(!spec_root.join(&derived).exists(), "and NOT under a second, invented name");
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
            slug: None,
            scope: "light".into(),
            lang: "pt-BR".into(),
            signals: None,
            output: Some(out.clone()),
            material: None,
            waves: 0,
            plan: None,
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
            slug: None,
            scope: "full".into(),
            lang: "pt-BR".into(),
            signals: None,
            output: Some(out.clone()),
            material: None,
            waves: 3,
            plan: None,
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
                slug: None,
                scope: scope.into(),
                lang: lang.into(),
                signals: None,
                output: Some(out.clone()),
                material: None,
                waves,
                plan: None,
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
            slug: None,
            scope: "full".into(),
            lang: "pt-BR".into(),
            signals: None,
            output: Some(dir.path().join("specs").join("demo")),
            material: None,
            waves: 2,
            plan: None,
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
            slug: None,
            scope: "light".into(),
            lang: "pt".into(),
            signals: None,
            output: Some(dir.path().join("out")),
            material: None,
            waves: 0,
            plan: None,
            force: false,
            query_terms: None,
            force_scope: false,
        };
        run(opts);
        // Output dir should not have been populated.
        assert!(!dir.path().join("out").join("spec.md").exists());
    }

    // --- The conversation channel (--material) ----------------------------

    /// Draft a Light spec into `out`, optionally carrying `material_json`
    /// through the channel, and return the resulting `spec.md` body.
    fn draft_with_material(
        dir: &std::path::Path,
        out: &std::path::Path,
        material_json: Option<&str>,
    ) -> String {
        let material = material_json.map(|json| {
            let path = dir.join("material.json");
            std::fs::write(&path, json).unwrap();
            path
        });
        run(SpecDraftOpts {
            intent: "Demo intent".into(),
            slug: None,
            scope: "light".into(),
            lang: "en-US".into(),
            signals: None,
            output: Some(out.to_path_buf()),
            material,
            waves: 0,
            plan: None,
            force: false,
            query_terms: None,
            force_scope: false,
        });
        std::fs::read_to_string(out.join("spec.md")).expect("draft written")
    }

    /// The whole point of the channel: one definition, one decision WITH its
    /// reason, and one finding all reach the materialized spec — each under a
    /// heading of its own — while the prose-only opening section is left
    /// exactly as a draft without the channel would have written it.
    #[test]
    fn drafter_carries_conversation_material_into_its_own_sections() {
        use crate::commands::spec::spec_sections::section_block;
        let dir = tempdir().unwrap();
        let bare = draft_with_material(dir.path(), &dir.path().join("bare"), None);
        let carried = draft_with_material(
            dir.path(),
            &dir.path().join("carried"),
            Some(
                r#"{
                    "definitions": [
                        {"term": "wave", "meaning": "one dispatchable unit of the plan"}
                    ],
                    "decisions": [
                        {"decision": "carry the material through a file",
                         "reason": "a shell argument mangles newlines and non-ASCII"}
                    ],
                    "findings": [
                        {"statement": "the drafter takes no material argument today",
                         "file": "apps/rt/src/commands/spec/spec_draft.rs", "line": 81}
                    ]
                }"#,
            ),
        );

        // Three sections, three headings, each carrying its own kind.
        assert!(carried.contains("\n## Definitions\n"), "definitions heading:\n{carried}");
        assert!(
            carried.contains("- **wave** — one dispatchable unit of the plan"),
            "definition body:\n{carried}"
        );
        assert!(carried.contains("\n## Decisions\n"), "decisions heading:\n{carried}");
        assert!(carried.contains("- carry the material through a file"), "decision:\n{carried}");
        assert!(
            carried.contains("  Reason: a shell argument mangles newlines and non-ASCII"),
            "a decision carries its REASON, not just the choice:\n{carried}"
        );
        assert!(carried.contains("\n## Evidence\n"), "evidence heading:\n{carried}");
        assert!(
            carried.contains("- the drafter takes no material argument today"),
            "finding:\n{carried}"
        );

        // The opening section is untouched — byte-identical to the draft that
        // carried nothing. The material landed BESIDE it, never inside it.
        assert_eq!(
            section_block(&carried, "context"),
            section_block(&bare, "context"),
            "the prose-only Context must be identical with and without the channel",
        );
        // And the bare draft grew no empty headings.
        for heading in ["## Definitions", "## Decisions", "## Evidence"] {
            assert!(!bare.contains(heading), "empty channel emits no `{heading}`:\n{bare}");
        }
    }

    /// The two halves of the defect, proven together: a finding citing a file
    /// AND a line survives materialization intact, and the prose rule that
    /// rejects that same path in `## Context` raises nothing about it — because
    /// `## Evidence` is where it now lives. The contrast case keeps the rule
    /// honest: the same reference moved back into Context still WARNs.
    #[test]
    fn evidence_section_keeps_file_and_line_references() {
        let dir = tempdir().unwrap();
        let out = dir.path().join("evidence");
        let body = draft_with_material(
            dir.path(),
            &out,
            Some(
                r#"{"findings": [
                    {"statement": "the prose rule rejects paths in the opening section",
                     "file": "apps/rt/src/commands/review/analyze_validation.rs", "line": 294},
                    {"statement": "the scaffold owns the canonical layout",
                     "file": "apps/rt/src/commands/spec/spec_scaffold.rs"}
                 ]}"#,
            ),
        );

        // File AND line survive verbatim, in a backticked reference.
        assert!(
            body.contains("  Evidence: `apps/rt/src/commands/review/analyze_validation.rs:294`"),
            "file:line reference intact:\n{body}"
        );
        // A file-level finding (no line) renders the file alone — no `:0` noise.
        assert!(
            body.contains("  Evidence: `apps/rt/src/commands/spec/spec_scaffold.rs`"),
            "line-less finding keeps the bare file:\n{body}"
        );
        assert!(!body.contains(".rs:0`"), "no fabricated line number:\n{body}");

        // The validator raises NO prose complaint — the evidence section
        // accepts exactly what the prose section rejects.
        let root = std::path::PathBuf::from(crate::shared::context::project_dir());
        let spec_md = out.join("spec.md");
        let issues =
            crate::commands::review::analyze_validation::validate(&root, &spec_md, &body);
        assert!(
            !issues.iter().any(|i| i["type"] == json!("context-not-prose")),
            "a finding in Evidence is not a Context violation: {issues:?}"
        );
        assert!(issues.is_empty(), "the carried draft still validates clean: {issues:?}");

        // Contrast: the SAME reference inside Context still WARNs, and the
        // message now names Evidence as its destination.
        let polluted = body.replace(
            "Demo intent.",
            "Demo intent, verified at apps/rt/src/commands/review/analyze_validation.rs line 294.",
        );
        let polluted_issues =
            crate::commands::review::analyze_validation::validate(&root, &spec_md, &polluted);
        let warn = polluted_issues
            .iter()
            .find(|i| i["type"] == json!("context-not-prose"))
            .unwrap_or_else(|| panic!("prose rule must still fire: {polluted_issues:?}"));
        let msg = warn["message"].as_str().unwrap_or_default();
        assert!(msg.contains("Evidence"), "the rejection names the destination: {msg}");
    }

    /// Invisible when unused: a channel that carries nothing produces the exact
    /// bytes a draft with no channel produces — no empty headings, no
    /// placeholders, no trailing whitespace drift. Both drafts land in a
    /// directory of the SAME name (the leaf seeds the `id:` frontmatter), so
    /// the only variable left is the channel.
    #[test]
    fn empty_material_leaves_the_draft_byte_identical() {
        let dir = tempdir().unwrap();
        let bare = draft_with_material(dir.path(), &dir.path().join("a").join("demo"), None);
        let empty = draft_with_material(
            dir.path(),
            &dir.path().join("b").join("demo"),
            Some(r#"{"definitions": [], "decisions": [], "findings": []}"#),
        );
        assert_eq!(empty, bare, "an empty channel must not change a single byte");
    }

    /// The channel is FAIL-CLOSED: half an entry (a decision with no reason, a
    /// finding with no file) and a mistyped key are refused instead of silently
    /// dropping the material — the exact failure mode this feature exists to
    /// end. Nothing is written when the material is refused.
    #[test]
    fn malformed_material_is_refused_before_anything_is_written() {
        let dir = tempdir().unwrap();
        for (name, json) in [
            ("no-reason", r#"{"decisions": [{"decision": "x", "reason": "  "}]}"#),
            ("no-file", r#"{"findings": [{"statement": "x", "file": ""}]}"#),
            ("typo-key", r#"{"decision": [{"decision": "x", "reason": "y"}]}"#),
            ("not-json", "definitions: none"),
        ] {
            let path = dir.path().join(format!("{name}.json"));
            std::fs::write(&path, json).unwrap();
            assert!(
                load_material(&path).is_err(),
                "{name}: malformed material must be refused, not degraded to empty"
            );
            let out = dir.path().join(name);
            run(SpecDraftOpts {
                intent: "Demo intent".into(),
                slug: None,
                scope: "light".into(),
                lang: "en-US".into(),
                signals: None,
                output: Some(out.clone()),
                material: Some(path),
                waves: 0,
                plan: None,
                force: false,
                query_terms: None,
                force_scope: false,
            });
            assert!(
                !out.join("spec.md").exists(),
                "{name}: no half-drafted spec is left behind"
            );
        }
    }

    // --- The fused materialisation (`--plan`) -----------------------------

    /// A command that comes back RED on both shells (`cmd.exe` and `sh`): the
    /// directory does not exist, so `cd` exits non-zero — a criterion that CAN
    /// fail, which is what the negative proof demands.
    const RED_COMMAND: &str = "cd no-such-directory-abc";
    /// A command that comes back GREEN on both shells — `cd .` is a builtin
    /// everywhere and always succeeds, so the criterion was never proven able
    /// to fail.
    const GREEN_COMMAND: &str = "cd .";

    /// Write a one-wave plan whose single criterion carries `command`, plus the
    /// trailing build-green safety criterion. `files` is load-bearing: a wave
    /// that claims a criterion while declaring nowhere to do the work is refused
    /// by the scaffold's claim-support gate, and these fixtures exist to isolate
    /// the PROOF.
    fn write_fused_plan(project: &std::path::Path, name: &str, command: &str) -> std::path::PathBuf {
        let plan_path = project.join(name);
        std::fs::write(
            &plan_path,
            serde_json::to_string(&json!({
                "waves": [{
                    "n": 1, "role": "rt", "summary": "wire it",
                    "tasks": ["wire the fused path"],
                    "files": ["apps/rt/src/commands/spec/spec_draft.rs"],
                    "acceptance": [
                        format!("**AC-1** — when the plan is handed to the draft, then the layout lands in one call.\n  Command: `{command}`"),
                        format!("**AC-2** — build green.\n  Command: `{GREEN_COMMAND}`"),
                    ],
                    "satisfies": ["AC-1", "AC-2"],
                }],
                "total_waves": 1,
                "lang": "en-US"
            }))
            .unwrap(),
        )
        .unwrap();
        plan_path
    }

    /// Draft options for the fused path — auto `--output` (so the spec lands in
    /// the tempdir project's own `.claude/spec/`), Full scope, one wave.
    fn fused_opts(intent: &str, plan: &std::path::Path) -> SpecDraftOpts {
        SpecDraftOpts {
            intent: intent.to_string(),
            slug: None,
            scope: "full".into(),
            lang: "en-US".into(),
            signals: None,
            output: None,
            material: None,
            waves: 1,
            plan: Some(plan.to_path_buf()),
            force: false,
            query_terms: None,
            force_scope: false,
        }
    }

    /// The whole point of the fusion: ONE invocation produces `spec.md`,
    /// `meta.json`, `wave-plan.md` AND every wave directory, with the negative
    /// proof taken in the same pass. Before this, `--waves 1` recorded the wave
    /// decision in `meta.json` and materialised none of it — the layout only
    /// appeared after a second, separate command.
    #[test]
    fn spec_draft_materialises_the_whole_layout_in_one_call() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        plant_project(project);
        let plan = write_fused_plan(project, "plan.json", RED_COMMAND);

        let intent = "Fuse the draft with the plan";
        let code = run_at(project, fused_opts(intent, &plan));
        assert_eq!(code, 0, "a proven plan materialises and exits clean");

        let spec_dir = project
            .join(".claude")
            .join("spec")
            .join(crate::commands::spec::spec_slug::canonical(intent, Locale::EnUs));
        // The draft's own two artefacts...
        assert!(spec_dir.join("spec.md").exists(), "spec.md");
        assert!(spec_dir.join("meta.json").exists(), "meta.json");
        // ...and the whole layout, from the SAME call.
        assert!(spec_dir.join("wave-plan.md").exists(), "wave-plan.md in one call");
        assert!(
            spec_dir.join("wave-1-rt").join("spec.md").exists(),
            "the wave directory in one call",
        );
        // The PLAN transition was recorded — the composite ran to its end.
        let events = mustard_core::view::projection::read_harness_events_from_ndjson_dir(
            &spec_dir.join(".events"),
        );
        assert!(
            events.iter().any(|e| {
                e.event == "pipeline.phase"
                    && e.payload.get("to").and_then(serde_json::Value::as_str) == Some("PLAN")
            }),
            "the fused call emits the PLAN transition: {events:?}",
        );
        // The plan's own criteria are the spec's criteria — the drafted
        // skeleton placeholder is gone, so the proof judged something real.
        let body = std::fs::read_to_string(spec_dir.join("spec.md")).unwrap();
        assert!(body.contains(RED_COMMAND), "the plan's criterion reached spec.md:\n{body}");
        assert!(
            !body.contains("<runnable command that verifies this criterion>"),
            "the skeleton placeholder must be superseded:\n{body}",
        );
    }

    /// The other half, and the obligation that comes with fusing: a criterion
    /// that was never proven ABLE to fail refuses the call (exit 2) and leaves
    /// NO layout behind — so the operator's retry meets a directory it did not
    /// create instead of one this run half-built.
    ///
    /// The draft's own `spec.md` / `meta.json` deliberately survive: the
    /// criterion to fix lives in the first of them.
    #[test]
    fn spec_draft_plan_refuses_an_unproven_criterion() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        plant_project(project);
        let plan = write_fused_plan(project, "vacuous.json", GREEN_COMMAND);

        let intent = "Refuse the vacuous criterion";
        let code = run_at(project, fused_opts(intent, &plan));
        assert_eq!(code, 2, "an unproven criterion refuses the fused call");

        let spec_dir = project
            .join(".claude")
            .join("spec")
            .join(crate::commands::spec::spec_slug::canonical(intent, Locale::EnUs));
        assert!(
            !spec_dir.join("wave-plan.md").exists(),
            "a refusal must leave no wave-plan.md behind",
        );
        assert!(
            !spec_dir.join("wave-1-rt").exists(),
            "a refusal must leave no wave directory behind",
        );
        // The draft itself stands — that is where the criterion is fixed.
        assert!(spec_dir.join("spec.md").exists(), "the draft survives the refusal");
        // And no PLAN transition was recorded for a plan that did not land.
        let events = mustard_core::view::projection::read_harness_events_from_ndjson_dir(
            &spec_dir.join(".events"),
        );
        assert!(
            !events.iter().any(|e| {
                e.event == "pipeline.phase"
                    && e.payload.get("to").and_then(serde_json::Value::as_str) == Some("PLAN")
            }),
            "a refused plan never reaches PLAN: {events:?}",
        );
    }

    /// The refusal's documented WAY OUT actually leads somewhere.
    ///
    /// A refusal is only a gate if the operator can get past it by fixing the
    /// defect. Re-running `spec-draft --plan` is NOT that way out: the rollback
    /// keeps `spec.md` + `meta.json` on purpose, so the second draft answers
    /// `output exists`. The way out is the RE-materialisation door — the
    /// criterion is fixed in the `spec.md` the rollback preserved (which is what
    /// the proof reads) and `plan-materialize` takes the repaired spec through
    /// the same composite.
    ///
    /// Both halves are asserted: the second DRAFT is refused, and the
    /// re-materialisation succeeds — so the test fails if either the dead end or
    /// the exit stops being true.
    #[test]
    fn a_refused_plan_is_repaired_through_the_rematerialisation_door() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        plant_project(project);
        let vacuous = write_fused_plan(project, "vacuous.json", GREEN_COMMAND);

        let intent = "Repair the vacuous criterion";
        assert_eq!(run_at(project, fused_opts(intent, &vacuous)), 2, "the proof refuses");

        let spec_dir = project
            .join(".claude")
            .join("spec")
            .join(crate::commands::spec::spec_slug::canonical(intent, Locale::EnUs));

        // The dead end the prose must not send anyone down: the draft's own
        // artefacts survived, so drafting again refuses instead of repairing.
        assert_eq!(
            run_at(project, fused_opts(intent, &vacuous)),
            0,
            "a second draft does not repair — it reports `output exists` and stops",
        );
        assert!(
            !spec_dir.join("wave-plan.md").exists(),
            "the second draft materialised nothing, so the layout is still absent",
        );

        // The documented repair: fix the criterion where the proof reads it, and
        // keep the plan's copy in step so the two do not drift.
        let body = std::fs::read_to_string(spec_dir.join("spec.md")).unwrap();
        // Only AC-1: the trailing build-green criterion is exempt by design and
        // stays as the plan writes it.
        std::fs::write(
            spec_dir.join("spec.md"),
            body.replacen(GREEN_COMMAND, RED_COMMAND, 1),
        )
        .unwrap();
        let fixed = write_fused_plan(project, "fixed.json", RED_COMMAND);

        let report = plan_materialize::materialize(project, &spec_dir, &fixed);
        assert!(
            !plan_materialize::refused(&report),
            "the repaired criterion must clear the composite: {report}",
        );
        assert!(spec_dir.join("wave-plan.md").exists(), "the layout lands on the repair pass");
        assert!(spec_dir.join("wave-1-rt").exists(), "and so does the wave directory");
    }

    /// The section swap is surgical: only the AC block changes, and the plan's
    /// line survives verbatim (its `Command:` continuation included).
    #[test]
    fn replace_acceptance_block_swaps_only_that_section() {
        let body = "# S\n\n## Context\nprose.\n\n## Acceptance Criteria\n\n- **AC-1** — old.\n  Command: `x`\n\n## Files\n- a.rs\n";
        let bullets = vec!["- **AC-9** — new.\n  Command: `true`".to_string()];
        let out = replace_acceptance_block(body, &bullets).expect("the heading is there");
        assert!(out.contains("## Context\nprose."), "prose untouched:\n{out}");
        assert!(out.contains("- **AC-9** — new.\n  Command: `true`"), "verbatim:\n{out}");
        assert!(!out.contains("**AC-1**"), "the old block is gone:\n{out}");
        assert!(out.contains("## Files\n- a.rs"), "the next section survives:\n{out}");
        // No heading at all ⇒ nothing is invented.
        assert!(replace_acceptance_block("# S\n\n## Files\n- a.rs\n", &bullets).is_none());

        // The PLAN divider sits between the AC section and the next heading on
        // every Full draft, and it belongs to the DOCUMENT, not to the section
        // being replaced. Dropped, the dashboard slices the PRD at a marker that
        // is no longer there and renders an empty tab — for every spec born
        // through the fused door, permanently, since `spec.md` is written here
        // and nowhere else. Both halves, so the assertion can fail: the divider
        // survives, and the old criterion still does not.
        let full = "# S\n\n<!-- PRD -->\n\n## Acceptance Criteria\n\n- **AC-1** — old.\n  Command: `x`\n\n<!-- PLAN -->\n\n## Files\n- a.rs\n";
        let out = replace_acceptance_block(full, &bullets).expect("the heading is there");
        assert!(out.contains("<!-- PLAN -->"), "the divider must survive the swap:\n{out}");
        assert!(out.contains("<!-- PRD -->"), "…and so must the one before the section:\n{out}");
        assert!(
            out.find("<!-- PLAN -->") < out.find("## Files"),
            "the divider must still open the plan half:\n{out}",
        );
        assert!(!out.contains("**AC-1**"), "the old block is still gone:\n{out}");
        assert!(out.contains("- **AC-9** — new."), "the new block landed:\n{out}");
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
