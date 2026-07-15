// SPEC LANG: pt-allowed — section heading is canonical pt-BR (`## Funções tocadas`).
//! `touched_functions` — parser, validator and fallback resolver for the
//! canonical `## Funções tocadas` section of a Mustard spec.
//!
//! Every Rust identifier in this module is EN per [`project_code_language_policy`]
//! and the Spec A wave-0 hard rule (struct fields `added`/`extended`/`modified`,
//! enum variants `Status::{Added, Extended, Modified}`). The on-disk vocabulary
//! is **pt-BR by contract**: the section heading is `## Funções tocadas` and the
//! status tokens that appear in the markdown are `NOVO` / `ESTENDIDO` /
//! `MODIFICADO` (`FunctionStatus::label()` round-trips through
//! `FunctionStatus::parse`). EN aliases (`Added` / `Extended` / `Modified` /
//! `Touched Functions`) are accepted by the parser for resilience but are not
//! the canonical spelling — `label()` always emits the pt-BR form.
//!
//! ## Why this module exists
//!
//! Mustard v4 Spec A introduces the canonical declaration of which public
//! functions a wave will touch and at what status (`NOVO` / `ESTENDIDO` /
//! `MODIFICADO`). The downstream regression-check gate (W2 + W4) and the
//! before/after snapshot (W2) both consume this declaration as the source of
//! truth for *what the wave promised to preserve*. A drift between this list
//! and the diff is the signal that a stub-fail-open ([[feedback_refactor_no_stub_deferral]])
//! is happening in plain sight.
//!
//! This module is **the** parser of that section. Every consumer in `apps/rt`
//! depends on it instead of running its own ad-hoc string scan, so a future
//! format extension changes here only (Open/Closed).
//!
//! ## Canonical format (R1-R6 — design in `funcoes-tocadas.md`)
//!
//! ```text
//! ## Funções tocadas
//!
//! ### Em `packages/core/src/regression_check/` (NOVO)
//! - `regression_check::Snapshot::capture_for_spec`
//! - `regression_check::Snapshot::compare_to` — comparison primitive
//!
//! ### Em `apps/rt/src/run/` (ESTENDIDO)
//! - `resume_bootstrap::run`
//! ```
//!
//! Rules:
//!
//! - **R1.** Each subsection starts with ``### Em `{path}` ({status})`` where
//!   `status ∈ {NOVO, ESTENDIDO, MODIFICADO}`.
//! - **R2.** Each function line starts with `- ` followed by a backtick-quoted
//!   qualifier.
//! - **R3.** Qualifier has three shapes: `crate::module::function`,
//!   `module::function`, or `path/to/file::function`.
//! - **R4.** Trailing `— justificativa` comment (em-dash separator) allowed.
//! - **R5.** Only public functions; helpers and tests do not belong here.
//! - **R6.** `NOVO` = new function; `ESTENDIDO` = exists, gains responsibility;
//!   `MODIFICADO` = behaviour changes materially.
//!
//! ## Design (SOLID + fail-open)
//!
//! - **Single responsibility.** Parses the section and nothing else. No
//!   filesystem traversal beyond reading the spec text; no LLM, no diff.
//! - **Pure.** [`parse`] is a pure function on `&str`.
//! - **Fail-open.** A parser failure on a single line skips that line instead
//!   of panicking. Hook-safe.

use crate::domain::spec::header_region_lines;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Declared status of a touched function. Drives the AC-typing rules that
/// W2 + W4 enforce (`Added` = needs ≥1 positive AC, `Modified` = needs ≥1
/// regression AC, etc. — see the Fase B design). Variant names are EN per
/// the Spec A wave-0 hard rule; the pt-BR tokens `NOVO`/`ESTENDIDO`/
/// `MODIFICADO` are the on-disk markdown vocabulary and are produced /
/// consumed by [`FunctionStatus::label`] and [`FunctionStatus::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionStatus {
    /// Function does not exist yet in the codebase — the wave creates it.
    /// On-disk token: `NOVO` (EN alias `NEW` / `ADDED` also parsed).
    Added,
    /// Function exists and gains a new responsibility. On-disk token:
    /// `ESTENDIDO` (EN alias `EXTENDED` also parsed).
    Extended,
    /// Function exists and changes behaviour materially. On-disk token:
    /// `MODIFICADO` (EN alias `MODIFIED` also parsed).
    Modified,
}

impl FunctionStatus {
    /// Parse a header-status token. Recognises both pt-BR (canonical) and EN
    /// spellings, case-insensitively. Unknown tokens return `None`.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let t = raw.trim().to_ascii_uppercase();
        match t.as_str() {
            "NOVO" | "NOVA" | "NEW" | "ADDED" => Some(Self::Added),
            "ESTENDIDO" | "ESTENDIDA" | "EXTENDED" => Some(Self::Extended),
            "MODIFICADO" | "MODIFICADA" | "MODIFIED" => Some(Self::Modified),
            _ => None,
        }
    }

    /// Canonical pt-BR label written in spec markdown (round-trips through
    /// [`FunctionStatus::parse`]).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Added => "NOVO",
            Self::Extended => "ESTENDIDO",
            Self::Modified => "MODIFICADO",
        }
    }
}

/// Shape of the qualifier on a `## Funções tocadas` line (R3 — three forms).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Qualifier {
    /// Module-and-function form: `module::sub::function`. No `/` in the path,
    /// at least one `::`.
    Module(String),
    /// File-path form: `path/to/file::function`. Contains `/` before the first
    /// `::`.
    PathHint {
        /// Path portion (left of the first `::`).
        path: String,
        /// Function portion (right of the first `::`).
        function: String,
    },
    /// Bare function name — accepted when the line has no `::` at all. Used by
    /// legacy specs that listed pure function names; the validator surfaces a
    /// `MissingQualifier` violation when this shape appears inside a spec that
    /// otherwise uses qualified forms.
    Pure(String),
}

impl Qualifier {
    /// Parse a stripped qualifier token (already without backticks). Whitespace
    /// is trimmed. Returns `None` only when the token is empty.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let t = raw.trim();
        if t.is_empty() {
            return None;
        }
        if t.contains('/') {
            // Path-hint form: split on the first `::`. If no `::` follows the
            // path, treat it as PathHint with empty function (caller surfaces
            // the issue via `MissingFunction` in the validator).
            let (path, function) = t.split_once("::").unwrap_or((t, ""));
            return Some(Self::PathHint {
                path: path.trim().to_string(),
                function: function.trim().to_string(),
            });
        }
        if t.contains("::") {
            return Some(Self::Module(t.to_string()));
        }
        Some(Self::Pure(t.to_string()))
    }

    /// The string form a caller would see in the spec (used for error
    /// messages, snapshot keys, and equality across the three shapes).
    #[must_use]
    pub fn as_str(&self) -> String {
        match self {
            Self::Module(s) | Self::Pure(s) => s.clone(),
            Self::PathHint { path, function } => format!("{path}::{function}"),
        }
    }

    /// Final identifier — the function name, stripped of every namespace /
    /// path prefix. Used to match against `extract_function_signatures`
    /// output in W1.5 (AST) and against diff output (W4 / fallback).
    #[must_use]
    pub fn function_name(&self) -> String {
        match self {
            Self::Module(s) => s
                .rsplit("::")
                .next()
                .unwrap_or(s)
                .trim()
                .to_string(),
            Self::PathHint { function, .. } => function.trim().to_string(),
            Self::Pure(s) => s.trim().to_string(),
        }
    }
}

/// One declared function on a `## Funções tocadas` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TouchedFunction {
    /// Qualifier as written (R3 — three forms).
    pub qualifier: Qualifier,
    /// Status inherited from the enclosing subsection header (R1 / R6).
    pub status: FunctionStatus,
    /// Path hint declared on the enclosing subsection header (R1) — typically
    /// the directory or file the function lives in. Used by the regression
    /// snapshot to locate the AST.
    pub path_hint: String,
    /// Optional trailing comment (R4 — em-dash separator).
    pub justification: Option<String>,
}

/// Parsed `## Funções tocadas` section. Empty vectors are legal — a spec that
/// declares the section but lists no functions parses as `TouchedFunctions {
/// added: [], extended: [], modified: [] }`. Field names are EN per the wave-0
/// hard rule; the pt-BR tokens that appear in markdown (`NOVO` / `ESTENDIDO`
/// / `MODIFICADO`) map onto these fields through [`FunctionStatus::parse`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TouchedFunctions {
    /// Functions with status [`FunctionStatus::Added`] (on-disk token `NOVO`).
    pub added: Vec<TouchedFunction>,
    /// Functions with status [`FunctionStatus::Extended`] (on-disk token `ESTENDIDO`).
    pub extended: Vec<TouchedFunction>,
    /// Functions with status [`FunctionStatus::Modified`] (on-disk token `MODIFICADO`).
    pub modified: Vec<TouchedFunction>,
}

impl TouchedFunctions {
    /// Flat iterator across every declared function regardless of status. The
    /// validator and the regression-check snapshot consume this view.
    pub fn all(&self) -> impl Iterator<Item = &TouchedFunction> {
        self.added
            .iter()
            .chain(self.extended.iter())
            .chain(self.modified.iter())
    }

    /// `true` when the section parsed empty (no declared functions). Different
    /// from "section absent", which [`parse`] signals by returning `None`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.extended.is_empty() && self.modified.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

const SECTION_HEADING_PT: &str = "## Funções tocadas";
const SECTION_HEADING_EN: &str = "## Touched Functions";

/// `true` when `line` opens the `## Funções tocadas` section in either PT-BR
/// (canonical) or EN spelling. Other `## ` headings end the section.
fn is_section_open(line: &str) -> bool {
    let t = line.trim_start();
    t.eq_ignore_ascii_case(SECTION_HEADING_PT) || t.eq_ignore_ascii_case(SECTION_HEADING_EN)
}

/// `true` when `line` is a body-level (`## `) heading — closes the section.
fn is_body_section(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("## ") && !is_section_open(line)
}

/// Strip the surrounding backticks (one per side) from `value`. When the
/// value is not backtick-wrapped, returns it unchanged.
fn strip_backticks(value: &str) -> &str {
    let t = value.trim();
    t.strip_prefix('`')
        .and_then(|inner| inner.strip_suffix('`'))
        .unwrap_or(t)
}

/// Parse a subsection header like:
///
/// ```text
/// ### Em `packages/core/src/regression_check/` (NOVO)
/// ```
///
/// Returns `Some((path_hint, status_token))` when the line matches the shape.
/// `status_token` is the raw text inside the parentheses (the caller routes
/// it through [`FunctionStatus::parse`] so the diagnostic can name the bad
/// token verbatim).
fn parse_subsection_header(line: &str) -> Option<(String, String)> {
    let t = line.trim_start();
    let rest = t.strip_prefix("###")?.trim_start();
    // Accept both `Em ` (PT) and `In ` (EN). Case-insensitive on the keyword.
    let after_kw = rest
        .strip_prefix("Em ")
        .or_else(|| rest.strip_prefix("em "))
        .or_else(|| rest.strip_prefix("In "))
        .or_else(|| rest.strip_prefix("in "))?;
    // The path is in backticks; status follows in parens. Tolerant of extra
    // whitespace between them.
    let after_kw = after_kw.trim_start();
    let after_open = after_kw.strip_prefix('`')?;
    let (path, after_path) = after_open.split_once('`')?;
    let after_path = after_path.trim_start();
    let after_paren = after_path.strip_prefix('(')?;
    let (status, _tail) = after_paren.split_once(')')?;
    Some((path.trim().to_string(), status.trim().to_string()))
}

/// Parse one function line — `- \`qualifier\` [— justification]`. Returns
/// `None` when the line is not a `- ` bullet or carries no qualifier text.
fn parse_function_line(line: &str) -> Option<(Qualifier, Option<String>)> {
    let t = line.trim_start();
    let rest = t.strip_prefix("- ").or_else(|| t.strip_prefix("-\t"))?;
    // Split off any em-dash justification (R4). The em-dash is the canonical
    // separator; the ASCII `--` and ` - ` (space-dash-space) variants are also
    // tolerated so handwritten specs do not fail validation on a stylistic
    // miss.
    let (qualifier_part, justification) = split_justification(rest);
    let qualifier_text = strip_backticks(qualifier_part);
    let qualifier = Qualifier::parse(qualifier_text)?;
    Some((qualifier, justification))
}

/// Split a function-line body into `(qualifier_text, justification_or_none)`.
/// Recognises three separators in priority order: em-dash (`—`), ASCII
/// double-dash (` -- `), space-dash-space (` - `). The first match wins so
/// `- a — b -- c` parses with em-dash as the separator and `b -- c` as the
/// justification.
fn split_justification(body: &str) -> (&str, Option<String>) {
    for sep in ["—", " -- ", " - "] {
        if let Some((q, j)) = body.split_once(sep) {
            return (q, Some(j.trim().to_string()));
        }
    }
    (body, None)
}

/// Parse the `## Funções tocadas` section out of `spec_md`. Returns
/// `Some(parsed)` when the section is present (even if empty), or `None` when
/// the section is absent (drives the fallback path).
///
/// Header-region scoped: a `## Funções tocadas` accidentally written inside a
/// fenced code block earlier in the file is **not** read as the section open.
/// The parser walks line-by-line and ignores anything inside a fence.
#[must_use]
pub fn parse(spec_md: &str) -> Option<TouchedFunctions> {
    // Skip the lifecycle header region — keeps `## Funções tocadas` mentioned
    // in a header-region comment from being parsed as content.
    let region = header_region_lines(spec_md);

    let mut in_section = false;
    let mut in_fence = false;
    let mut current_path: Option<String> = None;
    let mut current_status: Option<FunctionStatus> = None;
    let mut out = TouchedFunctions::default();

    for (idx, raw) in spec_md.lines().enumerate() {
        let trimmed = raw.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if idx < region {
            continue;
        }

        if is_section_open(raw) {
            in_section = true;
            current_path = None;
            current_status = None;
            continue;
        }
        if !in_section {
            continue;
        }
        if is_body_section(raw) {
            // Another `## ` heading closes the section.
            break;
        }

        if let Some((path, status_raw)) = parse_subsection_header(raw) {
            current_path = Some(path);
            current_status = FunctionStatus::parse(&status_raw);
            // Unknown status: surfaced by validator; parser still tracks the
            // section so subsequent function lines under it are skipped
            // rather than mis-attributed.
            continue;
        }

        if let Some((qualifier, justification)) = parse_function_line(raw) {
            // A function line outside a subsection (no prior `### Em` header)
            // is dropped — the parser cannot attach a status to it. The
            // validator catches that via the missing-section case.
            let Some(status) = current_status else {
                continue;
            };
            let path_hint = current_path.clone().unwrap_or_default();
            let touched = TouchedFunction {
                qualifier,
                status,
                path_hint,
                justification,
            };
            match status {
                FunctionStatus::Added => out.added.push(touched),
                FunctionStatus::Extended => out.extended.push(touched),
                FunctionStatus::Modified => out.modified.push(touched),
            }
        }
    }

    if in_section { Some(out) } else { None }
}

// ---------------------------------------------------------------------------
// Validator
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Fallback resolver
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse ---------------------------------------------------------

    #[test]
    fn parse_returns_none_when_section_absent() {
        let md = "# Spec\n\n## Contexto\n\nbody\n";
        assert!(parse(md).is_none());
    }

    #[test]
    fn parse_returns_empty_when_section_present_but_empty() {
        let md = "# Spec\n\n## Funções tocadas\n\n## Próxima seção\n";
        let parsed = parse(md).expect("section opens");
        assert!(parsed.is_empty());
    }

    #[test]
    fn parse_three_qualifier_shapes() {
        let md = "\
# Spec

## Funções tocadas

### Em `packages/core/src/regression_check/` (NOVO)
- `regression_check::Snapshot::capture_for_spec`
- `apps/rt/src/run/gate.rs::run`
- `bare_function`

### Em `apps/rt/src/run/` (ESTENDIDO)
- `resume_bootstrap::run` — adds pruning by token budget
";
        let parsed = parse(md).expect("parses");
        assert_eq!(parsed.added.len(), 3);
        assert_eq!(parsed.extended.len(), 1);

        // R3 — module form
        assert!(matches!(
            parsed.added[0].qualifier,
            Qualifier::Module(_)
        ));
        assert_eq!(
            parsed.added[0].qualifier.function_name(),
            "capture_for_spec"
        );

        // R3 — path-hint form
        assert!(matches!(
            parsed.added[1].qualifier,
            Qualifier::PathHint { .. }
        ));
        assert_eq!(parsed.added[1].qualifier.function_name(), "run");

        // R3 — pure form
        assert!(matches!(parsed.added[2].qualifier, Qualifier::Pure(_)));
        assert_eq!(parsed.added[2].qualifier.function_name(), "bare_function");

        // R4 — justification
        assert_eq!(
            parsed.extended[0].justification.as_deref(),
            Some("adds pruning by token budget")
        );

        // R1 — status inherited from subsection header
        assert_eq!(parsed.added[0].status, FunctionStatus::Added);
        assert_eq!(parsed.extended[0].status, FunctionStatus::Extended);
    }

    #[test]
    fn parse_status_modificado_and_en_aliases() {
        let md = "\
# S

## Funções tocadas

### Em `crate::a/` (MODIFICADO)
- `a::run`

### In `crate::b/` (Extended)
- `b::run`
";
        let parsed = parse(md).expect("parses");
        assert_eq!(parsed.modified.len(), 1);
        assert_eq!(parsed.extended.len(), 1);
    }

    #[test]
    fn parse_ignores_section_inside_code_fence() {
        let md = "\
# S

```text
## Funções tocadas
### Em `foo/` (NOVO)
- `foo::bar`
```

## Próxima seção
body
";
        // The only `## Funções tocadas` was inside a fence; the parser must
        // not treat it as an opening.
        assert!(parse(md).is_none());
    }

    #[test]
    fn parse_stops_at_next_body_section() {
        let md = "\
# S

## Funções tocadas

### Em `a/` (NOVO)
- `a::one`

## Acceptance Criteria

- `should_not_be_parsed::two`
";
        let parsed = parse(md).expect("parses");
        assert_eq!(parsed.added.len(), 1);
        assert_eq!(parsed.added[0].qualifier.function_name(), "one");
    }

    #[test]
    fn parse_unknown_status_drops_lines() {
        let md = "\
# S

## Funções tocadas

### Em `a/` (BOGUS)
- `a::run`
";
        let parsed = parse(md).expect("opens");
        // Status unknown ⇒ functions dropped, but section parsed empty.
        assert!(parsed.is_empty());
    }

    // --- validate ------------------------------------------------------

    // --- fallback -----------------------------------------------------

    // --- AC-FT-6 — self-validation against Spec A spec.md --------------
    //
    // Loads `.claude/spec/2026-05-27-mustard-v4-foundation/spec.md` at test
    // time and runs the parser+validator against it. The spec is committed
    // to the repo so the path is stable for CI; the test is skipped (with a
    // diagnostic) when the file is missing so a partial checkout still
    // builds.

}
