//! `wave_summary` — canonical `_summary.md` generator for a closed Mustard wave.
//!
//! Spec A v4 / Wave 3 introduces the canonical schema of the `_summary.md` file
//! written at the end of each wave. The schema has **7 required sections** —
//! every heading flows through [`mustard_core::i18n::translate`] so the output
//! follows the project locale declared in `.claude/mustard.json#lang`. No
//! user-facing string is hardcoded in this module; only structural separators
//! (`##`, `|`, `---`) are literal.
//!
//! ## Schema (AC-A-8)
//!
//! ```text
//! ## {heading.summary.objective}
//! {free-form objective body}
//!
//! ## {heading.summary.inheritance}
//! - [[wikilink]]
//!
//! ## {heading.summary.decisions}
//! - …
//!
//! ## {heading.summary.code}
//! | qualifier | status | path |
//!
//! ## {heading.summary.ac}
//! - AC-A-N: pass|fail
//!
//! ## {heading.summary.verdict}
//! {green|amber|red} — …
//!
//! ## {heading.summary.next_steps}
//! - [[wikilink]]
//! ```
//!
//! ## Design
//!
//! - **Single responsibility.** Markdown rendering + atomic write. No event
//!   emission, no diff capture, no spec mutation.
//! - **Idempotent.** [`build`] depends only on its [`WaveSummaryInput`]; running
//!   it twice with identical input returns byte-identical strings. No
//!   `SystemTime::now()` ever enters the body — callers pass formatted
//!   timestamps explicitly when needed.
//! - **i18n-pure.** Every heading is read from the catalogue. Adding a new
//!   locale = adding two arms in `core::i18n::translate`, never editing this
//!   module.
//! - **Fail-open writes.** [`write`] surfaces IO errors via
//!   [`WaveSummaryError::Io`] but never panics; callers can decide whether the
//!   wave-close pipeline degrades or halts on a failed write.

use mustard_core::fs as mfs;
use mustard_core::i18n::{translate, SupportedLocale as Locale};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// One outgoing wikilink target — rendered as `[[name]]` (or `[[name|alias]]`
/// when an alias is provided). The alias slot keeps the link free of
/// hardcoded display text in the renderer; PT-BR vs EN-US presentation comes
/// from the caller, not from this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLink {
    /// Target identifier (e.g. spec slug, wave id, memory note name).
    pub target: String,
    /// Optional display alias. When `Some`, rendered as `[[target|alias]]`.
    pub alias: Option<String>,
}

impl WikiLink {
    /// Build a link without an alias.
    #[must_use]
    pub fn new(target: impl Into<String>) -> Self {
        Self { target: target.into(), alias: None }
    }

    /// Build a link with an alias.
    #[must_use]
    pub fn with_alias(target: impl Into<String>, alias: impl Into<String>) -> Self {
        Self { target: target.into(), alias: Some(alias.into()) }
    }

    /// Render the link as a literal `[[ ]]` token.
    #[must_use]
    pub fn render(&self) -> String {
        match &self.alias {
            Some(a) => format!("[[{}|{a}]]", self.target),
            None => format!("[[{}]]", self.target),
        }
    }
}

/// One row in the `## Código` table — the qualifier touched + its status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionEntry {
    /// Qualifier as declared in the spec's `## Funções tocadas` section
    /// (R3 — module, path-hint or pure form). Verbatim.
    pub qualifier: String,
    /// On-disk status label written verbatim — typically the output of
    /// [`mustard_core::spec::touched_functions::FunctionStatus::label`]
    /// (`NOVO` / `ESTENDIDO` / `MODIFICADO`).
    pub status: String,
    /// Path hint where the function lives, as declared in the spec.
    pub path_hint: String,
}

/// One row in the `## Critérios de Aceitação` section — an AC id + result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcEntry {
    /// AC identifier (e.g. `AC-A-8`).
    pub id: String,
    /// Result text rendered verbatim — typically `pass` / `fail` / `skip`.
    pub result: String,
    /// Optional one-line note appended after an em-dash separator.
    pub note: Option<String>,
}

/// Verdict block — color label + reason text. Both flow through the caller
/// (typically populated from `regression_check::Diff` summaries + the
/// `gate.verdict.*` catalogue keys). The renderer is deliberately
/// content-agnostic to keep this module independent of W2 / W4 types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictDisplay {
    /// Verdict label, rendered verbatim (e.g. the result of
    /// `translate("gate.verdict.green.label", locale)`).
    pub label: String,
    /// Reason line, rendered verbatim. Typically the matching
    /// `gate.verdict.*.message` translation.
    pub message: String,
}

/// Input to [`build`]. Every string field is a *data value* (already-resolved
/// domain content); the seven *headings* come from the i18n catalogue at
/// render time, never from this struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveSummaryInput {
    /// Spec slug under `.claude/spec/`. Used for the wikilink rendered above
    /// the inheritance section in callers; not embedded in the body itself.
    pub spec_slug: String,
    /// Wave directory name (e.g. `wave-3-rt`). Used by [`write`] to resolve
    /// the output path; not embedded in the body.
    pub wave_id: String,
    /// Free-form objective body — domain content, never translated by this
    /// module.
    pub objective: String,
    /// Wikilinks to prior waves / spec memory inherited by this wave.
    pub inheritance: Vec<WikiLink>,
    /// Free-form decision lines (one bullet per entry).
    pub decisions: Vec<String>,
    /// Touched-functions table rows.
    pub code_table: Vec<FunctionEntry>,
    /// AC status rows.
    pub ac_status: Vec<AcEntry>,
    /// Verdict block, optional — `None` when the wave produced no regression
    /// check (e.g. doc-only waves).
    pub verdict: Option<VerdictDisplay>,
    /// Wikilinks to follow-up specs, next-wave seeds, or memory notes.
    pub next_steps: Vec<WikiLink>,
}

/// Errors surfaced by [`write`].
#[derive(Debug)]
pub enum WaveSummaryError {
    /// IO failure during the atomic write (target path, root cause).
    Io {
        /// The path the writer attempted to materialise.
        path: PathBuf,
        /// Wrapped IO error from `mustard_core::fs::write_atomic`.
        source: std::io::Error,
    },
    /// `_context.md` exceeded the 8 000-word cap (W3.T3.2 / AC-A-9). Reused
    /// here so [`crate::run::wave::wave_context`] can share the error type without
    /// pulling another module.
    ContextTooLong {
        /// Actual word count of the rendered context body.
        actual_words: usize,
        /// The hard cap (8 000 — AC-A-9).
        cap: usize,
    },
}

impl std::fmt::Display for WaveSummaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "wave-summary write failed at {}: {source}", path.display())
            }
            Self::ContextTooLong { actual_words, cap } => write!(
                f,
                "wave-context exceeds cap: {actual_words} words > {cap} cap"
            ),
        }
    }
}

impl std::error::Error for WaveSummaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::ContextTooLong { .. } => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Render the canonical `_summary.md` body for a wave. The output always
/// carries the 7 required sections of AC-A-8, with every heading sourced
/// from `core::i18n::translate(key, locale)`.
///
/// The function is pure: same `input` + same `locale` produces byte-identical
/// output. No timestamps, no clock reads, no env reads.
#[must_use]
pub fn build(input: &WaveSummaryInput, locale: Locale) -> String {
    let mut body = String::new();

    // 1. Objetivo
    write_heading(&mut body, "heading.summary.objective", locale);
    if input.objective.trim().is_empty() {
        // Fail-open: render the placeholder rather than an empty section so
        // downstream parsers always find content after the heading.
        let _ = writeln!(body, "{}", translate("placeholder.fill", locale));
    } else {
        let _ = writeln!(body, "{}", input.objective.trim_end());
    }
    body.push('\n');

    // 2. Herança
    write_heading(&mut body, "heading.summary.inheritance", locale);
    write_wikilink_list(&mut body, &input.inheritance, locale);
    body.push('\n');

    // 3. Decisões
    write_heading(&mut body, "heading.summary.decisions", locale);
    write_text_bullets(&mut body, &input.decisions, locale);
    body.push('\n');

    // 4. Código — touched-functions table.
    write_heading(&mut body, "heading.summary.code", locale);
    write_code_table(&mut body, &input.code_table, locale);
    body.push('\n');

    // 5. Critérios de Aceitação
    write_heading(&mut body, "heading.summary.ac", locale);
    write_ac_rows(&mut body, &input.ac_status, locale);
    body.push('\n');

    // 6. Verdict
    write_heading(&mut body, "heading.summary.verdict", locale);
    match &input.verdict {
        Some(v) => {
            // Label and message both come from the caller (post-translation).
            let _ = writeln!(body, "{} — {}", v.label, v.message);
        }
        None => {
            let _ = writeln!(body, "{}", translate("placeholder.fill", locale));
        }
    }
    body.push('\n');

    // 7. Próximos passos
    write_heading(&mut body, "heading.summary.next_steps", locale);
    write_wikilink_list(&mut body, &input.next_steps, locale);

    body
}

/// Atomically write a rendered `_summary.md` body to
/// `{spec_root}/{wave_id}/_summary.md`. Returns the resolved path on success.
///
/// # Errors
///
/// Returns [`WaveSummaryError::Io`] when the underlying atomic write fails.
pub fn write(
    spec_root: &Path,
    wave_id: &str,
    content: &str,
) -> Result<PathBuf, WaveSummaryError> {
    let target = spec_root.join(wave_id).join("_summary.md");
    mfs::write_atomic(&target, content.as_bytes()).map_err(|e| WaveSummaryError::Io {
        path: target.clone(),
        source: std::io::Error::other(e.to_string()),
    })?;
    Ok(target)
}

// ---------------------------------------------------------------------------
// Section writers — private, isolated for readability + testability.
// ---------------------------------------------------------------------------

/// Emit a `## {translate(key, locale)}\n\n` block. Single source for the
/// heading shape so every section renders identically.
fn write_heading(body: &mut String, key: &str, locale: Locale) {
    let _ = writeln!(body, "## {}", translate(key, locale));
    body.push('\n');
}

/// Render a wikilink bullet list. Empty input emits the catalogue's
/// `placeholder.fill` so the section is never silently blank.
fn write_wikilink_list(body: &mut String, links: &[WikiLink], locale: Locale) {
    if links.is_empty() {
        let _ = writeln!(body, "{}", translate("placeholder.fill", locale));
        return;
    }
    for link in links {
        let _ = writeln!(body, "- {}", link.render());
    }
}

/// Render a free-text bullet list. Empty input emits `placeholder.fill`.
fn write_text_bullets(body: &mut String, items: &[String], locale: Locale) {
    if items.is_empty() {
        let _ = writeln!(body, "{}", translate("placeholder.fill", locale));
        return;
    }
    for item in items {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        let _ = writeln!(body, "- {trimmed}");
    }
}

/// Render the code-touched table. Empty input emits `placeholder.fill`.
///
/// Column labels are intentionally identifier-like (literal `qualifier` /
/// `status` / `path`) because they double as schema keys consumed by the W6
/// resume-bootstrap parser. Localising them would break the parser without
/// adding any user-facing value — the column meanings are obvious from the
/// surrounding section heading.
fn write_code_table(body: &mut String, rows: &[FunctionEntry], locale: Locale) {
    if rows.is_empty() {
        let _ = writeln!(body, "{}", translate("placeholder.fill", locale));
        return;
    }
    body.push_str("| qualifier | status | path |\n");
    body.push_str("|-----------|--------|------|\n");
    for row in rows {
        let _ = writeln!(
            body,
            "| `{}` | {} | `{}` |",
            row.qualifier.replace('|', "\\|"),
            row.status.replace('|', "\\|"),
            row.path_hint.replace('|', "\\|"),
        );
    }
}

/// Render the AC bullet rows. Empty input emits `placeholder.fill`.
fn write_ac_rows(body: &mut String, rows: &[AcEntry], locale: Locale) {
    if rows.is_empty() {
        let _ = writeln!(body, "{}", translate("placeholder.fill", locale));
        return;
    }
    for row in rows {
        match &row.note {
            Some(note) => {
                let _ = writeln!(body, "- {}: {} — {}", row.id, row.result, note);
            }
            None => {
                let _ = writeln!(body, "- {}: {}", row.id, row.result);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_input() -> WaveSummaryInput {
        WaveSummaryInput {
            spec_slug: "2026-05-27-mustard-v4-foundation".into(),
            wave_id: "wave-3-rt".into(),
            objective: "Entregar o formato canonico do _summary.md".into(),
            inheritance: vec![WikiLink::new("wave-2-rt")],
            decisions: vec!["Heading via translate".into()],
            code_table: vec![FunctionEntry {
                qualifier: "wave_summary::build".into(),
                status: "NOVO".into(),
                path_hint: "apps/rt/src/run/".into(),
            }],
            ac_status: vec![AcEntry {
                id: "AC-A-8".into(),
                result: "pass".into(),
                note: None,
            }],
            verdict: Some(VerdictDisplay {
                label: translate("gate.verdict.green.label", Locale::PtBr).to_string(),
                message: translate("gate.verdict.green.message", Locale::PtBr).to_string(),
            }),
            next_steps: vec![WikiLink::new("wave-4-rt")],
        }
    }

    // AC-A-8 — PT-BR catalogue produces the canonical pt-BR headings.
    #[test]
    fn test_seven_required_headings_pt_br() {
        let input = sample_input();
        let body = build(&input, Locale::PtBr);

        for key in [
            "heading.summary.objective",
            "heading.summary.inheritance",
            "heading.summary.decisions",
            "heading.summary.code",
            "heading.summary.ac",
            "heading.summary.verdict",
            "heading.summary.next_steps",
        ] {
            let expected = format!("## {}", translate(key, Locale::PtBr));
            assert!(
                body.contains(&expected),
                "missing pt-BR heading `{expected}` (key {key}). Body:\n{body}"
            );
        }
    }

    // AC-A-8 — EN-US catalogue produces the canonical en-US headings.
    #[test]
    fn test_seven_required_headings_en_us() {
        let input = sample_input();
        let body = build(&input, Locale::EnUs);

        for key in [
            "heading.summary.objective",
            "heading.summary.inheritance",
            "heading.summary.decisions",
            "heading.summary.code",
            "heading.summary.ac",
            "heading.summary.verdict",
            "heading.summary.next_steps",
        ] {
            let expected = format!("## {}", translate(key, Locale::EnUs));
            assert!(
                body.contains(&expected),
                "missing en-US heading `{expected}` (key {key}). Body:\n{body}"
            );
        }
    }

    // T3.3 — build twice with the same input must produce byte-identical strings.
    #[test]
    fn build_is_idempotent_byte_identical() {
        let input = sample_input();
        let a = build(&input, Locale::PtBr);
        let b = build(&input, Locale::PtBr);
        assert_eq!(a, b, "build() must be deterministic");
    }

    #[test]
    fn empty_sections_render_placeholder_not_blank() {
        let input = WaveSummaryInput {
            spec_slug: "demo".into(),
            wave_id: "wave-1-rt".into(),
            objective: String::new(),
            inheritance: vec![],
            decisions: vec![],
            code_table: vec![],
            ac_status: vec![],
            verdict: None,
            next_steps: vec![],
        };
        let body = build(&input, Locale::PtBr);
        let placeholder = translate("placeholder.fill", Locale::PtBr);
        // Every one of the 7 sections falls back to the placeholder when its
        // input is empty (fail-open contract).
        let occurrences = body.matches(placeholder).count();
        assert!(
            occurrences >= 7,
            "expected ≥7 placeholder occurrences for fully-empty input; got {occurrences}. Body:\n{body}"
        );
    }

    #[test]
    fn write_creates_summary_at_wave_dir() {
        let dir = tempfile::tempdir().unwrap();
        let spec_root = dir.path();
        let content = "## Objetivo\n\nbody\n";
        let written = write(spec_root, "wave-3-rt", content).expect("write succeeds");
        assert_eq!(written, spec_root.join("wave-3-rt").join("_summary.md"));
        let read = std::fs::read_to_string(&written).expect("readable");
        assert_eq!(read, content);
    }

    #[test]
    fn wikilink_render_with_and_without_alias() {
        assert_eq!(WikiLink::new("a").render(), "[[a]]");
        assert_eq!(WikiLink::with_alias("a", "alias").render(), "[[a|alias]]");
    }
}
