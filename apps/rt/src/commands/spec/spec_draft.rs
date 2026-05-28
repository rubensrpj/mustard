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
//!   meta.json            # canonical lifecycle metadata
//!   memory/_index.md     # T1.9 — stub memory index
//!   wave-plan.md         # only when scope=full
//!   wave-1-{role}/spec.md ... wave-N-{role}/spec.md  # only when scope=full
//! ```
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
    AcceptanceCriterion, SectionBody, SpecInput, PLAN_SECTIONS, PRD_SECTIONS,
};
use mustard_core::{
    domain::model::view::Phase,
    platform::i18n::{translate, Locale, Tone},
    Outcome, Scope, Stage,
};
use serde_json::json;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Read `tone` from `<project_root>/mustard.json` (fail-open to
/// [`Tone::default`] / `Didactic`). The drafter wires this into the spec
/// narrative prompt so generated specs respect the user's tone preference —
/// see `feedback_didactic_responses` + `feedback_templates_derive_from_mustard_json`.
fn read_mustard_tone(project_root: &Path) -> Tone {
    let path = project_root.join("mustard.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Tone::default();
    };
    let Ok(value): Result<serde_json::Value, _> = serde_json::from_str(&text) else {
        return Tone::default();
    };
    value
        .get("tone")
        .and_then(serde_json::Value::as_str)
        .and_then(Tone::parse)
        .unwrap_or_default()
}

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
    /// Number of waves to scaffold under Full scope (default 1).
    pub waves: u32,
    /// Role assigned to each scaffolded wave (default `mixed`).
    pub role: String,
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
    let slug = slug_from_intent(&opts.intent);
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

    // ---- Build the canonical input + validate before writing. ----
    let input = build_input(&slug, &opts.intent, scope, &opts.lang, opts.waves, lang_locale);
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
    let project_root = PathBuf::from(project_dir());
    let tone = read_mustard_tone(&project_root);

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

    if let Err(e) = write_memory_stub(&output, &input, lang_locale) {
        eprintln!("spec-draft: WARN: memory/_index.md write failed — {e}");
    } else {
        written.push(output.join("memory").join("_index.md").display().to_string());
    }

    if matches!(scope, Scope::Full) {
        let wave_paths = write_wave_plan(&output, &input, opts.waves, &opts.role, lang_locale);
        match wave_paths {
            Ok(paths) => written.extend(paths),
            Err(e) => emit_error("write wave-plan", &e),
        }
    }

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
/// the user is expected to flesh them out. Localised via `lang_locale` (every
/// user-facing string goes through [`translate`]; the canonical section *names*
/// in [`PRD_SECTIONS`] / [`PLAN_SECTIONS`] stay in their contract spelling).
fn build_input(
    slug: &str,
    intent: &str,
    scope: Scope,
    lang: &str,
    waves: u32,
    lang_locale: Locale,
) -> SpecInput {
    SpecInput {
        slug: slug.to_string(),
        title: intent.to_string(),
        stage: Some(Stage::Plan),
        outcome: Some(Outcome::Active),
        phase: Some(Phase::Plan),
        scope: Some(scope),
        lang: Some(lang.to_string()),
        total_waves: if matches!(scope, Scope::Full) {
            Some(waves.max(1))
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
        acceptance_criteria: vec![AcceptanceCriterion {
            id: "AC-1".to_string(),
            statement: "Pipeline build green".to_string(),
            command: "rtk cargo build".to_string(),
        }],
    }
}

/// Default body for a PRD section. `name` is a canonical contract key
/// (PT-BR — `"Contexto"`, `"Usuários"`, etc.). The returned body is fully
/// localised via the catalogue.
fn prd_section_default(name: &str, intent: &str, lang: Locale) -> String {
    let fill_why_now = translate("placeholder.fill_why_now", lang);
    match name {
        "Contexto" => format!("{intent}.\n\n{fill_why_now}"),
        "Usuários" => translate("placeholder.fill_beneficiary", lang).to_string(),
        "Métrica" => translate("placeholder.fill_metric", lang).to_string(),
        "Não-Objetivos" => translate("placeholder.fill_excluded", lang).to_string(),
        "Critérios de Aceitação" => translate("placeholder.see_below", lang).to_string(),
        _ => translate("placeholder.fill", lang).to_string(),
    }
}

/// Default body for a Plan section. `name` is a canonical contract key
/// (PT-BR — `"Arquivos"`, `"Tarefas"`, `"Limites"`).
fn plan_section_default(name: &str, lang: Locale) -> String {
    match name {
        "Arquivos" => translate("placeholder.fill_files", lang).to_string(),
        "Tarefas" => "- [ ] T1 — ...".to_string(),
        "Limites" => "IN: ...\nOUT: ...".to_string(),
        _ => translate("placeholder.fill", lang).to_string(),
    }
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
        raw: serde_json::Value::Null,
    }
}

/// T1.9 — drop a tiny `memory/_index.md` stub so consumers can immediately
/// add principles via `spec-memory create`. Every user-facing string flows
/// through `translate` so PT-BR / EN-US specs each get their own heading set.
fn write_memory_stub(output: &Path, input: &SpecInput, lang: Locale) -> Result<(), String> {
    let dir = output.join("memory");
    mfs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let title_template = translate("memory.index.title", lang);
    let title = title_template.replace("{title}", &input.title);
    let intro = translate("memory.index.intro", lang);
    let principles_heading = translate("heading.memory.principles", lang);
    let empty_line = translate("memory.index.empty", lang);
    let body = format!(
        "# {title}\n\n{intro}\n\n## {principles_heading} (0)\n\n{empty_line}\n\n```\nmustard-rt run spec-memory create --spec {slug} --name <kebab> --kind principle --origin-wave wave-N-<role>\n```\n",
        slug = input.slug,
    );
    mfs::write_atomic(dir.join("_index.md"), body.as_bytes()).map_err(|e| e.to_string())
}

/// Materialise `wave-plan.md` + `wave-N-{role}/spec.md` directories. Headings
/// and placeholder copy are localised; the AC ids (`AC-W1.1`) and the
/// `Command:` keyword stay canonical so QA can grep them across locales.
fn write_wave_plan(
    output: &Path,
    input: &SpecInput,
    waves: u32,
    role: &str,
    lang: Locale,
) -> Result<Vec<String>, String> {
    let n = waves.max(1);
    let mut written: Vec<String> = Vec::new();
    let fill = translate("placeholder.fill", lang);

    let mut plan = String::new();
    let waveplan_title = translate("waveplan.title", lang).replace("{title}", &input.title);
    let _ = write!(plan, "# {waveplan_title}\n\n");
    plan.push_str("### Stage: Plan\n### Outcome: Active\n### Flags: \n\n");
    let _ = write!(
        plan,
        "## {table_heading}\n\n| # | {spec_col} | {role_col} | {summary_col} |\n|---|---|---|---|\n",
        table_heading = translate("waveplan.table_heading", lang),
        spec_col = translate("waveplan.column.spec", lang),
        role_col = translate("waveplan.column.role", lang),
        summary_col = translate("waveplan.column.summary", lang),
    );
    for i in 1..=n {
        let _ = writeln!(plan, "| {i} | [[wave-{i}-{role}]] | {role} | {fill} |");
    }
    mfs::write_atomic(output.join("wave-plan.md"), plan.as_bytes())
        .map_err(|e| format!("wave-plan.md: {e}"))?;
    written.push(output.join("wave-plan.md").display().to_string());

    let wave_title_tpl = translate("wave.title_placeholder", lang);
    let context_heading = translate("heading.spec.context", lang);
    let tasks_heading = translate("heading.spec.tasks", lang);
    let ac_heading = translate("heading.spec.ac", lang);
    let limits_heading = translate("heading.spec.limits", lang);
    for i in 1..=n {
        let wave_dir = output.join(format!("wave-{i}-{role}"));
        mfs::create_dir_all(&wave_dir).map_err(|e| format!("{}: {e}", wave_dir.display()))?;
        let title = wave_title_tpl.replace("{n}", &i.to_string());
        let mut body = String::new();
        let _ = write!(body, "# {title}\n\n");
        body.push_str("### Stage: Plan\n### Outcome: Active\n### Flags: \n\n");
        let _ = write!(
            body,
            "## {context_heading}\n\n{fill}\n\n## {tasks_heading}\n\n- [ ] T1 — ...\n\n## {ac_heading}\n\n"
        );
        let _ = write!(
            body,
            "- **AC-W{i}.1** — Build green. Command: `rtk cargo build`\n\n## {limits_heading}\n\nIN: ...\nOUT: ...\n"
        );
        let path = wave_dir.join("spec.md");
        mfs::write_atomic(&path, body.as_bytes())
            .map_err(|e| format!("{}: {e}", path.display()))?;
        written.push(path.display().to_string());
    }
    Ok(written)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Derive a kebab-case slug from a free-text intent. Mirrors
/// [`mustard_core::platform::i18n::slugify`] tolerances but stays local: no datestamp,
/// no truncation beyond 60 chars.
fn slug_from_intent(intent: &str) -> String {
    let mut s: String = intent
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    // Collapse runs of `-` and trim leading/trailing.
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    let trimmed = s.trim_matches('-').to_string();
    if trimmed.len() > 60 {
        trimmed.chars().take(60).collect()
    } else {
        trimmed
    }
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
        assert_eq!(slug_from_intent("Add user CRUD"), "add-user-crud");
        assert_eq!(slug_from_intent("  ---  Fix login   bug  "), "fix-login-bug");
    }

    #[test]
    fn build_input_validates() {
        let input = build_input("demo", "Demo", Scope::Full, "pt-BR", 2, Locale::PtBr);
        assert!(mustard_core::domain::spec::contract::validate(&input).is_ok());
    }

    #[test]
    fn build_input_validates_in_en_us() {
        // Section *names* stay PT-BR (canonical contract keys); bodies are EN.
        let input = build_input("demo", "Demo", Scope::Full, "en-US", 2, Locale::EnUs);
        assert!(mustard_core::domain::spec::contract::validate(&input).is_ok());
        // Body strings should be EN, not PT.
        let users = input
            .prd_sections
            .iter()
            .find(|s| s.name == "Usuários")
            .unwrap();
        assert!(users.body.contains("fill in"), "EN body got: {}", users.body);
    }

    #[test]
    fn section_heading_for_localises() {
        use crate::commands::spec::spec_scaffold::section_heading_for;
        assert_eq!(section_heading_for("Contexto", Locale::EnUs), "Context");
        assert_eq!(section_heading_for("Contexto", Locale::PtBr), "Contexto");
        // Unknown section name passes through unchanged.
        assert_eq!(section_heading_for("Extra", Locale::EnUs), "Extra");
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
            role: "mixed".into(),
            force: false,
        };
        run(opts);
        let root = dir.path().join("specs").join("demo");
        assert!(root.join("spec.md").exists());
        assert!(root.join("meta.json").exists());
        assert!(root.join("memory").join("_index.md").exists());
        assert!(root.join("wave-plan.md").exists());
        assert!(root.join("wave-1-mixed").join("spec.md").exists());
        assert!(root.join("wave-2-mixed").join("spec.md").exists());
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
            role: "mixed".into(),
            force: false,
        };
        run(opts);
        // Output dir should not have been populated.
        assert!(!dir.path().join("out").join("spec.md").exists());
    }
}
