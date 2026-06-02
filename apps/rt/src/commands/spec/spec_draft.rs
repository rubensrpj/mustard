//! `mustard-rt run spec-draft` — generate a spec.md + meta.json (+ wave-plan)
//! conforming to [`mustard_core::domain::spec::contract`].
//!
//! Replaces the ~80 lines of literal-template boilerplate that lived inline in
//! `apps/cli/templates/commands/mustard/feature/SKILL.md` (W6 will remove the
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

    if output.exists() && !opts.force {
        emit_error("output exists; pass --force to overwrite", &output.display().to_string());
        return;
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

    // ---- Enrich the Context section with the scan digest (the same insumos
    // `feature::run` emits). Deterministic, token-free, fail-open: a missing
    // model or empty match degrades to the plain placeholder. The same digest
    // also seeds the trackable `## Checklist` (one item per scan anchor). ----
    let digest = scan_digest(&opts.intent);
    let context_block = digest
        .as_ref()
        .and_then(|d| render_context_block(&d.0, &d.1, lang_locale));
    let anchors: &[String] = digest.as_ref().map_or(&[], |d| d.0.as_slice());

    // ---- Build the canonical input + validate before writing. ----
    let input = build_input(
        &slug,
        &opts.intent,
        scope,
        &opts.lang,
        opts.waves,
        lang_locale,
        &build_command,
        context_block.as_deref(),
        anchors,
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

    // D6: the `memory/_index.md` is NOT born at draft time. A fresh spec used to
    // ship an empty stub (and, before the i18n keys existed, a `<missing-key>`
    // line). The index is now born on the FIRST knowledge capture via
    // `spec-memory create` (see `spec_memory::ensure_index`), so an unused spec
    // carries no orphan index file.

    // Full-scope wave decomposition is owned by `wave-scaffold` (plan-driven:
    // per-wave roles/summaries/deps + review/qa scaffolds). `spec-draft` only
    // materialises the top-level spec.md + meta.json — `meta.json` already
    // records `scope=full` + `totalWaves` + `isWavePlan`, so consumers know a
    // wave plan is expected before `wave-scaffold` fills it in.

    let report = json!({
        "ok": true,
        "spec": slug,
        "scope": scope_str(scope),
        "lang": opts.lang,
        "tone": tone.as_str(),
        "tone_instruction": tone_prompt_instruction(tone),
        "output": output.display().to_string(),
        "files": written,
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
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
    context_block: Option<&str>,
    anchors: &[String],
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
                body: prd_section_default(n, intent, lang_locale, context_block),
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
        acceptance_criteria: vec![AcceptanceCriterion {
            id: "AC-1".to_string(),
            statement: "Pipeline build green".to_string(),
            command: build_command.to_string(),
        }],
        checklist: build_checklist(scope, anchors, lang_locale),
    }
}

/// Max trackable checklist items materialised at draft time. The scan digest
/// already caps anchors at [`SCAN_ANCHOR_CAP`]; full scope mirrors that, light
/// scope keeps the list short (a Light spec touches ≤5 files by definition).
const CHECKLIST_LIGHT_CAP: usize = 5;

/// Build the trackable `## Checklist` for a fresh draft. One item per scan
/// anchor (the auto-mark hook keys off the ` → <path>` arrow), so a `Write` of
/// that file flips the box automatically. Falls back to a single task item when
/// the scan surfaced no anchors (fail-open: the checklist is never empty, so the
/// contract's `ChecklistEmpty` rule and the close-gate checklist gate always
/// have something to track). Full scope keeps every anchor; Light caps the list.
fn build_checklist(scope: Scope, anchors: &[String], lang: Locale) -> Vec<ChecklistItem> {
    let cap = if matches!(scope, Scope::Full) {
        SCAN_ANCHOR_CAP
    } else {
        CHECKLIST_LIGHT_CAP
    };
    let items: Vec<ChecklistItem> = anchors
        .iter()
        .take(cap)
        .map(|path| ChecklistItem {
            label: translate("checklist.touch_file", lang).to_string(),
            path: Some(path.clone()),
        })
        .collect();
    if items.is_empty() {
        // No precedent from the scan — seed a single hand-trackable task item
        // mirroring the `tasks` plan placeholder (`T1`). No path ⇒ no auto-mark
        // anchor, but the gate + `mark-checklist-item` still track it by text.
        vec![ChecklistItem {
            label: translate("checklist.first_task", lang).to_string(),
            path: None,
        }]
    } else {
        items
    }
}

/// Default body for a PRD section. `name` is a canonical contract key — a
/// language-agnostic EN identifier from [`PRD_SECTIONS`] (`"context"`,
/// `"users"`, …). The returned body is fully localised via the catalogue
/// (the body is part of the spec-facing narrative; only the keys are EN).
fn prd_section_default(
    name: &str,
    intent: &str,
    lang: Locale,
    context_block: Option<&str>,
) -> String {
    let fill_why_now = translate("placeholder.fill_why_now", lang);
    match name {
        "context" => match context_block {
            Some(block) => format!("{intent}.\n\n{block}\n\n{fill_why_now}"),
            None => format!("{intent}.\n\n{fill_why_now}"),
        },
        "users" => translate("placeholder.fill_beneficiary", lang).to_string(),
        "metric" => translate("placeholder.fill_metric", lang).to_string(),
        "non-goals" => translate("placeholder.fill_excluded", lang).to_string(),
        "acceptance-criteria" => translate("placeholder.see_below", lang).to_string(),
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

/// Query the scan digest for the intent — the same deterministic insumos
/// `feature::run` emits, recomputed here. It costs no tokens (a local query
/// against `grain.model.json`, not an AI call). Returns `(anchors, slice
/// labels)`: the anchors seed both the Context enrichment block and the
/// trackable `## Checklist`; the slice labels feed only the Context block.
/// Returns `None` when the model is absent or the query failed (fail-open: both
/// consumers degrade to their placeholder).
fn scan_digest(intent: &str) -> Option<(Vec<String>, Vec<String>)> {
    let model = PathBuf::from(project_dir())
        .join(".claude")
        .join("grain.model.json");
    let terms = crate::commands::feature::domain_terms(intent);
    let q = Scan::locate().digest_query(&model, &terms).ok()?;
    let slice_labels: Vec<String> = q
        .slices
        .iter()
        .map(|s| format!("{} (×{})", s.label, s.recurrence))
        .collect();
    Some((q.files, slice_labels))
}

/// Render the Context enrichment markdown from already-extracted anchors +
/// slice labels. Pure (no I/O) so it is unit-testable; the digest query lives
/// in [`context_enrichment`]. Returns `None` when there is nothing to show.
fn render_context_block(
    anchors: &[String],
    slice_labels: &[String],
    lang: Locale,
) -> Option<String> {
    let mut block = String::new();
    if !anchors.is_empty() {
        let _ = writeln!(block, "{}:", translate("context.scan_anchors", lang));
        for f in anchors.iter().take(SCAN_ANCHOR_CAP) {
            let _ = writeln!(block, "- {f}");
        }
    }
    if !slice_labels.is_empty() {
        let joined = slice_labels
            .iter()
            .take(SCAN_SLICE_CAP)
            .cloned()
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
        raw: serde_json::Value::Null,
    }
}

// D6: the `memory/_index.md` is no longer materialised at draft time (the old
// `write_memory_stub` shipped an empty stub on every spec). The index is now
// created/updated on the first `spec-memory create`, in
// `spec_memory::ensure_index`, using the `memory.index.intro` / `.empty` keys.

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
    fn slug_caps_on_word_boundary() {
        // 10 content words → first 5 kept, cut on a boundary (no partial word).
        let s = slug_from_intent(
            "alpha beta gamma delta epsilon zeta eta theta iota kappa",
            Locale::EnUs,
        );
        assert_eq!(s, "alpha-beta-gamma-delta-epsilon");
    }

    #[test]
    fn render_context_block_lists_anchors_and_slices() {
        let anchors = vec!["src/list.rs".to_string(), "src/view.rs".to_string()];
        let slices = vec!["List (×3)".to_string()];
        let s = render_context_block(&anchors, &slices, Locale::PtBr).unwrap();
        assert!(s.contains("Âncoras (do scan):"));
        assert!(s.contains("- src/list.rs"));
        assert!(s.contains("Fatias recorrentes"));
        assert!(s.contains("List (×3)"));
    }

    #[test]
    fn render_context_block_none_when_empty() {
        assert!(render_context_block(&[], &[], Locale::EnUs).is_none());
    }

    #[test]
    fn render_context_block_caps_anchors_and_uses_en_heading() {
        let anchors: Vec<String> = (0..20).map(|i| format!("f{i}.rs")).collect();
        let s = render_context_block(&anchors, &[], Locale::EnUs).unwrap();
        assert!(s.contains("Anchors (from scan):"));
        assert_eq!(s.matches("- f").count(), SCAN_ANCHOR_CAP);
    }

    #[test]
    fn build_input_validates() {
        let input = build_input("demo", "Demo", Scope::Full, "pt-BR", 2, Locale::PtBr, "rtk cargo build", None, &[]);
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
                "demo", "Demo", Scope::Full, "pt-BR", waves, Locale::PtBr,
                "rtk cargo build", None, &[],
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
            "demo", "Demo", Scope::Light, "en-US", 0, Locale::EnUs,
            "rtk cargo build", None, &[],
        );
        assert_eq!(light.total_waves, None, "Light carries no waves");
        let light_meta = build_meta_from_input(&light);
        assert_eq!(light_meta.is_wave_plan, None);
        assert_eq!(light_meta.total_waves, None);
    }

    #[test]
    fn build_input_validates_in_en_us() {
        // Section *keys* are canonical EN identifiers; bodies are localised.
        let input = build_input("demo", "Demo", Scope::Full, "en-US", 2, Locale::EnUs, "rtk cargo build", None, &[]);
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
    fn build_input_ac_uses_build_command_not_hardcoded() {
        // AC command comes from the resolved build command, not `rtk cargo build`.
        let input = build_input("demo", "Demo", Scope::Light, "en-US", 0, Locale::EnUs, "pnpm build", None, &[]);
        assert_eq!(input.acceptance_criteria[0].command, "pnpm build");
        // Neutral fallback flows through verbatim when no buildCommand is set.
        let input2 = build_input(
            "demo",
            "Demo",
            Scope::Light,
            "en-US",
            0,
            Locale::EnUs,
            mustard_core::BUILD_COMMAND_FALLBACK,
            None,
            &[],
        );
        assert_eq!(
            input2.acceptance_criteria[0].command,
            mustard_core::BUILD_COMMAND_FALLBACK
        );
    }

    #[test]
    fn build_checklist_full_one_item_per_anchor() {
        let anchors = vec!["src/list.rs".to_string(), "src/view.rs".to_string()];
        let items = build_checklist(Scope::Full, &anchors, Locale::EnUs);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].path.as_deref(), Some("src/list.rs"));
        assert!(!items[0].label.is_empty());
    }

    #[test]
    fn build_checklist_light_caps_items() {
        let anchors: Vec<String> = (0..10).map(|i| format!("f{i}.rs")).collect();
        let items = build_checklist(Scope::Light, &anchors, Locale::EnUs);
        assert_eq!(items.len(), CHECKLIST_LIGHT_CAP);
    }

    #[test]
    fn build_checklist_falls_back_to_single_task_when_no_anchors() {
        // No scan precedent → never empty (contract requires ≥1 item).
        let light = build_checklist(Scope::Light, &[], Locale::EnUs);
        assert_eq!(light.len(), 1);
        assert!(light[0].path.is_none());
        let full = build_checklist(Scope::Full, &[], Locale::PtBr);
        assert_eq!(full.len(), 1);
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
        };
        run(opts);
        let root = dir.path().join("specs").join("demo");
        assert!(root.join("spec.md").exists());
        assert!(root.join("meta.json").exists());
        // D6: a fresh draft no longer ships a `memory/_index.md` stub — the
        // index is born on the first `spec-memory create`.
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
        };
        run(opts);
        // Output dir should not have been populated.
        assert!(!dir.path().join("out").join("spec.md").exists());
    }
}
