//! Byte-stable spec layout contract (Wave 1 of `2026-05-25-mustard-deep-refactor`).
//!
//! ## What this is
//!
//! The single canonical description of what a `spec.md` + `meta.json`
//! (+ optional `wave-plan.md`) bundle looks like. Every consumer that
//! generates or validates a spec — `spec-draft`, `spec-validate`, the
//! `agent-prompt-render` task slicer, the dashboard's spec card — derives its
//! shape from the types in this module.
//!
//! ## Why
//!
//! Before this contract the layout was implicit: scattered string templates
//! inside `apps/cli/templates/commands/mustard/feature/SKILL.md` literally
//! wrote out ~80 lines of `## Contexto`, `## Tarefas`, `## Critérios de
//! Aceitação` boilerplate per intent. Skill drift, header drift, AC drift were
//! permanent. The contract pins the canonical sections, makes generation
//! deterministic, and gives validators an unambiguous referent.
//!
//! ## Design (SOLID + fail-open)
//!
//! - **Single responsibility.** This module owns the layout shape and nothing
//!   else. It does not parse legacy headers (that is [`crate::domain::spec`]), does
//!   not open IO ([`crate::domain::meta`] writes `meta.json`).
//! - **Pure functions.** Every public entry point is a deterministic function
//!   of its input. [`validate`] returns `Result<(), Vec<ContractViolation>>`
//!   — it never panics.
//! - **Strict but lenient input.** Inputs deserialize via lenient `Option`
//!   fields so partial test fixtures still drive the validator.

use crate::platform::i18n::UserLocale;
use crate::domain::model::view::{Outcome, Phase, Scope, Stage};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

// ---------------------------------------------------------------------------
// Canonical section names
// ---------------------------------------------------------------------------

/// Section list a spec narrative must carry, in order, after the PRD divider.
/// Names match the canonical PT-BR wording used in every Mustard spec; the
/// validator matches case-insensitively and tolerates the EN equivalents
/// (`Context` / `Users` / `Metric` / `Non-Goals` / `Acceptance Criteria`).
pub const PRD_SECTIONS: &[&str] = &[
    "Contexto",
    "Usuários",
    "Métrica",
    "Não-Objetivos",
    "Critérios de Aceitação",
];

/// Section list a spec plan must carry, in order, after the Plan divider.
pub const PLAN_SECTIONS: &[&str] = &["Arquivos", "Tarefas", "Limites"];

/// Markdown comment marker dividing the PRD half from the plan half.
pub const PRD_DIVIDER: &str = "<!-- PRD -->";

/// Markdown comment marker opening the plan half.
pub const PLAN_DIVIDER: &str = "<!-- PLAN -->";

// ---------------------------------------------------------------------------
// Acceptance criterion
// ---------------------------------------------------------------------------

/// One acceptance criterion. The `command` field must be a runnable shell
/// invocation (typically `rtk ...`); the validator asserts it is non-empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptanceCriterion {
    /// Identifier such as `AC-1`, `AC-W1.2`, `AC-G3`.
    pub id: String,
    /// One-sentence statement of what the AC asserts (narrative locale).
    pub statement: String,
    /// Runnable command — exit 0 ⇒ pass.
    pub command: String,
}

// ---------------------------------------------------------------------------
// Spec input
// ---------------------------------------------------------------------------

/// Input fed to [`validate`]. Represents the spec the caller wants to draft
/// (or has just drafted). Optional sections allow partial inputs during
/// migration; the validator surfaces missing required pieces as
/// [`ContractViolation::MissingSection`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpecInput {
    /// Spec slug (kebab-case). Required.
    #[serde(default)]
    pub slug: String,
    /// Spec title (free text). Required for narrative.
    #[serde(default)]
    pub title: String,
    /// Lifecycle stage (`Plan` / `Execute` / ...). Required.
    #[serde(default)]
    pub stage: Option<Stage>,
    /// Terminal outcome (`Active` for a fresh draft). Required.
    #[serde(default)]
    pub outcome: Option<Outcome>,
    /// Active pipeline phase (informational; mirrors [`Stage`]).
    #[serde(default)]
    pub phase: Option<Phase>,
    /// Pipeline scope (`Full` for wave plans, `Light` for single-shot).
    #[serde(default)]
    pub scope: Option<Scope>,
    /// BCP-47 narrative locale (`pt-BR`, `en-US`, `fr-FR`, ...). Required.
    ///
    /// Stored as `String` at the boundary because the rt-side callers
    /// (`spec_draft`, `spec_validate`) already work in BCP-47 strings; the
    /// validator routes the value through [`UserLocale::new`] so any
    /// short-form / malformed input is surfaced as
    /// [`ContractViolation::InvalidLang`]. W7 promotes this to
    /// `Option<UserLocale>` once the rt callsites are swept onto the new
    /// types.
    #[serde(default)]
    pub lang: Option<String>,
    /// Number of waves under this spec; required when scope = Full.
    #[serde(default)]
    pub total_waves: Option<u32>,
    /// PRD-side body: one entry per [`PRD_SECTIONS`] name.
    #[serde(default)]
    pub prd_sections: Vec<SectionBody>,
    /// Plan-side body: one entry per [`PLAN_SECTIONS`] name.
    #[serde(default)]
    pub plan_sections: Vec<SectionBody>,
    /// Acceptance criteria (3-8 typical; validator only enforces non-empty
    /// + command runnable).
    #[serde(default)]
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
}

/// One narrative section: heading + body text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionBody {
    /// Section name — must match one of [`PRD_SECTIONS`] or [`PLAN_SECTIONS`]
    /// (case-insensitive). Free text after that.
    pub name: String,
    /// Section body (markdown). Trimmed by the validator.
    pub body: String,
}

// ---------------------------------------------------------------------------
// Violations
// ---------------------------------------------------------------------------

/// One thing the validator found wrong. The same `kind` may repeat with
/// different `detail` for a multi-faceted failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum ContractViolation {
    /// A required scalar field is missing.
    #[error("missing field: {0}")]
    MissingField(String),
    /// One of the canonical sections is absent or empty.
    #[error("missing section: {0}")]
    MissingSection(String),
    /// Sections are in the wrong order (e.g. `## Tarefas` before `## Arquivos`).
    #[error("section order: {0}")]
    SectionOrder(String),
    /// `lang` is not a recognised BCP-47 code (the short `pt`/`en` short forms
    /// fail here).
    #[error("invalid lang: {0}")]
    InvalidLang(String),
    /// An acceptance criterion has an empty `command` field.
    #[error("AC {0} missing runnable Command")]
    AcMissingCommand(String),
    /// At least one acceptance criterion is required.
    #[error("no acceptance criteria")]
    AcEmpty,
    /// Full scope requires a `total_waves` ≥ 1.
    #[error("Full scope requires total_waves ≥ 1")]
    FullScopeNoWaves,
}

// ---------------------------------------------------------------------------
// Validator
// ---------------------------------------------------------------------------

/// Validate a [`SpecInput`] against the contract. Returns `Ok(())` when every
/// rule passes; otherwise a `Vec<ContractViolation>` listing every issue
/// found.
///
/// # Errors
///
/// Returns the collected violations when any rule fails. The returned
/// vector is non-empty.
pub fn validate(input: &SpecInput) -> Result<(), Vec<ContractViolation>> {
    let mut violations: Vec<ContractViolation> = Vec::new();

    // --- Scalars ----------------------------------------------------------
    if input.slug.trim().is_empty() {
        violations.push(ContractViolation::MissingField("slug".into()));
    }
    if input.title.trim().is_empty() {
        violations.push(ContractViolation::MissingField("title".into()));
    }
    if input.stage.is_none() {
        violations.push(ContractViolation::MissingField("stage".into()));
    }
    if input.outcome.is_none() {
        violations.push(ContractViolation::MissingField("outcome".into()));
    }
    // The string boundary routes through `UserLocale::new` so any short-form
    // or malformed shape surfaces as `InvalidLang`. The catalogue check
    // (i.e. "does Mustard ship strings for this locale?") is *not* enforced
    // here — the user is free to write specs in any BCP-47 locale; banner
    // rendering bridges via `to_supported().unwrap_or_default()` at the
    // callsite.
    match input.lang.as_deref() {
        None | Some("") => violations.push(ContractViolation::MissingField("lang".into())),
        Some(raw) => {
            if UserLocale::from_str(raw).is_err() {
                violations.push(ContractViolation::InvalidLang(raw.to_string()));
            }
        }
    }

    // --- Scope / wave-plan ------------------------------------------------
    if matches!(input.scope, Some(Scope::Full)) && input.total_waves.unwrap_or(0) == 0 {
        violations.push(ContractViolation::FullScopeNoWaves);
    }

    // --- PRD section presence + order ------------------------------------
    check_sections(&input.prd_sections, PRD_SECTIONS, "PRD", &mut violations);

    // --- Plan section presence + order (skipped for Light scope) ---------
    if !matches!(input.scope, Some(Scope::Light)) {
        check_sections(
            &input.plan_sections,
            PLAN_SECTIONS,
            "Plan",
            &mut violations,
        );
    }

    // --- Acceptance criteria ---------------------------------------------
    if input.acceptance_criteria.is_empty() {
        violations.push(ContractViolation::AcEmpty);
    }
    for ac in &input.acceptance_criteria {
        if ac.command.trim().is_empty() {
            violations.push(ContractViolation::AcMissingCommand(ac.id.clone()));
        }
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

/// Check section presence + ordering for one side (PRD or Plan).
fn check_sections(
    provided: &[SectionBody],
    expected: &[&str],
    side: &str,
    violations: &mut Vec<ContractViolation>,
) {
    // Presence + non-empty body.
    for want in expected {
        let found = provided
            .iter()
            .find(|s| s.name.trim().eq_ignore_ascii_case(want));
        match found {
            None => violations.push(ContractViolation::MissingSection(format!(
                "{side}.{want}"
            ))),
            Some(s) if s.body.trim().is_empty() => {
                violations.push(ContractViolation::MissingSection(format!(
                    "{side}.{want} (empty body)"
                )));
            }
            _ => {}
        }
    }

    // Ordering: the canonical sequence must appear in the same order. We
    // compare only the sections that the canonical list mentions; extra
    // sections in `provided` are tolerated and skipped.
    let canonical_indices: Vec<usize> = provided
        .iter()
        .filter_map(|s| {
            expected
                .iter()
                .position(|w| s.name.trim().eq_ignore_ascii_case(w))
        })
        .collect();
    if !canonical_indices.windows(2).all(|w| w[0] < w[1]) {
        violations.push(ContractViolation::SectionOrder(format!(
            "{side} sections out of canonical order"
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(scope: Scope) -> SpecInput {
        let prd: Vec<SectionBody> = PRD_SECTIONS
            .iter()
            .map(|n| SectionBody {
                name: (*n).to_string(),
                body: "body".to_string(),
            })
            .collect();
        let plan: Vec<SectionBody> = PLAN_SECTIONS
            .iter()
            .map(|n| SectionBody {
                name: (*n).to_string(),
                body: "body".to_string(),
            })
            .collect();
        SpecInput {
            slug: "demo".into(),
            title: "Demo".into(),
            stage: Some(Stage::Plan),
            outcome: Some(Outcome::Active),
            phase: Some(Phase::Plan),
            scope: Some(scope),
            lang: Some("pt-BR".to_string()),
            total_waves: if matches!(scope, Scope::Full) { Some(2) } else { None },
            prd_sections: prd,
            plan_sections: plan,
            acceptance_criteria: vec![AcceptanceCriterion {
                id: "AC-1".into(),
                statement: "x".into(),
                command: "rtk echo ok".into(),
            }],
        }
    }

    #[test]
    fn validates_full_fixture() {
        assert!(validate(&fixture(Scope::Full)).is_ok());
    }

    #[test]
    fn validates_light_fixture_without_plan_sections() {
        let mut input = fixture(Scope::Light);
        input.plan_sections.clear();
        // Light scope skips the plan-side check.
        assert!(validate(&input).is_ok());
    }

    #[test]
    fn rejects_missing_scalars() {
        let mut input = fixture(Scope::Full);
        input.slug = String::new();
        input.lang = None;
        input.stage = None;
        let err = validate(&input).unwrap_err();
        assert!(err.contains(&ContractViolation::MissingField("slug".into())));
        assert!(err.contains(&ContractViolation::MissingField("lang".into())));
        assert!(err.contains(&ContractViolation::MissingField("stage".into())));
    }

    #[test]
    fn rejects_short_lang_codes() {
        let mut input = fixture(Scope::Full);
        input.lang = Some("pt".into());
        let err = validate(&input).unwrap_err();
        assert!(err.iter().any(|v| matches!(v, ContractViolation::InvalidLang(_))));
    }

    #[test]
    fn accepts_non_catalogue_user_locales() {
        // Mustard does not need a translation catalogue for `fr-FR` for the
        // user to write a spec in that locale — the validator must accept
        // any well-shaped BCP-47 code, only rejecting short / malformed.
        let mut input = fixture(Scope::Full);
        input.lang = Some("fr-FR".into());
        assert!(validate(&input).is_ok());
        input.lang = Some("de-DE".into());
        assert!(validate(&input).is_ok());
        input.lang = Some("en-GB".into());
        assert!(validate(&input).is_ok());
    }

    #[test]
    fn rejects_malformed_lang() {
        let mut input = fixture(Scope::Full);
        input.lang = Some("ptbr".into());
        let err = validate(&input).unwrap_err();
        assert!(err.iter().any(|v| matches!(v, ContractViolation::InvalidLang(_))));
    }

    #[test]
    fn rejects_full_scope_without_waves() {
        let mut input = fixture(Scope::Full);
        input.total_waves = None;
        let err = validate(&input).unwrap_err();
        assert!(err.contains(&ContractViolation::FullScopeNoWaves));
    }

    #[test]
    fn rejects_missing_section() {
        let mut input = fixture(Scope::Full);
        input.prd_sections.pop(); // drop AC section
        let err = validate(&input).unwrap_err();
        assert!(err.iter().any(|v| matches!(v, ContractViolation::MissingSection(_))));
    }

    #[test]
    fn rejects_out_of_order_sections() {
        let mut input = fixture(Scope::Full);
        input.plan_sections.swap(0, 2); // Tarefas before Arquivos.
        let err = validate(&input).unwrap_err();
        assert!(err.iter().any(|v| matches!(v, ContractViolation::SectionOrder(_))));
    }

    #[test]
    fn rejects_ac_without_command() {
        let mut input = fixture(Scope::Full);
        input.acceptance_criteria[0].command = String::new();
        let err = validate(&input).unwrap_err();
        assert!(err.iter().any(|v| matches!(v, ContractViolation::AcMissingCommand(_))));
    }

    #[test]
    fn rejects_empty_acceptance_criteria() {
        let mut input = fixture(Scope::Full);
        input.acceptance_criteria.clear();
        let err = validate(&input).unwrap_err();
        assert!(err.contains(&ContractViolation::AcEmpty));
    }
}
